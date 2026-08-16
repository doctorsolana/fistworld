//! Settlement demand, permits and geography-aware site selection.

use super::development_market::{
    investor_score, investor_threshold, minimum_startup_capital, private_opportunities,
    replicated_opportunity_board, DevelopmentMarketSignals, DevelopmentOpportunity,
};
use super::*;

/// Autonomous founders preserve a few days of personal purchasing power.
/// Company formation is an investment decision, not permission to commit the
/// resident's last meal money. Player-directed contributions remain explicit.
const NPC_PERSONAL_INVESTMENT_RESERVE: u64 = 3 * PENNIES_PER_COIN;

/// A settlement can approve distinct permits concurrently, but it cannot turn
/// every newly arrived resident into an independent construction crew on the
/// same morning. Capacity grows with population and stays bounded while this
/// region is tactically embodied; completed sites immediately free a slot.
const CONSTRUCTION_CREW_RESIDENTS_PER_SITE: u32 = 12;
const MIN_CONCURRENT_SETTLEMENT_WORKSITES: usize = 3;
const MAX_CONCURRENT_SETTLEMENT_WORKSITES: usize = 12;
const MAX_SETTLEMENT_SEARCH_RADIUS: f32 = 320.0;

/// Optional fine-grained permit telemetry used by the deterministic lab.
///
/// Production does not insert this resource, so ordinary server ticks pay no
/// vector-growth cost. Keeping the probes at the actual search calls lets a
/// stress run distinguish site geometry from the rest of the civic/economy
/// schedule instead of guessing from one aggregate core duration.
#[derive(Resource, Default)]
pub struct PermitPlanningDiagnostics {
    pub primary_site_milliseconds: Vec<f64>,
    pub alternative_site_milliseconds: Vec<f64>,
    pub fishing_site_milliseconds: Vec<f64>,
    pub final_access_milliseconds: Vec<f64>,
}

#[derive(Debug, Default)]
struct CompanyExpansionFunds {
    available: u64,
    entity: Option<Entity>,
}

fn debit_company_expansion(
    funds: &mut CompanyExpansionFunds,
    accounts: &mut Query<(
        Entity,
        &shared::components::CompanyId,
        &mut shared::economy::CompanyAccount,
    )>,
    amount: u64,
) -> bool {
    if funds.available < amount {
        return false;
    }
    let Some(entity) = funds.entity else {
        return false;
    };
    let Ok((_, _, mut account)) = accounts.get_mut(entity) else {
        return false;
    };
    if !account.debit(amount) {
        return false;
    }
    funds.available -= amount;
    true
}

#[cfg(test)]
mod company_funding_tests {
    use super::*;
    use bevy::ecs::system::SystemState;

    #[test]
    fn company_expansion_debit_conserves_cash_and_never_touches_reserves() {
        let mut world = World::new();
        let company = shared::components::CompanyId(8);
        let company_entity = world
            .spawn((
                company,
                shared::economy::CompanyAccount {
                    cash: 1_500,
                    ..default()
                },
            ))
            .id();
        let mut funds = CompanyExpansionFunds {
            available: 800,
            entity: Some(company_entity),
        };
        let mut state = SystemState::<
            Query<(
                Entity,
                &shared::components::CompanyId,
                &mut shared::economy::CompanyAccount,
            )>,
        >::new(&mut world);
        {
            let mut accounts = state
                .get_mut(&mut world)
                .expect("valid company account query");
            assert!(debit_company_expansion(&mut funds, &mut accounts, 650));
        }
        state.apply(&mut world);

        assert_eq!(funds.available, 150);
        assert_eq!(
            world
                .get::<shared::economy::CompanyAccount>(company_entity)
                .unwrap()
                .cash,
            850
        );
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SiteSearchRejections {
    sampled: u32,
    earthworks: u32,
    bounds: u32,
    water: u32,
    props: u32,
    occupied: u32,
    roads: u32,
    access_or_work: u32,
}

impl SiteSearchRejections {
    fn summary(self) -> String {
        format!(
            "sampled={} rejected earthworks={} bounds={} water={} permanent_props={} occupied={} roads={} access/work={}",
            self.sampled,
            self.earthworks,
            self.bounds,
            self.water,
            self.props,
            self.occupied,
            self.roads,
            self.access_or_work,
        )
    }
}

pub(super) fn concurrent_worksite_capacity(residents: u32) -> usize {
    (residents.div_ceil(CONSTRUCTION_CREW_RESIDENTS_PER_SITE) as usize).clamp(
        MIN_CONCURRENT_SETTLEMENT_WORKSITES,
        MAX_CONCURRENT_SETTLEMENT_WORKSITES,
    )
}

pub(crate) fn development_pipeline_has_capacity(
    residents: u32,
    worksites: usize,
    connectors: usize,
) -> bool {
    worksites.saturating_add(connectors) < concurrent_worksite_capacity(residents)
}

/// Processing permits depend on an operating upstream building, not merely an
/// approved plot. Pending sites still count for duplicate suppression in
/// `have`, but a mill cannot process a Farmstead's promise and a bakery cannot
/// bake with an unfinished windmill.
fn processing_upstream_is_complete(
    kind: SettlementBuildingKind,
    completed: &HashMap<SettlementBuildingKind, usize>,
) -> bool {
    let has = |upstream| completed.get(&upstream).copied().unwrap_or(0) > 0;
    match kind {
        SettlementBuildingKind::Windmill => has(SettlementBuildingKind::Farmstead),
        SettlementBuildingKind::Bakery => has(SettlementBuildingKind::Windmill),
        _ => true,
    }
}

fn include_resumable_search_cursor(
    min_radius: f32,
    max_radius: f32,
    cursor: Option<f32>,
) -> (f32, f32) {
    let Some(cursor) = cursor else {
        return (min_radius, max_radius);
    };
    let cursor = cursor.clamp(min_radius, MAX_SETTLEMENT_SEARCH_RADIUS);
    // The cursor is authoritative progress through the physical envelope. It
    // must expand the current preferred band as it advances; clamping it back
    // to `max_radius` repeatedly sampled the same ring and eventually recorded
    // a false "320 m exhausted" result without ever visiting outer ground.
    (min_radius.max(cursor), max_radius.max(cursor))
}

/// Tier infrastructure is requested only after survival/housing shortages are
/// satisfied. The charter influences where it goes, not whether demand and the
/// current tier justify it.
fn next_civic_need(
    tier: shared::components::SettlementTier,
    existing: &HashMap<SettlementBuildingKind, usize>,
) -> Option<SettlementBuildingKind> {
    let has = |kind| existing.get(&kind).copied().unwrap_or(0) > 0;
    match tier {
        shared::components::SettlementTier::Village => {
            if !has(SettlementBuildingKind::Market) {
                Some(SettlementBuildingKind::Market)
            } else if !has(SettlementBuildingKind::Tavern) {
                Some(SettlementBuildingKind::Tavern)
            } else {
                None
            }
        }
        shared::components::SettlementTier::Town => {
            (!has(SettlementBuildingKind::Church)).then_some(SettlementBuildingKind::Church)
        }
        _ => None,
    }
}

/// A coastal settlement should diversify its second food workplace instead of
/// building Farmsteads forever merely because the shortage model continues to
/// return `Farmstead`. The shoreline search remains authoritative: inland
/// settlements fall straight back to the requested farm.
pub(super) fn should_try_complementary_fishing(
    requested: Option<SettlementBuildingKind>,
    planned_farms: usize,
    planned_fishers: usize,
) -> bool {
    // Keep advancing the bounded shoreline search while houses or another
    // urgent permit temporarily leads the shortage model. Waiting until that
    // model asks for Food again lets a migration wave fill the only usable
    // waterfront before the one-ring-at-a-time search ever reaches it. A
    // successful coast claim pauses just one ordinary permit and then this
    // condition switches off permanently.
    requested == Some(SettlementBuildingKind::Farmstead)
        && planned_farms > 0
        && planned_fishers == 0
}

/// A resident applies for a permit, and it is approved if it is valid.
///
/// Distinct decisions may be in flight together. Planned kinds count as already
/// had, each applicant can hold only one active build, and pending plots reserve
/// their ground, so concurrency cannot duplicate or overlap construction.
/// Needed housing approval is free. A business permit belongs to a company:
/// retained company cash pays the actual civic fee, while a first-time sole
/// founder explicitly capitalises a company before its site is approved.
#[allow(clippy::too_many_arguments)]
pub fn consider_permits(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    mut clock: ResMut<VillageClock>,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
    mut commands: Commands,
    mut planning: PermitPlanningResources,
    mut settlements: Query<(
        Entity,
        &mut Settlement,
        &PlayerPosition,
        &shared::components::SettlementId,
        Option<&mut shared::economy::CivicAccount>,
        Option<&SettlementPolicies>,
        Option<&MootMarket>,
        Option<&shared::components::SettlementOpportunityBoard>,
    )>,
    economies: Query<&SettlementEconomy>,
    developments: Query<&shared::components::SettlementDevelopment>,
    mut buildings: ParamSet<(
        Query<(
            Entity,
            &SettlementBuilding,
            &shared::components::BuildingOf,
            Option<&shared::components::OwnedBy>,
            Option<&BusinessCondition>,
            Option<&GoodsInventory>,
            Option<&BusinessAccount>,
            Option<&shared::components::BuildingId>,
            Option<&shared::components::OperatedBy>,
            Option<&BusinessWagePolicy>,
            Option<&BusinessStaffingPolicy>,
        )>,
        Query<(
            Entity,
            &shared::components::CompanyId,
            &mut shared::economy::CompanyAccount,
        )>,
    )>,
    pending: Query<(&UnderConstruction, &GoodsInventory)>,
    road_requests: Query<&RoadRequest>,
    placed: Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    // ONE query, read then written. Two -- a read of `&VillagerIntent` and a
    // write of `&mut VillagerIntent` -- is a genuine conflict Bevy refuses at
    // runtime, and iterating a mutable query gives read-only items anyway.
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &mut VillagerIntent,
        Option<&WorkStatus>,
        Option<&CharacterAttributes>,
        Option<&shared::components::LivesAt>,
        Option<&strategic::StrategicPerson>,
        Option<&shared::components::CivicEmployment>,
    )>,
    mut wallets: Query<&mut Wallet>,
) {
    let civic_day = world_time
        .iter()
        .next()
        .map_or(1, |clock| clock.day.saturating_add(1));
    clock.permit += simulation_time.world_seconds();
    if clock.permit < PERMIT_INTERVAL {
        return;
    }
    clock.permit = 0.0;
    clock.permit_round = clock.permit_round.wrapping_add(1);
    let permit_round = clock.permit_round;

    let Some(terrain) = planning.terrain.as_deref() else {
        return;
    };

    // A Master normally controls one craft company. If inheritance or player
    // activity leaves an NPC in charge of several, choose the oldest stable
    // CompanyId deterministically instead of depending on ECS iteration order.
    let mut company_by_master =
        HashMap::<shared::components::PersonId, shared::components::CompanyId>::new();
    let mut company_strategies =
        HashMap::<shared::components::CompanyId, shared::economy::BusinessStrategy>::new();
    for (id, leadership, policy, _) in planning.companies.iter() {
        company_strategies.insert(*id, policy.strategy);
        company_by_master
            .entry(leadership.master)
            .and_modify(|current| {
                if *id < *current {
                    *current = *id;
                }
            })
            .or_insert(*id);
    }
    let company_reserve_days: HashMap<shared::components::CompanyId, u8> = planning
        .companies
        .iter()
        .map(|(id, _, policy, _)| (*id, policy.payroll_reserve_days))
        .collect();
    // A sole proprietor can inject more of their own money without changing
    // anybody else's economic interest. Once shares are split, an automatic
    // personal top-up would enrich the other shareholders for free; co-owned
    // firms therefore expand only from retained cash until explicit company
    // loans or primary share issuance are introduced.
    let personal_capital_companies: HashSet<shared::components::CompanyId> = planning
        .companies
        .iter()
        .filter_map(|(id, leadership, _, ownership)| {
            ownership
                .is_none_or(|ownership| {
                    ownership.share_count(leadership.master)
                        == shared::components::COMPANY_TOTAL_SHARES
                })
                .then_some(*id)
        })
        .collect();
    let mut company_funds = HashMap::<shared::components::CompanyId, CompanyExpansionFunds>::new();
    let mut protected_payroll = HashMap::<shared::components::CompanyId, u64>::new();
    {
        let business_read = buildings.p0();
        for (_, building, _, _, _, _, account, _, operated_by, wage, staffing) in
            business_read.iter()
        {
            let (Some(_account), Some(operated_by)) = (account, operated_by) else {
                continue;
            };
            let reserve_days = u64::from(
                company_reserve_days
                    .get(&operated_by.0)
                    .copied()
                    .unwrap_or(1),
            );
            let fallback_wage = BusinessWagePolicy {
                daily_wage: FOUNDING_DAILY_WAGE,
                automatic: true,
                ..default()
            };
            let enabled_positions = staffing
                .copied()
                .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
                .target_for(building.kind);
            let payroll = u64::from(enabled_positions)
                .saturating_mul(wage.unwrap_or(&fallback_wage).daily_wage)
                .saturating_mul(reserve_days);
            let protected = protected_payroll.entry(operated_by.0).or_default();
            *protected = protected.saturating_add(payroll);
        }
    }
    {
        let mut accounts = buildings.p1();
        for (entity, company_id, account) in accounts.iter_mut() {
            company_funds.insert(
                *company_id,
                CompanyExpansionFunds {
                    available: shared::economy::company_expansion_cash(
                        &account,
                        protected_payroll
                            .get(company_id)
                            .copied()
                            .unwrap_or_default(),
                    ),
                    entity: Some(entity),
                },
            );
        }
    }

    for (
        settlement_entity,
        mut settlement,
        hall,
        settlement_id,
        mut civic_account,
        policies,
        market,
        current_board,
    ) in settlements.iter_mut()
    {
        let policies = policies.copied().unwrap_or_default();
        // Count what stands AND what is already approved, or three residents
        // deciding on successive permit ticks all build the same thing.
        let mut have: HashMap<SettlementBuildingKind, usize> = HashMap::new();
        let mut completed: HashMap<SettlementBuildingKind, usize> = HashMap::new();
        let business_read = buildings.p0();
        for (_, building, _, _, _, _, _, _, _, _, _) in
            business_read
                .iter()
                .filter(|(_, _, building_of, _, condition, _, _, _, _, _, _)| {
                    building_of.0 == *settlement_id
                        && !condition
                            .is_some_and(|condition| !condition.state.counts_as_active_capacity())
                })
        {
            *have.entry(building.kind).or_default() += 1;
            *completed.entry(building.kind).or_default() += 1;
        }
        for (under, _) in pending.iter() {
            if under.settlement == settlement_entity {
                *have.entry(under.kind).or_default() += 1;
            }
        }
        // The hall is always there; it is the founding act, not a need.
        have.insert(SettlementBuildingKind::Hall, 1);

        let active_worksites = pending
            .iter()
            .filter(|(under, _)| under.settlement == settlement_entity)
            .count()
            + planning
                .hall_upgrades
                .iter()
                .filter(|(_, building_of, _)| building_of.0 == *settlement_id)
                .count();
        let active_connectors = road_requests
            .iter()
            .filter(|request| request.settlement == settlement_entity)
            .count()
            + roads
                .iter()
                .filter(|(road, road_of)| road_of.0 == *settlement_id && !road.is_complete())
                .count();
        let pipeline_has_capacity = development_pipeline_has_capacity(
            settlement.residents,
            active_worksites,
            active_connectors,
        );

        let count = |kind| have.get(&kind).copied().unwrap_or(0);
        let mut signals = DevelopmentMarketSignals {
            residents: settlement.residents,
            farms: count(SettlementBuildingKind::Farmstead),
            fishers: count(SettlementBuildingKind::FishermansHut),
            windmills: count(SettlementBuildingKind::Windmill),
            bakeries: count(SettlementBuildingKind::Bakery),
            storage_halls: count(SettlementBuildingKind::StorageHall),
            completed_windmills: completed
                .get(&SettlementBuildingKind::Windmill)
                .copied()
                .unwrap_or(0),
            completed_bakeries: completed
                .get(&SettlementBuildingKind::Bakery)
                .copied()
                .unwrap_or(0),
            lumber_huts: count(SettlementBuildingKind::LumberjackHut),
            stone_quarries: count(SettlementBuildingKind::StoneQuarry),
            houses: count(SettlementBuildingKind::House),
            // A Village does not create a speculative Stone shortage merely
            // because Town is its eventual next tier. Demand begins when its
            // physical Town Works exists, or when another settlement posts a
            // cash-backed tender which this town could supply.
            town_hall_stone_demand: planning
                .trade_contracts
                .iter()
                .filter(|contract| {
                    contract.origin.is_none()
                        && contract.destination != *settlement_id
                        && contract.good == Good::Stone
                        && contract.status.is_active()
                })
                .map(|contract| contract.remaining_units())
                .fold(0u32, u32::saturating_add),
            ..Default::default()
        };
        signals.export_contract_bulk = planning
            .trade_contracts
            .iter()
            .filter(|contract| {
                contract.origin == Some(*settlement_id) && contract.status.is_active()
            })
            .map(|contract| {
                contract
                    .remaining_units()
                    .saturating_mul(contract.good.bulk_per_unit())
            })
            .fold(0u32, u32::saturating_add);
        for (_, building, building_of, _, condition, inventory, account, _, _, _, staffing) in
            business_read.iter()
        {
            if building_of.0 != *settlement_id {
                continue;
            }
            // Physical stock remains market information after a firm stops
            // operating. Hiding a liquidator's full store makes the permit
            // system construct a replacement into the same unresolved glut.
            if let Some(inventory) = inventory {
                signals.wheat_stock = signals
                    .wheat_stock
                    .saturating_add(inventory.amount(Good::Wheat));
                signals.flour_stock = signals
                    .flour_stock
                    .saturating_add(inventory.amount(Good::Flour));
                signals.bread_stock = signals
                    .bread_stock
                    .saturating_add(inventory.amount(Good::Bread));
                signals.wood_stock = signals
                    .wood_stock
                    .saturating_add(inventory.amount(Good::Wood));
                signals.stone_stock = signals
                    .stone_stock
                    .saturating_add(inventory.amount(Good::Stone));
                if building.kind == SettlementBuildingKind::StorageHall {
                    if condition.is_none_or(|condition| condition.state.can_operate()) {
                        signals.active_storage_free_bulk = signals
                            .active_storage_free_bulk
                            .saturating_add(inventory.free_bulk());
                    }
                } else if let Some(output) = super::business_output(building.kind) {
                    let units = inventory.amount(output);
                    let bulk = units.saturating_mul(output.bulk_per_unit());
                    signals.stranded_output_bulk =
                        signals.stranded_output_bulk.saturating_add(bulk);
                    let quote =
                        market.map_or(output.base_price(), |market| market.suggested_price(output));
                    signals.stranded_output_value = signals
                        .stranded_output_value
                        .saturating_add(u64::from(units).saturating_mul(quote));
                }
            }
            if condition.is_some_and(|condition| !condition.state.counts_as_active_capacity()) {
                if condition
                    .is_some_and(|condition| condition.state.counts_as_recoverable_capacity())
                {
                    match building.kind {
                        SettlementBuildingKind::Farmstead => signals.recoverable_farms += 1,
                        SettlementBuildingKind::FishermansHut => signals.recoverable_fishers += 1,
                        SettlementBuildingKind::Windmill => signals.recoverable_windmills += 1,
                        SettlementBuildingKind::Bakery => signals.recoverable_bakeries += 1,
                        SettlementBuildingKind::LumberjackHut => {
                            signals.recoverable_lumber_huts += 1
                        }
                        SettlementBuildingKind::StoneQuarry => {
                            signals.recoverable_stone_quarries += 1
                        }
                        SettlementBuildingKind::StorageHall => {
                            signals.recoverable_storage_halls += 1
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if let Some(capacity) = super::rated_daily_production(building.kind, building.quality) {
                let enabled = staffing
                    .copied()
                    .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
                    .target_for(building.kind);
                let staffed = capacity.output_units.saturating_mul(u32::from(enabled))
                    / u32::from(building.kind.positions().max(1));
                let idle = capacity.output_units.saturating_sub(staffed);
                match building.kind {
                    SettlementBuildingKind::Windmill => {
                        signals.active_windmill_output_capacity = signals
                            .active_windmill_output_capacity
                            .saturating_add(staffed);
                        signals.idle_windmill_output_capacity =
                            signals.idle_windmill_output_capacity.saturating_add(idle);
                    }
                    SettlementBuildingKind::Bakery => {
                        signals.active_bakery_output_capacity = signals
                            .active_bakery_output_capacity
                            .saturating_add(staffed);
                        signals.idle_bakery_output_capacity =
                            signals.idle_bakery_output_capacity.saturating_add(idle);
                    }
                    _ => {}
                }
            }
            if matches!(
                building.kind,
                SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut
            ) {
                let anticipated = super::rated_daily_production(building.kind, building.quality)
                    .map_or(0, |capacity| capacity.output_units);
                match building.kind {
                    SettlementBuildingKind::Farmstead => {
                        signals.anticipated_wheat_output =
                            signals.anticipated_wheat_output.saturating_add(anticipated);
                    }
                    SettlementBuildingKind::FishermansHut => {
                        signals.anticipated_fish_output =
                            signals.anticipated_fish_output.saturating_add(anticipated);
                    }
                    _ => {}
                }
            }
            if let Some(account) = account {
                if let Some(output) = super::business_output(building.kind) {
                    let dispatched = account
                        .current_day
                        .sold_units
                        .saturating_add(account.previous_day.sold_units)
                        .div_ceil(2)
                        .saturating_mul(output.bulk_per_unit());
                    signals.recent_logistics_bulk =
                        signals.recent_logistics_bulk.saturating_add(dispatched);
                }
                if building.kind == SettlementBuildingKind::StorageHall {
                    signals.recent_storage_cost = signals
                        .recent_storage_cost
                        .saturating_add(account.current_day.wage_expense)
                        .saturating_add(account.previous_day.wage_expense);
                }
                let recent = account
                    .current_day
                    .produced_units
                    .saturating_add(account.previous_day.produced_units);
                match building.kind {
                    SettlementBuildingKind::Farmstead => {
                        signals.recent_wheat_output =
                            signals.recent_wheat_output.saturating_add(recent);
                    }
                    SettlementBuildingKind::FishermansHut => {
                        signals.recent_fish_output =
                            signals.recent_fish_output.saturating_add(recent);
                    }
                    SettlementBuildingKind::Windmill => {
                        signals.unproven_windmill |= condition
                            .is_some_and(|condition| condition.state == BusinessState::New);
                        signals.recent_flour_output =
                            signals.recent_flour_output.saturating_add(recent);
                        signals.recent_windmill_input =
                            signals.recent_windmill_input.saturating_add(
                                account
                                    .current_day
                                    .purchased_input_units
                                    .saturating_add(account.previous_day.purchased_input_units),
                            );
                        signals.recent_windmill_sales =
                            signals.recent_windmill_sales.saturating_add(
                                account
                                    .current_day
                                    .sold_units
                                    .saturating_add(account.previous_day.sold_units),
                            );
                        signals.recent_windmill_profit = signals
                            .recent_windmill_profit
                            .saturating_add(account.current_day.profit())
                            .saturating_add(account.previous_day.profit());
                        if !condition.is_some_and(|condition| condition.state == BusinessState::New)
                            && account.current_day.day != u32::MAX
                            && account.previous_day.day != u32::MAX
                            && account
                                .current_day
                                .profit()
                                .saturating_add(account.previous_day.profit())
                                <= 0
                        {
                            signals.lossmaking_windmills += 1;
                        }
                    }
                    SettlementBuildingKind::Bakery => {
                        signals.unproven_bakery |= condition
                            .is_some_and(|condition| condition.state == BusinessState::New);
                        signals.recent_bread_output =
                            signals.recent_bread_output.saturating_add(recent);
                        signals.recent_bakery_input = signals.recent_bakery_input.saturating_add(
                            account
                                .current_day
                                .purchased_input_units
                                .saturating_add(account.previous_day.purchased_input_units),
                        );
                        signals.recent_bakery_sales = signals.recent_bakery_sales.saturating_add(
                            account
                                .current_day
                                .sold_units
                                .saturating_add(account.previous_day.sold_units),
                        );
                        signals.recent_bakery_profit = signals
                            .recent_bakery_profit
                            .saturating_add(account.current_day.profit())
                            .saturating_add(account.previous_day.profit());
                        if !condition.is_some_and(|condition| condition.state == BusinessState::New)
                            && account.current_day.day != u32::MAX
                            && account.previous_day.day != u32::MAX
                            && account
                                .current_day
                                .profit()
                                .saturating_add(account.previous_day.profit())
                                <= 0
                        {
                            signals.lossmaking_bakeries += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(market) = market {
            signals.wheat_stock = signals
                .wheat_stock
                .saturating_add(market.listed_units(Good::Wheat));
            signals.flour_stock = signals
                .flour_stock
                .saturating_add(market.listed_units(Good::Flour));
            signals.bread_stock = signals
                .bread_stock
                .saturating_add(market.listed_units(Good::Bread));
            signals.wood_stock = signals
                .wood_stock
                .saturating_add(market.listed_units(Good::Wood));
            signals.stone_stock = signals
                .stone_stock
                .saturating_add(market.listed_units(Good::Stone));
        }
        for (under, inventory) in pending.iter() {
            if under.settlement == settlement_entity {
                if matches!(
                    under.kind,
                    SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut
                ) {
                    let anticipated = super::rated_daily_production(under.kind, under.quality)
                        .map_or(0, |capacity| capacity.output_units);
                    match under.kind {
                        SettlementBuildingKind::Farmstead => {
                            signals.anticipated_wheat_output =
                                signals.anticipated_wheat_output.saturating_add(anticipated);
                            signals.pending_wheat_output =
                                signals.pending_wheat_output.saturating_add(anticipated);
                        }
                        SettlementBuildingKind::FishermansHut => {
                            signals.anticipated_fish_output =
                                signals.anticipated_fish_output.saturating_add(anticipated);
                            signals.pending_fish_output =
                                signals.pending_fish_output.saturating_add(anticipated);
                        }
                        _ => {}
                    }
                }
                signals.construction_wood_demand = signals.construction_wood_demand.saturating_add(
                    under
                        .kind
                        .construction_wood_required()
                        .saturating_sub(inventory.amount(Good::Wood)),
                );
            }
        }
        for (upgrade, building_of, inventory) in planning.hall_upgrades.iter() {
            if building_of.0 != *settlement_id {
                continue;
            }
            let remaining = upgrade
                .material_required
                .saturating_sub(inventory.amount(upgrade.material));
            match upgrade.material {
                Good::Wood => {
                    signals.construction_wood_demand =
                        signals.construction_wood_demand.saturating_add(remaining);
                }
                Good::Stone => {
                    signals.town_hall_stone_demand =
                        signals.town_hall_stone_demand.saturating_add(remaining);
                }
                _ => {}
            }
        }

        // Index ownership once. Recounting every building for every resident
        // and every opportunity makes permit review O(people × opportunities
        // × buildings), which is exactly the wrong curve for a mature town.
        let mut holding_counts = HashMap::<shared::components::PersonId, usize>::new();
        let mut private_site_counts = HashMap::<shared::components::PersonId, usize>::new();
        let mut storage_holders = HashSet::<shared::components::PersonId>::new();
        let mut holdings_by_kind =
            HashSet::<(shared::components::PersonId, SettlementBuildingKind)>::new();
        for (_, building, building_of, owner, _, _, _, _, _, _, _) in business_read.iter() {
            if building_of.0 == *settlement_id {
                if let Some(owner) = owner {
                    *holding_counts.entry(owner.0).or_default() += 1;
                    holdings_by_kind.insert((owner.0, building.kind));
                    if building.kind == SettlementBuildingKind::StorageHall {
                        storage_holders.insert(owner.0);
                    } else if is_private_business(building.kind) {
                        *private_site_counts.entry(owner.0).or_default() += 1;
                    }
                }
            }
        }
        for (under, _) in pending.iter() {
            if under.settlement_id == *settlement_id {
                if let Some(owner) = under.owner_id {
                    *holding_counts.entry(owner).or_default() += 1;
                    holdings_by_kind.insert((owner, under.kind));
                    if under.kind == SettlementBuildingKind::StorageHall {
                        storage_holders.insert(owner);
                    } else if is_private_business(under.kind) {
                        *private_site_counts.entry(owner).or_default() += 1;
                    }
                }
            }
        }
        let holdings = |who: shared::components::PersonId| -> usize {
            holding_counts.get(&who).copied().unwrap_or(0)
        };
        // NPC depots must belong to an established local branch. A player is
        // still free to buy the tier-unlocked permit from the public board,
        // but autonomous founders do not create a 2,400-bulk warehouse as
        // their first or only business.
        let export_warehouse_needed = signals.export_contract_bulk > 0;
        let may_found_storage = |who: shared::components::PersonId| -> bool {
            company_by_master.contains_key(&who)
                && private_site_counts.get(&who).copied().unwrap_or(0)
                    >= if export_warehouse_needed { 1 } else { 2 }
                && !storage_holders.contains(&who)
        };
        let mut blocked_portfolios = HashSet::<shared::components::PersonId>::new();
        for (owner, condition, for_sale) in planning.portfolios.iter() {
            if for_sale.is_some()
                || condition.is_some_and(|condition| condition.state.blocks_owner_expansion())
            {
                blocked_portfolios.insert(owner.0);
            }
        }
        for (under, _) in pending.iter() {
            if under.settlement_id == *settlement_id && is_private_business(under.kind) {
                if let Some(owner) = under.owner_id {
                    blocked_portfolios.insert(owner);
                }
            }
        }

        // Copy the small facts used by subjective scoring once per resident.
        // Names and mutable intent stay in the authoritative query and are
        // touched only after geography has selected one actual applicant.
        let permit_candidates: Vec<_> = villagers
            .iter()
            .filter_map(
                |(
                    entity,
                    person_id,
                    _,
                    intent,
                    status,
                    attributes,
                    lives_at,
                    strategic,
                    civic_job,
                )| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
                        .then_some((
                            entity,
                            *person_id,
                            status.copied(),
                            attributes.copied(),
                            lives_at.is_some(),
                            strategic.is_some(),
                            civic_job.copied(),
                            planning.permit_busy.get(entity).is_ok(),
                        ))
                },
            )
            .collect();
        let investment_balance = |entity: Entity,
                                  person: shared::components::PersonId,
                                  kind: SettlementBuildingKind|
         -> u64 {
            let personal = wallets
                .get(entity)
                .map(|wallet| wallet.balance())
                .unwrap_or(shared::economy::STARTING_VILLAGER_MONEY);
            if !is_private_business(kind) {
                return personal;
            }
            let personal = personal.saturating_sub(NPC_PERSONAL_INVESTMENT_RESERVE);
            let Some(company) = company_by_master.get(&person) else {
                return personal;
            };
            let retained = company_funds
                .get(company)
                .map_or(0, |funds| funds.available);
            if personal_capital_companies.contains(company) {
                personal.saturating_add(retained)
            } else {
                retained
            }
        };
        let investment_strategy =
            |person: shared::components::PersonId, attributes: Option<&CharacterAttributes>| {
                company_by_master
                    .get(&person)
                    .and_then(|company| company_strategies.get(company))
                    .copied()
                    .unwrap_or_else(|| super::automatic_owner_strategy(Some(person), attributes))
            };

        let mut opportunities = private_opportunities(
            signals,
            economies.get(settlement_entity).ok(),
            market,
            &policies,
        );
        if clock
            .failed_fishing_terrain_versions
            .get(&settlement_entity)
            .is_some_and(|version| *version == terrain.modification_version())
        {
            // Geography is market information once the bounded shoreline
            // survey has actually proved it. Do not keep advertising and
            // subsidising an impossible fishing firm in an inland town; a
            // terrain edit changes the version and reopens the opportunity.
            opportunities
                .retain(|opportunity| opportunity.kind != SettlementBuildingKind::FishermansHut);
        }
        let opportunity_is_deferred = |kind| {
            clock
                .deferred_opportunities
                .get(&(settlement_entity, kind))
                .is_some_and(|until| permit_round < *until)
        };
        let mut selected: Option<(DevelopmentOpportunity, f32)> = None;
        for opportunity in opportunities.iter().copied() {
            if opportunity_is_deferred(opportunity.kind)
                || !processing_upstream_is_complete(opportunity.kind, &completed)
            {
                continue;
            }
            let best_applicant_score = permit_candidates
                .iter()
                .filter_map(
                    |(
                        entity,
                        person_id,
                        status,
                        attributes,
                        housed,
                        strategic,
                        civic_job,
                        busy,
                    )| {
                        if *strategic
                            || civic_job.is_some()
                            || *busy
                            || (opportunity.kind == SettlementBuildingKind::House && *housed)
                            || (opportunity.kind != SettlementBuildingKind::House
                                && blocked_portfolios.contains(person_id))
                            || (opportunity.requires_independent_owner
                                && holdings_by_kind.contains(&(*person_id, opportunity.kind)))
                            || (opportunity.kind == SettlementBuildingKind::StorageHall
                                && !may_found_storage(*person_id))
                        {
                            return None;
                        }
                        let holding_count = holdings(*person_id);
                        let fee = permit_price_with_subsidy(
                            opportunity.kind,
                            holding_count,
                            opportunity.civic_priority,
                            policies.business_permit_subsidy_bps,
                        );
                        let balance = investment_balance(*entity, *person_id, opportunity.kind);
                        let startup_capital = minimum_startup_capital(opportunity.kind, market);
                        if balance < fee.saturating_add(startup_capital) {
                            return None;
                        }
                        if opportunity.kind == SettlementBuildingKind::House {
                            let personal = ((*person_id).0.wrapping_mul(31) % 11) as f32 - 5.0;
                            return Some(opportunity.score + personal - holding_count as f32 * 4.0);
                        }
                        let strategy = investment_strategy(*person_id, attributes.as_ref());
                        let mut score = investor_score(
                            opportunity,
                            strategy,
                            0.5,
                            market,
                            holding_count,
                            person_id.0,
                        );
                        if status.is_some_and(|status| status == WorkStatus::Chilling)
                            && holding_count > 0
                        {
                            score += 5.0;
                        }
                        (score >= investor_threshold(strategy)).then_some(score)
                    },
                )
                .max_by(f32::total_cmp);
            if let Some(applicant_score) = best_applicant_score {
                let replace = selected.is_none_or(|(_, score)| applicant_score > score);
                if replace {
                    selected = Some((opportunity, applicant_score));
                }
            }
        }

        let civic_need =
            next_civic_need(settlement.tier, &have).filter(|kind| !opportunity_is_deferred(*kind));
        let civic_opportunity = civic_need.map(|kind| DevelopmentOpportunity {
            kind,
            score: 62.0,
            civic_priority: true,
            requires_independent_owner: false,
        });
        let board =
            replicated_opportunity_board(&opportunities, civic_opportunity, settlement.tier);
        if current_board != Some(&board) {
            commands.entity(settlement_entity).insert(board);
        }
        if !pipeline_has_capacity {
            // Continue publishing every player permit and its current signal
            // while municipal crews are saturated. Only automatic NPC/public
            // approvals wait here; a player's private hero does not consume
            // this development pipeline.
            continue;
        }
        let mut selected_opportunity = selected.map(|(opportunity, _)| opportunity);
        // Tier infrastructure is a real public choice. It waits behind an
        // acute private shortage, then outranks low-score speculation.
        if civic_need.is_some()
            && selected_opportunity.is_none_or(|opportunity| opportunity.score < 62.0)
        {
            selected_opportunity = civic_opportunity;
        }
        let Some(selected_opportunity) = selected_opportunity else {
            continue;
        };
        let requested = Some(selected_opportunity.kind);
        let planned_farms = have
            .get(&SettlementBuildingKind::Farmstead)
            .copied()
            .unwrap_or(0);
        let planned_fishers = have
            .get(&SettlementBuildingKind::FishermansHut)
            .copied()
            .unwrap_or(0);
        let initial_food_request = planned_farms + planned_fishers == 0;
        let may_add_complementary_fishing =
            should_try_complementary_fishing(requested, planned_farms, planned_fishers);

        if requested.is_none() && !may_add_complementary_fishing {
            continue;
        }

        let missing = selected_opportunity.kind;

        // Site selection can inspect thousands of terrain/prop samples in a
        // mature settlement. Do not repeat that work every permit interval
        // while the only eligible Reeve is already building a civic project,
        // or while no private resident can pay for the requested firm. The
        // actual applicant is selected again after geography chooses the exact
        // kind, so this is only a cheap admission guard, never an approval.
        let has_eligible_applicant = permit_candidates.iter().any(
            |(entity, person_id, _, _, housed, strategic, civic_job, busy)| {
                if *strategic || *busy || (missing == SettlementBuildingKind::House && *housed) {
                    return false;
                }
                if missing != SettlementBuildingKind::House
                    && !missing.is_civic()
                    && blocked_portfolios.contains(person_id)
                {
                    return false;
                }
                if missing == SettlementBuildingKind::StorageHall && !may_found_storage(*person_id)
                {
                    return false;
                }
                if missing.is_civic() {
                    if !civic_job.is_some_and(|job| {
                        job.settlement == *settlement_id
                            && job.role == shared::components::CivicRole::Reeve
                    }) {
                        return false;
                    }
                } else if civic_job.is_some() {
                    return false;
                }
                let fee = permit_price_with_subsidy(
                    missing,
                    holdings(*person_id),
                    selected_opportunity.civic_priority,
                    policies.business_permit_subsidy_bps,
                );
                investment_balance(*entity, *person_id, missing) >= fee
            },
        );
        if !has_eligible_applicant {
            clock
                .deferred_opportunities
                .insert((settlement_entity, missing), permit_round.saturating_add(2));
            debug!(
                "Village '{}': nobody is currently eligible for its next {} permit",
                settlement.name,
                missing.label()
            );
            continue;
        }

        // Occupied ground, so a new building does not land on an old one.
        let mut occupied: Vec<(Vec3, f32)> = placed
            .iter()
            .filter(|(_, building_of, _, _)| building_of.0 == *settlement_id)
            .map(|(building, _, position, _)| (position.0, building.kind.clearance()))
            .chain(std::iter::once((
                hall.0,
                SettlementBuildingKind::Hall.clearance(),
            )))
            .chain(pending.iter().filter_map(|(under, _)| {
                (under.settlement == settlement_entity)
                    .then_some((under.position, under.kind.clearance()))
            }))
            .collect();

        // A Farmstead owns more ground than its cabin. Reserve the separate
        // crop plot as well, including while construction is pending, so a
        // later building cannot be approved on top of the wheat rows.
        occupied.extend(
            placed
                .iter()
                .filter(|(_, building_of, _, _)| building_of.0 == *settlement_id)
                .flat_map(|(building, _, position, rotation)| {
                    let rotation = rotation.map_or(0.0, |rotation| rotation.0);
                    let radius = building
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN);
                    building
                        .kind
                        .field_positions(position.0, rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );
        occupied.extend(
            pending
                .iter()
                .filter(|(under, _)| under.settlement == settlement_entity)
                .flat_map(|(under, _)| {
                    let radius = under
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN);
                    under
                        .kind
                        .field_positions(under.position, under.rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );

        let village_roads: Vec<_> = roads
            .iter()
            .filter_map(|(road, road_of)| (road_of.0 == *settlement_id).then_some(road))
            .collect();
        let existing_accesses: Vec<_> = planning
            .planned_road_accesses
            .iter()
            .filter(|access| access.settlement_id == *settlement_id)
            .cloned()
            .collect();
        let mut access_blockers: Vec<_> = placed
            .iter()
            .filter(|(_, building_of, _, _)| building_of.0 == *settlement_id)
            .flat_map(|(building, _, position, rotation)| {
                road_access_blockers_for_plot(
                    building.kind,
                    position.0,
                    rotation.map_or(0.0, |rotation| rotation.0),
                )
            })
            .collect();
        access_blockers.extend(
            pending
                .iter()
                .filter(|(under, _)| under.settlement == settlement_entity)
                .flat_map(|(under, _)| {
                    road_access_blockers_for_plot(under.kind, under.position, under.rotation)
                }),
        );
        let search_signature = FailedSiteSearch {
            kind: missing,
            occupied_plots: occupied.len(),
            roads: village_roads.len(),
            terrain_version: terrain.modification_version(),
        };
        let repeated_failed_search = clock
            .failed_site_searches
            .get(&settlement_entity)
            .is_some_and(|failed| *failed == search_signature);
        if repeated_failed_search {
            // This opportunity has already exhausted unchanged geometry. Do
            // not let it occupy every four-second review: other households and
            // investors may proceed, while a completed plot/road or terrain
            // edit naturally invalidates the failed-search signature.
            clock
                .deferred_opportunities
                .insert((settlement_entity, missing), permit_round.saturating_add(4));
            continue;
        }
        // Fishing is a first-class private opportunity. The founding food
        // search may still discover a shoreline before the town has enough
        // evidence to rank farm and fish separately, but later Fisher permits
        // always use the authored hut-and-pier geography rather than the
        // generic land-building search.
        let fishing_attempted = requested == Some(SettlementBuildingKind::FishermansHut)
            || initial_food_request
            || may_add_complementary_fishing;
        let fishing_started = fishing_attempted
            .then(|| {
                planning
                    .diagnostics
                    .as_ref()
                    .map(|_| std::time::Instant::now())
            })
            .flatten();
        let coastal_site = fishing_attempted
            .then(|| {
                find_incremental_fishing_site(
                    terrain,
                    hall.0,
                    &occupied,
                    &village_roads,
                    settlement_entity,
                    &mut clock,
                )
            })
            .flatten();
        if let (Some(diagnostics), Some(started)) =
            (planning.diagnostics.as_deref_mut(), fishing_started)
        {
            diagnostics
                .fishing_site_milliseconds
                .push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let primary_started = planning
            .diagnostics
            .as_ref()
            .map(|_| std::time::Instant::now());
        let mut primary_rejections = SiteSearchRejections::default();
        let ordinary_site = requested.and_then(|kind| {
            if kind == SettlementBuildingKind::FishermansHut {
                return None;
            }
            let bounded_resource_search = matches!(
                kind,
                SettlementBuildingKind::Farmstead
                    | SettlementBuildingKind::LumberjackHut
                    | SettlementBuildingKind::Windmill
                    | SettlementBuildingKind::StoneQuarry
            );
            find_site_with_plan_diagnostics(
                terrain,
                hall.0,
                kind,
                &occupied,
                &village_roads,
                &existing_accesses,
                &access_blockers,
                developments.get(settlement_entity).ok(),
                planning.colliders.as_deref(),
                planning.derived.as_deref(),
                bounded_resource_search
                    .then(|| {
                        clock
                            .site_search_radii
                            .get(&(settlement_entity, kind))
                            .copied()
                    })
                    .flatten(),
                bounded_resource_search.then_some(1),
                Some(&mut primary_rejections),
            )
            .map(|(position, rotation)| (kind, position, rotation))
        });
        if let (Some(diagnostics), Some(started)) =
            (planning.diagnostics.as_deref_mut(), primary_started)
        {
            diagnostics
                .primary_site_milliseconds
                .push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let mut approved_site = if let Some((position, rotation, quality)) = coastal_site {
            Some((
                SettlementBuildingKind::FishermansHut,
                position,
                rotation,
                quality,
            ))
        } else if let Some((kind, position, rotation)) = ordinary_site {
            Some((
                kind,
                position,
                rotation,
                site_quality(terrain, kind, position),
            ))
        } else {
            None
        };

        // Food demand belongs to the settlement, not to one hard-coded trade.
        // Once the best sampled Farmstead plots fill, a coastal settlement
        // must be allowed to add another fishing business instead of logging
        // "nowhere to put a Farmstead" forever while residents go hungry.
        // Prefer the requested Farmstead when it is viable; fishing is the
        // geography-aware fallback, so inland settlements retain their normal
        // behaviour and no speculative hut appears without real food demand.
        if approved_site.is_none()
            && requested == Some(SettlementBuildingKind::Farmstead)
            && !fishing_attempted
        {
            let fishing_started = planning
                .diagnostics
                .as_ref()
                .map(|_| std::time::Instant::now());
            approved_site = find_incremental_fishing_site(
                terrain,
                hall.0,
                &occupied,
                &village_roads,
                settlement_entity,
                &mut clock,
            )
            .map(|(position, rotation, quality)| {
                (
                    SettlementBuildingKind::FishermansHut,
                    position,
                    rotation,
                    quality,
                )
            });
            if let (Some(diagnostics), Some(started)) =
                (planning.diagnostics.as_deref_mut(), fishing_started)
            {
                diagnostics
                    .fishing_site_milliseconds
                    .push(started.elapsed().as_secs_f64() * 1_000.0);
            }
        }

        let Some((kind, position, rotation, quality)) = approved_site else {
            if requested == Some(SettlementBuildingKind::FishermansHut)
                && !clock
                    .failed_fishing_terrain_versions
                    .get(&settlement_entity)
                    .is_some_and(|version| *version == terrain.modification_version())
            {
                // `find_incremental_fishing_site` deliberately scans one ring
                // per review. A miss before the final ring is resumable work,
                // not a failed geometry signature; caching it here would make
                // an explicit Fisher opportunity inspect exactly one ring for
                // the rest of the settlement's life.
                clock.failed_site_searches.remove(&settlement_entity);
                clock.deferred_opportunities.insert(
                    (settlement_entity, SettlementBuildingKind::FishermansHut),
                    permit_round.saturating_add(2),
                );
                continue;
            }
            if let Some(searched_kind) = requested.filter(|kind| {
                matches!(
                    kind,
                    SettlementBuildingKind::Farmstead
                        | SettlementBuildingKind::LumberjackHut
                        | SettlementBuildingKind::Windmill
                        | SettlementBuildingKind::StoneQuarry
                )
            }) {
                let cursor = clock
                    .site_search_radii
                    .entry((settlement_entity, searched_kind))
                    .or_insert_with(|| searched_kind.preferred_ring().0);
                if *cursor < MAX_SETTLEMENT_SEARCH_RADIUS {
                    // One outward ring per decision is a deliberate server
                    // budget. Resume at the next ring on the next permit tick
                    // instead of turning one mature-city search into a hitch.
                    *cursor = (*cursor + 6.0).min(MAX_SETTLEMENT_SEARCH_RADIUS);
                    clock.failed_site_searches.remove(&settlement_entity);
                    clock.deferred_opportunities.insert(
                        (settlement_entity, searched_kind),
                        permit_round.saturating_add(2),
                    );
                    continue;
                }
            }
            info!(
                "Village '{}': nowhere to put a {} yet ({})",
                settlement.name,
                missing.label(),
                primary_rejections.summary(),
            );
            clock
                .failed_site_searches
                .insert(settlement_entity, search_signature);
            clock
                .deferred_opportunities
                .insert((settlement_entity, missing), permit_round.saturating_add(4));
            continue;
        };
        if !processing_upstream_is_complete(kind, &completed) {
            // Alternative-site selection can provisionally pretend an
            // unavailable request exists to discover the next useful plot.
            // Never let that bookkeeping fiction authorize a processor whose
            // physical input source has not actually been completed.
            continue;
        }
        let access_started = planning
            .diagnostics
            .as_ref()
            .map(|_| std::time::Instant::now());
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall.0, 0.0);
        let connected_road_keys = crate::world::village_roads::hall_connected_road_keys(
            Vec2::new(hall_door3.x, hall_door3.z),
            &village_roads,
        );
        let planned_access = planned_road_access_path(
            terrain,
            hall.0,
            kind,
            position,
            rotation,
            &village_roads,
            &access_blockers,
            &connected_road_keys,
        );
        if let (Some(diagnostics), Some(started)) =
            (planning.diagnostics.as_deref_mut(), access_started)
        {
            diagnostics
                .final_access_milliseconds
                .push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let Some(planned_access_points) = planned_access else {
            if kind == SettlementBuildingKind::FishermansHut
                && advance_incremental_fishing_search(&mut clock, settlement_entity)
            {
                clock.failed_site_searches.remove(&settlement_entity);
            } else if matches!(
                kind,
                SettlementBuildingKind::Farmstead
                    | SettlementBuildingKind::LumberjackHut
                    | SettlementBuildingKind::Windmill
                    | SettlementBuildingKind::StoneQuarry
            ) {
                let cursor = clock
                    .site_search_radii
                    .entry((settlement_entity, kind))
                    .or_insert_with(|| kind.preferred_ring().0);
                if *cursor < MAX_SETTLEMENT_SEARCH_RADIUS {
                    *cursor = (*cursor + 6.0).min(MAX_SETTLEMENT_SEARCH_RADIUS);
                    clock.failed_site_searches.remove(&settlement_entity);
                } else {
                    clock
                        .failed_site_searches
                        .insert(settlement_entity, search_signature);
                }
            } else {
                clock
                    .failed_site_searches
                    .insert(settlement_entity, search_signature);
            }
            continue;
        };
        if existing_accesses.iter().any(|access| {
            let footprint_radius = kind.art().definition().root_footprint_radius() + 0.45;
            access.intersects_circle(Vec2::new(position.x, position.z), footprint_radius)
        }) {
            // Fishing uses its own shoreline search and alternative selection,
            // so repeat the generic access proof here. Ordinary plots already
            // passed it inside `find_site_with_plan`; this is deliberately a
            // cheap safety net rather than approving an inaccessible coast.
            if kind == SettlementBuildingKind::FishermansHut
                && advance_incremental_fishing_search(&mut clock, settlement_entity)
            {
                clock.failed_site_searches.remove(&settlement_entity);
            } else {
                clock
                    .failed_site_searches
                    .insert(settlement_entity, search_signature);
            }
            continue;
        }
        clock.failed_site_searches.remove(&settlement_entity);
        if matches!(
            kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::Windmill
                | SettlementBuildingKind::StoneQuarry
        ) {
            clock.site_search_radii.insert(
                (settlement_entity, kind),
                Vec2::new(position.x - hall.0.x, position.z - hall.0.z).length(),
            );
        }

        // A resident applies only after geography has selected the actual
        // permit kind. This prevents an impossible lumber permit from applying
        // lumber prices or eligibility rules to a fallback house or farm.
        // The applicant is the resident whose strategy, expected margin and
        // personal judgement most strongly support this exact site.
        let applicant = villagers
            .iter()
            .filter(|(_, _, _, intent, _, _, _, strategic, _)| {
                strategic.is_none()
                    && matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
            })
            .filter_map(
                |(
                    entity,
                    person_id,
                    name,
                    _,
                    status,
                    attributes,
                    lives_at,
                    _,
                    civic_job,
                )| {
                // Owning a building does not create a second job, but its
                // construction still needs the applicant's physical time.
                // Never tear somebody out of a field, workplace doorway or
                // fishing pier mid-shift. Employed residents become eligible
                // again after their bounded work routine ends for the day.
                if planning.permit_busy.get(entity).is_ok() {
                    return None;
                }
                if kind == SettlementBuildingKind::House && lives_at.is_some() {
                    return None;
                }
                if kind != SettlementBuildingKind::House
                    && !kind.is_civic()
                    && blocked_portfolios.contains(person_id)
                {
                    return None;
                }
                if kind == SettlementBuildingKind::StorageHall
                    && !may_found_storage(*person_id)
                {
                    return None;
                }
                if kind.is_civic() {
                    if !civic_job.is_some_and(|job| {
                        job.settlement == *settlement_id
                            && job.role == shared::components::CivicRole::Reeve
                    }) {
                        return None;
                    }
                } else if civic_job.is_some() {
                    return None;
                }
                let actual_opportunity = if kind == selected_opportunity.kind {
                    selected_opportunity
                } else if kind == SettlementBuildingKind::FishermansHut
                    && selected_opportunity.kind == SettlementBuildingKind::Farmstead
                {
                    opportunities
                        .iter()
                        .copied()
                        .find(|opportunity| opportunity.kind == kind)
                        .unwrap_or(DevelopmentOpportunity {
                            kind,
                            requires_independent_owner: false,
                            ..selected_opportunity
                        })
                } else {
                    opportunities
                        .iter()
                        .copied()
                        .find(|opportunity| opportunity.kind == kind)
                        .unwrap_or(DevelopmentOpportunity {
                            kind,
                            score: selected_opportunity.score,
                            civic_priority: selected_opportunity.civic_priority,
                            requires_independent_owner: false,
                        })
                };
                if actual_opportunity.requires_independent_owner
                    && holdings_by_kind.contains(&(*person_id, kind))
                {
                    return None;
                }
                let holding_count = holdings(*person_id);
                let fee = permit_price_with_subsidy(
                    kind,
                    holding_count,
                    actual_opportunity.civic_priority,
                    policies.business_permit_subsidy_bps,
                );
                let balance = investment_balance(entity, *person_id, kind);
                let startup_capital = minimum_startup_capital(kind, market);
                if balance < fee.saturating_add(startup_capital) {
                    return None;
                }
                let mut decision_score = actual_opportunity.score;
                if !kind.is_civic() && kind != SettlementBuildingKind::House {
                    let strategy = investment_strategy(*person_id, attributes);
                    decision_score = investor_score(
                        actual_opportunity,
                        strategy,
                        quality,
                        market,
                        holding_count,
                        person_id.0,
                    );
                    if status.is_some_and(|status| *status == WorkStatus::Chilling)
                        && holding_count > 0
                    {
                        decision_score += 5.0;
                    }
                    if decision_score < investor_threshold(strategy) {
                        return None;
                    }
                } else if kind == SettlementBuildingKind::House {
                    decision_score -= holding_count as f32 * 4.0;
                }
                Some((
                    entity,
                    *person_id,
                    name.0.clone(),
                    holding_count,
                    decision_score,
                    actual_opportunity.civic_priority,
                    startup_capital,
                ))
            },
            )
            .max_by(|a, b| {
                a.4.total_cmp(&b.4)
                    .then_with(|| b.3.cmp(&a.3))
                    .then_with(|| b.1.cmp(&a.1))
            });
        let Some((
            builder,
            applicant_id,
            applicant,
            applicant_holdings,
            _,
            demand_subsidized,
            startup_capital,
        )) = applicant
        else {
            clock
                .deferred_opportunities
                .insert((settlement_entity, kind), permit_round.saturating_add(3));
            debug!(
                "Village '{}': no resident currently chooses its offered {} permit",
                settlement.name,
                kind.label()
            );
            continue;
        };

        // Charge the actual approved kind. The coastal substitution has the
        // same base as a Farmstead today, but keeping this exact lets their
        // prices diverge later without charging for a building never granted.
        let fee = permit_price_with_subsidy(
            kind,
            applicant_holdings,
            demand_subsidized,
            policies.business_permit_subsidy_bps,
        );
        let prudent_company_cash = fee.saturating_add(startup_capital);
        let mut operating_company = is_private_business(kind)
            .then(|| company_by_master.get(&applicant_id).copied())
            .flatten();
        let company_paid = if let Some(company) = operating_company {
            if company_funds
                .get(&company)
                .is_some_and(|funds| funds.available >= prudent_company_cash)
            {
                let mut accounts = buildings.p1();
                debit_company_expansion(
                    company_funds
                        .get_mut(&company)
                        .expect("checked company expansion funds"),
                    &mut accounts,
                    fee,
                )
            } else {
                false
            }
        } else {
            false
        };
        let mut contributed_capital = 0;
        if !company_paid {
            if operating_company
                .is_some_and(|company| !personal_capital_companies.contains(&company))
            {
                // Defensive recheck: the applicant scoring already excludes
                // this case, but another approved permit may have reduced the
                // pooled expansion balance earlier in the same update.
                continue;
            }
            if let Some(company) = operating_company {
                let Some(funds) = company_funds.get_mut(&company) else {
                    continue;
                };
                let shortfall = prudent_company_cash.saturating_sub(funds.available);
                if let Ok(mut wallet) = wallets.get_mut(builder) {
                    if wallet
                        .balance()
                        .saturating_sub(NPC_PERSONAL_INVESTMENT_RESERVE)
                        < shortfall
                        || !wallet.debit(shortfall)
                    {
                        continue;
                    }
                } else {
                    commands.entity(builder).insert(Wallet::new(
                        shared::economy::STARTING_VILLAGER_MONEY.saturating_sub(shortfall),
                    ));
                }
                let Some(company_entity) = funds.entity else {
                    continue;
                };
                let mut accounts = buildings.p1();
                let Ok((_, _, mut account)) = accounts.get_mut(company_entity) else {
                    continue;
                };
                account.credit(shortfall);
                if !account.debit(fee) {
                    continue;
                }
                account.contributed_capital = account.contributed_capital.saturating_add(shortfall);
                funds.available = funds
                    .available
                    .saturating_add(shortfall)
                    .saturating_sub(fee);
                contributed_capital = shortfall;
            } else {
                if let Ok(mut wallet) = wallets.get_mut(builder) {
                    if wallet
                        .balance()
                        .saturating_sub(NPC_PERSONAL_INVESTMENT_RESERVE)
                        < prudent_company_cash
                        || !wallet.debit(prudent_company_cash)
                    {
                        continue;
                    }
                } else {
                    // Old/test villagers without a wallet migrate into the live
                    // rule with the same founding endowment, minus this capital.
                    commands.entity(builder).insert(Wallet::new(
                        shared::economy::STARTING_VILLAGER_MONEY
                            .saturating_sub(prudent_company_cash),
                    ));
                }
                if is_private_business(kind) {
                    let company = planning.ids.company();
                    commands.spawn(super::new_company_bundle(
                        company,
                        format!("{} & Company", applicant),
                        civic_day.saturating_sub(1),
                        applicant_id,
                        startup_capital,
                        prudent_company_cash,
                    ));
                    operating_company = Some(company);
                    contributed_capital = prudent_company_cash;
                }
            }
        }
        settlement.treasury = settlement.treasury.saturating_add(fee);
        if let Some(account) = civic_account.as_deref_mut() {
            account.record_permit_income(civic_day, fee);
        }

        // Auto-approved after payment. A permit still builds nothing: private
        // owners buy or gather the Wood, while civic work draws physical Wood
        // from the settlement's common hall inventory.
        // How good this ground is for this trade, sampled where it will stand
        // rather than at the hall. A farmstead on the settlement's best soil is
        // worth more than one behind the woodshed, and that has to be decided
        // by the plot, not the village.
        // Beside the plot, in front of it. The builder must not stand where the
        // building is about to rise.
        let stand = shared::components::builder_stand_position(
            position,
            rotation,
            kind.art().definition().footprint.y,
        );

        let site = commands
            .spawn((
                UnderConstruction {
                    kind,
                    position,
                    rotation,
                    // Progression amenities are public works. The applicant
                    // supplies builder time, while the settlement owns the
                    // completed structure and its common-stock material bill.
                    owner: (!kind.is_civic()).then_some(applicant.clone()),
                    owner_id: (!kind.is_civic()).then_some(applicant_id),
                    builder: Some(builder),
                    settlement: settlement_entity,
                    settlement_id: *settlement_id,
                    stand,
                    failed_stand_routes: 0,
                    stage: BuildStage::Supplying,
                    quality,
                },
                // The replicated half, so the panel can show it as approved.
                shared::components::ConstructionSite {
                    kind,
                    settlement: settlement.name.clone(),
                    raising: false,
                    stand,
                    rotation,
                },
                GoodsInventory::new(kind.construction_storage_bulk()),
                PlannedRoadAccess {
                    settlement_id: *settlement_id,
                    points: planned_access_points,
                    half_width: RoadClass::Lane.initial_reserved_width() * 0.5,
                },
                PlayerPosition(position),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        if is_private_business(kind) {
            commands.entity(site).insert((
                BusinessProjectAccounting {
                    company: operating_company,
                    contributed_capital,
                    capital_expenditure: fee,
                },
                shared::components::OperatedBy(
                    operating_company.expect("private permit formed or selected a company"),
                ),
            ));
        }

        if !kind.is_civic() {
            // Constructing an approved private plot is the applicant's one
            // daytime occupation until the shell is finished. An off-shift
            // employee may choose entrepreneurship, but they must resign the
            // old position instead of becoming a farmer/porter and builder at
            // once. Otherwise the two routines repeatedly replace each
            // other's MoveTarget and can strand the worksite forever.
            commands
                .entity(builder)
                .insert((Occupation(None), WorkStatus::LookingForWork))
                .remove::<shared::components::EmployedAt>()
                .remove::<CompanyPorter>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<QuarryRoutine>()
                .remove::<ProcessingRoutine>()
                .remove::<InternalDeliveryRoutine>()
                .remove::<MarketCollectionRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<PierTraversal>()
                .remove::<WorkerOffDuty>()
                .remove::<HomeRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
        }

        // Approval reserves the plot and money immediately, but a tactical
        // applicant first joins the Moot forecourt line to collect the stamped
        // permit. Focused unit tests without the shared runtime retain the
        // direct seam; both paths begin identical physical material work.
        if let Some(queue_clock) = queue_clock.as_deref_mut() {
            moot_services::wait_for_permit(
                &mut commands,
                queue_clock,
                builder,
                settlement_entity,
                site,
            );
        } else {
            commands.entity(builder).remove::<MoveTarget>().insert((
                ConstructionMaterialRoutine::new(site),
                CharacterActivity::Idle,
            ));
        }
        commands
            .entity(builder)
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .remove::<ambient::AmbientRoutine>();
        if let Ok(mut intent) = villagers
            .get_mut(builder)
            .map(|(_, _, _, intent, ..)| intent)
        {
            *intent = VillagerIntent::Building {
                settlement: settlement_entity,
                site,
            };
        }
        info!(
            "Village '{}': {applicant} paid {} coin for a {} permit at {:.0},{:.0}",
            settlement.name,
            shared::economy::format_money(fee),
            kind.label(),
            position.x,
            position.z
        );
    }
}

/// How well this ground suits what is being built, 0..1.
///
/// Reads the SAME `BiomeField::resources` the grass density and the economy
/// read, so a farmstead standing in thick grass really is standing on good
/// soil — the thing you can see is the thing the number says.
///
/// Slope is passed as zero deliberately. Site quality describes the soil that
/// remains after construction; the separate earthwork score decides how much
/// labour is needed to terrace a moderately uneven Farmstead.
pub fn site_quality(terrain: &WorldTerrain, kind: SettlementBuildingKind, at: Vec3) -> f32 {
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        // Hand-authored maps carry no biome field. Neutral rather than zero: a
        // building that works nowhere is worse than one that works averagely.
        return 0.5;
    };
    let profile = field.resources(at.x, at.z, at.y, 0.0);
    kind.yield_quality(&profile)
}

/// Authoritative result of a player-selected plot.
///
/// The client predicts these facts for a responsive ghost, but only this
/// result is allowed to consume a permit or reserve land.
#[derive(Debug, Clone)]
pub(crate) struct ManualPlotApproval {
    pub position: Vec3,
    pub rotation: f32,
    pub quality: f32,
    pub road_access: Vec<Vec2>,
    pub road_snapped: bool,
}

/// Validate one exact player-selected plot with the same physical rules used
/// by automatic settlement planning.
///
/// This intentionally receives compact snapshots rather than ECS queries so
/// the player/network domain cannot grow a second planner. Any future terrain,
/// field, collision or road rule belongs here and therefore governs both NPC
/// and player construction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_manual_plot(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    requested_position: Vec3,
    requested_rotation: f32,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    planned_accesses: &[PlannedRoadAccess],
    access_blockers: &[RoadAccessBlocker],
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Result<ManualPlotApproval, String> {
    if !requested_position.is_finite() || !requested_rotation.is_finite() {
        return Err("That plot position is not valid.".into());
    }
    let ground = terrain.get_height(requested_position.x, requested_position.z);
    let position = Vec3::new(requested_position.x, ground, requested_position.z);
    let rotation = requested_rotation.rem_euclid(std::f32::consts::TAU);
    let distance = Vec2::new(position.x - hall.x, position.z - hall.z).length();
    if distance > MAX_SETTLEMENT_SEARCH_RADIUS {
        return Err(format!(
            "That plot is outside the settlement's {:.0}m charter.",
            MAX_SETTLEMENT_SEARCH_RADIUS
        ));
    }

    if kind == SettlementBuildingKind::Farmstead {
        if farmstead_earthwork_effort(terrain, position, rotation).is_none() {
            return Err("The farmyard or one of its fields needs excessive earthworks.".into());
        }
    } else if slope_at(terrain, position.x, position.z) > MAX_BUILD_SLOPE {
        return Err("The ground is too steep for this building.".into());
    }
    if !plot_fits_navigation_bounds(kind, position, rotation) {
        return Err("Part of this plot would lie outside the playable world.".into());
    }
    if shared::components::minimum_building_water_clearance(terrain, position, kind, rotation)
        < FREEBOARD
    {
        return Err("The building and its doorway must remain safely above the waterline.".into());
    }
    if !crate::world::village_roads::doorway_road_apron_is_dry(terrain, kind, position, rotation) {
        return Err("The doorway has no dry approach.".into());
    }
    if colliders.zip(derived).is_some_and(|(colliders, derived)| {
        !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
            kind, position, rotation, colliders, derived,
        )
    }) {
        return Err("A permanent object blocks the doorway.".into());
    }

    let clearance = kind.clearance();
    if occupied.iter().any(|(other, other_clearance)| {
        Vec2::new(position.x - other.x, position.z - other.z).length() < clearance + other_clearance
    }) {
        return Err("This plot overlaps an existing or reserved building.".into());
    }

    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        for field in fields {
            if shared::components::minimum_rotated_rect_water_clearance(
                terrain,
                field,
                field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                rotation,
            ) < FREEBOARD
            {
                return Err("One of the two wheat fields reaches wet ground.".into());
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
                    Vec2::new(field.x, field.z),
                    field_half,
                    rotation,
                    shared::components::FARM_FIELD_TERRACE_MARGIN,
                    colliders,
                    derived,
                )
            }) {
                return Err("A permanent object blocks one of the wheat fields.".into());
            }
            let field_clearance =
                field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN;
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(field.x - other.x, field.z - other.z).length()
                    < field_clearance + other_clearance
            }) {
                return Err("One of the two wheat fields overlaps reserved land.".into());
            }
            if roads.iter().any(|road| {
                road.intersects_rotated_rect(
                    Vec2::new(field.x, field.z),
                    field_half,
                    rotation,
                    shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }) {
                return Err("A road reservation crosses one of the wheat fields.".into());
            }
            if planned_accesses.iter().any(|access| {
                access.intersects_circle(
                    Vec2::new(field.x, field.z),
                    field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }) {
                return Err("Another building's access lane crosses a wheat field.".into());
            }
        }
    }

    let point = Vec2::new(position.x, position.z);
    let footprint_radius = kind.art().definition().root_footprint_radius() + 0.45;
    if roads
        .iter()
        .any(|road| road.contains_reserved_point(point, footprint_radius))
    {
        return Err("The building footprint overlaps a road reservation.".into());
    }
    if planned_accesses
        .iter()
        .any(|access| access.intersects_circle(point, footprint_radius))
    {
        return Err("The building footprint overlaps another reserved access lane.".into());
    }

    if kind == SettlementBuildingKind::FishermansHut {
        let water = terrain
            .water_level()
            .ok_or_else(|| "This world has no fishing water.".to_string())?;
        let nets = kind
            .nets_position(position, rotation)
            .ok_or_else(|| "The fishing hut has no usable shore-side work point.".to_string())?;
        let nets_ground = terrain.get_height(nets.x, nets.z);
        if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
            return Err("The fishing hut's side route is not safe dry ground.".into());
        }
        let fishing = kind
            .fishing_position(position, rotation)
            .ok_or_else(|| "The pier has no fishing position.".to_string())?;
        if fishing_water_quality(terrain, fishing, rotation, water) <= 0.0 {
            return Err("Rotate or move the hut so its pier reaches broad open water.".into());
        }
    }

    if !resource_plot_is_viable(terrain, hall, kind, position, rotation, colliders, derived) {
        return Err(match kind {
            SettlementBuildingKind::Farmstead => {
                "Workers cannot reach both fields safely from this farmstead."
            }
            SettlementBuildingKind::LumberjackHut => {
                "This hut has no reachable working forest nearby."
            }
            _ => "Builders cannot reach this plot safely.",
        }
        .into());
    }

    let hall_door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    let connected = crate::world::village_roads::hall_connected_road_keys(
        Vec2::new(hall_door.x, hall_door.z),
        roads,
    );
    let road_access = planned_road_access_path(
        terrain,
        hall,
        kind,
        position,
        rotation,
        roads,
        access_blockers,
        &connected,
    )
    .ok_or_else(|| {
        "No dry access lane can connect this doorway to the Hall network.".to_string()
    })?;
    let road_snapped = road_access.last().is_some_and(|point| {
        connected.contains(&crate::world::village_roads::road_point_key(*point))
    });

    let quality = if kind == SettlementBuildingKind::FishermansHut {
        let water = terrain.water_level().unwrap_or(0.0);
        kind.fishing_position(position, rotation)
            .map_or(0.5, |fishing| {
                fishing_water_quality(terrain, fishing, rotation, water)
            })
    } else {
        site_quality(terrain, kind, position)
    };
    Ok(ManualPlotApproval {
        position,
        rotation,
        quality,
        road_access,
        road_snapped,
    })
}

/// Plot-ranking preference, distinct from the completed building's yield.
/// This lets Windmills seek open ground without giving them a fictitious soil
/// percentage or changing Flour throughput.
fn site_placement_suitability(
    terrain: &WorldTerrain,
    kind: SettlementBuildingKind,
    at: Vec3,
) -> f32 {
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        return 0.5;
    };
    let profile = field.resources(at.x, at.z, at.y, 0.0);
    kind.placement_suitability(&profile)
}

/// Steepness at a point, as a rise over the sampling distance.
pub(super) fn slope_at(terrain: &WorldTerrain, x: f32, z: f32) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(x, z);
    let dx = (terrain.get_height(x + STEP, z) - here).abs();
    let dz = (terrain.get_height(x, z + STEP) - here).abs();
    dx.max(dz) / STEP
}

/// Steepest ground a building will accept.
const MAX_BUILD_SLOPE: f32 = 0.30;

/// Maximum vertical cut/fill from a plot's own centre height. These are modest
/// farm terraces, not mountain levelling: each field is graded independently
/// and retains a blended edge into the authored terrain.
const FARMYARD_MAX_CUT_FILL: f32 = 1.75;
const FARM_FIELD_MAX_CUT_FILL: f32 = 2.25;

fn rect_max_cut_fill(terrain: &WorldTerrain, center: Vec2, half: Vec2, rotation: f32) -> f32 {
    let target = terrain.get_height(center.x, center.y);
    rect_max_cut_fill_to(terrain, center, half, rotation, target)
}

fn rect_max_cut_fill_to(
    terrain: &WorldTerrain,
    center: Vec2,
    half: Vec2,
    rotation: f32,
    target: f32,
) -> f32 {
    [-1.0_f32, 0.0, 1.0]
        .into_iter()
        .flat_map(|x| {
            [-1.0_f32, 0.0, 1.0].into_iter().map(move |z| {
                shared::rotation::local_to_world_xz(Vec2::new(half.x * x, half.y * z), rotation)
            })
        })
        .map(|offset| (terrain.get_height(center.x + offset.x, center.y + offset.y) - target).abs())
        .fold(0.0, f32::max)
}

pub(super) fn farmstead_earthwork_effort(
    terrain: &WorldTerrain,
    candidate: Vec3,
    rotation: f32,
) -> Option<f32> {
    let kind = SettlementBuildingKind::Farmstead;
    let definition = kind.art().definition();
    let yard_center = definition.world_footprint_center(candidate, rotation);
    let yard_cut_fill =
        rect_max_cut_fill(terrain, yard_center, definition.footprint * 0.5, rotation);
    if yard_cut_fill > FARMYARD_MAX_CUT_FILL {
        return None;
    }
    let fields = kind.field_positions(candidate, rotation)?;
    let field_half = kind.field_half_extents()?;
    let field_target = fields
        .iter()
        .map(|field| terrain.get_height(field.x, field.z))
        .sum::<f32>()
        / fields.len() as f32;
    let mut total = yard_cut_fill / FARMYARD_MAX_CUT_FILL;
    for field in fields {
        let cut_fill = rect_max_cut_fill_to(
            terrain,
            Vec2::new(field.x, field.z),
            field_half,
            rotation,
            field_target,
        );
        if cut_fill > FARM_FIELD_MAX_CUT_FILL {
            return None;
        }
        total += cut_fill / FARM_FIELD_MAX_CUT_FILL;
    }
    Some(total / 3.0)
}

/// How far above the waterline anything a settlement builds must stand, in metres.
///
/// Not zero: ground exactly at the waterline is shoreline, and a farmstead with
/// its doorstep in the lake reads as a bug even though the maths permitted it.
pub const FREEBOARD: f32 = 1.5;

/// Find a dry Fisherman's Hut plot whose authored rear pier reaches genuine
/// open water.
///
/// This is intentionally geometry-led rather than biome-led. A northern rock
/// coast and a southern dry coast are both viable if there is a safe hut pad,
/// a dry route around the hut, and water beneath the working end of the pier.
/// The returned rotation points the hut's local +Z (its `Anchor_Pier` side)
/// seaward.
pub fn find_fishing_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32, f32)> {
    find_fishing_site_with_limits(terrain, hall, occupied, roads, None, None)
}

/// Keep every authored interaction point on navigable terrain.
///
/// The settlement radius is intentionally allowed to grow, but the active map
/// is still a hard boundary for embodied simulation. Merely checking a plot's
/// centre is not enough near an edge: a valid-looking farmhouse can put its
/// fields outside the map, and a hut can leave its door or pier unreachable.
fn plot_fits_navigation_bounds(
    kind: SettlementBuildingKind,
    candidate: Vec3,
    rotation: f32,
) -> bool {
    let point_is_inside =
        |point: Vec3| point.is_finite() && shared::terrain::world_pos_in_bounds(point.x, point.z);
    let rect_is_inside = |center: Vec3, half: Vec2| {
        [
            Vec2::new(-half.x, -half.y),
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(half.x, half.y),
        ]
        .into_iter()
        .map(|corner| shared::rotation::local_to_world_xz(corner, rotation))
        .all(|offset| {
            shared::terrain::world_pos_in_bounds(center.x + offset.x, center.z + offset.y)
        })
    };

    let definition = kind.art().definition();
    let footprint = definition.footprint * 0.5 + Vec2::splat(0.45);
    let footprint_center = definition.world_footprint_center(candidate, rotation);
    if !point_is_inside(candidate)
        || !rect_is_inside(
            Vec3::new(footprint_center.x, candidate.y, footprint_center.y),
            footprint,
        )
        || !point_is_inside(kind.entrance_position(candidate, rotation))
        || !point_is_inside(shared::components::builder_stand_position(
            candidate,
            rotation,
            definition.footprint.y,
        ))
    {
        return false;
    }

    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(candidate, rotation),
        kind.field_half_extents(),
    ) {
        let reserved_half = field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN);
        if fields
            .into_iter()
            .any(|field| !rect_is_inside(field, reserved_half))
        {
            return false;
        }
    }

    [
        kind.nets_position(candidate, rotation),
        kind.pier_position(candidate, rotation),
        kind.fishing_position(candidate, rotation),
    ]
    .into_iter()
    .flatten()
    .all(point_is_inside)
}

fn find_fishing_site_with_limits(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    minimum_radius_hint: Option<f32>,
    maximum_search_rings: Option<usize>,
) -> Option<(Vec3, f32, f32)> {
    const BEARINGS: usize = 24;
    const FACINGS: usize = 24;
    const RING_STEP: f32 = 4.0;

    let water = terrain.water_level()?;
    let kind = SettlementBuildingKind::FishermansHut;
    let (min_radius, max_radius) = kind.preferred_ring();
    let clearance = kind.clearance();
    let mut radius = minimum_radius_hint
        .map(|hint| hint.clamp(min_radius, max_radius))
        .unwrap_or(min_radius);
    let mut rings_scanned = 0_usize;

    while radius <= max_radius {
        let mut best_at_radius: Option<(Vec3, f32, f32)> = None;
        for i in 0..BEARINGS {
            let turn = (i as f32 + (radius / RING_STEP) * 0.5) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let x = hall.x + angle.cos() * radius;
            let z = hall.z + angle.sin() * radius;
            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(candidate.x - other.x, candidate.z - other.z).length()
                    < clearance + other_clearance
            }) {
                continue;
            }
            let footprint_radius = kind.art().definition().root_footprint_radius() + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                continue;
            }

            for facing in 0..FACINGS {
                let rotation = facing as f32 / FACINGS as f32 * std::f32::consts::TAU;
                if !plot_fits_navigation_bounds(kind, candidate, rotation) {
                    continue;
                }
                if shared::components::minimum_building_water_clearance(
                    terrain, candidate, kind, rotation,
                ) < shared::components::SETTLEMENT_FREEBOARD
                {
                    continue;
                }
                if !crate::world::village_roads::doorway_road_apron_is_dry(
                    terrain, kind, candidate, rotation,
                ) {
                    continue;
                }

                // The side-route anchor is where a fisher rounds the solid
                // building on the way from its front door to its rear pier.
                // It must remain dry and reasonably level with the hut pad.
                let Some(nets) = kind.nets_position(candidate, rotation) else {
                    continue;
                };
                let nets_ground = terrain.get_height(nets.x, nets.z);
                if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
                    continue;
                }

                let Some(fish_spot) = kind.fishing_position(candidate, rotation) else {
                    continue;
                };
                let quality = fishing_water_quality(terrain, fish_spot, rotation, water);
                if quality <= 0.0 {
                    continue;
                }
                let stand = shared::components::builder_stand_position(
                    candidate,
                    rotation,
                    kind.art().definition().footprint.y,
                );
                if !crate::world::village_roads::embodied_land_route_exists(terrain, hall, stand) {
                    continue;
                }
                let replace = best_at_radius
                    .as_ref()
                    .is_none_or(|(_, _, best_quality)| quality > *best_quality);
                if replace {
                    best_at_radius = Some((candidate, rotation, quality));
                }
            }
        }
        if best_at_radius.is_some() {
            return best_at_radius;
        }
        rings_scanned += 1;
        if maximum_search_rings.is_some_and(|maximum| rings_scanned >= maximum) {
            break;
        }
        radius += RING_STEP;
    }
    None
}

/// Spend at most one shoreline ring per live permit decision and resume on the
/// next permit tick. Once every ring is exhausted, only a terrain edit can
/// create new coast; monotonically added houses and roads cannot make an
/// occupied shoreline freer than it was before.
fn find_incremental_fishing_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    settlement: Entity,
    clock: &mut VillageClock,
) -> Option<(Vec3, f32, f32)> {
    const RING_STEP: f32 = 4.0;
    let kind = SettlementBuildingKind::FishermansHut;
    let (min_radius, max_radius) = kind.preferred_ring();
    let terrain_version = terrain.modification_version();
    if clock
        .failed_fishing_terrain_versions
        .get(&settlement)
        .is_some_and(|failed_version| *failed_version == terrain_version)
    {
        return None;
    }
    if clock
        .failed_fishing_terrain_versions
        .remove(&settlement)
        .is_some()
    {
        clock
            .site_search_radii
            .insert((settlement, kind), min_radius);
    }
    if terrain.water_level().is_none() {
        clock
            .failed_fishing_terrain_versions
            .insert(settlement, terrain_version);
        return None;
    }
    let radius = clock
        .site_search_radii
        .get(&(settlement, kind))
        .copied()
        .unwrap_or(min_radius)
        .clamp(min_radius, max_radius);
    let site = find_fishing_site_with_limits(terrain, hall, occupied, roads, Some(radius), Some(1));
    if site.is_some() {
        clock.site_search_radii.insert((settlement, kind), radius);
        clock.failed_fishing_terrain_versions.remove(&settlement);
    } else if radius < max_radius {
        clock
            .site_search_radii
            .insert((settlement, kind), (radius + RING_STEP).min(max_radius));
    } else {
        clock
            .failed_fishing_terrain_versions
            .insert(settlement, terrain_version);
    }
    site
}

/// Rejecting one otherwise valid shoreline plot must not poison the whole
/// coast. Advance to the next bounded ring after its final road/access proof
/// fails; `false` means the rejected plot was already on the last ring.
fn advance_incremental_fishing_search(clock: &mut VillageClock, settlement: Entity) -> bool {
    const RING_STEP: f32 = 4.0;
    let kind = SettlementBuildingKind::FishermansHut;
    let (minimum, maximum) = kind.preferred_ring();
    let cursor = clock
        .site_search_radii
        .entry((settlement, kind))
        .or_insert(minimum);
    if *cursor >= maximum {
        return false;
    }
    *cursor = (*cursor + RING_STEP).min(maximum);
    clock.failed_fishing_terrain_versions.remove(&settlement);
    true
}

/// Score water around the working end of the authored pier. Zero means the
/// three seaward samples are not all submerged, so the layout would visibly
/// terminate on land. Non-zero values reward deeper, broader water without
/// making oceans categorically better than rivers or lakes.
fn fishing_water_quality(
    terrain: &WorldTerrain,
    fish_spot: Vec3,
    rotation: f32,
    water: f32,
) -> f32 {
    let mut depth_score = 0.0;
    let mut samples = 0.0;
    for forward in [-1.0_f32, 0.75, 2.5] {
        for side in [-1.4_f32, 0.0, 1.4] {
            let offset = shared::rotation::local_to_world_xz(Vec2::new(side, forward), rotation);
            let ground = terrain.get_height(fish_spot.x + offset.x, fish_spot.z + offset.y);
            let depth = water - ground;
            // The outer row is load-bearing: a pier whose tip merely touches a
            // shallow puddle is not a fishing site.
            if forward >= 2.5 && depth < 0.18 {
                return 0.0;
            }
            depth_score += (depth / 2.5).clamp(0.0, 1.0);
            samples += 1.0;
        }
    }
    // Require the actual authored standing point to be above water, too.
    let tip_depth = water - terrain.get_height(fish_spot.x, fish_spot.z);
    if tip_depth < 0.12 {
        return 0.0;
    }
    (0.35 + 0.65 * depth_score / samples).clamp(0.35, 1.0)
}

/// Deterministic fallback search for a legal building plot.
///
/// Walks outward in rings from the hall, sampling a fixed number of bearings
/// per ring, and takes the first spot that is flat enough and clear of what is
/// already there. Deterministic on purpose: the same village in the same state
/// makes the same choice, so a bug is reproducible rather than a story about
/// what happened once.
///
/// Production permits call `find_site_with_plan` with the settlement charter,
/// obstacle indexes and adjacency context. This public no-context wrapper is a
/// conservative baseline used by geometry and road regression tests; it still
/// treats completed roads as occupied infrastructure.
pub fn find_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32)> {
    find_site_with_plan(
        terrain,
        hall,
        kind,
        occupied,
        roads,
        &[],
        &[],
        None,
        None,
        None,
        None,
        None,
    )
}

#[derive(Clone, Copy, Debug)]
struct PlannedPlotCandidate {
    /// Building centre relative to the Moot Hall.
    local: Vec2,
    /// Point on the intended lane, street, avenue, or neighbourhood green that
    /// the building's door should face.
    frontage: Vec2,
}

fn plan_axis(plan: &shared::components::SettlementDevelopment) -> (Vec2, Vec2) {
    let fraction = (plan.plan_seed.rotate_right(29) & 0xffff) as f32 / 65_535.0;
    let angle = fraction * std::f32::consts::TAU;
    let axis = Vec2::new(angle.cos(), angle.sin());
    (axis, Vec2::new(-axis.y, axis.x))
}

fn plan_center_offset(
    plan: &shared::components::SettlementDevelopment,
    axis: Vec2,
    side: Vec2,
) -> Vec2 {
    use shared::components::SettlementCenterStyle as Center;
    let handedness = if plan.plan_seed.rotate_right(17) & 1 == 0 {
        1.0
    } else {
        -1.0
    };
    match plan.center {
        Center::Green => side * handedness * 8.0,
        Center::Square => Vec2::ZERO,
        Center::Avenue => axis * 6.0,
        Center::Courtyard => (axis + side * handedness) * 5.0,
    }
}

/// Convert one deterministic search sample into a recognisable piece of the
/// settlement's street grammar. Buildings are placed BESIDE an implied street
/// and face that street. The road builder later surveys from the authored door
/// to the existing network, turning this inexpensive plan into real terrain-
/// aware paths without pre-baking a whole city.
fn planned_plot_candidate(
    plan: &shared::components::SettlementDevelopment,
    kind: SettlementBuildingKind,
    radius: f32,
    bearing: usize,
    base: Vec2,
) -> PlannedPlotCandidate {
    use shared::components::SettlementLayoutStyle as Style;

    let (axis, side) = plan_axis(plan);
    let centre = plan_center_offset(plan, axis, side);
    let handedness = if plan.plan_seed & 1 == 0 { 1.0 } else { -1.0 };
    let side_sign = if bearing & 1 == 0 { 1.0 } else { -1.0 };

    match plan.layout {
        Style::Organic => {
            // Three gently wandering lanes. Houses occupy alternating verges
            // instead of forming a ring around the hall.
            let branch = bearing % 3;
            let branch_angle = (plan.plan_seed.rotate_right(7) & 0xffff) as f32 / 65_535.0
                * std::f32::consts::TAU
                + branch as f32 * std::f32::consts::TAU / 3.0
                + (radius * 0.085 + branch as f32).sin() * 0.18;
            let direction = Vec2::new(branch_angle.cos(), branch_angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let bend = normal
                * (radius * 0.11 + bearing as f32 + (plan.plan_seed & 31) as f32 * 0.03).sin()
                * 5.5;
            let frontage = centre + direction * radius + bend;
            let setback = 8.5 + (bearing / 6) as f32 * 3.0;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * setback,
                frontage,
            }
        }
        Style::Radial => {
            // Buildings front the sides of several spokes, not the Moot Hall.
            let branches = 5 + (plan.plan_seed.rotate_right(13) & 1) as usize;
            let branch = (bearing / 2) % branches;
            let angle =
                axis.y.atan2(axis.x) + branch as f32 * std::f32::consts::TAU / branches as f32;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let frontage = centre + direction * radius;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * 9.0,
                frontage,
            }
        }
        Style::Grid => {
            // Seed-rotated orthogonal streets. Each sample is a building set
            // back from the nearest grid line with its facade parallel to it.
            const BLOCK: f32 = 26.0;
            const FRONTAGE_STEP: f32 = 11.0;
            const SETBACK: f32 = 9.0;
            let along = base.dot(axis);
            let across = base.dot(side);
            if bearing & 1 == 0 {
                let street_across = (across / BLOCK).round() * BLOCK;
                let frontage = centre
                    + axis * ((along / FRONTAGE_STEP).round() * FRONTAGE_STEP)
                    + side * street_across;
                let verge = if (across - street_across).abs() > 0.5 {
                    (across - street_across).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + side * verge * SETBACK,
                    frontage,
                }
            } else {
                let street_along = (along / BLOCK).round() * BLOCK;
                let frontage = centre
                    + axis * street_along
                    + side * ((across / FRONTAGE_STEP).round() * FRONTAGE_STEP);
                let verge = if (along - street_along).abs() > 0.5 {
                    (along - street_along).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + axis * verge * SETBACK,
                    frontage,
                }
            }
        }
        Style::Avenue => {
            // A long civic spine with buildings on both sides. Farther rings
            // extend the avenue rather than inflating another circle.
            let mut along = base.dot(axis) * 1.35;
            if along.abs() < 10.0 {
                along = side_sign * radius * 0.8;
            }
            let avenue = centre + axis * along;
            let verge = if base.dot(side).abs() > 0.5 {
                base.dot(side).signum()
            } else {
                side_sign * handedness
            };
            PlannedPlotCandidate {
                local: avenue + side * verge * (12.0 + (bearing / 8) as f32 * 5.0),
                frontage: avenue,
            }
        }
        Style::Polycentric => {
            // Three persistent neighbourhood centres. Workplaces sit in the
            // looser outer clusters while homes/civic buildings fill the near
            // neighbourhoods.
            let district = bearing % 3;
            let district_angle =
                axis.y.atan2(axis.x) + handedness * district as f32 * std::f32::consts::TAU / 3.0;
            let district_direction = Vec2::new(district_angle.cos(), district_angle.sin());
            let district_distance = match kind {
                SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::FishermansHut => 44.0,
                _ => 30.0,
            };
            let district_centre = centre + district_direction * district_distance;
            let orbit_angle = district_angle
                + std::f32::consts::FRAC_PI_2
                + (bearing / 3) as f32 * 0.75
                + radius * 0.035;
            let orbit = Vec2::new(orbit_angle.cos(), orbit_angle.sin())
                * (9.0 + ((radius / 6.0) as usize & 1) as f32 * 5.0);
            PlannedPlotCandidate {
                local: district_centre + orbit,
                frontage: district_centre,
            }
        }
    }
}

fn rotation_facing_frontage(building: Vec2, frontage: Vec2) -> f32 {
    // Building doors are authored on local -Z. Rotating local -Z toward the
    // target requires the vector FROM the target back to the building.
    let outward = building - frontage;
    outward.x.atan2(outward.y)
}

fn closest_point_on_segment(point: Vec2, start: Vec2, end: Vec2) -> Vec2 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= 1e-6 {
        return start;
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    start + segment * t
}

fn nearest_completed_road_frontage(candidate: Vec2, roads: &[&VillageRoad]) -> Option<Vec2> {
    roads
        .iter()
        .filter(|road| road.is_complete())
        .flat_map(|road| road.built_points().windows(2))
        .map(|pair| closest_point_on_segment(candidate, pair[0], pair[1]))
        .min_by(|a, b| {
            a.distance_squared(candidate)
                .total_cmp(&b.distance_squared(candidate))
        })
        .filter(|frontage| frontage.distance_squared(candidate) <= 48.0 * 48.0)
}

fn nearest_completed_road_access_point(
    candidate: Vec2,
    roads: &[&VillageRoad],
    blockers: &[RoadAccessBlocker],
    connected_keys: &HashSet<(i32, i32)>,
) -> Option<Vec2> {
    let mut best = None;
    let mut best_distance = f32::INFINITY;
    for point in roads
        .iter()
        .filter(|road| road.is_complete())
        .flat_map(|road| road.built_points().iter().copied())
    {
        if !connected_keys.contains(&crate::world::village_roads::road_point_key(point)) {
            continue;
        }
        let distance = point.distance_squared(candidate);
        // Only a new nearest point can replace the result. Testing every
        // farther road node against every building turned this O(road nodes ×
        // buildings) in a mature village even though almost all nodes could
        // never win.
        if distance >= best_distance || blockers.iter().any(|blocker| blocker.contains(point)) {
            continue;
        }
        best = Some(point);
        best_distance = distance;
    }
    best
}

#[derive(Clone, Copy)]
pub(crate) struct RoadAccessBlocker {
    center: Vec2,
    half: Vec2,
    rotation: f32,
}

impl RoadAccessBlocker {
    fn contains(self, point: Vec2) -> bool {
        let local = shared::rotation::world_to_local_xz(point - self.center, self.rotation);
        local.x.abs() <= self.half.x && local.y.abs() <= self.half.y
    }

    fn blocks_segment(self, start: Vec2, end: Vec2) -> bool {
        shared::spatial::segment_intersects_box_after_start(
            shared::rotation::world_to_local_xz(start - self.center, self.rotation),
            shared::rotation::world_to_local_xz(end - self.center, self.rotation),
            self.half,
        )
    }
}

pub(crate) fn road_access_blockers_for_plot(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
) -> Vec<RoadAccessBlocker> {
    let road_margin = RoadClass::Lane.initial_reserved_width() * 0.5 + 0.45;
    let definition = kind.art().definition();
    let mut blockers = vec![RoadAccessBlocker {
        center: definition.world_footprint_center(position, rotation),
        half: definition.footprint * 0.5 + Vec2::splat(road_margin),
        rotation,
    }];
    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        blockers.extend(fields.into_iter().map(|field| RoadAccessBlocker {
            center: Vec2::new(field.x, field.z),
            half: field_half
                + Vec2::splat(road_margin + shared::components::FARM_FIELD_TERRACE_MARGIN),
            rotation,
        }));
    }
    blockers
}

pub(super) fn planned_road_access_path(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    roads: &[&VillageRoad],
    blockers: &[RoadAccessBlocker],
    connected_keys: &HashSet<(i32, i32)>,
) -> Option<Vec<Vec2>> {
    const CELL: f32 = 4.0;
    const PADDING: f32 = 28.0;
    const MAX_NODES: usize = 2_000;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct Cell {
        x: i32,
        z: i32,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Open {
        estimate: i32,
        cell: Cell,
    }

    impl Ord for Open {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other
                .estimate
                .cmp(&self.estimate)
                .then_with(|| self.cell.x.cmp(&other.cell.x))
                .then_with(|| self.cell.z.cmp(&other.cell.z))
        }
    }

    impl PartialOrd for Open {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let (door, approach) = crate::world::village_roads::doorway_approach(kind, position, rotation);
    let (goal, hall_door) = if let Some(frontage) =
        nearest_completed_road_access_point(approach, roads, blockers, connected_keys)
    {
        (frontage, None)
    } else {
        let (hall_door, hall_approach) =
            crate::world::village_roads::doorway_approach(SettlementBuildingKind::Hall, hall, 0.0);
        (hall_approach, Some(hall_door))
    };
    let reserved_width = RoadClass::Lane.initial_reserved_width();
    let search_min = approach.min(goal) - Vec2::splat(PADDING);
    let search_max = approach.max(goal) + Vec2::splat(PADDING);
    let local_blockers: Vec<_> = blockers
        .iter()
        .copied()
        .filter(|blocker| {
            let radius = blocker.half.length();
            blocker.center.x + radius >= search_min.x
                && blocker.center.y + radius >= search_min.y
                && blocker.center.x - radius <= search_max.x
                && blocker.center.y - radius <= search_max.y
        })
        .collect();
    let mut local_blockers = local_blockers;
    // The permit is deciding where a future shell will stand. Existing plots
    // alone are not enough: without the proposed shell here, A* can leave the
    // authored apron and bend straight back through the cabin's future floor.
    // Construction then quite correctly sees that shell and rejects the same
    // reserved path forever. Match the road survey's source-building margin;
    // crop plots retain the wider permanent reservation below.
    let proposed_definition = kind.art().definition();
    local_blockers.push(RoadAccessBlocker {
        center: proposed_definition.world_footprint_center(position, rotation),
        half: proposed_definition.footprint * 0.5
            + Vec2::splat(crate::world::village_roads::VILLAGE_ROAD_WIDTH * 0.5 + 0.45),
        rotation,
    });
    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        let field_margin =
            reserved_width * 0.5 + 0.45 + shared::components::FARM_FIELD_TERRACE_MARGIN;
        local_blockers.extend(fields.into_iter().map(|field| RoadAccessBlocker {
            center: Vec2::new(field.x, field.z),
            half: field_half + Vec2::splat(field_margin),
            rotation,
        }));
    }
    // The hall is not a SettlementBuilding and therefore is absent from the
    // caller's ordinary blocker list. It must remain solid even when this plot
    // joins an existing street: the geometrically nearest street point can be
    // on the far side of the Hall, and a direct line to it would otherwise
    // reserve and later draw a road through the civic building. Only the
    // explicit hall-approach-to-door segment below may enter this margin.
    let non_hall_blockers = local_blockers.len();
    let road_margin = reserved_width * 0.5 + 0.45;
    local_blockers.push(RoadAccessBlocker {
        center: shared::components::CivicHallLevel::reserved_world_center(hall, 0.0),
        half: shared::components::CivicHallLevel::reserved_half_extents()
            + Vec2::splat(road_margin),
        rotation: 0.0,
    });
    let edge_has_walkable_slope = |start: Vec2, end: Vec2| {
        let steps = (start.distance(end) / crate::world::navgrid::NAVIGATION_SAMPLE_STEP)
            .ceil()
            .max(1.0) as usize;
        let mut previous_height = None;
        (0..=steps).all(|step| {
            let point = start.lerp(end, step as f32 / steps as f32);
            let height = terrain.get_height(point.x, point.y);
            let slope_clear =
                previous_height.is_none_or(|previous: f32| (height - previous).abs() <= 0.47);
            previous_height = Some(height);
            slope_clear
        })
    };
    let edge_is_coarsely_clear = |start: Vec2, end: Vec2| {
        crate::world::village_roads::road_segment_is_coarsely_dry_at_width(
            terrain,
            start,
            end,
            reserved_width,
        ) && local_blockers
            .iter()
            .all(|blocker| !blocker.blocks_segment(start, end))
            && edge_has_walkable_slope(start, end)
    };

    let mut route = if crate::world::village_roads::road_segment_is_dry_at_width(
        terrain,
        approach,
        goal,
        reserved_width,
    ) && local_blockers
        .iter()
        .all(|blocker| !blocker.blocks_segment(approach, goal))
        && edge_has_walkable_slope(approach, goal)
    {
        vec![approach, goal]
    } else {
        let min = search_min;
        let max = search_max;
        let cell_for = |point: Vec2| Cell {
            x: (point.x / CELL).round() as i32,
            z: (point.y / CELL).round() as i32,
        };
        let point_for = |cell: Cell| Vec2::new(cell.x as f32 * CELL, cell.z as f32 * CELL);
        let heuristic =
            |a: Cell, b: Cell| Vec2::new((a.x - b.x) as f32, (a.z - b.z) as f32).length();
        let start_cell = cell_for(approach);
        let goal_cell = cell_for(goal);
        let mut open = std::collections::BinaryHeap::new();
        let mut closed = HashSet::new();
        let mut scores = HashMap::new();
        let mut came_from = HashMap::new();
        scores.insert(start_cell, 0.0_f32);
        open.push(Open {
            estimate: (heuristic(start_cell, goal_cell) * 1_000.0) as i32,
            cell: start_cell,
        });
        let mut found = None;
        while let Some(Open { cell: current, .. }) = open.pop() {
            if !closed.insert(current) || closed.len() > MAX_NODES {
                continue;
            }
            let current_point = if current == start_cell {
                approach
            } else {
                point_for(current)
            };
            if current_point.distance(goal) <= CELL * 1.6
                && edge_is_coarsely_clear(current_point, goal)
            {
                found = Some(current);
                break;
            }
            let current_score = scores.get(&current).copied().unwrap_or(f32::INFINITY);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let next = Cell {
                        x: current.x + dx,
                        z: current.z + dz,
                    };
                    if closed.contains(&next) {
                        continue;
                    }
                    let next_point = point_for(next);
                    if next_point.x < min.x
                        || next_point.y < min.y
                        || next_point.x > max.x
                        || next_point.y > max.y
                        || !edge_is_coarsely_clear(current_point, next_point)
                    {
                        continue;
                    }
                    let step_cost = if dx != 0 && dz != 0 {
                        std::f32::consts::SQRT_2
                    } else {
                        1.0
                    };
                    let tentative = current_score + step_cost;
                    if tentative >= scores.get(&next).copied().unwrap_or(f32::INFINITY) {
                        continue;
                    }
                    scores.insert(next, tentative);
                    came_from.insert(next, current);
                    open.push(Open {
                        estimate: ((tentative + heuristic(next, goal_cell)) * 1_000.0) as i32,
                        cell: next,
                    });
                }
            }
        }
        let mut cursor = found?;
        let mut cells = vec![cursor];
        while let Some(previous) = came_from.get(&cursor).copied() {
            cells.push(previous);
            cursor = previous;
        }
        cells.reverse();
        let mut path = vec![approach];
        path.extend(cells.into_iter().skip(1).map(point_for));
        if path
            .last()
            .is_none_or(|point| point.distance_squared(goal) > 0.01)
        {
            path.push(goal);
        }
        // Coarse terrain sampling belongs only inside the search. Approval is
        // authoritative: certify the chosen four-metre polyline at the same
        // width and 20 cm spacing that physical road construction uses.
        if !crate::world::village_roads::road_corridor_is_dry(terrain, &path, reserved_width) {
            return None;
        }
        path
    };

    route.insert(0, door);
    if let Some(hall_door) = hall_door {
        if !crate::world::village_roads::road_segment_is_dry_at_width(
            terrain,
            goal,
            hall_door,
            reserved_width,
        ) || local_blockers[..non_hall_blockers]
            .iter()
            .any(|blocker| blocker.blocks_segment(goal, hall_door))
        {
            return None;
        }
        route.push(hall_door);
    }
    route.dedup_by(|a, b| a.distance_squared(*b) <= 0.01);
    if !crate::world::village_roads::road_corridor_is_dry(terrain, &route, reserved_width) {
        return None;
    }
    if !route
        .windows(2)
        .all(|segment| edge_has_walkable_slope(segment[0], segment[1]))
    {
        return None;
    }
    Some(route)
}

fn direct_road_access_is_coarsely_clear(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    roads: &[&VillageRoad],
    blockers: &[RoadAccessBlocker],
    connected_keys: &HashSet<(i32, i32)>,
) -> bool {
    let (_, approach) = crate::world::village_roads::doorway_approach(kind, position, rotation);
    let goal = nearest_completed_road_access_point(approach, roads, blockers, connected_keys)
        .unwrap_or_else(|| {
            crate::world::village_roads::doorway_approach(SettlementBuildingKind::Hall, hall, 0.0).1
        });
    crate::world::village_roads::road_segment_is_coarsely_dry_at_width(
        terrain,
        approach,
        goal,
        RoadClass::Lane.initial_reserved_width(),
    ) && blockers
        .iter()
        .all(|blocker| !blocker.blocks_segment(approach, goal))
}

#[cfg(test)]
mod road_access_tests {
    use super::*;

    #[test]
    fn processing_permits_require_completed_upstream_industries() {
        let mut completed = HashMap::new();
        assert!(!processing_upstream_is_complete(
            SettlementBuildingKind::Windmill,
            &completed
        ));
        completed.insert(SettlementBuildingKind::Farmstead, 1);
        assert!(processing_upstream_is_complete(
            SettlementBuildingKind::Windmill,
            &completed
        ));
        assert!(!processing_upstream_is_complete(
            SettlementBuildingKind::Bakery,
            &completed
        ));
        completed.insert(SettlementBuildingKind::Windmill, 1);
        assert!(processing_upstream_is_complete(
            SettlementBuildingKind::Bakery,
            &completed
        ));
    }

    #[test]
    fn resource_search_cursor_expands_beyond_the_preferred_layout_band() {
        assert_eq!(
            include_resumable_search_cursor(54.0, 120.0, Some(246.0)),
            (246.0, 246.0),
            "an outward retry must inspect its real cursor rather than resampling 120 m"
        );
        assert_eq!(
            include_resumable_search_cursor(54.0, 120.0, Some(999.0)),
            (MAX_SETTLEMENT_SEARCH_RADIUS, MAX_SETTLEMENT_SEARCH_RADIUS),
            "physical settlement search remains bounded"
        );
    }

    #[test]
    fn permit_access_routes_around_the_future_town_hall_shell() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        // This cabin is behind the civic centre. A direct connector to the
        // permanent front door would cross the rear half of the future Town
        // Hall even though only the smaller Moot Hall is visible today.
        let position = Vec3::new(1700.0, terrain.get_height(1700.0, 30.0), 30.0);
        let route = planned_road_access_path(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            position,
            0.0,
            &[],
            &[],
            &HashSet::new(),
        )
        .expect("the connector should bend around reserved civic ground");

        let reserved = RoadAccessBlocker {
            center: shared::components::CivicHallLevel::reserved_world_center(hall, 0.0),
            half: shared::components::CivicHallLevel::reserved_half_extents(),
            rotation: 0.0,
        };
        assert!(route.len() > 4, "the direct road was not bent: {route:?}");
        assert!(
            route
                .windows(2)
                .all(|segment| !reserved.blocks_segment(segment[0], segment[1])),
            "a permit reserved road through the future Town Hall: {route:?}"
        );
    }

    #[test]
    fn permit_access_bends_around_an_existing_building_and_remains_authoritative() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        let position = Vec3::new(1744.0, terrain.get_height(1744.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let rotation = 0.0;
        let (_, start) = crate::world::village_roads::doorway_approach(kind, position, rotation);
        let (_, goal) =
            crate::world::village_roads::doorway_approach(SettlementBuildingKind::Hall, hall, 0.0);
        let blocker = RoadAccessBlocker {
            center: start.lerp(goal, 0.5),
            half: Vec2::splat(5.0),
            rotation: 0.0,
        };
        let route = planned_road_access_path(
            &terrain,
            hall,
            kind,
            position,
            rotation,
            &[],
            &[blocker],
            &HashSet::new(),
        )
        .expect("a dry access reservation should bend around the occupied shell");

        assert!(
            route.len() > 4,
            "the blocked direct line must become a bend"
        );
        assert!(route
            .windows(2)
            .all(|segment| !blocker.blocks_segment(segment[0], segment[1])));
        let future_definition = kind.art().definition();
        let future_shell = RoadAccessBlocker {
            center: future_definition.world_footprint_center(position, rotation),
            half: future_definition.footprint * 0.5
                + Vec2::splat(crate::world::village_roads::VILLAGE_ROAD_WIDTH * 0.5 + 0.45),
            rotation,
        };
        assert!(
            route
                .windows(2)
                .skip(1)
                .all(|segment| !future_shell.blocks_segment(segment[0], segment[1])),
            "only the authored door apron may touch the proposed shell: {route:?}",
        );
        assert!(crate::world::village_roads::road_corridor_is_dry(
            &terrain,
            &route,
            RoadClass::Lane.initial_reserved_width(),
        ));
    }

    #[test]
    fn permit_access_never_anchors_on_a_detached_road_island() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        let position = Vec3::new(1744.0, terrain.get_height(1744.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let rotation = 0.0;
        let (_, approach) = crate::world::village_roads::doorway_approach(kind, position, rotation);
        let detached = VillageRoad {
            settlement: "Island".into(),
            builder: "Old builder".into(),
            points: vec![approach + Vec2::X * 4.0, approach + Vec2::X * 12.0],
            built_through: 2,
            width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: shared::components::RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        let roads = [&detached];
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
        let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
        let connected = crate::world::village_roads::hall_connected_road_keys(hall_door, &roads);

        let route = planned_road_access_path(
            &terrain,
            hall,
            kind,
            position,
            rotation,
            &roads,
            &[],
            &connected,
        )
        .expect("the plot should fall back to the Moot Hall component");

        assert!(route.last().unwrap().distance_squared(hall_door) <= 0.01);
        assert!(route.last().unwrap().distance_squared(detached.points[0]) > 0.01);
    }

    #[test]
    fn manual_house_plot_uses_the_authoritative_access_and_charter_rules() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        let position = Vec3::new(1744.0, terrain.get_height(1744.0, 0.0), 0.0);
        let approval = validate_manual_plot(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            position,
            0.0,
            &[(hall, SettlementBuildingKind::Hall.clearance())],
            &[],
            &[],
            &[],
            None,
            None,
        )
        .expect("a dry nearby player plot should reserve a real Hall connector");
        assert!(approval.road_access.len() >= 2);
        assert_eq!(
            approval.position.y,
            terrain.get_height(position.x, position.z)
        );

        let outside = Vec3::new(hall.x + MAX_SETTLEMENT_SEARCH_RADIUS + 1.0, 0.0, hall.z);
        let rejection = validate_manual_plot(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            outside,
            0.0,
            &[(hall, SettlementBuildingKind::Hall.clearance())],
            &[],
            &[],
            &[],
            None,
            None,
        )
        .unwrap_err();
        assert!(rejection.contains("charter"));
    }

    #[test]
    fn exhausted_fishing_coast_advances_one_ring_then_sleeps_until_terrain_changes() {
        let terrain = WorldTerrain::default();
        assert!(terrain.water_level().is_some());
        let hall = Vec3::new(1700.0, terrain.get_height(1700.0, 0.0), 0.0);
        // One deliberately enormous occupied plot makes every shoreline
        // candidate fail before its facing checks. The assertion here is the
        // live cursor contract, independent of a particular generated coast.
        let occupied = vec![(hall, 1_000.0)];
        let settlement = Entity::from_bits(9);
        let kind = SettlementBuildingKind::FishermansHut;
        let (minimum, maximum) = kind.preferred_ring();
        let rings = ((maximum - minimum) / 4.0).floor() as usize + 1;
        let mut clock = VillageClock::default();

        for _ in 0..rings {
            assert!(find_incremental_fishing_site(
                &terrain,
                hall,
                &occupied,
                &[],
                settlement,
                &mut clock,
            )
            .is_none());
        }

        assert_eq!(
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some(maximum),
        );
        assert_eq!(
            clock
                .failed_fishing_terrain_versions
                .get(&settlement)
                .copied(),
            Some(terrain.modification_version()),
        );
        // The next permit decision returns from the exhausted-coast cache and
        // does not restart at the founding ring.
        assert!(find_incremental_fishing_site(
            &terrain,
            hall,
            &occupied,
            &[],
            settlement,
            &mut clock,
        )
        .is_none());
        assert_eq!(
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some(maximum),
        );
    }

    #[test]
    fn rejected_fishing_access_advances_instead_of_poisoning_the_coast() {
        let settlement = Entity::from_bits(10);
        let kind = SettlementBuildingKind::FishermansHut;
        let (minimum, maximum) = kind.preferred_ring();
        let mut clock = VillageClock::default();
        clock.site_search_radii.insert((settlement, kind), minimum);
        clock.failed_fishing_terrain_versions.insert(settlement, 7);

        assert!(advance_incremental_fishing_search(&mut clock, settlement));
        assert_eq!(
            clock.site_search_radii.get(&(settlement, kind)).copied(),
            Some((minimum + 4.0).min(maximum))
        );
        assert!(!clock
            .failed_fishing_terrain_versions
            .contains_key(&settlement));

        clock.site_search_radii.insert((settlement, kind), maximum);
        assert!(!advance_incremental_fishing_search(&mut clock, settlement));
    }
}

#[allow(clippy::too_many_arguments)]
fn resource_plot_is_viable(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    candidate: Vec3,
    rotation: f32,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    let stand = shared::components::builder_stand_position(
        candidate,
        rotation,
        kind.art().definition().footprint.y,
    );
    let hall_entrance = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    // The reserved road proves that a future connector can reach the door;
    // material delivery starts before that road exists. Certify the builder's
    // current land route as well so construction cannot create a circular
    // dependency in which the unreachable shell must finish before its access
    // road may be built.
    if !crate::world::village_roads::permit_land_route_exists(terrain, hall_entrance, stand)
        || colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::navigation_point_is_clear_of_props(
                Vec2::new(stand.x, stand.z),
                colliders,
                derived,
            )
        })
    {
        return false;
    }
    if kind == SettlementBuildingKind::Farmstead
        && kind
            .field_positions(candidate, rotation)
            .is_some_and(|fields| {
                fields.into_iter().enumerate().any(|(index, field)| {
                    crate::world::village_roads::reachable_farm_work_stand(
                        terrain,
                        candidate,
                        rotation,
                        field,
                        index as u32,
                        None,
                        // The Farmstead land claim clears trees from both crop
                        // plots before the fields are planted. Permanent props
                        // were rejected by the rectangle check above, so prove
                        // the future post-earthwork route rather than rejecting
                        // a field because of a tree that the builders remove.
                        None,
                        None,
                    )
                    .is_none()
                })
            })
    {
        return false;
    }
    kind != SettlementBuildingKind::LumberjackHut
        || lumber_plot_has_reachable_tree(terrain, kind.entrance_position(candidate, rotation))
}

pub(super) fn find_site_with_plan(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    planned_accesses: &[PlannedRoadAccess],
    access_blockers: &[RoadAccessBlocker],
    development: Option<&shared::components::SettlementDevelopment>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    minimum_radius_hint: Option<f32>,
    maximum_search_rings: Option<usize>,
) -> Option<(Vec3, f32)> {
    find_site_with_plan_diagnostics(
        terrain,
        hall,
        kind,
        occupied,
        roads,
        planned_accesses,
        access_blockers,
        development,
        colliders,
        derived,
        minimum_radius_hint,
        maximum_search_rings,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn find_site_with_plan_diagnostics(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    planned_accesses: &[PlannedRoadAccess],
    access_blockers: &[RoadAccessBlocker],
    development: Option<&shared::components::SettlementDevelopment>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    minimum_radius_hint: Option<f32>,
    maximum_search_rings: Option<usize>,
    mut rejections: Option<&mut SiteSearchRejections>,
) -> Option<(Vec3, f32)> {
    macro_rules! reject {
        ($field:ident) => {
            if let Some(rejections) = rejections.as_deref_mut() {
                rejections.$field = rejections.$field.saturating_add(1);
            }
        };
    }
    const SEEDED_BEARINGS: usize = 12;
    const FALLBACK_BEARINGS: usize = 48;
    const RING_STEP: f32 = 6.0;
    const RESOURCE_PLOT_SHORTLIST: usize = 3;
    // Farms and timber plots still compare several directions and three
    // successive distance bands. Searching every ring out to an expanding
    // 320 m city boundary merely to replace a six-entry shortlist made one
    // permit decision monopolise a server tick in mature settlements.
    const RESOURCE_SEARCH_BANDS: usize = 3;

    let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    let connected_road_keys = crate::world::village_roads::hall_connected_road_keys(
        Vec2::new(hall_door3.x, hall_door3.z),
        roads,
    );

    let (mut min_radius, mut max_radius) = kind.preferred_ring();
    // The authored rings are the attractive founding core, not a hard city
    // boundary. Six buildings and their fields can fill that core quickly;
    // widen future search bands deterministically as the occupied envelope
    // grows so a migration wave cannot strand everyone after the sixth cabin.
    let expansion_bands = occupied.len().saturating_sub(6).div_ceil(6) as f32;
    let expansion_per_band = match kind {
        SettlementBuildingKind::House
        | SettlementBuildingKind::Market
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church
        | SettlementBuildingKind::Bakery
        | SettlementBuildingKind::StorageHall => 18.0,
        SettlementBuildingKind::Farmstead
        | SettlementBuildingKind::LumberjackHut
        | SettlementBuildingKind::FishermansHut
        | SettlementBuildingKind::Windmill
        | SettlementBuildingKind::StoneQuarry => 12.0,
        SettlementBuildingKind::Hall => 0.0,
    };
    max_radius =
        (max_radius + expansion_bands * expansion_per_band).min(MAX_SETTLEMENT_SEARCH_RADIUS);
    if let Some(plan) = development {
        use shared::components::SettlementCenterStyle as Center;
        if kind == SettlementBuildingKind::House {
            // Preserve the selected civic centre from the first cabin onward.
            min_radius = min_radius.max(match plan.center {
                Center::Green => 24.0,
                Center::Square => 27.0,
                Center::Avenue => 19.0,
                Center::Courtyard => 26.0,
            });
        }
    }
    // Monotonic construction means a ring exhausted before the previous
    // success cannot become less occupied. Keep testing the successful ring
    // (there may be unused bearings), then continue outward. The cursor also
    // expands the preferred band: it represents physical search progress, not
    // merely a ranking hint inside the founding layout radius.
    (min_radius, max_radius) =
        include_resumable_search_cursor(min_radius, max_radius, minimum_radius_hint);
    let clearance = kind.clearance();
    let resource_scored = matches!(
        kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::StoneQuarry
    );
    // The authored grammar needs only its stable lane/spoke samples. If that
    // preferred pass is exhausted, the open-land fallback deliberately probes
    // between those streets as well; twelve radial bearings left large wedges
    // completely invisible once several roads and crop plots existed.
    let bearings = if development.is_none() {
        FALLBACK_BEARINGS
    } else {
        SEEDED_BEARINGS
    };
    let mut best_resource_plots: Vec<(f32, Vec3, f32)> = Vec::new();
    let mut resource_rings_since_proof = 0_usize;

    let mut radius = min_radius;
    let mut rings_scanned = 0_usize;
    while radius <= max_radius {
        'candidate: for i in 0..bearings {
            reject!(sampled);
            // Offset each ring's bearings so successive rings do not line every
            // building up on the same spokes.
            let seeded_turn = development.map_or(0.0, |plan| {
                ((plan.plan_seed.rotate_right(9) & 0xffff) as f32 / 65_535.0) * 0.92
            });
            let ring_turn = (radius / RING_STEP) * 0.5;
            let turn = if development.is_none() {
                // The open-land fallback refines each bounded outward ring to
                // 48 bearings, closing the broad gaps left between the twelve
                // preferred grammar directions without creating one large
                // synchronous search.
                i as f32 / bearings as f32 + ring_turn / SEEDED_BEARINGS as f32
            } else {
                (i as f32 + ring_turn + seeded_turn) / SEEDED_BEARINGS as f32
            };
            let angle = turn * std::f32::consts::TAU;
            let base = Vec2::new(angle.cos(), angle.sin()) * radius;
            let planned = development.map_or(
                PlannedPlotCandidate {
                    local: base,
                    frontage: Vec2::ZERO,
                },
                |plan| planned_plot_candidate(plan, kind, radius, i, base),
            );
            let local = planned.local;
            let x = hall.x + local.x;
            let z = hall.z + local.y;

            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            // Prefer real completed frontage once a street exists. Before
            // that, face the implied street from the seed grammar. This is the
            // rotation used by water, field, collision, door, and road checks.
            let candidate2 = Vec2::new(x, z);
            let planned_frontage =
                Vec2::new(hall.x + planned.frontage.x, hall.z + planned.frontage.y);
            let completed_road_frontage = nearest_completed_road_frontage(candidate2, roads);
            // Once streets exist, cabins extend those streets. Letting a late
            // house fall back to a grammar-only frontage beyond the 48 m road
            // catchment can approve a perfectly buildable but permanently
            // isolated pocket. Resource workplaces remain allowed farther out
            // because soil, forest and shore quality legitimately outrank a
            // short connector for those plots.
            if kind == SettlementBuildingKind::House
                && roads.iter().any(|road| road.is_complete())
                && completed_road_frontage.is_none()
            {
                continue;
            }
            let frontage = completed_road_frontage.unwrap_or(planned_frontage);
            let rotation = rotation_facing_frontage(candidate2, frontage);
            let earthwork_effort = if kind == SettlementBuildingKind::Farmstead {
                let Some(effort) = farmstead_earthwork_effort(terrain, candidate, rotation) else {
                    reject!(earthworks);
                    continue;
                };
                effort
            } else {
                if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                    reject!(earthworks);
                    continue;
                }
                0.0
            };
            if !plot_fits_navigation_bounds(kind, candidate, rotation) {
                reject!(bounds);
                continue;
            }
            // Test every part of the rotated footprint and the authored door
            // against the LOCAL water surface. Comparing the centre to the
            // global ocean plane misses inland rivers entirely.
            if shared::components::minimum_building_water_clearance(
                terrain, candidate, kind, rotation,
            ) < FREEBOARD
            {
                reject!(water);
                continue;
            }
            if !crate::world::village_roads::doorway_road_apron_is_dry(
                terrain, kind, candidate, rotation,
            ) {
                reject!(water);
                continue;
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
                    kind, candidate, rotation, colliders, derived,
                )
            }) {
                reject!(props);
                continue;
            }
            let clashes = occupied.iter().any(|(other, other_clearance)| {
                let flat = Vec2::new(candidate.x - other.x, candidate.z - other.z).length();
                flat < clearance + other_clearance
            });
            if clashes {
                reject!(occupied);
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    if shared::components::minimum_rotated_rect_water_clearance(
                        terrain,
                        field,
                        field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                        rotation,
                    ) < FREEBOARD
                    {
                        reject!(water);
                        continue 'candidate;
                    }
                    if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                        !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
                            Vec2::new(field.x, field.z),
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_TERRACE_MARGIN,
                            colliders,
                            derived,
                        )
                    }) {
                        reject!(props);
                        continue 'candidate;
                    }
                    let field_clearance =
                        field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN;
                    if occupied.iter().any(|(other, other_clearance)| {
                        Vec2::new(field.x - other.x, field.z - other.z).length()
                            < field_clearance + other_clearance
                    }) {
                        reject!(occupied);
                        continue 'candidate;
                    }
                }
            }
            let footprint_radius = kind.art().definition().root_footprint_radius() + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                reject!(roads);
                continue;
            }
            if planned_accesses
                .iter()
                .any(|access| access.intersects_circle(candidate2, footprint_radius))
            {
                reject!(roads);
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    let field_center = Vec2::new(field.x, field.z);
                    if roads.iter().any(|road| {
                        road.intersects_rotated_rect(
                            field_center,
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_TERRACE_MARGIN,
                        )
                    }) {
                        reject!(roads);
                        continue 'candidate;
                    }
                    if planned_accesses.iter().any(|access| {
                        access.intersects_circle(
                            field_center,
                            field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                        )
                    }) {
                        reject!(roads);
                        continue 'candidate;
                    }
                }
            }
            if resource_scored {
                if !direct_road_access_is_coarsely_clear(
                    terrain,
                    hall,
                    kind,
                    candidate,
                    rotation,
                    roads,
                    access_blockers,
                    &connected_road_keys,
                ) {
                    reject!(roads);
                    continue;
                }
                // A layout grammar says which street this plot belongs to;
                // geography still decides whether a farm or timber workplace
                // is worth building there. A small travel penalty prevents a
                // negligible quality gain from sending the very first worker
                // to the edge of the full 120 m search band.
                let quality = site_placement_suitability(terrain, kind, candidate);
                let travel = candidate2.distance(Vec2::new(hall.x, hall.z));
                let builder_stand = shared::components::builder_stand_position(
                    candidate,
                    rotation,
                    kind.art().definition().footprint.y,
                );
                // A completed street is already a certified land route. Prove
                // only the new frontage-to-door leg when one is nearby rather
                // than repeatedly searching all the way back to the hall. In
                // a mature riverside settlement the latter could run a large
                // A* for every permit candidate even though the existing road
                // already winds safely around the water.
                let route_origin =
                    completed_road_frontage.unwrap_or_else(|| Vec2::new(hall.x, hall.z));
                // Rank obviously same-landmass plots before remote soil. The
                // old six-entry shortlist compared soil first, so a fertile
                // meadow across a river could occupy every slot; all six then
                // failed the bounded route proof while hundreds of reachable
                // candidates were never examined. This coarse water probe is
                // only a ranking hint: the selected candidate receives the
                // exact 20 cm proof below, and bounded A* still admits an
                // indirect route around an inlet.
                let direct_land = crate::world::village_roads::road_segment_is_coarsely_dry(
                    terrain,
                    route_origin,
                    Vec2::new(builder_stand.x, builder_stand.z),
                );
                // A resource workplace occupies far more ground than a
                // cabin. Prefer plots whose authored door has a direct lane
                // to the existing network; admitting six boxed-in farms to a
                // bounded A* shortlist made one mature-settlement permit
                // spend over a second proving the same negative geometry.
                // Cabins may still reserve a bent lane, while farms and timber
                // huts continue outward until a clean frontage exists.
                let score =
                    quality * 100.0 - travel / max_radius.max(1.0) * 9.0 - earthwork_effort * 12.0
                        + if direct_land { 1_000.0 } else { 0.0 };
                best_resource_plots.push((score, candidate, rotation));
                best_resource_plots.sort_by(|a, b| {
                    b.0.total_cmp(&a.0)
                        .then_with(|| a.1.x.total_cmp(&b.1.x))
                        .then_with(|| a.1.z.total_cmp(&b.1.z))
                });
                best_resource_plots.truncate(RESOURCE_PLOT_SHORTLIST);
            } else {
                if planned_road_access_path(
                    terrain,
                    hall,
                    kind,
                    candidate,
                    rotation,
                    roads,
                    access_blockers,
                    &connected_road_keys,
                )
                .is_none()
                {
                    reject!(access_or_work);
                    continue;
                }
                // This candidate already passed the authoritative full-width
                // access proof above; actor routing later certifies the
                // builder's changing obstacle world. A second independent
                // terrain route here is neither more authority nor a safe
                // mature-city tick cost.
                return Some((candidate, rotation));
            }
        }
        if resource_scored {
            resource_rings_since_proof += 1;
            if resource_rings_since_proof >= RESOURCE_SEARCH_BANDS {
                let proof_candidates = best_resource_plots.len() as u32;
                if let Some((_, candidate, rotation)) =
                    best_resource_plots
                        .iter()
                        .copied()
                        .find(|(_, candidate, rotation)| {
                            resource_plot_is_viable(
                                terrain, hall, kind, *candidate, *rotation, colliders, derived,
                            )
                        })
                {
                    return Some((candidate, rotation));
                }
                if let Some(rejections) = rejections.as_deref_mut() {
                    rejections.access_or_work =
                        rejections.access_or_work.saturating_add(proof_candidates);
                }
                // The best local choices can still fail the exact field-door,
                // tree or 20 cm road proof. Discard only this bounded batch
                // and continue with the next three rings; never mistake a bad
                // shortlist for proof that the whole settlement is full.
                best_resource_plots.clear();
                resource_rings_since_proof = 0;
            }
        }
        rings_scanned += 1;
        if maximum_search_rings.is_some_and(|maximum| rings_scanned >= maximum) {
            break;
        }
        radius += RING_STEP;
    }
    let proof_candidates = best_resource_plots.len() as u32;
    let selected = best_resource_plots
        .into_iter()
        .find(|(_, candidate, rotation)| {
            resource_plot_is_viable(
                terrain, hall, kind, *candidate, *rotation, colliders, derived,
            )
        })
        .map(|(_, candidate, rotation)| (candidate, rotation));
    if selected.is_none() {
        if let Some(rejections) = rejections.as_deref_mut() {
            rejections.access_or_work = rejections.access_or_work.saturating_add(proof_candidates);
        }
    }

    if selected.is_none() && development.is_some() {
        // A charter is a preference, never a hard buildable boundary. Dense
        // seeded spokes or clusters can eventually consume every candidate in
        // their small grammar even though suitable, reachable land remains
        // between them. Retry the same deterministic physical checks on an
        // unrestricted radial sweep before telling the settlement it has no
        // viable plot. Completed roads still determine frontage and every
        // water, prop, field, collision and route rule remains authoritative.
        return find_site_with_plan_diagnostics(
            terrain,
            hall,
            kind,
            occupied,
            roads,
            planned_accesses,
            access_blockers,
            None,
            colliders,
            derived,
            minimum_radius_hint,
            maximum_search_rings,
            rejections.as_deref_mut(),
        );
    }

    selected
}
