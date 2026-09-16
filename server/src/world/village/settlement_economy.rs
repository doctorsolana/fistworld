//! Settlement-level markets, food security and prosperity evidence.
//!
//! This module owns aggregate economic state. Embodied trade work remains in
//! `commerce` and physical resource creation remains in `trades`.

use super::*;

/// Daily food accounting occurs immediately after breakfast, while embodied
/// producers and porters refill the exchange during the following minutes.
/// One Bakery cycle creates four Bread, so allow at most that single embodied
/// delivery batch to be on the wrong side of the day boundary. Keeping this in
/// ration units rather than a fraction of population prevents the tolerance
/// growing into hundreds of free meals in a large town. The visible reserve
/// target remains three full days, and production plus zero unmet meals remain
/// independent requirements.
const FOOD_SECURITY_BOUNDARY_TOLERANCE_RATIONS: u32 = 4;

fn has_secure_food_day(residents: u32, economy: &SettlementEconomy) -> bool {
    let target_stock = (residents as f32 * FOOD_SECURITY_TARGET_DAYS).ceil() as u32;
    residents > 0
        && economy
            .edible_stock
            .saturating_add(FOOD_SECURITY_BOUNDARY_TOLERANCE_RATIONS)
            >= target_stock
        && economy.recent_food_production >= residents as f32
        && economy.unmet_food == 0
}

const UNREST_HUNGER_WEIGHT: f32 = 55.0;
const UNREST_HOMELESSNESS_WEIGHT: f32 = 25.0;
const UNREST_UNPAID_WAGE_WEIGHT: f32 = 20.0;
const MAX_DAILY_UNREST_INCREASE: f32 = 10.0;
const MAX_DAILY_UNREST_RECOVERY: f32 = 5.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct UnrestPressures {
    hunger: f32,
    housing: f32,
    wages: f32,
}

impl UnrestPressures {
    fn target(self) -> f32 {
        (self.hunger + self.housing + self.wages).clamp(0.0, 100.0)
    }
}

fn unrest_pressures(
    residents: u32,
    hungry: u32,
    homeless: u32,
    unpaid_workers: u16,
) -> UnrestPressures {
    if residents == 0 {
        return UnrestPressures::default();
    }
    let residents = residents as f32;
    UnrestPressures {
        hunger: (hungry as f32 / residents).clamp(0.0, 1.0) * UNREST_HUNGER_WEIGHT,
        housing: (homeless as f32 / residents).clamp(0.0, 1.0) * UNREST_HOMELESSNESS_WEIGHT,
        wages: (f32::from(unpaid_workers) / residents).clamp(0.0, 1.0) * UNREST_UNPAID_WAGE_WEIGHT,
    }
}

fn advance_unrest(current: f32, target: f32) -> f32 {
    let current = if current.is_finite() {
        current.clamp(0.0, 100.0)
    } else {
        0.0
    };
    let target = if target.is_finite() {
        target.clamp(0.0, 100.0)
    } else {
        0.0
    };
    if target >= current {
        current + (target - current).min(MAX_DAILY_UNREST_INCREASE)
    } else {
        current - (current - target).min(MAX_DAILY_UNREST_RECOVERY)
    }
}

const FOOD_HISTORY_DAYS: usize = 3;
/// Spread discretionary funds over several meal boundaries. Relief does not
/// spend payroll reserves or pretend the ordinary food target is untouchable.
const EMERGENCY_RELIEF_BUDGET_DAYS: u64 = 3;

fn emergency_relief_budget(
    settlement: &Settlement,
    administration: Option<&MootAdministration>,
    policies: &SettlementPolicies,
) -> u64 {
    civic::civic_discretionary_budget(settlement, administration, Some(policies))
        / EMERGENCY_RELIEF_BUDGET_DAYS
}

#[derive(Debug)]
struct SettlementEconomyDay {
    last_world_day: u32,
    produced_today: u32,
    production_history: [u32; FOOD_HISTORY_DAYS],
    consumption_history: [u32; FOOD_HISTORY_DAYS],
    recorded_days: usize,
}

impl SettlementEconomyDay {
    fn new(day: u32) -> Self {
        Self {
            last_world_day: day,
            produced_today: 0,
            production_history: [0; FOOD_HISTORY_DAYS],
            consumption_history: [0; FOOD_HISTORY_DAYS],
            recorded_days: 0,
        }
    }

    fn finish_day(&mut self, consumed: u32) {
        self.production_history.rotate_right(1);
        self.consumption_history.rotate_right(1);
        self.production_history[0] = std::mem::take(&mut self.produced_today);
        self.consumption_history[0] = consumed;
        self.recorded_days = (self.recorded_days + 1).min(FOOD_HISTORY_DAYS);
    }

    fn recent_average(values: &[u32; FOOD_HISTORY_DAYS], days: usize) -> f32 {
        if days == 0 {
            return 0.0;
        }
        values[..days].iter().sum::<u32>() as f32 / days as f32
    }
}

/// Server-only daily buckets behind the small replicated economy summary.
#[derive(Resource, Default)]
pub struct SettlementEconomyRuntime {
    by_settlement: HashMap<Entity, SettlementEconomyDay>,
}

pub(super) fn sell_carried_to_moot(
    _commands: &mut Commands,
    _settlement: Entity,
    worker: shared::components::PersonId,
    _worker_wallet: Option<&mut Wallet>,
    owner: Option<shared::components::PersonId>,
    good: Good,
    carrier: &mut GoodsInventory,
    hall: &mut GoodsInventory,
    market: &mut MootMarket,
) -> u32 {
    if !market.can_trade(good) {
        return 0;
    }
    let room = hall.free_bulk() / good.bulk_per_unit();
    let offered = carrier.amount(good).min(room);
    let moved = carrier.transfer_to(hall, good, offered);
    if moved > 0 {
        market.consign(
            shared::economy::MarketSeller::Person(owner.unwrap_or(worker)),
            good,
            moved,
            market.suggested_price(good),
        );
    }
    moved
}

pub(super) fn buy_from_moot(
    day: u32,
    settlement: shared::components::SettlementId,
    good: Good,
    requested: u32,
    buyer: &mut Wallet,
    hall: &mut GoodsInventory,
    carrier: &mut GoodsInventory,
    market: &mut MootMarket,
    business_events: &mut BusinessEventQueue,
) -> u32 {
    let room = carrier.free_bulk() / good.bulk_per_unit();
    let available = requested.min(room).min(hall.amount(good));
    let purchase = market.purchase(good, available, buyer.balance(), None, None);
    if purchase.trade.units == 0 || !buyer.debit(purchase.trade.pennies) {
        return 0;
    }
    let moved = hall.transfer_to(carrier, good, purchase.trade.units);
    debug_assert_eq!(moved, purchase.trade.units);
    business_events.record_market_purchase(day, settlement, purchase.fills);
    moved
}

/// Backfill monetary state on old/test entities and initialise new foundations.
pub fn ensure_village_finances(
    mut commands: Commands,
    mut settlements: Query<(Entity, &mut Settlement, Option<&MootMarket>)>,
    villagers: Query<(Entity, &CharacterKind, Option<&Wallet>)>,
) {
    for (entity, mut settlement, market) in settlements.iter_mut() {
        if market.is_none() {
            if settlement.treasury == 0 {
                settlement.treasury = STARTING_TREASURY_MONEY;
            }
            commands.entity(entity).insert(MootMarket::founding());
        }
    }
    for (entity, kind, wallet) in villagers.iter() {
        if *kind == CharacterKind::Villager && wallet.is_none() {
            commands.entity(entity).insert(Wallet::founding_villager());
        }
    }
}

/// Reprice each Moot against real inventory and measurable local demand.
pub fn update_moot_market_targets(
    mut halls: Query<(
        &shared::components::SettlementId,
        &Settlement,
        &GoodsInventory,
        &mut MootMarket,
    )>,
    sites: Query<(&UnderConstruction, &GoodsInventory)>,
    marketplaces: Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        Option<&shared::components::MarketLevel>,
    )>,
) {
    for (settlement_id, settlement, inventory, mut market) in halls.iter_mut() {
        let outstanding_wood = sites
            .iter()
            .filter(|(site, _)| site.settlement_id == *settlement_id)
            .map(|(site, materials)| {
                site.kind
                    .construction_wood_required()
                    .saturating_sub(materials.amount(Good::Wood))
            })
            .fold(0u32, u32::saturating_add);
        let market_level = marketplaces
            .iter()
            .filter(|(building, building_of, _)| {
                building.kind == SettlementBuildingKind::Market && building_of.0 == *settlement_id
            })
            .map(|(_, _, level)| {
                level
                    .copied()
                    .unwrap_or_else(|| shared::components::MarketLevel::for_tier(settlement.tier))
            })
            .max_by_key(|level| match level {
                shared::components::MarketLevel::Earthen => 0_u8,
                shared::components::MarketLevel::Paved => 1_u8,
            });
        let has_marketplace = market_level.is_some();
        let trade_tier = match market_level {
            None => shared::economy::MarketTradeTier::Moot,
            Some(shared::components::MarketLevel::Earthen) => {
                shared::economy::MarketTradeTier::Marketplace
            }
            Some(shared::components::MarketLevel::Paved) => {
                shared::economy::MarketTradeTier::PavedMarketplace
            }
        };
        market.unlock_trade_tier(trade_tier);
        market.set_targets_with_marketplace(
            settlement.residents,
            outstanding_wood,
            has_marketplace,
        );
        market.reconcile_inventory(*settlement_id, inventory);
        market.refresh_all(inventory);
    }
}

/// Keep one authoritative public inventory per settlement. Completed
/// Marketplaces add physical capacity and another access point, but never own
/// a duplicate pile of goods or a second order book. The transfer also repairs
/// older saves which may have stock stranded on the Marketplace entity.
pub fn sync_public_market_storage(
    mut halls: Query<
        (&shared::components::SettlementId, &mut GoodsInventory),
        (With<Settlement>, Without<SettlementBuilding>),
    >,
    mut marketplaces: Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &mut GoodsInventory,
        ),
        (With<SettlementBuilding>, Without<Settlement>),
    >,
) {
    for (settlement_id, mut hall_store) in halls.iter_mut() {
        let marketplace_count = marketplaces
            .iter()
            .filter(|(building, building_of, _)| {
                building.kind == SettlementBuildingKind::Market && building_of.0 == *settlement_id
            })
            .count()
            .min(u32::MAX as usize) as u32;
        let desired_capacity_per_good = shared::economy::capacity::HALL
            .saturating_add(shared::economy::capacity::MARKET.saturating_mul(marketplace_count));
        hall_store.resize_partitioned_bulk_capacity(desired_capacity_per_good);

        for (building, building_of, mut marketplace_store) in marketplaces.iter_mut() {
            if building.kind != SettlementBuildingKind::Market || building_of.0 != *settlement_id {
                continue;
            }
            for good in Good::ALL {
                marketplace_store.transfer_to(&mut hall_store, good, u32::MAX);
            }
            // This component remains as a compatibility shell for generic
            // building inspection. Zero capacity makes accidental writes fail
            // instead of silently splitting the settlement's public stock.
            marketplace_store.resize_bulk_capacity(0);
        }
    }
}

impl SettlementEconomyRuntime {
    pub(super) fn historical_food(&self, settlement: Entity, history_index: usize) -> (u32, u32) {
        self.by_settlement.get(&settlement).map_or((0, 0), |state| {
            (
                state
                    .production_history
                    .get(history_index)
                    .copied()
                    .unwrap_or(0),
                state
                    .consumption_history
                    .get(history_index)
                    .copied()
                    .unwrap_or(0),
            )
        })
    }

    pub(super) fn record_food_production(&mut self, settlement: Entity, amount: u32) {
        if amount == 0 {
            return;
        }
        if let Some(day) = self.by_settlement.get_mut(&settlement) {
            day.produced_today = day.produced_today.saturating_add(amount);
        }
    }
}

/// Every hall owns one replicated economic reading; physical stock remains in
/// the inventories of the hall and its completed buildings.
pub fn ensure_settlement_economies(
    mut commands: Commands,
    settlements: Query<Entity, (With<Settlement>, Without<SettlementEconomy>)>,
) {
    for entity in settlements.iter() {
        commands.entity(entity).insert(SettlementEconomy::default());
    }
}

/// Consume one edible portion per resident at each world-day boundary, keep a
/// three-day production/consumption reading, derive prosperity from visible
/// facts, and promote food-secure Hamlets.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_settlement_economies(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut queue_clock: ResMut<MootQueueClock>,
    mut settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &mut Settlement,
        &mut SettlementEconomy,
        Option<&mut MootMarket>,
        Option<&SettlementPolicies>,
        Option<&MootAdministration>,
        Option<&mut shared::economy::CivicAccount>,
    )>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        &shared::components::BuildingOf,
        &shared::components::BuildingId,
        Option<&BusinessStaffingPolicy>,
        Option<&BusinessWagePolicy>,
        Option<&BusinessCondition>,
        Option<&BusinessAccount>,
        Option<&shared::components::HouseAppearance>,
    )>,
    employment: Query<
        (
            &shared::components::ResidentOf,
            Option<&shared::components::EmployedAt>,
            Option<&shared::components::CivicEmployment>,
            Option<&WorkStatus>,
            Option<&HomeAssignment>,
        ),
        With<CharacterKind>,
    >,
    mut residents: Query<
        (
            Entity,
            &VillagerIntent,
            Option<&mut Wallet>,
            Option<&mut Nutrition>,
            Option<&HomeAssignment>,
        ),
        With<CharacterKind>,
    >,
    service_state: Query<(Option<&MootMealRoutine>, Option<&MootQueueTicket>), With<CharacterKind>>,
    mut inventories: ParamSet<(
        Query<&mut GoodsInventory, (With<Settlement>, Without<SettlementBuilding>)>,
        Query<&mut GoodsInventory, With<SettlementBuilding>>,
        Query<&mut GoodsInventory, With<crate::world::shipping::crew::ShipCrew>>,
    )>,
    sea_crew: Query<(), With<crate::world::shipping::crew::ShipCrew>>,
) {
    let Some(day) = world_time.iter().next().map(|time| time.day) else {
        return;
    };

    let mut pantries: HashMap<shared::components::SettlementId, Vec<Entity>> = HashMap::new();
    let mut housing: HashMap<shared::components::SettlementId, u32> = HashMap::new();
    let mut employed: HashMap<shared::components::SettlementId, u32> = HashMap::new();
    for (entity, building, building_of, _, _, _, _, _, appearance) in buildings.iter() {
        if building.kind == SettlementBuildingKind::House {
            pantries.entry(building_of.0).or_default().push(entity);
        }
        *housing.entry(building_of.0).or_default() +=
            u32::from(building.kind.housing_capacity_with_house(appearance));
    }
    let mut filled_by_building = HashMap::<shared::components::BuildingId, u16>::new();
    let mut civic_filled = HashMap::<shared::components::SettlementId, u16>::new();
    let mut job_seekers = HashMap::<shared::components::SettlementId, u16>::new();
    let mut homeless = HashMap::<shared::components::SettlementId, u32>::new();
    for (resident_of, employed_at, civic_job, status, home) in employment.iter() {
        if employed_at.is_some() || civic_job.is_some() {
            *employed.entry(resident_of.0).or_default() += 1;
        }
        if let Some(employed_at) = employed_at {
            let filled = filled_by_building.entry(employed_at.0).or_default();
            *filled = filled.saturating_add(1);
        }
        if civic_job.is_some() {
            let filled = civic_filled.entry(resident_of.0).or_default();
            *filled = filled.saturating_add(1);
        }
        if employed_at.is_none()
            && civic_job.is_none()
            && status.is_some_and(|status| *status == WorkStatus::LookingForWork)
        {
            let seeking = job_seekers.entry(resident_of.0).or_default();
            *seeking = seeking.saturating_add(1);
        }
        if home.is_none() {
            let count = homeless.entry(resident_of.0).or_default();
            *count = count.saturating_add(1);
        }
    }
    let mut private_positions = HashMap::<shared::components::SettlementId, u16>::new();
    let mut private_filled = HashMap::<shared::components::SettlementId, u16>::new();
    let mut private_vacancies = HashMap::<shared::components::SettlementId, u16>::new();
    let mut best_open_wage = HashMap::<shared::components::SettlementId, u64>::new();
    let mut unpaid_private_workers = HashMap::<shared::components::SettlementId, u16>::new();
    for (_, building, building_of, building_id, staffing, wage, condition, account, _) in
        buildings.iter()
    {
        if !is_private_business(building.kind) {
            continue;
        }
        let filled = filled_by_building.get(building_id).copied().unwrap_or(0);
        if account.is_some_and(|account| account.wage_arrears > 0) {
            let unpaid = unpaid_private_workers.entry(building_of.0).or_default();
            *unpaid = unpaid.saturating_add(filled);
        }
        if condition.is_some_and(|condition| !condition.state.accepts_new_workers()) {
            continue;
        }
        let target = staffing.map_or_else(
            || building.kind.positions(),
            |staffing| staffing.target_for(building.kind),
        );
        let vacant = u16::from(target).saturating_sub(filled);
        let positions = private_positions.entry(building_of.0).or_default();
        *positions = positions.saturating_add(u16::from(target));
        let actual = private_filled.entry(building_of.0).or_default();
        *actual = actual.saturating_add(filled);
        let openings = private_vacancies.entry(building_of.0).or_default();
        *openings = openings.saturating_add(vacant);
        if vacant > 0 {
            let offer = wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage);
            let best = best_open_wage.entry(building_of.0).or_default();
            *best = (*best).max(offer);
        }
    }
    for pantry_entities in pantries.values_mut() {
        pantry_entities.sort_unstable_by_key(|entity| entity.to_bits());
    }

    for (
        entity,
        settlement_id,
        mut settlement,
        mut economy,
        mut market,
        policies,
        administration,
        mut civic_account,
    ) in settlements.iter_mut()
    {
        economy.housing_capacity = housing.get(settlement_id).copied().unwrap_or(0);
        economy.homeless_residents = homeless.get(settlement_id).copied().unwrap_or(0);
        let unpaid_civic = administration.map_or(0, |administration| {
            administration
                .payroll
                .iter()
                .filter(|entry| entry.active && entry.arrears > 0)
                .count()
                .min(usize::from(u16::MAX)) as u16
        });
        economy.unpaid_workers = unpaid_private_workers
            .get(settlement_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(unpaid_civic);
        let day_state = runtime
            .by_settlement
            .entry(entity)
            .or_insert_with(|| SettlementEconomyDay::new(day));
        let elapsed_days = day.saturating_sub(day_state.last_world_day);
        let mut advanced_day = false;

        for _ in 0..elapsed_days {
            let demand = settlement.residents;
            let meal_day = day_state.last_world_day.saturating_add(1);
            let consumed = if let Some(market) = market.as_deref_mut() {
                // A ration is now a real purchase. Only goods the Moot bought
                // into its physical hall inventory are market stock; a farmer's
                // private shed is not silently raided at midnight.
                let mut consumed = 0u32;
                let mut unaffordable = Vec::new();
                let mut individual_buyers = Vec::new();
                let mut relieved_residents = HashSet::new();
                // Housed residents eat from their shared physical pantry.
                // Unhoused residents still buy one ration personally at the
                // Moot because they have no household budget or storage.
                for (resident, intent, _, nutrition, home) in residents.iter_mut() {
                    if !intent.counts_as_resident() || intent.settlement() != Some(entity) {
                        // A chosen destination is not completed immigration.
                        // Keep the landing/long journey owner until registration.
                        continue;
                    }
                    // A direct Tavern meal is a real ration for this same meal
                    // boundary. Do not also empty the household pantry or
                    // charge the resident again at midnight.
                    if nutrition
                        .as_deref()
                        .is_some_and(|nutrition| nutrition.last_meal_day == Some(meal_day))
                    {
                        consumed = consumed.saturating_add(1);
                        continue;
                    }
                    if service_state
                        .get(resident)
                        .is_ok_and(|(meal, _)| meal.is_some())
                    {
                        // The already-purchased ration remains reserved until its
                        // owner physically collects it. Never buy or grant it twice.
                        continue;
                    }
                    // A sailor eats the provisions actually taken aboard.
                    // Do not empty a distant household pantry or summon a
                    // shore meal trip in the middle of a voyage.
                    if sea_crew.contains(resident) {
                        let fed = inventories
                            .p2()
                            .get_mut(resident)
                            .is_ok_and(|mut provisions| provisions.remove_edible(1) == 1);
                        if fed {
                            consumed = consumed.saturating_add(1);
                        }
                        if let Some(mut nutrition) = nutrition {
                            if fed {
                                nutrition.record_meal(meal_day);
                            } else {
                                nutrition.record_missed_meal();
                            }
                        }
                        continue;
                    }
                    let fed = home.is_some_and(|home| {
                        inventories
                            .p1()
                            .get_mut(home.home)
                            .is_ok_and(|mut pantry| pantry.remove_edible(1) == 1)
                    });
                    if fed {
                        consumed = consumed.saturating_add(1);
                        if let Some(mut nutrition) = nutrition {
                            nutrition.record_meal(meal_day);
                        }
                    } else if home.is_some() {
                        unaffordable.push(resident);
                    } else {
                        individual_buyers.push(resident);
                    }
                }
                if let Ok(mut hall) = inventories.p0().get_mut(entity) {
                    for resident in individual_buyers.iter().copied() {
                        if service_state
                            .get(resident)
                            .is_ok_and(|(_, ticket)| ticket.is_some())
                        {
                            continue;
                        }
                        let Ok((_, _, wallet, _, _)) = residents.get_mut(resident) else {
                            continue;
                        };
                        let Some(mut wallet) = wallet else {
                            unaffordable.push(resident);
                            continue;
                        };
                        let mut fed = false;
                        let chosen = households::cheapest_ready_food(&hall, market);
                        if chosen.is_none() {
                            let price = Good::Bread.base_price();
                            market.record_unmet_demand(
                                Good::Bread,
                                1,
                                0,
                                u32::from(wallet.balance() >= price),
                                price,
                            );
                        }
                        for good in chosen.into_iter() {
                            let purchase = market.purchase_recording_demand(
                                good,
                                1,
                                wallet.balance(),
                                None,
                                None,
                            );
                            if purchase.trade.units == 1 && wallet.debit(purchase.trade.pennies) {
                                let removed = hall.remove(good, 1);
                                debug_assert_eq!(removed, 1);
                                business_events.record_market_purchase(
                                    meal_day,
                                    *settlement_id,
                                    purchase.fills,
                                );
                                consumed = consumed.saturating_add(1);
                                fed = true;
                                moot_services::reserve_meal(
                                    &mut commands,
                                    &mut queue_clock,
                                    resident,
                                    entity,
                                    MootServiceKind::PersonalMeal,
                                    good,
                                    meal_day,
                                );
                                break;
                            }
                        }
                        if !fed {
                            unaffordable.push(resident);
                        }
                    }

                    // Personal purchases clear first. Relief has its own daily
                    // cash envelope; the ordinary multi-day stock target must
                    // not keep an affordable physical ration from an unfed person.
                    if let Some(policy) =
                        policies.filter(|policy| policy.poor_relief.allows_purchase())
                    {
                        let mut relief_remaining =
                            emergency_relief_budget(&settlement, administration, policy);
                        unaffordable.sort_unstable_by_key(|resident| resident.to_bits());
                        // Rotate scarce meals instead of feeding the same first
                        // entity whenever a town can afford only part of its need.
                        if !unaffordable.is_empty() {
                            let offset = meal_day as usize % unaffordable.len();
                            unaffordable.rotate_left(offset);
                        }
                        for resident in unaffordable.iter().copied() {
                            if service_state
                                .get(resident)
                                .is_ok_and(|(_, ticket)| ticket.is_some())
                            {
                                continue;
                            }
                            if relief_remaining == 0 {
                                break;
                            }
                            let mut relieved = false;
                            for good in households::cheapest_ready_food(&hall, market).into_iter() {
                                let purchase = market.purchase(
                                    good,
                                    1,
                                    relief_remaining.min(settlement.treasury),
                                    None,
                                    None,
                                );
                                if purchase.trade.units == 1
                                    && settlement.treasury >= purchase.trade.pennies
                                {
                                    settlement.treasury -= purchase.trade.pennies;
                                    relief_remaining -= purchase.trade.pennies;
                                    if let Some(account) = civic_account.as_deref_mut() {
                                        account.record_poor_relief_expense(
                                            meal_day,
                                            purchase.trade.pennies,
                                        );
                                    }
                                    let removed = hall.remove(good, 1);
                                    debug_assert_eq!(removed, 1);
                                    business_events.record_market_purchase(
                                        meal_day,
                                        *settlement_id,
                                        purchase.fills,
                                    );
                                    consumed = consumed.saturating_add(1);
                                    relieved = true;
                                    moot_services::reserve_meal(
                                        &mut commands,
                                        &mut queue_clock,
                                        resident,
                                        entity,
                                        MootServiceKind::PoorRelief,
                                        good,
                                        meal_day,
                                    );
                                    break;
                                }
                            }
                            if relieved {
                                relieved_residents.insert(resident);
                            }
                        }
                    }
                } else {
                    unaffordable.extend(individual_buyers.iter().copied());
                }
                for resident in unaffordable {
                    if relieved_residents.contains(&resident) {
                        continue;
                    }
                    if let Ok((_, _, _, Some(mut nutrition), _)) = residents.get_mut(resident) {
                        nutrition.record_missed_meal();
                    }
                }
                consumed.min(demand)
            } else {
                // A missing public market cannot authorize taking private stock.
                // No purchase occurred, so record unmet meals without moving goods.
                for (_, intent, _, nutrition, _) in residents.iter_mut() {
                    if intent.counts_as_resident() && intent.settlement() == Some(entity) {
                        if let Some(mut nutrition) = nutrition {
                            if nutrition.last_meal_day != Some(meal_day) {
                                nutrition.record_missed_meal();
                            }
                        }
                    }
                }
                0
            };
            economy.unmet_food = demand.saturating_sub(consumed);
            let pressures = unrest_pressures(
                demand,
                economy.unmet_food,
                economy.homeless_residents,
                economy.unpaid_workers,
            );
            economy.unrest_hunger_pressure = pressures.hunger;
            economy.unrest_housing_pressure = pressures.housing;
            economy.unrest_wage_pressure = pressures.wages;
            economy.unrest_target = pressures.target();
            let before = economy.unrest;
            economy.unrest = advance_unrest(economy.unrest, economy.unrest_target);
            economy.unrest_change = economy.unrest - before;
            day_state.finish_day(consumed);
            day_state.last_world_day = day_state.last_world_day.saturating_add(1);
            advanced_day = true;
        }

        let mut stock = inventories
            .p0()
            .get_mut(entity)
            .map_or(0, |inventory| inventory.edible_amount());
        for pantry in pantries.get(settlement_id).into_iter().flatten() {
            stock = stock.saturating_add(
                inventories
                    .p1()
                    .get_mut(*pantry)
                    .map_or(0, |inventory| inventory.edible_amount()),
            );
        }
        let residents = settlement.residents;
        economy.edible_stock = stock;
        economy.reserve_days = if residents == 0 {
            0.0
        } else {
            stock as f32 / residents as f32
        };
        economy.recent_food_production = SettlementEconomyDay::recent_average(
            &day_state.production_history,
            day_state.recorded_days,
        );
        economy.recent_food_consumption = SettlementEconomyDay::recent_average(
            &day_state.consumption_history,
            day_state.recorded_days,
        );
        economy.observed_days = economy
            .observed_days
            .saturating_add(elapsed_days.min(u32::from(u16::MAX)) as u16);

        let demand = residents.max(1) as f32;
        economy.reserve_prosperity =
            (economy.reserve_days / FOOD_SECURITY_TARGET_DAYS).clamp(0.0, 1.0) * 40.0;
        economy.production_prosperity =
            (economy.recent_food_production / demand).clamp(0.0, 1.0) * 30.0;
        economy.housing_prosperity =
            (housing.get(settlement_id).copied().unwrap_or(0) as f32 / demand).clamp(0.0, 1.0)
                * 20.0;
        economy.employment_prosperity =
            (employed.get(settlement_id).copied().unwrap_or(0) as f32 / demand).clamp(0.0, 1.0)
                * 10.0;
        economy.hunger_penalty = -(economy.unmet_food as f32 / demand).clamp(0.0, 1.0) * 30.0;
        economy.prosperity = (economy.reserve_prosperity
            + economy.production_prosperity
            + economy.housing_prosperity
            + economy.employment_prosperity
            + economy.hunger_penalty)
            .clamp(0.0, 100.0);
        economy.private_job_positions = private_positions.get(settlement_id).copied().unwrap_or(0);
        economy.private_filled_jobs = private_filled.get(settlement_id).copied().unwrap_or(0);
        economy.private_vacant_jobs = private_vacancies.get(settlement_id).copied().unwrap_or(0);
        economy.civic_job_positions = u16::try_from(super::civic::desired_civic_positions(
            settlement.tier,
            policies.copied().unwrap_or_default().staffing_posture,
            settlement.residents,
        ))
        .unwrap_or(u16::MAX);
        economy.civic_filled_jobs = civic_filled.get(settlement_id).copied().unwrap_or(0);
        economy.civic_vacant_jobs = economy
            .civic_job_positions
            .saturating_sub(economy.civic_filled_jobs);
        economy.job_seekers = job_seekers.get(settlement_id).copied().unwrap_or(0);
        economy.best_open_private_wage = best_open_wage.get(settlement_id).copied().unwrap_or(0);
        let pressures = unrest_pressures(
            residents,
            economy.unmet_food,
            economy.homeless_residents,
            economy.unpaid_workers,
        );
        economy.unrest_hunger_pressure = pressures.hunger;
        economy.unrest_housing_pressure = pressures.housing;
        economy.unrest_wage_pressure = pressures.wages;
        economy.unrest_target = pressures.target();

        if advanced_day {
            let secure = has_secure_food_day(residents, &economy);
            economy.food_secure_days = if secure {
                economy.food_secure_days.saturating_add(1)
            } else {
                0
            };

            // Civic tier changes are physical Hall projects. This system owns
            // the economic evidence only; settlement development purchases,
            // stages and constructs the required materials before promotion.
        }
    }
}

#[cfg(test)]
mod unrest_tests {
    use super::*;

    #[test]
    fn housing_summary_counts_completed_levels_without_counting_worksites() {
        use shared::components::{
            BuildingId, BuildingOf, HouseAppearance, HouseLevel, SettlementId, SettlementTier,
        };
        let mut app = App::new();
        app.init_resource::<SettlementEconomyRuntime>()
            .init_resource::<BusinessEventQueue>()
            .init_resource::<MootQueueClock>()
            .add_systems(Update, update_settlement_economies);
        app.world_mut().spawn(WorldTime::new_default());
        let hall = app
            .world_mut()
            .spawn((
                SettlementId(1),
                Settlement {
                    name: "Eight Beds".into(),
                    tier: SettlementTier::Village,
                    residents: 0,
                    treasury: 0,
                },
                SettlementEconomy::default(),
                GoodsInventory::new(200),
            ))
            .id();
        let mut homes = Vec::new();
        for id in 1..=3 {
            homes.push(
                app.world_mut()
                    .spawn((
                        BuildingId(id),
                        BuildingOf(SettlementId(1)),
                        SettlementBuilding {
                            kind: SettlementBuildingKind::House,
                            settlement: "Eight Beds".into(),
                            owner: None,
                            quality: 1.0,
                            workers: Vec::new(),
                        },
                        GoodsInventory::new(80),
                    ))
                    .id(),
            );
        }
        app.world_mut()
            .entity_mut(homes[0])
            .insert(HouseAppearance::default());
        app.world_mut()
            .entity_mut(homes[1])
            .insert(HouseAppearance {
                level: HouseLevel::UpperStorey,
                ..default()
            });
        app.world_mut().spawn((
            BuildingOf(SettlementId(1)),
            shared::components::ConstructionSite {
                kind: SettlementBuildingKind::House,
                settlement: "Eight Beds".into(),
                raising: true,
                stand: Vec3::ZERO,
                rotation: 0.0,
            },
            HouseAppearance {
                level: HouseLevel::UpperStorey,
                ..default()
            },
        ));
        app.update();
        assert_eq!(
            app.world()
                .get::<SettlementEconomy>(hall)
                .unwrap()
                .housing_capacity,
            16
        );
        app.world_mut()
            .get_mut::<HouseAppearance>(homes[0])
            .unwrap()
            .level = HouseLevel::UpperStorey;
        app.update();
        assert_eq!(
            app.world()
                .get::<SettlementEconomy>(hall)
                .unwrap()
                .housing_capacity,
            20
        );
    }

    #[test]
    fn unrest_uses_only_the_three_visible_hardships() {
        let pressures = unrest_pressures(20, 10, 4, 2);
        assert!((pressures.hunger - 27.5).abs() < f32::EPSILON);
        assert!((pressures.housing - 5.0).abs() < f32::EPSILON);
        assert!((pressures.wages - 2.0).abs() < f32::EPSILON);
        assert!((pressures.target() - 34.5).abs() < f32::EPSILON);
    }

    #[test]
    fn unrest_rises_and_recovers_at_readable_daily_limits() {
        assert_eq!(advance_unrest(0.0, 100.0), 10.0);
        assert_eq!(advance_unrest(70.0, 0.0), 65.0);
        assert_eq!(advance_unrest(32.0, 34.5), 34.5);
    }

    #[test]
    fn an_empty_foundation_is_calm() {
        assert_eq!(unrest_pressures(0, 10, 10, 10), UnrestPressures::default());
    }

    #[test]
    fn food_security_ignores_a_sub_delivery_boundary_rounding_gap() {
        let almost_three_days = SettlementEconomy {
            edible_stock: 57,
            reserve_days: 2.85,
            recent_food_production: 20.0,
            unmet_food: 0,
            ..default()
        };
        assert!(has_secure_food_day(20, &almost_three_days));

        let real_shortage = SettlementEconomy {
            edible_stock: 55,
            reserve_days: 2.75,
            ..almost_three_days
        };
        assert!(!has_secure_food_day(20, &real_shortage));

        let hungry = SettlementEconomy {
            unmet_food: 1,
            ..almost_three_days
        };
        assert!(!has_secure_food_day(20, &hungry));
    }
}
