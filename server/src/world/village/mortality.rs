//! Character health, starvation mortality, estates and business succession.
//!
//! Meals remain owned by `settlement_economy`; this module converts each
//! newly recorded missed meal into health damage exactly once, then resolves
//! every relationship before the dead entity is despawned. Stable PersonIds
//! make that cleanup identity-safe even when display names repeat.

use std::collections::{HashMap, VecDeque};

use super::*;

use shared::components::{
    CharacterAffiliation, CommandedBy, DeathCause, Health, Hero, LivesAt, OwnedBy, PersonId,
    ResidentOf, STARVATION_DAMAGE_PER_DAY,
};
use shared::economy::{
    permit_price, BusinessForSale, BusinessLiquidation, BusinessSaleReason, BusinessState,
};

const MAX_RETAINED_DEATHS: usize = 10_000;
const TAKEOVER_PERSONAL_RESERVE: u64 = 2 * PENNIES_PER_COIN;
const NUTRITION_HEALTH_TICK_WORLD_SECONDS: f32 = 5.0;
const HUNGER_HEALTH_LOSS_PER_WORLD_SECOND: f32 = 0.10;
const FED_HEALTH_RECOVERY_PER_WORLD_SECOND: f32 = 0.20;
const PROTECTED_HUNGRY_DAYS: u16 = 10;

/// Server-only counters preventing a changed Nutrition component from applying
/// the same missed or successful meal twice.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(crate) struct AppliedNutritionHealth {
    missed_meals: u32,
    successful_meals: u32,
}

/// A short-lived, server-only job. Healthy fed characters carry no component
/// and therefore cost nothing between daily meal boundaries.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NutritionHealthAdjustment {
    target_fraction: f32,
}

impl NutritionHealthAdjustment {
    #[cfg(test)]
    pub(crate) const fn recovering() -> Self {
        Self {
            target_fraction: 1.0,
        }
    }
}

/// One bounded, durable-in-session obituary used by the roster and lab report.
#[derive(Debug, Clone)]
pub(crate) struct DeathRecord {
    pub id: PersonId,
    pub name: String,
    pub kind: CharacterKind,
    pub affiliation: CharacterAffiliation,
    pub attributes: CharacterAttributes,
    pub commanded_by: Option<String>,
    pub day: u32,
    pub cause: DeathCause,
}

/// Deaths are rare relative to simulation ticks. A VecDeque gives the living
/// world a bounded history without making every dead person remain an ECS body.
#[derive(Resource, Debug, Default)]
pub struct MortalityLedger {
    deaths: VecDeque<DeathRecord>,
    pub total_deaths: u64,
}

impl MortalityLedger {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &DeathRecord> {
        self.deaths.iter()
    }

    fn record(&mut self, record: DeathRecord) {
        self.total_deaths = self.total_deaths.saturating_add(1);
        if self.deaths.len() == MAX_RETAINED_DEATHS {
            self.deaths.pop_front();
        }
        self.deaths.push_back(record);
    }
}

/// Backfill authored characters and old tests as well as normal spawn paths.
/// Every character receives both vitals; only people whose Nutrition records a
/// missed meal take starvation damage.
pub fn ensure_character_vitals(
    mut commands: Commands,
    missing_health: Query<Entity, (With<CharacterKind>, Without<Health>)>,
    missing_nutrition: Query<Entity, (With<CharacterKind>, Without<Nutrition>)>,
) {
    for entity in missing_health.iter() {
        commands.entity(entity).insert(Health::default());
    }
    for entity in missing_nutrition.iter() {
        commands.entity(entity).insert(Nutrition::default());
    }
}

/// Translate changed meal history into one sparse Health adjustment.
///
/// Hunger first lowers the safe Health ceiling; only misses beyond ten
/// consecutive days deal direct lethal damage. Eating immediately restores the
/// ceiling, while [`advance_nutrition_health`] performs gradual regeneration.
pub fn apply_nutrition_condition(
    mut commands: Commands,
    mut characters: Query<
        (
            Entity,
            &Nutrition,
            &mut Health,
            Option<&mut AppliedNutritionHealth>,
            Option<&mut NutritionHealthAdjustment>,
        ),
        Changed<Nutrition>,
    >,
) {
    for (entity, nutrition, mut health, applied, adjustment) in characters.iter_mut() {
        let previously_missed = applied.as_deref().map_or(0, |applied| applied.missed_meals);
        let previously_successful = applied
            .as_deref()
            .map_or(0, |applied| applied.successful_meals);
        let newly_missed = nutrition
            .total_missed_meals
            .saturating_sub(previously_missed);
        let newly_successful = nutrition.total_meals.saturating_sub(previously_successful);
        if health.is_dead() {
            continue;
        }

        if newly_missed > 0 {
            let missed_in_batch = newly_missed.min(u32::from(u16::MAX)) as u16;
            let previous_streak = nutrition
                .consecutive_missed_meals
                .saturating_sub(missed_in_batch);
            let newly_critical = nutrition
                .consecutive_missed_meals
                .saturating_sub(PROTECTED_HUNGRY_DAYS)
                .saturating_sub(previous_streak.saturating_sub(PROTECTED_HUNGRY_DAYS));
            if newly_critical > 0 {
                // A single large simulation step may contain several daily meal
                // outcomes. Crossing the protected ten-day window means the
                // unobserved time was already long enough to reach its 10%
                // hunger floor, so settle that floor before applying lethal
                // misses. This makes 10x/100x catch-up equivalent to ordinary
                // one-day updates instead of allowing time-warp survivors.
                let starvation_floor =
                    health.max * f32::from(nutrition.health_ceiling_percent()) / 100.0;
                health.current = health.current.min(starvation_floor);
                health.take_damage(f32::from(newly_critical) * STARVATION_DAMAGE_PER_DAY);
            }
        }

        let changed_condition = newly_missed > 0 || newly_successful > 0;
        if changed_condition && !health.is_dead() {
            let target_fraction = f32::from(nutrition.health_ceiling_percent()) / 100.0;
            if let Some(mut adjustment) = adjustment {
                adjustment.target_fraction = target_fraction;
            } else if health.current > health.max * target_fraction + f32::EPSILON
                || target_fraction >= 1.0 && health.current < health.max - f32::EPSILON
            {
                commands
                    .entity(entity)
                    .insert(NutritionHealthAdjustment { target_fraction });
            }
        }
        if let Some(mut applied) = applied {
            if applied.missed_meals != nutrition.total_missed_meals
                || applied.successful_meals != nutrition.total_meals
            {
                applied.missed_meals = nutrition.total_missed_meals;
                applied.successful_meals = nutrition.total_meals;
            }
        } else {
            commands.entity(entity).insert(AppliedNutritionHealth {
                missed_meals: nutrition.total_missed_meals,
                successful_meals: nutrition.total_meals,
            });
        }
    }
}

/// Progress only characters whose food condition is actively changing Health.
/// Five-world-second buckets keep replication smooth enough to watch while a
/// healthy population of any size has zero steady per-frame iteration.
pub fn advance_nutrition_health(
    mut commands: Commands,
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut accumulated_world_seconds: Local<f32>,
    mut characters: Query<
        (Entity, &NutritionHealthAdjustment, &mut Health),
        Without<crate::player::hero::OfflineHero>,
    >,
) {
    *accumulated_world_seconds += simulation_time.world_seconds().max(0.0);
    if *accumulated_world_seconds < NUTRITION_HEALTH_TICK_WORLD_SECONDS {
        return;
    }
    let elapsed = std::mem::take(&mut *accumulated_world_seconds);

    for (entity, adjustment, mut health) in characters.iter_mut() {
        if health.is_dead() {
            commands
                .entity(entity)
                .remove::<NutritionHealthAdjustment>();
            continue;
        }
        let target = health.max * adjustment.target_fraction.clamp(0.0, 1.0);
        if health.current > target + 0.01 {
            health.current =
                (health.current - HUNGER_HEALTH_LOSS_PER_WORLD_SECOND * elapsed).max(target);
        } else if adjustment.target_fraction >= 1.0 && health.current < target - 0.01 {
            health.heal(FED_HEALTH_RECOVERY_PER_WORLD_SECOND * elapsed);
        }

        let finished = if adjustment.target_fraction >= 1.0 {
            health.current >= target - 0.01
        } else {
            health.current <= target + 0.01
        };
        if finished {
            commands
                .entity(entity)
                .remove::<NutritionHealthAdjustment>();
        }
    }
}

pub(crate) fn takeover_price(kind: SettlementBuildingKind) -> u64 {
    permit_price(kind, 0, false).max(PENNIES_PER_COIN)
}

fn unused_permit_escrow(ledger: &shared::components::PlayerPermitLedger) -> u64 {
    ledger
        .permits
        .iter()
        .map(shared::components::PlayerPermit::total_escrow)
        .fold(0, u64::saturating_add)
}

#[derive(Debug)]
struct DyingCharacter {
    entity: Entity,
    id: PersonId,
    name: String,
    kind: CharacterKind,
    affiliation: CharacterAffiliation,
    attributes: CharacterAttributes,
    commanded_by: Option<String>,
    home: Option<shared::components::BuildingId>,
    settlement: Option<shared::components::SettlementId>,
    civic_job: Option<shared::components::CivicEmployment>,
    wallet: u64,
    permit_escrow: u64,
    goods: [u32; Good::COUNT],
    cause: DeathCause,
}

/// Resolve a zero-health character atomically from the economy's perspective.
///
/// - employment components disappear with the person, exposing the vacancy;
/// - household money/goods pass into the shared home estate;
/// - civic roster aliases and unpayable claims are removed;
/// - private workplaces become replicated takeover listings;
/// - unfinished projects retain their delivered materials instead of vanishing;
/// - heroes are removed from their live/persisted hero slot.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn process_character_deaths(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut ledger: ResMut<MortalityLedger>,
    mut business_events: ResMut<BusinessEventQueue>,
    characters: Query<
        (
            Entity,
            &PersonId,
            &CharacterName,
            &CharacterKind,
            &CharacterAffiliation,
            &CharacterAttributes,
            &Health,
            Option<&Nutrition>,
            Option<&CommandedBy>,
            Option<&LivesAt>,
            Option<&ResidentOf>,
            Option<&shared::components::CivicEmployment>,
            Option<&Wallet>,
            Option<&GoodsInventory>,
            Option<&Hero>,
        ),
        Changed<Health>,
    >,
    permit_ledgers: Query<&shared::components::PlayerPermitLedger>,
    mut houses: Query<
        (
            &shared::components::BuildingId,
            &mut Household,
            &mut HouseholdEconomy,
            &mut GoodsInventory,
        ),
        (With<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &mut Settlement,
            &mut GoodsInventory,
            &mut MootAdministration,
            &mut MootMarket,
        ),
        (
            With<Settlement>,
            Without<SettlementBuilding>,
            Without<CharacterKind>,
        ),
    >,
    mut properties: Query<(
        Entity,
        &SettlementBuilding,
        &OwnedBy,
        Option<&BusinessAccount>,
        Option<&mut BusinessCondition>,
    )>,
    mut worksites: Query<(Entity, &mut UnderConstruction)>,
    mut profiles: Option<ResMut<crate::persistence::profiles::PlayerProfiles>>,
    mut hero_index: Option<ResMut<crate::player::hero::HeroIndex>>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let dying: Vec<DyingCharacter> = characters
        .iter()
        .filter(|(_, _, _, _, _, _, health, ..)| health.is_dead())
        .map(
            |(
                entity,
                id,
                name,
                kind,
                affiliation,
                attributes,
                _,
                nutrition,
                commanded_by,
                home,
                resident_of,
                civic_job,
                wallet,
                inventory,
                _,
            )| {
                let mut goods = [0; Good::COUNT];
                if let Some(inventory) = inventory {
                    for good in Good::ALL {
                        goods[good.index()] = inventory.amount(good);
                    }
                }
                DyingCharacter {
                    entity,
                    id: *id,
                    name: name.0.clone(),
                    kind: *kind,
                    affiliation: *affiliation,
                    attributes: *attributes,
                    commanded_by: commanded_by.map(|account| account.0.clone()),
                    home: home.map(|home| home.0),
                    settlement: resident_of.map(|resident| resident.0),
                    civic_job: civic_job.copied(),
                    wallet: wallet.map_or(0, |wallet| wallet.balance()),
                    permit_escrow: permit_ledgers.get(entity).map_or(0, unused_permit_escrow),
                    goods,
                    cause: if nutrition.is_some_and(|nutrition| nutrition.is_hungry()) {
                        DeathCause::Starvation
                    } else {
                        DeathCause::Unknown
                    },
                }
            },
        )
        .collect();

    for dead in dying {
        business_events.reroute_deceased_person_sales(dead.id, dead.settlement);
        // An unused stamped permit is fully refundable. If its holder dies,
        // return that escrow to the same household/settlement estate path as
        // their wallet instead of silently destroying coin with the entity.
        let mut estate_cash = dead.wallet.saturating_add(dead.permit_escrow);

        // Close the exact durable civic post and settle whatever the treasury
        // can still pay before the personal estate moves into its household.
        if let Some(job) = dead.civic_job {
            if let Some((_, mut settlement, _, mut administration, _)) = halls
                .iter_mut()
                .find(|(settlement_id, ..)| **settlement_id == job.settlement)
            {
                if let Some(entry) = administration
                    .payroll
                    .iter_mut()
                    .find(|entry| entry.person_id == dead.id && entry.role == job.role)
                {
                    let paid = entry.arrears.min(settlement.treasury);
                    settlement.treasury -= paid;
                    estate_cash = estate_cash.saturating_add(paid);
                    if entry.arrears > paid {
                        warn!(
                            "{} died with {} coin of unpaid civic wages written off",
                            dead.name,
                            shared::economy::format_money(entry.arrears - paid),
                        );
                    }
                    entry.arrears = 0;
                    entry.active = false;
                }
                administration
                    .payroll
                    .retain(|entry| entry.person_id != dead.id);
                administration.wage_arrears = administration
                    .payroll
                    .iter()
                    .map(|entry| entry.arrears)
                    .fold(0, u64::saturating_add);
                if administration.reeve.as_deref() == Some(dead.name.as_str()) {
                    administration.reeve = None;
                }
                if administration.road_steward.as_deref() == Some(dead.name.as_str()) {
                    administration.road_steward = None;
                }
                if administration.market_porter.as_deref() == Some(dead.name.as_str()) {
                    administration.market_porter = None;
                }
                administration
                    .city_workers
                    .retain(|name| name != &dead.name);
                administration.guards.retain(|name| name != &dead.name);
            }
        }

        // The home receives the liquid estate and as much carried property as
        // its bounded store accepts. Any overflow falls through to the hall.
        let mut remaining_goods = dead.goods;
        let mut inherited_at_home = false;
        if let Some(home_id) = dead.home {
            if let Some((_, mut household, mut economy, mut pantry)) = houses
                .iter_mut()
                .find(|(building_id, ..)| **building_id == home_id)
            {
                household.resident_ids.retain(|person| *person != dead.id);
                household.residents.retain(|name| name != &dead.name);
                if economy.shopper == Some(dead.id) {
                    economy.shopper = None;
                }
                economy.pennies = economy.pennies.saturating_add(estate_cash);
                estate_cash = 0;
                for good in Good::ALL {
                    let accepted = pantry.add(good, remaining_goods[good.index()]);
                    remaining_goods[good.index()] -= accepted;
                }
                inherited_at_home = true;
            }
        }

        if let Some(settlement_id) = dead.settlement {
            if let Some((_, mut settlement, mut hall, _, mut market)) = halls
                .iter_mut()
                .find(|(candidate, ..)| **candidate == settlement_id)
            {
                market.transfer_seller(
                    shared::economy::MarketSeller::Person(dead.id),
                    shared::economy::MarketSeller::Treasury(settlement_id),
                );
                settlement.treasury = settlement.treasury.saturating_add(estate_cash);
                estate_cash = 0;
                for good in Good::ALL {
                    let accepted = hall.add(good, remaining_goods[good.index()]);
                    remaining_goods[good.index()] -= accepted;
                }
            }
        }
        if estate_cash > 0 || remaining_goods.iter().any(|amount| *amount > 0) {
            warn!(
                "{} left an unclaimed estate: {} coin and {:?} goods (home inheritance={})",
                dead.name,
                shared::economy::format_money(estate_cash),
                remaining_goods,
                inherited_at_home,
            );
        }

        // Remove every durable ownership reference. Productive workplaces are
        // retained as real sale listings; houses simply become unowned while
        // their surviving household remains intact.
        for (property, building, owner, _account, condition) in properties.iter_mut() {
            if owner.0 != dead.id {
                continue;
            }
            let mut updated = building.clone();
            updated.owner = None;
            let mut property_commands = commands.entity(property);
            property_commands.insert(updated).remove::<OwnedBy>();
            if business_output(building.kind).is_some() {
                if let Some(mut condition) = condition {
                    condition.state = BusinessState::Liquidating;
                    condition.liquidation_days = 0;
                }
                property_commands.insert((
                    BusinessForSale {
                        previous_owner: dead.id,
                        asking_price: takeover_price(building.kind),
                        listed_day: day,
                        reason: BusinessSaleReason::OwnerDied,
                    },
                    BusinessLiquidation::owner_died(day),
                ));
            }
        }

        // A dead builder never deletes a supplied site. Private business sites
        // join the same takeover market; housing/public works wait for another
        // available resident to adopt the construction duty.
        for (site_entity, mut site) in worksites.iter_mut() {
            if site.builder == Some(dead.entity) {
                site.builder = None;
            }
            if site.owner_id != Some(dead.id) {
                continue;
            }
            site.owner_id = None;
            site.owner = None;
            site.builder = None;
            if business_output(site.kind).is_some() {
                commands.entity(site_entity).insert(BusinessForSale {
                    previous_owner: dead.id,
                    asking_price: takeover_price(site.kind),
                    listed_day: day,
                    reason: BusinessSaleReason::OwnerDied,
                });
            }
        }

        if dead.kind == CharacterKind::Hero {
            if let Some(account) = dead.commanded_by.as_deref() {
                if let Some(index) = hero_index.as_deref_mut() {
                    index.by_name.remove(account);
                }
                if let Some(profile) = profiles
                    .as_deref_mut()
                    .and_then(|profiles| profiles.profiles.get_mut(account))
                {
                    profile.hero = None;
                }
            }
        }

        info!("{} died on day {} ({})", dead.name, day, dead.cause.label());
        ledger.record(DeathRecord {
            id: dead.id,
            name: dead.name,
            kind: dead.kind,
            affiliation: dead.affiliation,
            attributes: dead.attributes,
            commanded_by: dead.commanded_by,
            day,
            cause: dead.cause,
        });
        commands.entity(dead.entity).despawn();
    }
}

fn reopened_state(account: &BusinessAccount) -> BusinessState {
    if account.wage_arrears > 0 || account.tax_arrears > 0 {
        BusinessState::Distressed
    } else {
        BusinessState::New
    }
}

/// Residents buy inherited workplaces with personal money. Payment becomes
/// firm capital, so ownership transfer cannot mint coin or pay a dead seller.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn acquire_businesses_for_sale(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut completed: Query<
        (
            Entity,
            &BusinessForSale,
            &mut SettlementBuilding,
            &shared::components::BuildingOf,
            &mut BusinessAccount,
            &mut BusinessCondition,
            &mut BusinessSalePolicy,
            Option<&OwnedBy>,
        ),
        (With<SettlementBuilding>, Without<UnderConstruction>),
    >,
    mut sites: Query<
        (
            Entity,
            &BusinessForSale,
            &mut UnderConstruction,
            Option<&InheritedBusinessCapital>,
        ),
        (With<UnderConstruction>, Without<SettlementBuilding>),
    >,
    active_business_owners: Query<
        (&OwnedBy, &BusinessCondition),
        (
            With<BusinessAccount>,
            With<SettlementBuilding>,
            Without<BusinessForSale>,
        ),
    >,
    mut residents: Query<(
        Entity,
        &PersonId,
        &CharacterName,
        &ResidentOf,
        &mut VillagerIntent,
        &mut Wallet,
        Option<&WorkStatus>,
        Option<&Occupation>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        Option<&Health>,
    )>,
) {
    if completed.is_empty() && sites.is_empty() {
        return;
    }
    let day = world_time
        .iter()
        .next()
        .map_or(u32::MAX, |world_time| world_time.day);
    let mut holdings = HashMap::<PersonId, usize>::new();
    let mut blocked_portfolios = HashSet::<PersonId>::new();
    for (owner, condition) in active_business_owners.iter() {
        *holdings.entry(owner.0).or_default() += 1;
        if condition.state.blocks_owner_expansion() {
            blocked_portfolios.insert(owner.0);
        }
    }
    for (_, listing, _, _, _, condition, _, owner) in completed.iter() {
        if let Some(owner) = owner {
            *holdings.entry(owner.0).or_default() += 1;
            if listing.asking_price > 0 || condition.state.blocks_owner_expansion() {
                blocked_portfolios.insert(owner.0);
            }
        }
    }

    let choose_buyer = |settlement: shared::components::SettlementId,
                        price: u64,
                        must_build: bool,
                        previous_owner: PersonId,
                        residents: &mut Query<(
        Entity,
        &PersonId,
        &CharacterName,
        &ResidentOf,
        &mut VillagerIntent,
        &mut Wallet,
        Option<&WorkStatus>,
        Option<&Occupation>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        Option<&Health>,
    )>,
                        holdings: &HashMap<PersonId, usize>,
                        blocked_portfolios: &HashSet<PersonId>| {
        residents
            .iter()
            .filter(
                |(
                    _,
                    person_id,
                    _,
                    resident_of,
                    intent,
                    wallet,
                    status,
                    occupation,
                    employed,
                    civic,
                    health,
                )| {
                    resident_of.0 == settlement
                        && intent.counts_as_resident()
                        && **person_id != previous_owner
                        && !blocked_portfolios.contains(*person_id)
                        && civic.is_none()
                        && !health.is_some_and(|health| health.is_dead())
                        && wallet.balance() >= price.saturating_add(TAKEOVER_PERSONAL_RESERVE)
                        && (!must_build
                            || (intent.is_settled()
                                && employed.is_none()
                                && occupation.is_none_or(|occupation| occupation.0.is_none())
                                && status
                                    .is_none_or(|status| *status == WorkStatus::LookingForWork)))
                },
            )
            .min_by_key(|(_, person_id, _, _, _, wallet, ..)| {
                (
                    holdings.get(person_id).copied().unwrap_or(0),
                    std::cmp::Reverse(wallet.balance()),
                    person_id.0,
                )
            })
            .map(|(entity, person_id, name, ..)| (entity, *person_id, name.0.clone()))
    };

    for (business, listing, mut building, building_of, mut account, mut condition, mut sale, _) in
        completed.iter_mut()
    {
        if day.saturating_sub(listing.listed_day) < PROPERTY_MARKET_EXPOSURE_DAYS {
            continue;
        }
        let Some((buyer, buyer_id, buyer_name)) = choose_buyer(
            building_of.0,
            listing.asking_price,
            false,
            listing.previous_owner,
            &mut residents,
            &holdings,
            &blocked_portfolios,
        ) else {
            continue;
        };
        let Ok((_, _, _, _, _, mut wallet, ..)) = residents.get_mut(buyer) else {
            continue;
        };
        if !wallet.debit(listing.asking_price) {
            continue;
        }
        account.contribute_capital(listing.asking_price);
        condition.state = reopened_state(&account);
        condition.insolvent_days = 0;
        condition.cash_tight_days = 0;
        condition.opened_day = u32::MAX;
        condition.operating_days = 0;
        condition.liquidation_days = 0;
        sale.collection_enabled = true;
        building.owner = Some(buyer_name.clone());
        commands
            .entity(business)
            .insert(OwnedBy(buyer_id))
            .remove::<BusinessForSale>()
            .remove::<BusinessLiquidation>();
        *holdings.entry(buyer_id).or_default() += 1;
        blocked_portfolios.insert(buyer_id);
        info!(
            "{} bought the inherited {} in '{}' for {} coin",
            buyer_name,
            building.kind.label(),
            building.settlement,
            shared::economy::format_money(listing.asking_price),
        );
    }

    for (site_entity, listing, mut site, inherited_capital) in sites.iter_mut() {
        if day.saturating_sub(listing.listed_day) < PROPERTY_MARKET_EXPOSURE_DAYS {
            continue;
        }
        let Some((buyer, buyer_id, buyer_name)) = choose_buyer(
            site.settlement_id,
            listing.asking_price,
            true,
            listing.previous_owner,
            &mut residents,
            &holdings,
            &blocked_portfolios,
        ) else {
            continue;
        };
        let Ok((_, _, _, _, mut intent, mut wallet, ..)) = residents.get_mut(buyer) else {
            continue;
        };
        if !wallet.debit(listing.asking_price) {
            continue;
        }
        site.owner = Some(buyer_name.clone());
        site.owner_id = Some(buyer_id);
        site.builder = Some(buyer);
        *intent = VillagerIntent::Building {
            settlement: site.settlement,
            site: site_entity,
        };
        commands
            .entity(site_entity)
            .insert(InheritedBusinessCapital(
                inherited_capital
                    .map_or(0, |capital| capital.0)
                    .saturating_add(listing.asking_price),
            ))
            .remove::<BusinessForSale>();
        commands.entity(buyer).insert((
            ConstructionMaterialRoutine::new(site_entity),
            CharacterActivity::Idle,
        ));
        *holdings.entry(buyer_id).or_default() += 1;
        blocked_portfolios.insert(buyer_id);
        info!(
            "{} took over the unfinished {} for {} coin",
            buyer_name,
            site.kind.label(),
            shared::economy::format_money(listing.asking_price),
        );
    }
}

/// Adopt a non-business worksite whose builder vanished. Delivered materials,
/// plot reservation and road access all remain attached to the original site.
#[allow(clippy::type_complexity)]
pub fn recover_orphaned_construction(
    mut commands: Commands,
    listings: Query<(), With<BusinessForSale>>,
    player_projects: Query<(), With<crate::player::permits::PlayerConstructionProject>>,
    mut sites: Query<(Entity, &mut UnderConstruction)>,
    living: Query<(), (With<CharacterKind>, With<Health>)>,
    mut residents: Query<(
        Entity,
        &mut VillagerIntent,
        Option<&Occupation>,
        Option<&WorkStatus>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        &Health,
    )>,
) {
    let mut claimed = HashSet::new();
    for (site_entity, mut site) in sites.iter_mut() {
        if site
            .builder
            .is_some_and(|builder| living.get(builder).is_ok())
        {
            continue;
        }
        site.builder = None;
        if listings.get(site_entity).is_ok() || player_projects.get(site_entity).is_ok() {
            continue;
        }
        let replacement = residents
            .iter()
            .filter(|(entity, intent, occupation, status, employed, civic, health)| {
                !claimed.contains(entity)
                    && !health.is_dead()
                    && matches!(**intent, VillagerIntent::Resident { settlement } if settlement == site.settlement)
                    && occupation.is_none_or(|occupation| occupation.0.is_none())
                    && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
                    && employed.is_none()
                    && civic.is_none()
            })
            .min_by_key(|(entity, ..)| entity.to_bits())
            .map(|(entity, ..)| entity);
        let Some(replacement) = replacement else {
            continue;
        };
        let Ok((_, mut intent, ..)) = residents.get_mut(replacement) else {
            continue;
        };
        *intent = VillagerIntent::Building {
            settlement: site.settlement,
            site: site_entity,
        };
        site.builder = Some(replacement);
        claimed.insert(replacement);
        commands.entity(replacement).insert((
            ConstructionMaterialRoutine::new(site_entity),
            CharacterActivity::Idle,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_permit_money_remains_part_of_the_owners_estate() {
        let ledger = shared::components::PlayerPermitLedger {
            permits: vec![
                shared::components::PlayerPermit {
                    id: shared::components::PermitId(1),
                    settlement: shared::components::SettlementId(2),
                    kind: SettlementBuildingKind::Farmstead,
                    fee_escrow: 300,
                    startup_capital_escrow: 0,
                    purchased_day: 4,
                },
                shared::components::PlayerPermit {
                    id: shared::components::PermitId(2),
                    settlement: shared::components::SettlementId(2),
                    kind: SettlementBuildingKind::Windmill,
                    fee_escrow: 250,
                    startup_capital_escrow: 475,
                    purchased_day: 5,
                },
            ],
        };
        assert_eq!(unused_permit_escrow(&ledger), 1_025);
    }

    fn advance_world_seconds(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(seconds));
        app.update();
    }

    #[test]
    fn ten_hungry_days_reach_the_safe_floor_and_the_eleventh_is_fatal() {
        let mut app = App::new();
        app.init_resource::<Time>().add_systems(
            Update,
            (apply_nutrition_condition, advance_nutrition_health).chain(),
        );
        let person = app
            .world_mut()
            .spawn((
                Nutrition {
                    last_meal_day: None,
                    consecutive_missed_meals: 10,
                    total_meals: 0,
                    total_missed_meals: 10,
                },
                Health::default(),
            ))
            .id();

        advance_world_seconds(&mut app, WorldTime::DEFAULT_DAY_DURATION);
        assert_eq!(app.world().get::<Health>(person).unwrap().current, 10.0);
        assert!(!app.world().get::<Health>(person).unwrap().is_dead());

        app.world_mut()
            .get_mut::<Nutrition>(person)
            .unwrap()
            .record_missed_meal();
        advance_world_seconds(&mut app, NUTRITION_HEALTH_TICK_WORLD_SECONDS);
        assert_eq!(app.world().get::<Health>(person).unwrap().current, 0.0);
    }

    #[test]
    fn an_eleven_day_catch_up_is_as_fatal_as_eleven_daily_updates() {
        let mut app = App::new();
        app.add_systems(Update, apply_nutrition_condition);
        let person = app
            .world_mut()
            .spawn((
                Nutrition {
                    last_meal_day: None,
                    consecutive_missed_meals: 11,
                    total_meals: 0,
                    total_missed_meals: 11,
                },
                Health::default(),
            ))
            .id();

        app.update();

        assert_eq!(app.world().get::<Health>(person).unwrap().current, 0.0);
    }

    #[test]
    fn eating_restores_the_ceiling_then_health_recovers_gradually() {
        let mut app = App::new();
        app.init_resource::<Time>().add_systems(
            Update,
            (apply_nutrition_condition, advance_nutrition_health).chain(),
        );
        let person = app
            .world_mut()
            .spawn((Nutrition::default(), Health::default()))
            .id();

        app.world_mut()
            .get_mut::<Nutrition>(person)
            .unwrap()
            .record_missed_meal();
        advance_world_seconds(&mut app, WorldTime::DEFAULT_DAY_DURATION);
        assert_eq!(app.world().get::<Health>(person).unwrap().current, 80.0);

        app.world_mut()
            .get_mut::<Nutrition>(person)
            .unwrap()
            .record_missed_meal();
        advance_world_seconds(&mut app, WorldTime::DEFAULT_DAY_DURATION);
        assert_eq!(app.world().get::<Health>(person).unwrap().current, 70.0);

        app.world_mut()
            .get_mut::<Nutrition>(person)
            .unwrap()
            .record_meal(3);
        advance_world_seconds(&mut app, NUTRITION_HEALTH_TICK_WORLD_SECONDS);
        assert_eq!(app.world().get::<Health>(person).unwrap().current, 71.0);

        app.world_mut()
            .get_mut::<Nutrition>(person)
            .unwrap()
            .record_meal(3);
        advance_world_seconds(&mut app, 0.0);
        assert_eq!(
            app.world().get::<Health>(person).unwrap().current,
            71.0,
            "a duplicate meal event must not create instant healing"
        );
        assert_eq!(app.world().get::<Nutrition>(person).unwrap().total_meals, 1);

        advance_world_seconds(&mut app, 145.0);
        assert_eq!(app.world().get::<Health>(person).unwrap().current, 100.0);
    }

    #[test]
    fn death_settles_the_estate_and_lists_then_transfers_a_business() {
        let mut app = App::new();
        app.init_resource::<MortalityLedger>()
            .init_resource::<BusinessEventQueue>();
        // This is the production order: deaths create listings after the day's
        // acquisition pass, and automatic investors wait through the public
        // exposure day. Keeping the test on that cadence catches both deferred
        // command bugs and listings which disappear too quickly to inspect.
        app.add_systems(
            Update,
            (
                acquire_businesses_for_sale,
                process_character_deaths,
                fill_vacancies,
            )
                .chain(),
        );

        let settlement_id = shared::components::SettlementId(10);
        let house_id = shared::components::BuildingId(20);
        let business_id = shared::components::BuildingId(21);
        let dead_id = PersonId(1);
        let buyer_id = PersonId(2);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let settlement = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Test Moot".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 0,
                },
                GoodsInventory::new(100),
                MootAdministration::default(),
                MootMarket::founding(),
            ))
            .id();

        let mut carried = GoodsInventory::new(8);
        assert_eq!(carried.add(Good::Wood, 2), 2);
        let dead = app
            .world_mut()
            .spawn((
                dead_id,
                CharacterName("Alda".to_string()),
                CharacterKind::Villager,
                CharacterAffiliation::default(),
                CharacterAttributes::default(),
                Health {
                    current: 0.0,
                    max: shared::components::CHARACTER_MAX_HEALTH,
                },
                Nutrition {
                    last_meal_day: None,
                    consecutive_missed_meals: 10,
                    total_meals: 0,
                    total_missed_meals: 10,
                },
                LivesAt(house_id),
                ResidentOf(settlement_id),
                shared::components::EmployedAt(business_id),
                Wallet::new(7 * PENNIES_PER_COIN),
                carried,
            ))
            .id();

        let house = app
            .world_mut()
            .spawn((
                house_id,
                shared::components::BuildingOf(settlement_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Test Moot".to_string(),
                    owner: Some("Alda".to_string()),
                    quality: 1.0,
                    workers: Vec::new(),
                },
                OwnedBy(dead_id),
                Household {
                    resident_ids: vec![dead_id, buyer_id],
                    residents: vec!["Alda".to_string(), "Borin".to_string()],
                },
                HouseholdEconomy {
                    pennies: PENNIES_PER_COIN,
                    ..default()
                },
                GoodsInventory::new(100),
            ))
            .id();

        let business = app
            .world_mut()
            .spawn((
                business_id,
                shared::components::BuildingOf(settlement_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Test Moot".to_string(),
                    owner: Some("Alda".to_string()),
                    quality: 1.0,
                    workers: vec!["Alda".to_string()],
                },
                OwnedBy(dead_id),
                BusinessAccount::with_capital(3 * PENNIES_PER_COIN),
                BusinessCondition::default(),
                BusinessSalePolicy::for_good(Good::Wheat),
                PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
            ))
            .id();

        let buyer_starting_money = 20 * PENNIES_PER_COIN;
        let buyer = app
            .world_mut()
            .spawn((
                buyer_id,
                CharacterName("Borin".to_string()),
                ResidentOf(settlement_id),
                VillagerIntent::Resident { settlement },
                Wallet::new(buyer_starting_money),
                WorkStatus::LookingForWork,
                Occupation::default(),
                Health::default(),
                PlayerPosition(Vec3::ZERO),
            ))
            .id();
        app.world_mut()
            .get_mut::<MootMarket>(settlement)
            .unwrap()
            .consign(
                shared::economy::MarketSeller::Person(dead_id),
                Good::Wood,
                1,
                48,
            );

        app.update();

        assert!(app.world().get_entity(dead).is_err());
        let household = app.world().get::<Household>(house).unwrap();
        assert_eq!(household.resident_ids, vec![buyer_id]);
        assert_eq!(household.residents, vec!["Borin"]);
        assert_eq!(
            app.world().get::<HouseholdEconomy>(house).unwrap().pennies,
            8 * PENNIES_PER_COIN
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(house)
                .unwrap()
                .amount(Good::Wood),
            2
        );
        assert!(app.world().get::<OwnedBy>(house).is_none());
        assert_eq!(
            app.world()
                .get::<SettlementBuilding>(house)
                .unwrap()
                .owner
                .as_deref(),
            None
        );
        assert!(app.world().get::<OwnedBy>(business).is_none());
        assert!(
            app.world()
                .get::<shared::components::EmployedAt>(buyer)
                .is_none(),
            "an ownerless firm must not hire while its takeover is unresolved"
        );
        assert!(app.world().get::<BusinessLiquidation>(business).is_some());
        let market = app.world().get::<MootMarket>(settlement).unwrap();
        assert_eq!(
            market.seller_total_listed_units(shared::economy::MarketSeller::Person(dead_id)),
            0
        );
        assert_eq!(
            market.seller_listed_units(
                shared::economy::MarketSeller::Treasury(settlement_id),
                Good::Wood,
            ),
            1,
            "a future buyer must pay the estate rather than a despawned person"
        );
        let takeover_price = app
            .world()
            .get::<BusinessForSale>(business)
            .unwrap()
            .asking_price;
        assert_eq!(app.world().resource::<MortalityLedger>().total_deaths, 1);

        app.update();

        assert!(
            app.world().get::<BusinessForSale>(business).is_some(),
            "new property must remain visible on the public board for a full world day"
        );
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        assert_eq!(
            app.world().get::<OwnedBy>(business),
            Some(&OwnedBy(buyer_id))
        );
        assert!(app.world().get::<BusinessForSale>(business).is_none());
        assert!(app.world().get::<BusinessLiquidation>(business).is_none());
        assert_eq!(
            app.world().get::<Wallet>(buyer).unwrap().balance(),
            buyer_starting_money - takeover_price
        );
        let account = app.world().get::<BusinessAccount>(business).unwrap();
        assert_eq!(account.cash, 3 * PENNIES_PER_COIN + takeover_price);
        assert_eq!(
            app.world()
                .get::<SettlementBuilding>(business)
                .unwrap()
                .owner
                .as_deref(),
            Some("Borin")
        );
        assert!(
            app.world()
                .get::<BusinessCondition>(business)
                .is_some_and(|condition| condition.state.can_operate()),
            "the buyer should reopen the inherited firm"
        );
    }
}
