//! Businesses, civic market work, payroll and employment-state reconciliation.
//!
//! Physical production belongs to `trades`; this module owns the commercial
//! decisions and transfers wrapped around that production.

use super::*;
use shared::economy::BusinessStrategy;

pub(crate) fn automatic_owner_strategy(
    owner: Option<shared::components::PersonId>,
    attributes: Option<&CharacterAttributes>,
) -> shared::economy::BusinessStrategy {
    use shared::economy::BusinessStrategy;
    if attributes.is_some_and(|attributes| attributes.intelligence() >= 75) {
        return BusinessStrategy::Cautious;
    }
    if attributes.is_some_and(|attributes| attributes.charm() >= 75) {
        return BusinessStrategy::HighMargin;
    }
    if attributes.is_some_and(|attributes| attributes.physique() >= 75) {
        return BusinessStrategy::Growth;
    }
    match owner.map_or(0, |owner| owner.0) % 5 {
        0 => BusinessStrategy::Balanced,
        1 => BusinessStrategy::Growth,
        2 => BusinessStrategy::HighMargin,
        3 => BusinessStrategy::Cautious,
        _ => BusinessStrategy::Opportunistic,
    }
}

pub(crate) fn business_output(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        SettlementBuildingKind::Windmill => Some(Good::Flour),
        SettlementBuildingKind::Bakery => Some(Good::Bread),
        _ => None,
    }
}

pub(crate) const fn is_private_business(kind: SettlementBuildingKind) -> bool {
    matches!(
        kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::StorageHall
    )
}

/// Give active public and private porters their cart capacity, then return
/// former porters to personal capacity once any protected in-flight cargo has
/// been unloaded. The marker keeps this O(active porters), not O(all people).
pub fn sync_porter_cargo_capacity(
    mut commands: Commands,
    mut new_porters: Query<
        (Entity, &mut GoodsInventory),
        (
            With<CharacterKind>,
            Or<(With<MarketPorter>, With<CompanyPorter>)>,
            Without<PorterCargoCapacity>,
        ),
    >,
    mut tracked: Query<
        (
            Entity,
            &mut GoodsInventory,
            Has<MarketPorter>,
            Has<CompanyPorter>,
        ),
        (With<CharacterKind>, With<PorterCargoCapacity>),
    >,
) {
    for (entity, mut inventory) in new_porters.iter_mut() {
        inventory.resize_bulk_capacity(shared::economy::capacity::PORTER);
        commands.entity(entity).insert(PorterCargoCapacity);
    }
    for (entity, mut inventory, public_porter, company_porter) in tracked.iter_mut() {
        if public_porter || company_porter {
            inventory.resize_bulk_capacity(shared::economy::capacity::PORTER);
            continue;
        }
        inventory.resize_bulk_capacity(shared::economy::capacity::VILLAGER);
        if inventory.bulk_capacity() == shared::economy::capacity::VILLAGER {
            commands.entity(entity).remove::<PorterCargoCapacity>();
        }
    }
}

fn procurement_for(kind: SettlementBuildingKind) -> BusinessProcurementPolicy {
    match kind {
        SettlementBuildingKind::Windmill => BusinessProcurementPolicy::none().with_rule(
            Good::Wheat,
            BusinessInputRule {
                enabled: true,
                coverage_days: shared::economy::DEFAULT_INPUT_COVERAGE_DAYS,
                reorder_below: 0,
                target_units: 0,
                maximum_unit_price: Good::Wheat.base_price().saturating_mul(175) / 100,
            },
        ),
        SettlementBuildingKind::Bakery => BusinessProcurementPolicy::none().with_rule(
            Good::Flour,
            BusinessInputRule {
                enabled: true,
                coverage_days: shared::economy::DEFAULT_INPUT_COVERAGE_DAYS,
                reorder_below: 0,
                target_units: 0,
                maximum_unit_price: Good::Flour.base_price().saturating_mul(175) / 100,
            },
        ),
        _ => BusinessProcurementPolicy::none(),
    }
}

fn supply_policy_for(kind: SettlementBuildingKind) -> BusinessSupplyPolicy {
    match kind {
        SettlementBuildingKind::Windmill => BusinessSupplyPolicy::none().with_rule(
            Good::Wheat,
            BusinessPrivateInputRule {
                enabled: true,
                sourcing: BusinessSourcingMode::PreferOwned,
                preferred_supplier: None,
            },
        ),
        SettlementBuildingKind::Bakery => BusinessSupplyPolicy::none().with_rule(
            Good::Flour,
            BusinessPrivateInputRule {
                enabled: true,
                sourcing: BusinessSourcingMode::PreferOwned,
                preferred_supplier: None,
            },
        ),
        _ => BusinessSupplyPolicy::none(),
    }
}

fn sale_policy_for(_kind: SettlementBuildingKind, output: Good) -> BusinessSalePolicy {
    BusinessSalePolicy::for_good(output)
}

fn liquidation_price(good: Good, liquidation_days: u16) -> u64 {
    let mut price = good.base_price().max(1);
    for _ in 0..liquidation_days.min(10) {
        let movement = price.saturating_mul(1_500).div_ceil(BASIS_POINTS).max(1);
        price = price.saturating_sub(movement).max(1);
    }
    price.max(good.base_price().saturating_mul(25) / 100).max(1)
}

/// Add the small ledgers used by every productive workplace. Founding working
/// capital is transferred from the owner when possible, never minted.
pub fn ensure_business_economies(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    halls: Query<(&shared::components::SettlementId, &MootMarket), With<Settlement>>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        &shared::components::BuildingOf,
        Option<&shared::components::OwnedBy>,
        Option<&BusinessAccount>,
        Option<&BusinessSalePolicy>,
        Option<&BusinessStaffingPolicy>,
        Option<&BusinessWagePolicy>,
        Option<&BusinessProcurementPolicy>,
        Option<&BusinessSupplyPolicy>,
        Option<&BusinessManagementPolicy>,
        Option<&BusinessCondition>,
        Option<&InheritedBusinessCapital>,
        Option<&BusinessProjectAccounting>,
    )>,
    mut owners: Query<(
        &shared::components::PersonId,
        Option<&CharacterAttributes>,
        &mut Wallet,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let market_snapshots: HashMap<shared::components::SettlementId, OpeningMarketSnapshot> = halls
        .iter()
        .map(|(settlement_id, market)| {
            let mut prices = [0; Good::COUNT];
            let mut scarce = [false; Good::COUNT];
            for good in Good::ALL {
                let pool = market.pool(good);
                prices[good.index()] = market
                    .suggested_price(good)
                    .max(pool.day.high_ask)
                    .max(pool.previous_day.high_ask);
                scarce[good.index()] = pool
                    .day
                    .unmet_units()
                    .saturating_add(pool.previous_day.unmet_units())
                    > 0
                    || market.listed_units(good) < pool.target_stock.max(2) / 2;
            }
            (
                *settlement_id,
                OpeningMarketSnapshot {
                    prices,
                    scarce,
                    market_fee_bps: market.market_fee_bps(),
                },
            )
        })
        .collect();
    for (
        entity,
        building,
        building_of,
        owner_id,
        account,
        policy,
        staffing,
        wage_policy,
        procurement,
        supply,
        management,
        condition,
        inherited_capital,
        project_accounting,
    ) in buildings.iter()
    {
        if !is_private_business(building.kind) {
            continue;
        }
        let output = business_output(building.kind);
        let strategy = owners
            .iter()
            .find(|(person_id, ..)| owner_id.is_some_and(|owner| **person_id == owner.0))
            .map_or_else(
                || automatic_owner_strategy(owner_id.map(|owner| owner.0), None),
                |(person_id, attributes, _)| automatic_owner_strategy(Some(*person_id), attributes),
            );
        let mut entity_commands = commands.entity(entity);
        if account.is_none() {
            let opening_cash = if let Some(inherited_capital) = inherited_capital {
                inherited_capital.0
            } else {
                let mut opening_cash = 0;
                if let Some((_, _, mut wallet)) = owners
                    .iter_mut()
                    .find(|(person_id, _, _)| owner_id.is_some_and(|owner| **person_id == owner.0))
                {
                    let wanted = if processing_recipe(building.kind).is_some() {
                        6 * PENNIES_PER_COIN
                    } else {
                        2 * PENNIES_PER_COIN
                    };
                    // Processors need working capital but never strip the owner of
                    // their final two personal coins. A partially funded firm can
                    // still buy a first small input batch and prove demand.
                    let contribution = wallet
                        .balance()
                        .saturating_sub(2 * PENNIES_PER_COIN)
                        .min(wanted);
                    if contribution > 0 && wallet.debit(contribution) {
                        opening_cash = contribution;
                    }
                }
                opening_cash
            };
            let account = project_accounting.map_or_else(
                || BusinessAccount::with_capital(opening_cash),
                |project| {
                    BusinessAccount::with_project_funding(
                        opening_cash,
                        project.contributed_capital,
                        project.capital_expenditure,
                        day,
                    )
                },
            );
            entity_commands.insert(account);
            if inherited_capital.is_some() {
                entity_commands.remove::<InheritedBusinessCapital>();
            }
            if project_accounting.is_some() {
                entity_commands.remove::<BusinessProjectAccounting>();
            }
        }
        if policy.is_none() {
            let mut sale = output.map_or_else(BusinessSalePolicy::default, |output| {
                let mut sale = sale_policy_for(building.kind, output);
                sale.target_margin_bps = strategy.target_margin_bps();
                sale.max_daily_price_change_bps = strategy.daily_price_step_bps();
                sale.asking_unit_price = owner_opening_asking_price(
                    building.kind,
                    output,
                    sale.minimum_unit_price,
                    strategy,
                    market_snapshots.get(&building_of.0),
                );
                sale
            });
            if output.is_none() {
                sale.collection_enabled = false;
            }
            entity_commands.insert(sale);
        }
        if staffing.is_none() {
            entity_commands.insert(BusinessStaffingPolicy::new(automatic_opening_positions(
                building.kind,
            )));
        }
        if wage_policy.is_none() {
            entity_commands.insert(BusinessWagePolicy::default());
        }
        if procurement.is_none() {
            entity_commands.insert(procurement_for(building.kind));
        }
        if supply.is_none() {
            entity_commands.insert(supply_policy_for(building.kind));
        }
        if management.is_none() {
            entity_commands.insert(BusinessManagementPolicy::for_strategy(strategy));
        }
        if condition.is_none() {
            entity_commands.insert(BusinessCondition::default());
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OpeningMarketSnapshot {
    prices: [u64; Good::COUNT],
    scarce: [bool; Good::COUNT],
    market_fee_bps: u16,
}

fn owner_opening_asking_price(
    kind: SettlementBuildingKind,
    output: Good,
    minimum_price: u64,
    strategy: BusinessStrategy,
    market: Option<&OpeningMarketSnapshot>,
) -> u64 {
    let local_price = market.map_or(output.base_price(), |market| {
        market.prices[output.index()].max(1)
    });
    let scarce = market.is_some_and(|market| market.scarce[output.index()]);
    let positioned_price = local_price
        .saturating_mul(u64::from(strategy.opening_market_position_bps(scarce)))
        .div_ceil(BASIS_POINTS);

    let sustainable = rated_daily_production(kind, 1.0).map_or(minimum_price, |capacity| {
        let input_cost = capacity.input.map_or(0, |(input, units)| {
            let input_price = market.map_or(input.base_price(), |market| {
                market.prices[input.index()].max(1)
            });
            u64::from(units).saturating_mul(input_price)
        });
        let payroll = u64::from(kind.positions()).saturating_mul(FOUNDING_DAILY_WAGE);
        let unit_cost = input_cost
            .saturating_add(payroll)
            .div_ceil(u64::from(capacity.output_units.max(1)));
        shared::economy::sustainable_unit_price(
            unit_cost,
            market.map_or(0, |market| market.market_fee_bps),
            strategy.target_margin_bps(),
        )
    });
    positioned_price.max(sustainable).max(minimum_price).max(1)
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn new_sites_release_surplus_after_real_company_requests() {
        assert_eq!(
            sale_policy_for(SettlementBuildingKind::Windmill, Good::Flour).company_reserve_units,
            0
        );
        assert_eq!(
            sale_policy_for(SettlementBuildingKind::Bakery, Good::Bread).company_reserve_units,
            0
        );
        assert_eq!(
            sale_policy_for(SettlementBuildingKind::Farmstead, Good::Wheat).company_reserve_units,
            0
        );
    }

    #[test]
    fn entrants_choose_different_opening_prices_from_their_owner_strategy() {
        let mut prices = [0; Good::COUNT];
        prices[Good::Wheat.index()] = Good::Wheat.base_price();
        prices[Good::Flour.index()] = 6 * PENNIES_PER_COIN;
        let mut scarce = [false; Good::COUNT];
        scarce[Good::Flour.index()] = true;
        let market = OpeningMarketSnapshot {
            prices,
            scarce,
            market_fee_bps: 500,
        };
        let price = |strategy| {
            owner_opening_asking_price(
                SettlementBuildingKind::Windmill,
                Good::Flour,
                1,
                strategy,
                Some(&market),
            )
        };

        assert_eq!(price(BusinessStrategy::Growth), 540);
        assert_eq!(price(BusinessStrategy::Balanced), 600);
        assert_eq!(price(BusinessStrategy::Cautious), 600);
        assert_eq!(price(BusinessStrategy::HighMargin), 690);
        assert_eq!(price(BusinessStrategy::Opportunistic), 720);
    }
}

/// Fill the Reeve position and retire legacy standalone Market Porters. The
/// combined Moot Steward is staffed by the road domain and owns both hauling
/// and road work.
pub fn staff_moot_hall_roles(
    mut commands: Commands,
    mut halls: Query<(
        Entity,
        &Settlement,
        &mut MootAdministration,
        &shared::components::SettlementId,
        &SettlementPolicies,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        &mut WorkStatus,
        Option<&MarketPorter>,
        Option<&crate::world::village_roads::RoadSteward>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
    )>,
) {
    for (hall, settlement, mut administration, settlement_id, policies) in halls.iter_mut() {
        let mut steward = None;
        let mut reeve = None;
        let mut legacy_porters = Vec::new();
        for (entity, person_id, name, intent, _, _, _, _, _, civic_job) in villagers.iter() {
            if intent.settlement() != Some(hall) || !intent.counts_as_resident() {
                continue;
            }
            match civic_job.filter(|job| job.settlement == *settlement_id) {
                Some(job)
                    if matches!(
                        job.role,
                        shared::components::CivicRole::MootSteward
                            | shared::components::CivicRole::RoadSteward
                    ) =>
                {
                    let candidate = (entity, *person_id, name.0.clone());
                    if steward
                        .as_ref()
                        .is_none_or(|(_, current, _)| candidate.1 < *current)
                    {
                        steward = Some(candidate);
                    }
                }
                Some(job) if job.role == shared::components::CivicRole::Reeve => {
                    let candidate = (entity, *person_id, name.0.clone());
                    if reeve
                        .as_ref()
                        .is_none_or(|(_, current, _)| candidate.1 < *current)
                    {
                        reeve = Some(candidate);
                    }
                }
                Some(job) if job.role == shared::components::CivicRole::MarketPorter => {
                    legacy_porters.push(entity);
                }
                _ => {}
            }
        }

        // An old save may have two people in what is now one job. Release the
        // standalone porter cleanly; the Moot Steward receives the marker.
        for legacy in legacy_porters {
            if Some(legacy) == steward.as_ref().map(|(entity, ..)| *entity) {
                continue;
            }
            if let Ok((_, _, _, _, mut occupation, mut status, _, _, _, _)) =
                villagers.get_mut(legacy)
            {
                occupation.0 = None;
                *status = WorkStatus::LookingForWork;
            }
            commands
                .entity(legacy)
                .remove::<shared::components::CivicEmployment>()
                .remove::<MarketPorter>()
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
        }

        administration.road_steward = steward.as_ref().map(|(_, _, name)| name.clone());
        administration.market_porter = administration.road_steward.clone();
        administration.reeve = reeve.as_ref().map(|(_, _, name)| name.clone());
        if reeve.is_some()
            || usize::from(steward.is_some()) >= settlement.residents.saturating_sub(1) as usize
            || !crate::world::village::civic::can_afford_new_civic_hire(
                settlement,
                &administration,
                policies,
            )
        {
            continue;
        }
        let candidate = villagers
            .iter()
            .filter(
                |(_, _, _, intent, occupation, status, _, _, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                        && **status == WorkStatus::LookingForWork
                },
            )
            .min_by_key(|(_, person_id, ..)| **person_id)
            .map(|(entity, ..)| entity);
        let Some(candidate) = candidate else { continue };
        let Ok((_, _, name, _, mut occupation, mut status, _, _, _, _)) =
            villagers.get_mut(candidate)
        else {
            continue;
        };
        occupation.0 = Some("Reeve".to_string());
        *status = WorkStatus::Employed;
        administration.reeve = Some(name.0.clone());
        commands
            .entity(candidate)
            .insert(shared::components::CivicEmployment {
                settlement: *settlement_id,
                role: shared::components::CivicRole::Reeve,
            });
        info!(
            "Village '{}': {} took the Reeve position",
            settlement.name, name.0
        );
    }
}

/// Keep the compact work-state component aligned with real rosters. `Chilling`
/// is an intentional choice and is never silently converted back into job
/// seeking merely because the occupation title is empty.
pub fn reconcile_work_statuses(
    mut villagers: Query<
        (
            &Occupation,
            Option<&shared::components::EmployedAt>,
            Option<&shared::components::CivicEmployment>,
            &mut WorkStatus,
        ),
        With<CharacterKind>,
    >,
) {
    for (occupation, employed_at, civic_job, mut status) in villagers.iter_mut() {
        let next = if occupation.0.is_some() || employed_at.is_some() || civic_job.is_some() {
            WorkStatus::Employed
        } else if *status == WorkStatus::Chilling {
            WorkStatus::Chilling
        } else {
            WorkStatus::LookingForWork
        };
        if *status != next {
            *status = next;
        }
    }
}

pub(crate) const MUNICIPAL_DELIVERY_PENNIES_PER_BULK: u64 = 1;

pub(crate) fn internal_transfer_unit_value(
    good: Good,
    account: &BusinessAccount,
    management: &BusinessManagementPolicy,
) -> u64 {
    let estimated_cost = account
        .estimated_unit_cost
        .max(good.base_price().saturating_mul(75) / 100)
        .max(1);
    estimated_cost
        .saturating_mul(
            BASIS_POINTS.saturating_add(u64::from(management.strategy.target_margin_bps())),
        )
        .div_ceil(BASIS_POINTS)
        .max(1)
}

#[derive(Clone, Copy)]
struct InternalDeliveryCandidate {
    supplier: Entity,
    supplier_id: shared::components::BuildingId,
    receiver: Entity,
    receiver_id: shared::components::BuildingId,
    company: shared::components::CompanyId,
    good: Good,
    units: u32,
    unit_value: u64,
    supplier_entrance: Vec3,
    shortage: u32,
    preferred: bool,
    distance_millimetres: u32,
}

#[derive(Clone)]
struct InternalLogisticsSite {
    entity: Entity,
    id: shared::components::BuildingId,
    settlement: shared::components::SettlementId,
    company: shared::components::CompanyId,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    inventory: GoodsInventory,
    procurement: BusinessProcurementPolicy,
    supply: BusinessSupplyPolicy,
    management: BusinessManagementPolicy,
    account: BusinessAccount,
    can_operate: bool,
}

/// Dispatch same-company goods directly between productive sites. The Moot
/// Steward is a municipal carrier only: no Hall inventory, public listing or
/// market-side payment is touched by this path.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_internal_deliveries(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    road_requests: Query<&RoadRequest>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &mut Settlement,
            &mut shared::economy::CivicAccount,
            &MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut businesses: Query<
        (
            Entity,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
            &shared::components::OperatedBy,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            &BusinessProcurementPolicy,
            &BusinessSupplyPolicy,
            &BusinessManagementPolicy,
            &mut BusinessAccount,
            Option<&BusinessCondition>,
        ),
        Without<CharacterKind>,
    >,
    mut porters: Query<
        (
            Entity,
            Option<&MarketPorter>,
            Option<&CompanyPorter>,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            Option<&mut InternalDeliveryRoutine>,
            Option<&MarketCollectionRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&MootQueueTicket>,
            Option<&MootMealRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        (
            With<CharacterKind>,
            Without<strategic::StrategicPerson>,
            Or<(With<MarketPorter>, With<CompanyPorter>)>,
        ),
    >,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let active: Vec<_> = porters
        .iter()
        .filter_map(|(_, _, _, _, _, _, _, routine, ..)| routine.cloned())
        .collect();
    let mut reserved_output: HashMap<(Entity, Good), u32> = HashMap::new();
    let mut reserved_input: HashMap<(Entity, Good), u32> = HashMap::new();
    let companies_by_id: HashMap<shared::components::CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let mut reserved_fees: HashMap<shared::components::CompanyId, u64> = HashMap::new();
    for routine in active {
        if routine.phase == InternalDeliveryPhase::GoingToSupplier {
            *reserved_output
                .entry((routine.supplier, routine.good))
                .or_default() += routine.reserved_units;
        }
        *reserved_input
            .entry((routine.receiver, routine.good))
            .or_default() += routine.reserved_units;
        if routine.municipal_fee {
            *reserved_fees.entry(routine.company).or_default() += u64::from(routine.reserved_units)
                .saturating_mul(u64::from(routine.good.bulk_per_unit()))
                .saturating_mul(MUNICIPAL_DELIVERY_PENNIES_PER_BULK);
        }
    }
    let logistics_sites: Vec<InternalLogisticsSite> = businesses
        .iter()
        .map(
            |(
                entity,
                id,
                building_of,
                operated_by,
                building,
                position,
                rotation,
                inventory,
                _sale,
                procurement,
                supply,
                management,
                account,
                condition,
            )| InternalLogisticsSite {
                entity,
                id: *id,
                settlement: building_of.0,
                company: operated_by.0,
                kind: building.kind,
                position: position.0,
                rotation: rotation.0,
                inventory: inventory.clone(),
                procurement: *procurement,
                supply: *supply,
                management: *management,
                account: *account,
                can_operate: condition.is_none_or(|condition| condition.state.can_operate()),
            },
        )
        .collect();
    let mut receivers_by_settlement: HashMap<shared::components::SettlementId, Vec<usize>> =
        HashMap::new();
    let mut suppliers_by_key: HashMap<
        (
            shared::components::SettlementId,
            shared::components::CompanyId,
            Good,
        ),
        Vec<usize>,
    > = HashMap::new();
    for (index, site) in logistics_sites.iter().enumerate() {
        receivers_by_settlement
            .entry(site.settlement)
            .or_default()
            .push(index);
        if let Some(good) = business_output(site.kind) {
            suppliers_by_key
                .entry((site.settlement, site.company, good))
                .or_default()
                .push(index);
        }
        if site.kind == SettlementBuildingKind::StorageHall {
            for good in Good::ALL {
                if site.inventory.amount(good) > 0 {
                    suppliers_by_key
                        .entry((site.settlement, site.company, good))
                        .or_default()
                        .push(index);
                }
            }
        }
    }

    for (
        porter_entity,
        civic_porter,
        company_porter,
        position,
        mut activity,
        mut carrier,
        move_target,
        routine,
        market_trip,
        home,
        road_work,
        shopping,
        queue_ticket,
        meal,
        route_failed,
    ) in porters.iter_mut()
    {
        let Some(porter_hall) = civic_porter
            .map(|porter| porter.settlement)
            .or_else(|| company_porter.map(|porter| porter.settlement))
        else {
            continue;
        };
        let porter_company = company_porter.map(|porter| porter.company);
        if market_trip.is_some()
            || home.is_some()
            || road_work.is_some()
            || shopping.is_some()
            || queue_ticket.is_some()
            || meal.is_some()
        {
            continue;
        }
        let assigned_road_repair = road_requests
            .iter()
            .any(|request| request.builder == porter_entity);

        if let Some(failed) = route_failed {
            match routine.as_deref() {
                Some(active) if active.phase == InternalDeliveryPhase::GoingToSupplier => {
                    warn!(
                        "Company delivery could not reach supplier at {:.1},{:.1}; releasing the reservation",
                        failed.goal.x, failed.goal.z
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<InternalDeliveryRoutine>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                Some(active) => {
                    let target = businesses.get(active.receiver).ok().map(
                        |(_, _, _, _, building, at, rotation, ..)| {
                            building.kind.entrance_position(at.0, rotation.0)
                        },
                    );
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if let Some(target) = target {
                        porter_commands.insert(MoveTarget(target));
                    }
                }
                None => {}
            }
            continue;
        }

        let Some(mut routine) = routine else {
            if assigned_road_repair || carrier.used_bulk() > 0 {
                continue;
            }
            let Ok((settlement_id, _settlement, _civic, market)) = halls.get_mut(porter_hall)
            else {
                continue;
            };
            let settlement_id = *settlement_id;
            let carrier_bulk = carrier.free_bulk();
            let mut candidates = Vec::<InternalDeliveryCandidate>::new();
            for receiver_index in receivers_by_settlement
                .get(&settlement_id)
                .into_iter()
                .flatten()
            {
                let receiver = &logistics_sites[*receiver_index];
                if porter_company.is_some_and(|company| company != receiver.company) {
                    continue;
                }
                if !receiver.supply.automatic || !receiver.can_operate {
                    continue;
                }
                for good in Good::ALL {
                    let public_rule = receiver.procurement.rule(good);
                    let private_rule = receiver.supply.rule(good);
                    if !public_rule.enabled || !private_rule.enabled {
                        continue;
                    }
                    let incoming = reserved_input
                        .get(&(receiver.entity, good))
                        .copied()
                        .unwrap_or_default();
                    let held = receiver.inventory.amount(good).saturating_add(incoming);
                    if held >= public_rule.reorder_below {
                        continue;
                    }
                    let receiver_room = receiver.inventory.free_bulk() / good.bulk_per_unit();
                    let wanted = public_rule
                        .target_units
                        .saturating_sub(held)
                        .min(receiver_room);
                    if wanted == 0 {
                        continue;
                    }
                    for supplier_index in suppliers_by_key
                        .get(&(settlement_id, receiver.company, good))
                        .into_iter()
                        .flatten()
                    {
                        let supplier = &logistics_sites[*supplier_index];
                        if supplier.entity == receiver.entity || !supplier.can_operate {
                            continue;
                        }
                        let committed = reserved_output
                            .get(&(supplier.entity, good))
                            .copied()
                            .unwrap_or_default();
                        let available = supplier.inventory.amount(good).saturating_sub(committed);
                        let units = wanted
                            .min(available)
                            .min(carrier_bulk / good.bulk_per_unit());
                        if units == 0 {
                            continue;
                        }
                        let unit_value = internal_transfer_unit_value(
                            good,
                            &supplier.account,
                            &supplier.management,
                        );
                        let fee_per_bulk = if company_porter.is_some() {
                            0
                        } else {
                            MUNICIPAL_DELIVERY_PENNIES_PER_BULK
                        };
                        let landed = unit_value.saturating_add(
                            u64::from(good.bulk_per_unit()).saturating_mul(fee_per_bulk),
                        );
                        if landed > public_rule.maximum_unit_price
                            || (private_rule.sourcing == BusinessSourcingMode::CheapestAvailable
                                && landed > market.suggested_price(good))
                        {
                            continue;
                        }
                        let fee = u64::from(units)
                            .saturating_mul(u64::from(good.bulk_per_unit()))
                            .saturating_mul(fee_per_bulk);
                        let company_free = companies_by_id
                            .get(&receiver.company)
                            .and_then(|entity| company_accounts.get(*entity).ok())
                            .map_or(0, |company| {
                                company
                                    .cash
                                    .saturating_sub(company.wage_arrears)
                                    .saturating_sub(company.tax_arrears)
                            });
                        if company_free.saturating_sub(
                            reserved_fees
                                .get(&receiver.company)
                                .copied()
                                .unwrap_or_default(),
                        ) < fee
                        {
                            continue;
                        }
                        let supplier_entrance = supplier
                            .kind
                            .entrance_position(supplier.position, supplier.rotation);
                        let preferred = private_rule.preferred_supplier == Some(supplier.id);
                        let distance_millimetres =
                            (ground_distance(receiver.position, supplier.position) * 1_000.0)
                                .max(0.0)
                                .min(u32::MAX as f32) as u32;
                        candidates.push(InternalDeliveryCandidate {
                            supplier: supplier.entity,
                            supplier_id: supplier.id,
                            receiver: receiver.entity,
                            receiver_id: receiver.id,
                            company: receiver.company,
                            good,
                            units,
                            unit_value,
                            supplier_entrance,
                            shortage: wanted,
                            preferred,
                            distance_millimetres,
                        });
                    }
                }
            }
            // A Storage Hall is a physical overflow buffer, not a larger
            // number painted onto every workshop. When an owned production
            // site is nearly full, a local porter can move its output into an
            // owned depot. Consumer shortages above remain more urgent.
            for warehouse in logistics_sites.iter().filter(|site| {
                site.settlement == settlement_id
                    && site.kind == SettlementBuildingKind::StorageHall
                    && site.can_operate
                    && porter_company.is_none_or(|company| company == site.company)
            }) {
                for supplier in logistics_sites.iter().filter(|site| {
                    site.settlement == settlement_id
                        && site.company == warehouse.company
                        && site.entity != warehouse.entity
                        && site.can_operate
                        && business_output(site.kind).is_some()
                        && site.inventory.used_bulk().saturating_mul(100)
                            >= site.inventory.bulk_capacity().saturating_mul(80)
                }) {
                    let Some(good) = business_output(supplier.kind) else {
                        continue;
                    };
                    let committed = reserved_output
                        .get(&(supplier.entity, good))
                        .copied()
                        .unwrap_or_default();
                    let local_floor =
                        (supplier.inventory.bulk_capacity() / good.bulk_per_unit().max(1)) / 2;
                    let available = supplier
                        .inventory
                        .amount(good)
                        .saturating_sub(committed)
                        .saturating_sub(local_floor);
                    let room = warehouse.inventory.free_bulk().saturating_sub(
                        reserved_input
                            .iter()
                            .filter(|((entity, _), _)| *entity == warehouse.entity)
                            .map(|((_, reserved_good), units)| {
                                units.saturating_mul(reserved_good.bulk_per_unit())
                            })
                            .fold(0u32, u32::saturating_add),
                    ) / good.bulk_per_unit();
                    let units = available
                        .min(room)
                        .min(carrier_bulk / good.bulk_per_unit())
                        .min(16);
                    if units == 0 {
                        continue;
                    }
                    candidates.push(InternalDeliveryCandidate {
                        supplier: supplier.entity,
                        supplier_id: supplier.id,
                        receiver: warehouse.entity,
                        receiver_id: warehouse.id,
                        company: warehouse.company,
                        good,
                        units,
                        unit_value: internal_transfer_unit_value(
                            good,
                            &supplier.account,
                            &supplier.management,
                        ),
                        supplier_entrance: supplier
                            .kind
                            .entrance_position(supplier.position, supplier.rotation),
                        shortage: 0,
                        preferred: false,
                        distance_millimetres: (ground_distance(
                            warehouse.position,
                            supplier.position,
                        ) * 1_000.0)
                            .max(0.0)
                            .min(u32::MAX as f32)
                            as u32,
                    });
                }
            }
            candidates.sort_unstable_by_key(|candidate| {
                (
                    std::cmp::Reverse(candidate.preferred),
                    std::cmp::Reverse(candidate.shortage),
                    candidate.distance_millimetres,
                    candidate.receiver_id,
                    candidate.supplier_id,
                )
            });
            let Some(candidate) = candidates.into_iter().next() else {
                continue;
            };
            *activity = CharacterActivity::Idle;
            commands.entity(porter_entity).insert((
                InternalDeliveryRoutine {
                    supplier: candidate.supplier,
                    supplier_id: candidate.supplier_id,
                    receiver: candidate.receiver,
                    receiver_id: candidate.receiver_id,
                    company: candidate.company,
                    hall: porter_hall,
                    good: candidate.good,
                    reserved_units: candidate.units,
                    unit_value: candidate.unit_value,
                    municipal_fee: company_porter.is_none(),
                    phase: InternalDeliveryPhase::GoingToSupplier,
                },
                MoveTarget(candidate.supplier_entrance),
            ));
            *reserved_output
                .entry((candidate.supplier, candidate.good))
                .or_default() += candidate.units;
            *reserved_input
                .entry((candidate.receiver, candidate.good))
                .or_default() += candidate.units;
            if company_porter.is_none() {
                *reserved_fees.entry(candidate.company).or_default() += u64::from(candidate.units)
                    .saturating_mul(u64::from(candidate.good.bulk_per_unit()))
                    .saturating_mul(MUNICIPAL_DELIVERY_PENNIES_PER_BULK);
            }
            continue;
        };

        if routine.hall != porter_hall
            || porter_company.is_some_and(|company| company != routine.company)
        {
            commands
                .entity(porter_entity)
                .remove::<InternalDeliveryRoutine>()
                .remove::<MoveTarget>();
            continue;
        }
        match routine.phase {
            InternalDeliveryPhase::GoingToSupplier => {
                let Ok((_, _, _, supplier_company, building, at, rotation, mut store, ..)) =
                    businesses.get_mut(routine.supplier)
                else {
                    commands
                        .entity(porter_entity)
                        .remove::<InternalDeliveryRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                };
                if supplier_company.0 != routine.company {
                    commands
                        .entity(porter_entity)
                        .remove::<InternalDeliveryRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let moved = store.transfer_to(&mut carrier, routine.good, routine.reserved_units);
                routine.reserved_units = moved;
                if moved == 0 {
                    commands
                        .entity(porter_entity)
                        .remove::<InternalDeliveryRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                let Ok((
                    _,
                    _,
                    _,
                    receiver_company,
                    receiver_building,
                    receiver_at,
                    receiver_rotation,
                    ..,
                )) = businesses.get(routine.receiver)
                else {
                    continue;
                };
                if receiver_company.0 != routine.company {
                    continue;
                }
                let entrance = receiver_building
                    .kind
                    .entrance_position(receiver_at.0, receiver_rotation.0);
                routine.phase = InternalDeliveryPhase::Delivering;
                *activity = CharacterActivity::Idle;
                commands.entity(porter_entity).insert(MoveTarget(entrance));
            }
            InternalDeliveryPhase::Delivering => {
                let Ok((
                    _,
                    _,
                    _,
                    receiver_company,
                    receiver_building,
                    receiver_at,
                    receiver_rotation,
                    ..,
                )) = businesses.get(routine.receiver)
                else {
                    continue;
                };
                if receiver_company.0 != routine.company {
                    continue;
                }
                let entrance = receiver_building
                    .kind
                    .entrance_position(receiver_at.0, receiver_rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let Ok([supplier, receiver]) =
                    businesses.get_many_mut([routine.supplier, routine.receiver])
                else {
                    continue;
                };
                let (_, _, _, _, _, _, _, _, _, _, _, _, mut supplier_account, _) = supplier;
                let (_, _, _, _, _, _, _, mut receiver_store, _, _, _, _, mut receiver_account, _) =
                    receiver;
                let deliverable = routine
                    .reserved_units
                    .min(carrier.amount(routine.good))
                    .min(receiver_store.free_bulk() / routine.good.bulk_per_unit());
                if deliverable == 0 {
                    continue;
                }
                let fee = if routine.municipal_fee {
                    u64::from(deliverable)
                        .saturating_mul(u64::from(routine.good.bulk_per_unit()))
                        .saturating_mul(MUNICIPAL_DELIVERY_PENNIES_PER_BULK)
                } else {
                    0
                };
                let Some(company_entity) = companies_by_id.get(&routine.company).copied() else {
                    continue;
                };
                let Ok(mut company_account) = company_accounts.get_mut(company_entity) else {
                    continue;
                };
                let protected = company_account
                    .wage_arrears
                    .saturating_add(company_account.tax_arrears);
                if company_account.cash.saturating_sub(protected) < fee
                    || !company_account.debit(fee)
                {
                    continue;
                }
                if fee > 0 {
                    receiver_account.record_delivery_fee(day, fee);
                }
                let delivered = carrier.transfer_to(&mut receiver_store, routine.good, deliverable);
                let value = routine.unit_value.saturating_mul(u64::from(delivered));
                supplier_account.record_internal_output(day, value, delivered);
                receiver_account.record_internal_input(day, value, delivered);
                if fee > 0 {
                    if let Ok((_id, mut settlement, mut civic, _)) = halls.get_mut(routine.hall) {
                        settlement.treasury = settlement.treasury.saturating_add(fee);
                        civic.record_delivery_fee_income(day, fee);
                    }
                }
                routine.reserved_units = routine.reserved_units.saturating_sub(delivered);
                if routine.reserved_units == 0 {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(porter_entity)
                        .remove::<InternalDeliveryRoutine>()
                        .remove::<MoveTarget>();
                }
            }
        }
    }
}

/// Collect saleable stock with the Moot porter. Goods retain their business
/// owner at the hall and no payment occurs until a real buyer purchases them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_market_collections(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    road_requests: Query<&RoadRequest>,
    mut business_events: ResMut<BusinessEventQueue>,
    private_supply: Query<(&BusinessSupplyPolicy, &shared::components::OperatedBy)>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut businesses: Query<
        (
            Entity,
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            &PlayerRotation,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            Option<&BusinessCondition>,
            &shared::components::BuildingId,
            Option<&BusinessProcurementPolicy>,
            &mut BusinessAccount,
            &BusinessWagePolicy,
            &shared::components::OperatedBy,
        ),
        Without<CharacterKind>,
    >,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    company_branch_policies: Query<(
        &shared::components::CompanyId,
        &shared::economy::CompanyBranchPolicies,
    )>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut porters: Query<
        (
            Entity,
            Option<&MarketPorter>,
            Option<&CompanyPorter>,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            Option<&mut MarketCollectionRoutine>,
            Option<&InternalDeliveryRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&MootQueueTicket>,
            Option<&MootMealRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        (
            With<CharacterKind>,
            Without<strategic::StrategicPerson>,
            Or<(With<MarketPorter>, With<CompanyPorter>)>,
        ),
    >,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let companies_by_id: HashMap<shared::components::CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    // Newly attached routines are deferred until the system ends, while the
    // underlying inventories mutate immediately. Keep an explicit reservation
    // ledger so two Moot Stewards cannot promise the same goods or place the
    // same processor order on one tick.
    let active_collections: Vec<_> = porters
        .iter()
        .filter_map(|(_, _, _, _, _, _, _, routine, ..)| routine.cloned())
        .collect();
    let active_internal: Vec<_> = porters
        .iter()
        .filter_map(|(_, _, _, _, _, _, _, _, routine, ..)| routine.cloned())
        .collect();
    let mut reserved_output: HashMap<(Entity, Good), u32> = HashMap::new();
    let mut reserved_input: HashMap<(Entity, Good), u32> = HashMap::new();
    let mut reserved_hall_bulk: HashMap<Entity, u32> = HashMap::new();
    let mut reserved_market_units: HashMap<(Entity, Good), u32> = HashMap::new();
    for routine in active_collections {
        match routine.phase {
            MarketCollectionPhase::GoingToBusiness => {
                *reserved_output
                    .entry((routine.business, routine.good))
                    .or_default() += routine.reserved_units;
                *reserved_hall_bulk.entry(routine.hall).or_default() += routine
                    .reserved_units
                    .saturating_mul(routine.good.bulk_per_unit());
                *reserved_market_units
                    .entry((routine.hall, routine.good))
                    .or_default() += routine.reserved_units;
            }
            MarketCollectionPhase::ReturningToHall => {
                *reserved_hall_bulk.entry(routine.hall).or_default() += routine
                    .reserved_units
                    .saturating_mul(routine.good.bulk_per_unit());
                *reserved_market_units
                    .entry((routine.hall, routine.good))
                    .or_default() += routine.reserved_units;
            }
            MarketCollectionPhase::DeliveringInput => {
                *reserved_input
                    .entry((routine.business, routine.good))
                    .or_default() += routine.reserved_units;
            }
        }
    }
    for routine in active_internal {
        if routine.phase == InternalDeliveryPhase::GoingToSupplier {
            *reserved_output
                .entry((routine.supplier, routine.good))
                .or_default() += routine.reserved_units;
        }
        *reserved_input
            .entry((routine.receiver, routine.good))
            .or_default() += routine.reserved_units;
    }
    let branch_policies: HashMap<
        (
            shared::components::CompanyId,
            shared::components::SettlementId,
            Good,
        ),
        shared::economy::CompanyResourcePolicy,
    > = company_branch_policies
        .iter()
        .flat_map(|(company, policies)| {
            policies.branches().iter().flat_map(move |branch| {
                Good::ALL
                    .into_iter()
                    .map(move |good| ((*company, branch.settlement, good), branch.resource(good)))
            })
        })
        .collect();
    let mut branch_totals = HashMap::<
        (
            shared::components::CompanyId,
            shared::components::SettlementId,
            Good,
        ),
        u32,
    >::new();
    let mut site_branches = HashMap::<
        Entity,
        (
            shared::components::CompanyId,
            shared::components::SettlementId,
        ),
    >::new();
    for (entity, _, building_of, _, _, inventory, _, _, _, _, _, _, operated_by) in
        businesses.iter()
    {
        site_branches.insert(entity, (operated_by.0, building_of.0));
        for good in Good::ALL {
            *branch_totals
                .entry((operated_by.0, building_of.0, good))
                .or_default() += inventory.amount(good);
        }
    }
    let mut branch_reserved = HashMap::<
        (
            shared::components::CompanyId,
            shared::components::SettlementId,
            Good,
        ),
        u32,
    >::new();
    for ((site, good), units) in &reserved_output {
        if let Some((company, settlement)) = site_branches.get(site) {
            *branch_reserved
                .entry((*company, *settlement, *good))
                .or_default() += *units;
        }
    }
    let mut branch_public_remaining: HashMap<_, _> = branch_totals
        .into_iter()
        .map(|(key, total)| {
            let policy = branch_policies.get(&key).copied().unwrap_or_default();
            let reserved = branch_reserved.get(&key).copied().unwrap_or_default();
            (key, policy.public_surplus(total, reserved))
        })
        .collect();
    let mut branch_asks = HashMap::<
        (
            shared::components::CompanyId,
            shared::components::SettlementId,
            Good,
        ),
        u64,
    >::new();
    for (_, building, building_of, _, _, _, sale, _, _, _, _, _, operated_by) in businesses.iter() {
        let Some(good) = business_output(building.kind) else {
            continue;
        };
        let ask = sale.asking_unit_price.max(sale.minimum_unit_price).max(1);
        branch_asks
            .entry((operated_by.0, building_of.0, good))
            .and_modify(|current| *current = (*current).min(ask))
            .or_insert(ask);
    }
    for (
        porter_entity,
        civic_porter,
        company_porter,
        position,
        mut activity,
        mut carrier,
        move_target,
        routine,
        internal_delivery,
        home,
        road_work,
        shopping,
        queue_ticket,
        meal,
        route_failed,
    ) in porters.iter_mut()
    {
        let Some(porter_hall) = civic_porter
            .map(|porter| porter.settlement)
            .or_else(|| company_porter.map(|porter| porter.settlement))
        else {
            continue;
        };
        let porter_company = company_porter.map(|porter| porter.company);
        let assigned_road_repair = road_requests
            .iter()
            .any(|request| request.builder == porter_entity);
        if internal_delivery.is_some()
            || home.is_some()
            || road_work.is_some()
            || shopping.is_some()
            || queue_ticket.is_some()
            || meal.is_some()
        {
            continue;
        }
        let Ok((settlement_id, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(porter_hall)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );

        if let Some(failed) = route_failed {
            match routine.as_deref() {
                Some(active) if active.phase == MarketCollectionPhase::GoingToBusiness => {
                    warn!(
                        "Market porter could not reach business at {:.1},{:.1}; cancelling that collection so another offer can be tried",
                        failed.goal.x, failed.goal.z
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                Some(active) if active.phase == MarketCollectionPhase::ReturningToHall => {
                    // Once stock is physically aboard it must reach the hall.
                    // Clear the static failure and explicitly dirty the target
                    // so the bounded planner gets a fresh request next tick.
                    warn!(
                        "Market porter retrying a loaded return to the Moot Hall after route failure at {:.1},{:.1}",
                        failed.goal.x, failed.goal.z
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert(MoveTarget(hall_entrance));
                }
                Some(active) => {
                    debug_assert_eq!(active.phase, MarketCollectionPhase::DeliveringInput);
                    let target = businesses.get(active.business).ok().map(
                        |(_, building, _, at, rotation, ..)| {
                            building.kind.entrance_position(at.0, rotation.0)
                        },
                    );
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if let Some(target) = target {
                        porter_commands.insert(MoveTarget(target));
                    }
                }
                None => {
                    // A stale failure without a live transaction must never
                    // prevent this unique civic worker from accepting work.
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if carrier.used_bulk() > 0 {
                        porter_commands.insert(MoveTarget(hall_entrance));
                    } else {
                        porter_commands.remove::<MoveTarget>();
                    }
                }
            }
            continue;
        }

        let Some(mut routine) = routine else {
            if carrier.used_bulk() > 0 {
                if let Some(private_porter) = company_porter {
                    let storage = businesses.iter().find_map(
                        |(
                            entity,
                            building,
                            building_of,
                            at,
                            rotation,
                            _,
                            _,
                            _,
                            id,
                            _,
                            _,
                            _,
                            operated_by,
                        )| {
                            (building.kind == SettlementBuildingKind::StorageHall
                                && *id == private_porter.storage_hall
                                && building_of.0 == private_porter.settlement_id
                                && operated_by.0 == private_porter.company)
                                .then_some((
                                    entity,
                                    building.kind.entrance_position(at.0, rotation.0),
                                ))
                        },
                    );
                    let Some((storage, entrance)) = storage else {
                        continue;
                    };
                    if ground_distance(position.0, entrance) > WORK_REACH {
                        ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                        continue;
                    }
                    if let Ok((_, _, _, _, _, mut store, ..)) = businesses.get_mut(storage) {
                        for good in Good::ALL {
                            let amount = carrier.amount(good);
                            carrier.transfer_to(&mut store, good, amount);
                        }
                    }
                    if carrier.is_empty() {
                        *activity = CharacterActivity::Indoors;
                        commands.entity(porter_entity).remove::<MoveTarget>();
                    }
                    continue;
                }
                // Recover an interrupted/legacy load. If its durable routine
                // vanished we no longer know a private claimant, so return it
                // as Treasury stock rather than trapping the unique porter in
                // an endless loaded-at-the-hall loop.
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                for good in Good::ALL {
                    let amount = carrier.amount(good);
                    let moved = carrier.transfer_to(&mut hall_store, good, amount);
                    if moved > 0 {
                        let price = market.suggested_price(good);
                        market.consign(
                            shared::economy::MarketSeller::Treasury(*settlement_id),
                            good,
                            moved,
                            price,
                        );
                    }
                }
                *activity = CharacterActivity::Indoors;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            }

            // The Moot Steward also owns road repair in a Hamlet. Let an
            // already-loaded market trip finish without losing private
            // goods, but do not immediately adopt another collection while a
            // completed building is explicitly waiting for this same person.
            // Otherwise a busy large-town market can starve its only road
            // worker forever after a connector resurvey.
            if assigned_road_repair {
                *activity = CharacterActivity::Idle;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            }

            // Input-consuming businesses use the same physical market and
            // porter as households. Protect wage liabilities plus the next
            // payroll day, then buy only when a rule's reorder point is crossed.
            let mut input_orders: Vec<(
                u32,
                Entity,
                shared::components::BuildingId,
                Good,
                u32,
                u64,
                u64,
                Vec3,
            )> = Vec::new();
            for (
                entity,
                building,
                building_of,
                at,
                rotation,
                inventory,
                _,
                condition,
                building_id,
                procurement,
                account,
                wage,
                operated_by,
            ) in businesses.iter()
            {
                if building_of.0 != *settlement_id
                    || porter_company.is_some_and(|company| company != operated_by.0)
                    || condition.is_some_and(|condition| !condition.state.can_operate())
                {
                    continue;
                }
                let Some(procurement) = procurement.filter(|policy| policy.needs_anything()) else {
                    continue;
                };
                // A brand-new processor must be allowed to buy its first
                // inputs before it has revenue. Once it has traded, protect
                // the next full staffed payroll day; two protected days can
                // lock a young processor below the cash needed to earn more.
                let payroll_reserve = if account.gross_revenue == 0
                    && account.current_day.produced_units == 0
                    && account.previous_day.produced_units == 0
                {
                    0
                } else {
                    wage.daily_wage
                        .saturating_mul(u64::from(building.kind.positions()))
                };
                let budget = companies_by_id
                    .get(&operated_by.0)
                    .and_then(|entity| company_accounts.get(*entity).ok())
                    .map_or(0, |company| {
                        company
                            .cash
                            .saturating_sub(company.wage_arrears)
                            .saturating_sub(company.tax_arrears)
                            .saturating_sub(payroll_reserve)
                    });
                if budget == 0 {
                    continue;
                }
                for good in Good::ALL {
                    let rule = procurement.rule(good);
                    let in_transit = reserved_input
                        .get(&(entity, good))
                        .copied()
                        .unwrap_or_default();
                    let held = inventory.amount(good).saturating_add(in_transit);
                    if !rule.enabled || held >= rule.reorder_below {
                        continue;
                    }
                    if let Ok((supply, _company)) = private_supply.get(entity) {
                        let private_rule = supply.rule(good);
                        if private_rule.enabled
                            && (private_rule.sourcing == BusinessSourcingMode::OwnedOnly
                                || in_transit > 0)
                        {
                            continue;
                        }
                    }
                    let carrier_room = carrier.free_bulk() / good.bulk_per_unit();
                    let store_room = inventory
                        .free_bulk()
                        .saturating_sub(in_transit.saturating_mul(good.bulk_per_unit()))
                        / good.bulk_per_unit();
                    let available_units = rule
                        .target_units
                        .saturating_sub(held)
                        .min(carrier_room)
                        .min(store_room)
                        .min(hall_store.amount(good));
                    let affordable = market.preview_purchase(
                        good,
                        available_units,
                        budget,
                        Some(rule.maximum_unit_price),
                        Some(shared::economy::MarketSeller::Business(*building_id)),
                    );
                    let units = viable_processing_input_purchase(
                        building.kind,
                        good,
                        held,
                        affordable.units,
                    );
                    if units == 0 {
                        continue;
                    }
                    input_orders.push((
                        rule.reorder_below.saturating_sub(held),
                        entity,
                        *building_id,
                        good,
                        units,
                        rule.maximum_unit_price,
                        budget,
                        building.kind.entrance_position(at.0, rotation.0),
                    ));
                }
            }
            input_orders.sort_unstable_by_key(|(shortage, entity, ..)| {
                (std::cmp::Reverse(*shortage), entity.to_bits())
            });
            if let Some((_, buyer_entity, buyer_id, good, wanted, max_price, budget, entrance)) =
                input_orders.into_iter().next()
            {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                let preview = market.preview_purchase(
                    good,
                    wanted,
                    budget,
                    Some(max_price),
                    Some(shared::economy::MarketSeller::Business(buyer_id)),
                );
                if preview.units > 0 {
                    let Ok((_, _, _, _, _, _, _, _, _, _, mut buyer, _, operated_by)) =
                        businesses.get_mut(buyer_entity)
                    else {
                        continue;
                    };
                    let Some(company_entity) = companies_by_id.get(&operated_by.0).copied() else {
                        continue;
                    };
                    let Ok(mut company) = company_accounts.get_mut(company_entity) else {
                        continue;
                    };
                    let protected = company.wage_arrears.saturating_add(company.tax_arrears);
                    if company.cash.saturating_sub(protected) < preview.pennies
                        || !company.debit(preview.pennies)
                    {
                        continue;
                    }
                    buyer.record_input_purchase(day, preview.pennies, preview.units);
                    let purchase = market.purchase(
                        good,
                        preview.units,
                        preview.pennies,
                        Some(max_price),
                        Some(shared::economy::MarketSeller::Business(buyer_id)),
                    );
                    debug_assert_eq!(purchase.trade, preview);
                    let moved = hall_store.transfer_to(&mut carrier, good, purchase.trade.units);
                    debug_assert_eq!(moved, purchase.trade.units);
                    business_events.record_market_purchase(day, *settlement_id, purchase.fills);
                    *activity = CharacterActivity::Idle;
                    commands.entity(porter_entity).insert((
                        MarketCollectionRoutine {
                            business: buyer_entity,
                            seller: buyer_id,
                            hall: porter_hall,
                            good,
                            reserved_units: moved,
                            unit_price: 0,
                            phase: MarketCollectionPhase::DeliveringInput,
                        },
                        MoveTarget(entrance),
                    ));
                    *reserved_input.entry((buyer_entity, good)).or_default() += moved;
                    continue;
                }
            }

            let mut offers: Vec<(
                Entity,
                shared::components::BuildingId,
                Good,
                u32,
                u64,
                Vec3,
                (
                    shared::components::CompanyId,
                    shared::components::SettlementId,
                    Good,
                ),
            )> = Vec::new();
            for (
                entity,
                building,
                building_of,
                at,
                rotation,
                inventory,
                policy,
                condition,
                building_id,
                _,
                _,
                _,
                operated_by,
            ) in businesses.iter()
            {
                if building_of.0 != *settlement_id
                    || porter_company.is_some_and(|company| company != operated_by.0)
                    || (!policy.collection_enabled
                        && building.kind != SettlementBuildingKind::StorageHall)
                {
                    continue;
                }
                let state = condition.map_or(BusinessState::Operating, |condition| condition.state);
                if !state.can_operate() && state != BusinessState::Liquidating {
                    continue;
                }
                let liquidating = state == BusinessState::Liquidating;
                for good in Good::ALL {
                    if !liquidating
                        && building.kind != SettlementBuildingKind::StorageHall
                        && business_output(building.kind) != Some(good)
                    {
                        continue;
                    }
                    let already_reserved = reserved_output
                        .get(&(entity, good))
                        .copied()
                        .unwrap_or_default();
                    let branch_key = (operated_by.0, building_of.0, good);
                    let branch_available = if liquidating {
                        u32::MAX
                    } else {
                        branch_public_remaining
                            .get(&branch_key)
                            .copied()
                            .unwrap_or_default()
                    };
                    let surplus = inventory
                        .amount(good)
                        .saturating_sub(already_reserved)
                        .min(branch_available);
                    let carrier_room = carrier.free_bulk() / good.bulk_per_unit();
                    let hall_room = hall_store.free_bulk().saturating_sub(
                        reserved_hall_bulk
                            .get(&porter_hall)
                            .copied()
                            .unwrap_or_default(),
                    ) / good.bulk_per_unit();
                    let shelf_room = market.collection_room(good).saturating_sub(
                        reserved_market_units
                            .get(&(porter_hall, good))
                            .copied()
                            .unwrap_or_default(),
                    );
                    let units = surplus
                        .min(if liquidating {
                            policy.max_units_per_collection.max(32)
                        } else {
                            policy.max_units_per_collection
                        })
                        .min(carrier_room)
                        .min(hall_room)
                        .min(shelf_room);
                    if units == 0 {
                        continue;
                    }
                    let unit_price = if liquidating {
                        liquidation_price(
                            good,
                            condition.map_or(0, |condition| condition.liquidation_days),
                        )
                    } else {
                        if building.kind == SettlementBuildingKind::StorageHall {
                            branch_asks
                                .get(&branch_key)
                                .copied()
                                .unwrap_or_else(|| good.base_price())
                        } else {
                            policy
                                .asking_unit_price
                                .max(policy.minimum_unit_price)
                                .max(1)
                        }
                    };
                    offers.push((
                        entity,
                        *building_id,
                        good,
                        units,
                        unit_price,
                        building.kind.entrance_position(at.0, rotation.0),
                        branch_key,
                    ));
                }
            }
            offers
                .sort_unstable_by_key(|(entity, _, _, _, price, _, _)| (*price, entity.to_bits()));
            let Some((business, seller, good, offered, unit_price, entrance, branch_key)) =
                offers.into_iter().next()
            else {
                *activity = CharacterActivity::Indoors;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            };
            *activity = CharacterActivity::Idle;
            commands.entity(porter_entity).insert((
                MarketCollectionRoutine {
                    business,
                    seller,
                    hall: porter_hall,
                    good,
                    reserved_units: offered,
                    unit_price,
                    phase: MarketCollectionPhase::GoingToBusiness,
                },
                MoveTarget(entrance),
            ));
            *reserved_output.entry((business, good)).or_default() += offered;
            *reserved_market_units
                .entry((porter_hall, good))
                .or_default() += offered;
            if let Some(remaining) = branch_public_remaining.get_mut(&branch_key) {
                *remaining = remaining.saturating_sub(offered);
            }
            *reserved_hall_bulk.entry(porter_hall).or_default() +=
                offered.saturating_mul(good.bulk_per_unit());
            continue;
        };

        if routine.hall != porter_hall {
            commands
                .entity(porter_entity)
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
            continue;
        }
        match routine.phase {
            MarketCollectionPhase::GoingToBusiness => {
                let Ok((_, building, _, at, rotation, mut store, _, _, _, _, _, _, _)) =
                    businesses.get_mut(routine.business)
                else {
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                };
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let moved = store.transfer_to(&mut carrier, routine.good, routine.reserved_units);
                routine.reserved_units = moved;
                if routine.reserved_units == 0 {
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                *activity = CharacterActivity::Idle;
                commands
                    .entity(porter_entity)
                    .insert(MoveTarget(hall_entrance));
                routine.phase = MarketCollectionPhase::ReturningToHall;
            }
            MarketCollectionPhase::ReturningToHall => {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                let delivered =
                    carrier.transfer_to(&mut hall_store, routine.good, routine.reserved_units);
                if delivered > 0 {
                    market.consign(
                        shared::economy::MarketSeller::Business(routine.seller),
                        routine.good,
                        delivered,
                        routine.unit_price,
                    );
                }
                *activity = CharacterActivity::Indoors;
                commands
                    .entity(porter_entity)
                    .remove::<MarketCollectionRoutine>()
                    .remove::<MoveTarget>();
            }
            MarketCollectionPhase::DeliveringInput => {
                let Ok((_, building, _, at, rotation, mut store, _, _, _, _, _, _, _)) =
                    businesses.get_mut(routine.business)
                else {
                    // The purchased input remains physically aboard. Drop the
                    // dead destination so the generic recovery branch returns
                    // it to the hall on the next tick.
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .insert(MoveTarget(hall_entrance));
                    continue;
                };
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let delivered =
                    carrier.transfer_to(&mut store, routine.good, routine.reserved_units);
                if delivered == routine.reserved_units {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                }
            }
        }
    }
}

/// Pay daily wages from the company treasury and let a wealthy owner leave hands-on
/// work when a replacement is ready. Profit withdrawals are reviewed by the
/// management system after payroll. This is daily O(people + workplaces), not
/// per-frame decision search.
pub fn run_business_payroll_and_owner_leisure(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    settlements: Query<(Entity, &shared::components::SettlementId)>,
    mut businesses: Query<(
        Entity,
        &SettlementBuilding,
        &shared::components::OperatedBy,
        &mut BusinessAccount,
        &mut BusinessWagePolicy,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&shared::components::OwnedBy>,
    )>,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &shared::components::PersonId,
        Option<&shared::components::EmployedAt>,
        &mut Wallet,
        &mut Occupation,
        &mut WorkStatus,
    )>,
    mut processed_day: Local<Option<u32>>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *processed_day == Some(day) {
        return;
    }
    *processed_day = Some(day);
    let settlements_by_id: HashMap<shared::components::SettlementId, Entity> = settlements
        .iter()
        .map(|(entity, settlement_id)| (*settlement_id, entity))
        .collect();
    let companies_by_id: HashMap<shared::components::CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    // Build one daily index. Looking up every roster name by scanning all
    // villagers made payroll O(businesses * population), and the wealthy-owner
    // check repeated that cost even on ticks with no day boundary.
    let mut people_by_id: HashMap<shared::components::PersonId, Entity> = HashMap::new();
    let mut workers_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    let mut available_replacements: HashMap<Entity, usize> = HashMap::new();
    for (entity, _name, intent, person_id, employed_at, _, occupation, status) in villagers.iter() {
        let Some(settlement) = intent.settlement() else {
            continue;
        };
        people_by_id.insert(*person_id, entity);
        if let Some(employment) = employed_at {
            workers_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
        if occupation.0.is_none() && employed_at.is_none() && *status == WorkStatus::LookingForWork
        {
            *available_replacements.entry(settlement).or_default() += 1;
        }
    }

    for (
        business_entity,
        building,
        operated_by,
        mut account,
        mut wage_policy,
        building_id,
        building_of,
        owner_id,
    ) in businesses.iter_mut()
    {
        if !is_private_business(building.kind) {
            continue;
        }
        let Some(settlement) = settlements_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let mut worker_entities: Vec<Entity> = workers_by_building
            .get(building_id)
            .cloned()
            .unwrap_or_default();
        worker_entities.sort_unstable_by_key(|entity| entity.to_bits());
        worker_entities.dedup();
        let worker_count = worker_entities.len();
        let owner_entity = owner_id.and_then(|owner| people_by_id.get(&owner.0).copied());
        if account.last_payroll_day == u32::MAX {
            account.last_payroll_day = day;
        }
        let elapsed = day.saturating_sub(account.last_payroll_day);
        if elapsed > 0 {
            account.last_payroll_day = day;
            wage_policy.daily_wage = wage_policy
                .daily_wage
                .clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
            let per_worker = wage_policy.daily_wage.saturating_mul(u64::from(elapsed));
            // The day boundary pays the shift which just finished. Record the
            // expense against that completed day so its wages and sales appear
            // in the same P&L rather than one calendar day apart.
            account.incur_completed_day_wages(
                day.saturating_sub(1),
                per_worker.saturating_mul(worker_count as u64),
            );

            // Arrears are a real workplace liability, not merely a warning
            // counter. When later sales make cash available, distribute the
            // entire affordable obligation evenly so no alphabetically-early
            // worker is always paid while everybody else starves.
            if !worker_entities.is_empty() {
                let company_entity = companies_by_id.get(&operated_by.0).copied();
                let payment_budget = company_entity
                    .and_then(|entity| company_accounts.get(entity).ok())
                    .map_or(0, |company| company.cash.min(account.wage_arrears));
                let worker_count = worker_entities.len() as u64;
                let equal_share = payment_budget / worker_count;
                let remainder = payment_budget % worker_count;
                let mut paid = 0_u64;
                for (index, worker) in worker_entities.iter().copied().enumerate() {
                    let payment = equal_share + u64::from((index as u64) < remainder);
                    let Ok((_, _, _, _, _, mut wallet, _, _)) = villagers.get_mut(worker) else {
                        continue;
                    };
                    wallet.credit(payment);
                    paid = paid.saturating_add(payment);
                }
                if let Some(company_entity) = company_entity {
                    if let Ok(mut company) = company_accounts.get_mut(company_entity) {
                        if company.debit(paid) {
                            let settled = account.settle_wage_claim(paid);
                            debug_assert_eq!(settled, paid);
                        }
                    }
                }
            }

            let company_cash = companies_by_id
                .get(&operated_by.0)
                .and_then(|entity| company_accounts.get(*entity).ok())
                .map_or(0, |company| company.cash);
            review_automatic_wage_offer(
                &mut wage_policy,
                elapsed,
                worker_count,
                building.kind.positions() as usize,
                company_cash,
                account.wage_arrears,
            );
        }

        let Some(_owner_id) = owner_id else {
            continue;
        };
        let Some(owner_entity) = owner_entity else {
            continue;
        };
        let owner_is_worker =
            villagers
                .get(owner_entity)
                .is_ok_and(|(_, _, _, _, employed_at, _, _, _)| {
                    employed_at.copied() == Some(shared::components::EmployedAt(*building_id))
                });
        if !owner_is_worker {
            continue;
        }
        let replacement_exists = available_replacements
            .get(&settlement)
            .copied()
            .unwrap_or(0)
            > 0;
        let owner_is_wealthy = villagers
            .get(owner_entity)
            .is_ok_and(|(_, _, _, _, _, wallet, _, _)| wallet.balance() >= WEALTHY_OWNER_MONEY);
        let payroll_secure = companies_by_id
            .get(&operated_by.0)
            .and_then(|entity| company_accounts.get(*entity).ok())
            .map_or(0, |company| {
                company
                    .cash
                    .saturating_sub(company.wage_arrears)
                    .saturating_sub(company.tax_arrears)
            })
            >= wage_policy
                .daily_wage
                .saturating_mul(worker_count as u64)
                .saturating_mul(2);
        if !replacement_exists || !owner_is_wealthy || !payroll_secure {
            continue;
        }
        if let Ok((_, owner_name, _, _, _, _, mut occupation, mut status)) =
            villagers.get_mut(owner_entity)
        {
            occupation.0 = None;
            *status = WorkStatus::Chilling;
            commands
                .entity(owner_entity)
                .remove::<shared::components::EmployedAt>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<MoveTarget>();
            info!(
                "Village '{}': {} became a wealthy owner and left daily {} work",
                building.settlement,
                owner_name.0,
                building.kind.trade().unwrap_or("business")
            );
        }
        let _ = business_entity;
    }
}
