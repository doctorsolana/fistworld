//! Company-owned inter-settlement routes and buyer-funded civic contracts.
//!
//! The first controlled loop moves a real civic construction order from one
//! public market to another. A completed private Storage Hall (the warehouse)
//! and one of its ordinary Company Porters are mandatory. The carrier never
//! owns the Stone: destination escrow pays the source seller at collection,
//! the destination owns the in-transit cargo, and the carrier earns its
//! freight fee only after physical delivery.

use super::*;
use crate::world::new_world::trade_access::{FoundingLandNetwork, LandTradeAccess};

use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::{
    CharacterObjective, CivicHallUpgradeWorksite, CivicTradeContract, CompanyTradeRoute,
    ConstructionSite, MaritimeTradeRoute, SettlementId, TradeContractId, TradeContractStatus,
    TradeRouteHistory, TradeRouteId, TradeRouteMode, TradeRouteSchedule, TradeRouteStatus,
    TradeRouteStop, TradeRouteStopAction, TradeRouteTrip,
};
use shared::economy::{FOUNDING_DAILY_WAGE, MarketSeller};

mod civic_review;

mod merchant_economics;
use merchant_economics::*;

/// Three pennies per bulk makes one eight-Stone Town Works load worth 1.44
/// coin to the carrier: enough to cover one ordinary daily wage while keeping
/// freight materially smaller than the Stone purchase itself.
pub const CONTRACT_DELIVERY_PENNIES_PER_BULK: u64 = 3;
/// A public contract must at least cover the carrier's fixed cost of sending
/// one employee and its wagon. Without a call-out minimum, a partly supplied
/// Town Works can post a perfectly real three-Stone order whose per-bulk
/// freight is forever below one daily wage, so no rational company can accept
/// it. Larger loads continue to use the ordinary per-bulk rate.
pub const CONTRACT_MINIMUM_DELIVERY_PENNIES: u64 = 200;
const JOURNEY_PENNIES_PER_100_METRES: u64 = 5;
const ROUTE_REACH: f32 = 1.5;
/// A hall doorway is shared by residents, civic queues and local roads. A
/// building or prop completed while a caravan is away can invalidate the one
/// exact approach that worked on its outward leg even though the public
/// forecourt remains reachable. Caravans may try these nearby loading bays;
/// they never bypass navigation or complete a transaction at a distance.
const TRADE_STOP_APPROACH_OFFSETS: [Vec2; 7] = [
    Vec2::ZERO,
    Vec2::new(-2.5, -1.5),
    Vec2::new(2.5, -1.5),
    Vec2::new(-4.5, -2.5),
    Vec2::new(4.5, -2.5),
    Vec2::new(-6.0, -3.5),
    Vec2::new(6.0, -3.5),
];
const ROUTE_MANAGEMENT_INTERVAL_WORLD_SECONDS: f64 = 5.0;
const TRADE_INTEL_MAX_AGE_DAYS: u32 = 12;
const TRADE_INTEL_CAPACITY_PER_COMPANY: usize = 192;
const AUTONOMOUS_ROUTE_RETRY_DAYS: u32 = 7;

/// One company's remembered reading of one public regional market. The
/// server retains the ground truth separately; autonomous managers may act
/// only on this sparse, possibly stale record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompanyTradeObservation {
    settlement: shared::components::SettlementId,
    good: Good,
    observed_day: u32,
    confidence: u8,
    asking_price: u64,
    listed_units: u32,
    recent_units_sold: u32,
    recent_sale_price: u64,
    funded_unmet_units: u32,
    funded_demand: shared::economy::FundedDemandCurve,
    substitute_demand: shared::economy::FundedDemandCurve,
    market_fee_bps: u16,
}

#[derive(Debug, Default)]
struct CompanyTradeKnowledge {
    observations: Vec<CompanyTradeObservation>,
}

impl CompanyTradeKnowledge {
    fn observe(&mut self, mut observation: CompanyTradeObservation) {
        if let Some(existing) = self.observations.iter_mut().find(|existing| {
            existing.settlement == observation.settlement && existing.good == observation.good
        }) {
            // Independent later rumours corroborate that a market and rough
            // price relationship really exist. A weak manager should learn
            // slowly, not remain permanently below the same confidence gate
            // after hearing the same opportunity for months. Exact branch or
            // caravan observations stay exact; hearsay remains capped at 85%.
            if existing.confidence < 100
                && observation.confidence < 100
                && observation.observed_day >= existing.observed_day
            {
                observation.confidence = observation
                    .confidence
                    .max(existing.confidence.saturating_add(5).min(85));
            }
            *existing = observation;
        } else {
            if self.observations.len() == TRADE_INTEL_CAPACITY_PER_COMPANY {
                let oldest = self
                    .observations
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, observation)| {
                        (
                            observation.observed_day,
                            observation.settlement,
                            observation.good.index(),
                        )
                    })
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                self.observations.swap_remove(oldest);
            }
            self.observations.push(observation);
        }
        self.observations
            .sort_unstable_by_key(|observation| (observation.settlement, observation.good.index()));
    }

    fn forget_stale(&mut self, day: u32) {
        self.observations.retain(|observation| {
            day.saturating_sub(observation.observed_day) <= TRADE_INTEL_MAX_AGE_DAYS
        });
    }
}

/// Server-only sparse commercial memory. It intentionally is not replicated:
/// the full regional truth would make both NPCs and remote player UI
/// omniscient. God/lab diagnostics can still inspect the resulting routes.
#[derive(Resource, Default)]
pub struct RegionalTradeIntelligence {
    processed_day: Option<u32>,
    companies: HashMap<shared::components::CompanyId, CompanyTradeKnowledge>,
    protected_company_cash: HashMap<shared::components::CompanyId, u64>,
}

/// Demonstrated or publicly advertised external demand fed back into the
/// ordinary permit market. This is aggregate settlement information, never a
/// command to construct a particular producer or warehouse.
#[derive(Resource, Default)]
pub(crate) struct RegionalMerchantDemand {
    export_units: HashMap<(shared::components::SettlementId, Good), u32>,
    /// Importers also need one local depot and porter, but imported demand
    /// must not masquerade as a reason to construct the same producer at the
    /// destination. Keep that logistics-only pressure separate from export
    /// units while exposing their combined bulk to Storage Hall planning.
    import_logistics_bulk: HashMap<shared::components::SettlementId, u32>,
}

impl RegionalMerchantDemand {
    pub(crate) fn units(&self, settlement: shared::components::SettlementId, good: Good) -> u32 {
        self.export_units
            .get(&(settlement, good))
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn bulk(&self, settlement: shared::components::SettlementId) -> u32 {
        let export_bulk = Good::ALL
            .into_iter()
            .map(|good| {
                self.units(settlement, good)
                    .saturating_mul(good.bulk_per_unit())
            })
            .fold(0u32, u32::saturating_add);
        export_bulk.saturating_add(
            self.import_logistics_bulk
                .get(&settlement)
                .copied()
                .unwrap_or(0),
        )
    }

    pub(crate) fn advertise(
        &mut self,
        settlement: shared::components::SettlementId,
        good: Good,
        units: u32,
    ) {
        self.export_units
            .entry((settlement, good))
            .and_modify(|current| *current = (*current).max(units))
            .or_insert(units);
    }

    pub(crate) fn advertise_import_logistics(
        &mut self,
        settlement: shared::components::SettlementId,
        bulk: u32,
    ) {
        self.import_logistics_bulk
            .entry(settlement)
            .and_modify(|current| *current = (*current).max(bulk))
            .or_insert(bulk);
    }
}

/// Extra lifecycle state exists only for NPC-authored speculative routes.
/// Player timetables and buyer-funded contracts are never silently rewritten
/// by this manager.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AutonomousMerchantRoute {
    last_review_day: u32,
    mothballed_day: Option<u32>,
    last_completed_trips: u32,
    disappointing_reviews: u8,
    expected_trip_profit: i64,
    confidence: u8,
    unsold_since_day: Option<u32>,
    minimum_cargo_net: u64,
    freight_cost: u64,
    minimum_return_bps: u64,
    destination_fee_bps: u16,
}

#[derive(Debug, Clone, Copy)]
struct PublicMarketSnapshot {
    entity: Entity,
    settlement: shared::components::SettlementId,
    position: Vec3,
    observations: [CompanyTradeObservation; Good::COUNT],
}

#[derive(Debug, Clone, Copy)]
struct MerchantOpportunity {
    origin: shared::components::SettlementId,
    destination: shared::components::SettlementId,
    good: Good,
    cargo_units: u32,
    maximum_purchase_price: u64,
    minimum_sale_price: u64,
    expected_profit: i64,
    confidence: u8,
    minimum_cargo_net: u64,
    freight_cost: u64,
    minimum_return_bps: u64,
    destination_fee_bps: u16,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TradeRouteRoutine {
    pub route: TradeRouteId,
    mode: TradeRouteMode,
    phase: TradeRoutePhase,
    stop_index: u8,
    stops_visited: u8,
    departed_day: u32,
    departed_world_seconds: f64,
    source_purchase_cost: u64,
    source_market_fees: u64,
    cargo_units: u32,
    consigned_value: u64,
    failed_approaches: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TradeRoutePhase {
    GoingToOrigin,
    InTransit,
    ReturningToOrigin,
    ReturningToWarehouse,
    MerchantTravellingToStop,
    MerchantReturningToOrigin,
    MerchantReturningToWarehouse,
}

impl TradeRouteRoutine {
    pub(crate) const fn objective(&self) -> CharacterObjective {
        match self.phase {
            TradeRoutePhase::GoingToOrigin => CharacterObjective::GoingToTradeRoutePickup,
            TradeRoutePhase::InTransit => CharacterObjective::HaulingInterSettlementCargo,
            TradeRoutePhase::MerchantTravellingToStop => {
                CharacterObjective::HaulingInterSettlementCargo
            }
            TradeRoutePhase::ReturningToOrigin
            | TradeRoutePhase::ReturningToWarehouse
            | TradeRoutePhase::MerchantReturningToOrigin
            | TradeRoutePhase::MerchantReturningToWarehouse => {
                CharacterObjective::ReturningFromTradeRoute
            }
        }
    }
}

#[derive(Clone, Copy)]
struct HallSnapshot {
    position: Vec3,
    rotation: f32,
}

#[derive(Clone, Copy)]
struct WarehouseSnapshot {
    id: shared::components::BuildingId,
    settlement: shared::components::SettlementId,
    company: shared::components::CompanyId,
    position: Vec3,
    rotation: f32,
    can_operate: bool,
}

fn hall_entrance(hall: HallSnapshot) -> Vec3 {
    SettlementBuildingKind::Hall.entrance_position(hall.position, hall.rotation)
}

fn offset_trade_approach(base: Vec3, rotation: f32, failed_approaches: u8) -> Vec3 {
    let local = TRADE_STOP_APPROACH_OFFSETS
        [usize::from(failed_approaches).min(TRADE_STOP_APPROACH_OFFSETS.len().saturating_sub(1))];
    let offset = Quat::from_rotation_y(rotation) * Vec3::new(local.x, 0.0, local.y);
    base + offset
}

fn hall_trade_approach(hall: HallSnapshot, failed_approaches: u8) -> Vec3 {
    offset_trade_approach(hall_entrance(hall), hall.rotation, failed_approaches)
}

fn contract_delivery_approach(
    hall: HallSnapshot,
    site: Option<&ConstructionSite>,
    failed_approaches: u8,
) -> Vec3 {
    site.map_or_else(
        || hall_trade_approach(hall, failed_approaches),
        |site| offset_trade_approach(site.stand, site.rotation, failed_approaches),
    )
}

fn warehouse_trade_approach(warehouse: WarehouseSnapshot, failed_approaches: u8) -> Vec3 {
    let entrance = SettlementBuildingKind::StorageHall
        .entrance_position(warehouse.position, warehouse.rotation);
    offset_trade_approach(entrance, warehouse.rotation, failed_approaches)
}

fn merchant_stop_target(
    stop: TradeRouteStop,
    company: shared::components::CompanyId,
    halls: &HashMap<shared::components::SettlementId, HallSnapshot>,
    warehouses: &[WarehouseSnapshot],
    failed_approaches: u8,
) -> Option<Vec3> {
    match stop.action {
        TradeRouteStopAction::Load | TradeRouteStopAction::Unload => warehouses
            .iter()
            .find(|warehouse| {
                warehouse.company == company
                    && warehouse.settlement == stop.settlement
                    && warehouse.can_operate
            })
            .map(|warehouse| warehouse_trade_approach(*warehouse, failed_approaches)),
        TradeRouteStopAction::Buy | TradeRouteStopAction::Sell => halls
            .get(&stop.settlement)
            .copied()
            .map(|hall| hall_trade_approach(hall, failed_approaches)),
        TradeRouteStopAction::ContractPickup | TradeRouteStopAction::ContractDelivery => None,
    }
}

fn retry_trade_approach(
    commands: &mut Commands,
    porter: Entity,
    routine: &mut TradeRouteRoutine,
    next_target: impl FnOnce(u8) -> Vec3,
) -> bool {
    let next = routine.failed_approaches.saturating_add(1);
    if usize::from(next) >= TRADE_STOP_APPROACH_OFFSETS.len() {
        return false;
    }
    routine.failed_approaches = next;
    commands
        .entity(porter)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<crate::world::village_roads::NavigationRouteBackoff>()
        .insert(MoveTarget(next_target(next)));
    true
}

fn absolute_world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

#[cfg(test)]
fn contract_delivery_pennies_per_bulk(good: Good, units: u32) -> u64 {
    let total_bulk = u64::from(units.max(1)).saturating_mul(u64::from(good.bulk_per_unit()));
    CONTRACT_DELIVERY_PENNIES_PER_BULK.max(CONTRACT_MINIMUM_DELIVERY_PENNIES.div_ceil(total_bulk))
}

fn mixed_trade_seed(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn merchant_diagnostics_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FISTWORLD_MERCHANT_DIAGNOSTICS").is_some())
}

fn market_observation(
    settlement: shared::components::SettlementId,
    market: &MootMarket,
    good: Good,
    day: u32,
) -> CompanyTradeObservation {
    let pool = market.pool(good);
    let direct_units_sold = pool
        .day
        .consumer_units
        .saturating_add(pool.previous_day.consumer_units);
    let direct_funded_unmet = pool
        .day
        .funded_unmet_units
        .saturating_add(pool.previous_day.funded_unmet_units);
    let mut funded_demand = pool.day.funded_demand;
    funded_demand.merge(&pool.previous_day.funded_demand);
    let mut substitute_demand = shared::economy::FundedDemandCurve::default();
    if good.is_edible() {
        for candidate in Good::ALL
            .into_iter()
            .filter(|candidate| candidate.is_edible())
        {
            let alternative = market.pool(candidate);
            substitute_demand.merge(&alternative.day.funded_demand);
            substitute_demand.merge(&alternative.previous_day.funded_demand);
        }
    }
    // A residual ration bid can support a conservative substitute-food
    // opportunity at that same funded price. Completed sales stay paired
    // with their own good's realised price: cheap Fish buyers are not evidence
    // that the same quantity will buy expensive Bread.
    let funded_unmet = if good.is_edible() {
        let aggregate_funded_unmet = Good::ALL
            .into_iter()
            .filter(|candidate| candidate.is_edible())
            .map(|candidate| {
                let flow = market.pool(candidate);
                flow.day
                    .funded_unmet_units
                    .saturating_add(flow.previous_day.funded_unmet_units)
            })
            .fold(0u64, u64::saturating_add);
        direct_funded_unmet.max(aggregate_funded_unmet.div_ceil(4))
    } else {
        direct_funded_unmet
    };
    let sale_coin = pool
        .day
        .consumer_coin
        .saturating_add(pool.previous_day.consumer_coin);
    CompanyTradeObservation {
        settlement,
        good,
        observed_day: day,
        confidence: 100,
        asking_price: market.suggested_price(good).max(1),
        listed_units: market.listed_units(good),
        recent_units_sold: direct_units_sold.min(u64::from(u32::MAX)) as u32,
        recent_sale_price: sale_coin.checked_div(direct_units_sold).unwrap_or(0),
        funded_unmet_units: funded_unmet.min(u64::from(u32::MAX)) as u32,
        funded_demand,
        substitute_demand,
        market_fee_bps: market.market_fee_bps(),
    }
}

fn rumor_observation(
    mut truth: CompanyTradeObservation,
    company: shared::components::CompanyId,
    master: shared::components::PersonId,
    attributes: CharacterAttributes,
    day: u32,
) -> CompanyTradeObservation {
    let seed = mixed_trade_seed(
        company.0
            ^ master.0.rotate_left(13)
            ^ truth.settlement.0.rotate_left(29)
            ^ (truth.good.index() as u64).rotate_left(41)
            ^ u64::from(day).rotate_left(7),
    );
    let intelligence = u64::from(attributes.intelligence());
    let maximum_error_bps = 2_200_u64.saturating_sub(intelligence.saturating_mul(12));
    let span = maximum_error_bps.saturating_mul(2).saturating_add(1);
    let signed_error = (seed % span) as i64 - maximum_error_bps as i64;
    truth.asking_price = if signed_error >= 0 {
        truth
            .asking_price
            .saturating_mul(10_000 + signed_error as u64)
            .div_ceil(10_000)
    } else {
        truth
            .asking_price
            .saturating_mul(10_000_u64.saturating_sub((-signed_error) as u64))
            / 10_000
    }
    .max(1);
    let age = 1 + ((seed >> 17) % 3) as u32;
    truth.observed_day = day.saturating_sub(age);
    truth.confidence = 35_u8
        .saturating_add(attributes.charm() / 3)
        .saturating_add(attributes.intelligence() / 4)
        .min(75);
    // Rumours communicate scale, not an audited warehouse count.
    truth.listed_units = truth.listed_units.div_ceil(4).saturating_mul(4);
    truth.funded_unmet_units = truth.funded_unmet_units.div_ceil(2).saturating_mul(2);
    truth
}

fn strategy_trade_terms(strategy: shared::economy::BusinessStrategy) -> (u64, u8, u64) {
    use shared::economy::BusinessStrategy;
    match strategy {
        // minimum return on committed cargo, minimum confidence and minimum
        // absolute trip profit in pennies.
        BusinessStrategy::Aggressive => (800, 35, 60),
        BusinessStrategy::Balanced => (1_500, 45, 100),
        BusinessStrategy::Conservative => (2_000, 65, 140),
    }
}

/// A stale quote is an estimate, not a one-penny take-it-or-leave-it order.
/// Less certain intelligence permits a small bounded cushion; the opportunity
/// evaluator below still clips this to a price which preserves the owner's
/// chosen minimum profit and return.
fn uncertain_purchase_limit(asking_price: u64, confidence: u8) -> u64 {
    let buffer_bps = u64::from(100_u8.saturating_sub(confidence)).saturating_mul(20);
    asking_price
        .saturating_mul(BASIS_POINTS.saturating_add(buffer_bps))
        .div_ceil(BASIS_POINTS)
        .max(asking_price)
}

/// Give autonomous companies bounded, stale commercial knowledge and let a
/// small number of solvent Company Masters open physical trial routes. This
/// runs once per world day; no villager performs a global market scan.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn review_autonomous_merchant_trade(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut intelligence: ResMut<RegionalTradeIntelligence>,
    mut merchant_demand: ResMut<RegionalMerchantDemand>,
    land_networks: Query<(&SettlementId, &FoundingLandNetwork), With<Settlement>>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            &GoodsInventory,
            &mut MootMarket,
            Entity,
        ),
        With<Settlement>,
    >,
    people: Query<(&shared::components::PersonId, &CharacterAttributes)>,
    companies: Query<(
        &shared::components::CompanyId,
        &shared::components::CompanyLeadership,
        &shared::economy::CompanyAccount,
        &shared::economy::CompanyManagementPolicy,
        &shared::economy::CompanyBranchPolicies,
    )>,
    sites: Query<(
        &shared::components::OperatedBy,
        &SettlementBuilding,
        Option<&BusinessCondition>,
        Option<&BusinessWagePolicy>,
        Option<&BusinessStaffingPolicy>,
    )>,
    warehouses: Query<(
        &shared::components::BuildingId,
        &shared::components::OperatedBy,
        &shared::components::BuildingOf,
        &SettlementBuilding,
        &GoodsInventory,
        Option<&BusinessCondition>,
        Option<&BusinessWagePolicy>,
    )>,
    porters: Query<&CompanyPorter>,
    mut routes: Query<
        (
            Entity,
            Option<&TradeRouteId>,
            &mut CompanyTradeRoute,
            &TradeRouteSchedule,
            &TradeRouteHistory,
            Option<&mut AutonomousMerchantRoute>,
        ),
        Without<MaritimeTradeRoute>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    if intelligence.processed_day == Some(day) {
        return;
    }
    intelligence.processed_day = Some(day);
    let land_access = LandTradeAccess::from_tags(land_networks.iter());

    let mut markets: Vec<_> = halls
        .iter()
        .filter(|(_, _, _, market, _)| market.supports_regional_trade())
        .map(
            |(settlement, position, _inventory, market, entity)| PublicMarketSnapshot {
                entity,
                settlement: *settlement,
                position: position.0,
                observations: std::array::from_fn(|index| {
                    market_observation(*settlement, market, Good::ALL[index], day)
                }),
            },
        )
        .collect();
    markets.sort_unstable_by_key(|market| market.settlement);
    let market_by_settlement: HashMap<_, _> = markets
        .iter()
        .map(|market| (market.settlement, *market))
        .collect();

    let attributes: HashMap<_, _> = people
        .iter()
        .map(|(person, attributes)| (*person, *attributes))
        .collect();
    let living_companies: HashSet<_> = companies.iter().map(|(company, ..)| *company).collect();
    intelligence
        .companies
        .retain(|company, _| living_companies.contains(company));

    let mut payroll_by_company = HashMap::<shared::components::CompanyId, u64>::new();
    for (company, building, condition, wage, staffing) in sites.iter() {
        if condition.is_some_and(|condition| !condition.state.can_operate()) {
            continue;
        }
        let positions = staffing
            .copied()
            .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
            .target_for(building.kind);
        let daily_wage = wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage);
        let payroll = daily_wage.saturating_mul(u64::from(positions));
        let total = payroll_by_company.entry(company.0).or_default();
        *total = total.saturating_add(payroll);
    }

    #[derive(Clone)]
    struct CompanySnapshot {
        id: shared::components::CompanyId,
        master: shared::components::PersonId,
        account: shared::economy::CompanyAccount,
        policy: shared::economy::CompanyManagementPolicy,
        branches: shared::economy::CompanyBranchPolicies,
    }
    let mut company_snapshots: Vec<_> = companies
        .iter()
        .map(
            |(id, leadership, account, policy, branches)| CompanySnapshot {
                id: *id,
                master: leadership.master,
                account: *account,
                policy: *policy,
                branches: branches.clone(),
            },
        )
        .collect();
    company_snapshots.sort_unstable_by_key(|company| company.id);
    intelligence.protected_company_cash.clear();
    for company in &company_snapshots {
        intelligence.protected_company_cash.insert(
            company.id,
            payroll_by_company
                .get(&company.id)
                .copied()
                .unwrap_or(0)
                .saturating_mul(u64::from(company.policy.payroll_reserve_days))
                .saturating_add(company.account.wage_arrears)
                .saturating_add(company.account.tax_arrears),
        );
    }

    #[derive(Clone, Copy)]
    struct AiWarehouse {
        id: shared::components::BuildingId,
        company: shared::components::CompanyId,
        settlement: shared::components::SettlementId,
        stock: [u32; Good::COUNT],
        daily_wage: u64,
    }
    let mut warehouse_snapshots: Vec<_> = warehouses
        .iter()
        .filter(|(_, _, _, building, _, condition, _)| {
            building.kind == SettlementBuildingKind::StorageHall
                && condition.is_none_or(|condition| condition.state.can_operate())
        })
        .map(
            |(id, company, building_of, _, inventory, _, wage)| AiWarehouse {
                id: *id,
                company: company.0,
                settlement: building_of.0,
                stock: std::array::from_fn(|index| inventory.amount(Good::ALL[index])),
                daily_wage: wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage),
            },
        )
        .collect();
    warehouse_snapshots.sort_unstable_by_key(|warehouse| warehouse.id);
    let warehouse_by_id: HashMap<_, _> = warehouse_snapshots
        .iter()
        .map(|warehouse| (warehouse.id, *warehouse))
        .collect();

    let porter_count: HashMap<_, usize> =
        porters.iter().fold(HashMap::new(), |mut counts, porter| {
            *counts.entry(porter.company).or_default() += 1;
            counts
        });
    let porter_count_by_warehouse: HashMap<_, usize> =
        porters.iter().fold(HashMap::new(), |mut counts, porter| {
            *counts
                .entry((porter.company, porter.storage_hall))
                .or_default() += 1;
            counts
        });
    let mut routes_per_company = HashMap::<shared::components::CompanyId, usize>::new();
    let mut active_routes_per_company = HashMap::<shared::components::CompanyId, usize>::new();
    let mut active_routes_by_warehouse = HashMap::<shared::components::BuildingId, usize>::new();
    let mut committed_purchases = HashMap::<shared::components::CompanyId, u64>::new();
    let mut incoming = HashMap::<(shared::components::SettlementId, Good), u32>::new();
    let mut existing_lanes = HashSet::new();
    let mut route_entities_by_company =
        HashMap::<shared::components::CompanyId, Vec<Entity>>::new();
    let mut manual_listing_keys = HashSet::new();
    let mut recent_arrival_reports =
        HashMap::<shared::components::CompanyId, Vec<CompanyTradeObservation>>::new();
    for (entity, _, route, schedule, history, autonomous) in routes.iter_mut() {
        route_entities_by_company
            .entry(route.company)
            .or_default()
            .push(entity);
        *routes_per_company.entry(route.company).or_default() += 1;
        // An idle reusable contract lane owns no porter and must not prevent
        // that employee from taking a merchant opportunity. Conversely, a
        // queued or embodied route reserves one unit of the finite porter
        // capacity even before `assigned_caravaner` is populated.
        if !matches!(
            route.status,
            TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
        ) {
            *active_routes_per_company.entry(route.company).or_default() += 1;
            *active_routes_by_warehouse
                .entry(route.warehouse)
                .or_default() += 1;
        }
        existing_lanes.insert((route.company, route.origin, route.destination, route.good));
        if route.mode == TradeRouteMode::Merchant
            && (!route.autonomous_management || autonomous.is_none())
        {
            for stop in schedule
                .stops()
                .iter()
                .filter(|stop| stop.action == TradeRouteStopAction::Sell)
            {
                manual_listing_keys.insert((route.warehouse, stop.settlement, route.good));
            }
        }
        if route.mode == TradeRouteMode::Merchant
            && !matches!(
                route.status,
                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed | TradeRouteStatus::Returning
            )
        {
            let units = incoming.entry((route.destination, route.good)).or_default();
            *units = units.saturating_add(route.cargo_target);
            if schedule
                .stops()
                .get(usize::from(route.current_stop))
                .is_some_and(|stop| stop.action == TradeRouteStopAction::Buy)
            {
                let cash = committed_purchases.entry(route.company).or_default();
                *cash = cash.saturating_add(
                    route
                        .maximum_purchase_price
                        .saturating_mul(u64::from(route.cargo_target)),
                );
            }
        }

        // A completed physical visit is better intelligence than hearsay.
        // Capture only fresh arrivals so a dormant old route does not provide
        // a permanent live price feed from its destination.
        let Some(last_trip) = history.trips().last() else {
            continue;
        };
        if day.saturating_sub(last_trip.completed_day) > 1 {
            continue;
        }
        let reports = recent_arrival_reports.entry(route.company).or_default();
        // A completed timetable reports every stop the porter physically
        // reached, including a source where its limit order failed. Recording
        // only the final destination made a one-penny source-quote miss
        // impossible for an autonomous merchant to learn from.
        for stop in schedule
            .stops()
            .iter()
            .take(usize::from(last_trip.stops_visited))
        {
            let Some(market) = market_by_settlement.get(&stop.settlement) else {
                continue;
            };
            for mut observation in market.observations {
                observation.confidence = 90;
                reports.push(observation);
            }
        }
    }

    // Publish only substantial, economically funded regional demand back to
    // settlement development. Existing cargo is deducted first, so a permit
    // board cannot mistake an approaching wagon for permanently unmet demand.
    merchant_demand.export_units.clear();
    merchant_demand.import_logistics_bulk.clear();
    for source in &markets {
        for destination in &markets {
            if source.settlement == destination.settlement
                || !land_access.allows(source.settlement, destination.settlement)
            {
                continue;
            }
            for good in Good::ALL {
                let committed = incoming
                    .get(&(destination.settlement, good))
                    .copied()
                    .unwrap_or(0);
                let Some(opportunity) = evaluate_merchant_opportunity(
                    source.observations[good.index()],
                    destination.observations[good.index()],
                    source.position,
                    destination.position,
                    shared::economy::BusinessStrategy::Balanced,
                    FOUNDING_DAILY_WAGE,
                    committed,
                ) else {
                    continue;
                };
                merchant_demand.advertise(source.settlement, good, opportunity.cargo_units);
                merchant_demand.advertise_import_logistics(
                    destination.settlement,
                    opportunity.cargo_units.saturating_mul(good.bulk_per_unit()),
                );
            }
        }
    }

    let mut reviewed_consignments = HashSet::new();
    let mut unsold_consignments = HashSet::new();
    for (settlement, _, _, market, _) in halls.iter() {
        for listing in market.listings() {
            if let MarketSeller::Business(warehouse) = listing.seller {
                if warehouse_by_id.contains_key(&warehouse) && listing.units > 0 {
                    unsold_consignments.insert((warehouse, *settlement, listing.good));
                }
            }
        }
    }
    for company in company_snapshots {
        let master_attributes = attributes.get(&company.master).copied().unwrap_or_default();
        let knowledge = intelligence.companies.entry(company.id).or_default();
        knowledge.forget_stale(day);

        // A company knows its own branch markets exactly.
        for branch in company.branches.branches() {
            let Some(market) = market_by_settlement.get(&branch.settlement) else {
                continue;
            };
            for observation in market.observations {
                knowledge.observe(observation);
            }
        }

        // The caravaner's latest destination report is company knowledge,
        // even when that town has no permanent company branch. It remains a
        // dated observation and expires like every other commercial memory.
        if let Some(reports) = recent_arrival_reports.get(&company.id) {
            for observation in reports.iter().copied() {
                knowledge.observe(observation);
            }
        }

        let protected_payroll = payroll_by_company
            .get(&company.id)
            .copied()
            .unwrap_or(0)
            .saturating_mul(u64::from(company.policy.payroll_reserve_days));
        let free_cash = company
            .account
            .cash
            .saturating_sub(protected_payroll)
            .saturating_sub(company.account.wage_arrears)
            .saturating_sub(company.account.tax_arrears);
        let mut pending_purchases = committed_purchases.get(&company.id).copied().unwrap_or(0);
        let mut available_cash = free_cash.saturating_sub(pending_purchases);
        // A completed trial owns real stock until somebody buys it. Review its
        // local consignment daily and require fresh profitable terms before
        // spending on another shipment. Manual timetables are never touched.
        for entity in route_entities_by_company
            .get(&company.id)
            .into_iter()
            .flatten()
        {
            let Ok((_, _, mut route, schedule, history, autonomous)) = routes.get_mut(*entity)
            else {
                continue;
            };
            let Some(mut autonomous) = autonomous else {
                continue;
            };
            if route.company != company.id
                || !route.autonomous_management
                || !company.policy.autopilot
                || autonomous.last_review_day == day
            {
                continue;
            }
            autonomous.last_review_day = day;
            let Some(warehouse) = warehouse_by_id.get(&route.warehouse) else {
                continue;
            };
            let (Some(origin), Some(destination)) = (
                market_by_settlement.get(&route.origin),
                market_by_settlement.get(&route.destination),
            ) else {
                continue;
            };
            let distance = Vec2::new(
                destination.position.x - origin.position.x,
                destination.position.z - origin.position.z,
            )
            .length();
            let consignment_key = (route.warehouse, route.destination, route.good);
            let already_reviewed = history.trips().iter().any(|trip| trip.units > 0)
                && !reviewed_consignments.insert(consignment_key);
            let unsold = halls
                .get_mut(destination.entity)
                .is_ok_and(|(_, _, _, mut market, _)| {
                    review_merchant_consignment(
                        &route,
                        &mut autonomous,
                        history,
                        &mut market,
                        warehouse.daily_wage,
                        clock.cycle_duration(),
                        distance,
                        day,
                        already_reviewed || manual_listing_keys.contains(&consignment_key),
                    )
                });
            // An embodied worker's cargo and price commitment remain intact.
            if route.assigned_caravaner.is_some() {
                continue;
            }
            let was_queued = !matches!(
                route.status,
                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed | TradeRouteStatus::Returning
            );
            if !matches!(
                route.status,
                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
            ) {
                let active = active_routes_per_company.entry(company.id).or_default();
                *active = active.saturating_sub(1);
                let active = active_routes_by_warehouse
                    .entry(route.warehouse)
                    .or_default();
                *active = active.saturating_sub(1);
            }
            if was_queued {
                let incoming = incoming.entry((route.destination, route.good)).or_default();
                *incoming = incoming.saturating_sub(route.cargo_target);
                if schedule
                    .stops()
                    .get(usize::from(route.current_stop))
                    .is_some_and(|stop| stop.action == TradeRouteStopAction::Buy)
                {
                    pending_purchases = pending_purchases.saturating_sub(
                        route
                            .maximum_purchase_price
                            .saturating_mul(u64::from(route.cargo_target)),
                    );
                    available_cash = free_cash.saturating_sub(pending_purchases);
                }
            }
            route.status = TradeRouteStatus::Mothballed;
            route.automatic = false;
            if unsold
                || company.account.wage_arrears > 0
                || company.account.tax_arrears > 0
                || !land_access.allows(route.origin, route.destination)
                || active_routes_per_company
                    .get(&company.id)
                    .copied()
                    .unwrap_or(0)
                    >= porter_count.get(&company.id).copied().unwrap_or(0)
                || active_routes_by_warehouse
                    .get(&route.warehouse)
                    .copied()
                    .unwrap_or(0)
                    >= porter_count_by_warehouse
                        .get(&(company.id, route.warehouse))
                        .copied()
                        .unwrap_or(0)
            {
                continue;
            }
            if autonomous.disappointing_reviews >= 2 {
                let stopped = *autonomous.mothballed_day.get_or_insert(day);
                if day.saturating_sub(stopped) < AUTONOMOUS_ROUTE_RETRY_DAYS {
                    continue;
                }
            }
            let find_observation = |settlement| {
                knowledge
                    .observations
                    .iter()
                    .find(|observation| {
                        observation.settlement == settlement && observation.good == route.good
                    })
                    .copied()
            };
            let (Some(mut source), Some(destination_observation)) = (
                find_observation(route.origin),
                find_observation(route.destination),
            ) else {
                continue;
            };
            let loads_owned = schedule
                .stops()
                .first()
                .is_some_and(|stop| stop.action == TradeRouteStopAction::Load);
            if loads_owned {
                source.listed_units = warehouse.stock[route.good.index()];
            } else {
                source.listed_units = source.listed_units.min(
                    (available_cash
                        / uncertain_purchase_limit(source.asking_price, source.confidence).max(1))
                    .min(u64::from(u32::MAX)) as u32,
                );
            }
            let committed = incoming
                .get(&(route.destination, route.good))
                .copied()
                .unwrap_or(0);
            let Some(opportunity) = evaluate_merchant_opportunity(
                source,
                destination_observation,
                origin.position,
                destination.position,
                company.policy.strategy,
                warehouse.daily_wage,
                committed,
            ) else {
                continue;
            };
            route.cargo_target = opportunity.cargo_units;
            route.maximum_purchase_price = opportunity.maximum_purchase_price;
            route.minimum_destination_price = opportunity.minimum_sale_price;
            route.expected_trip_profit = opportunity.expected_profit;
            route.decision_confidence = opportunity.confidence;
            route.status = TradeRouteStatus::WaitingForPorter;
            autonomous.expected_trip_profit = opportunity.expected_profit;
            autonomous.confidence = opportunity.confidence;
            autonomous.minimum_cargo_net = opportunity.minimum_cargo_net;
            autonomous.freight_cost = opportunity.freight_cost;
            autonomous.minimum_return_bps = opportunity.minimum_return_bps;
            autonomous.destination_fee_bps = opportunity.destination_fee_bps;
            if autonomous.disappointing_reviews >= 2 {
                // This branch was gated by a completed cooldown above. A
                // normal requeue must preserve the consecutive failure count.
                autonomous.disappointing_reviews = 0;
            }
            autonomous.mothballed_day = None;
            *active_routes_per_company.entry(company.id).or_default() += 1;
            *active_routes_by_warehouse
                .entry(route.warehouse)
                .or_default() += 1;
            if !loads_owned {
                pending_purchases = pending_purchases.saturating_add(
                    opportunity
                        .maximum_purchase_price
                        .saturating_mul(u64::from(opportunity.cargo_units)),
                );
                available_cash = free_cash.saturating_sub(pending_purchases);
            }
            let incoming = incoming.entry((route.destination, route.good)).or_default();
            *incoming = incoming.saturating_add(opportunity.cargo_units);
        }

        let review_due = (day + (company.id.0 % 3) as u32).is_multiple_of(3);
        if !review_due || !company.policy.autopilot || markets.len() < 2 {
            continue;
        }
        let diagnostics = merchant_diagnostics_enabled();
        let company_warehouses: Vec<_> = if diagnostics {
            warehouse_snapshots
                .iter()
                .filter(|warehouse| warehouse.company == company.id)
                .map(|warehouse| (warehouse.id, warehouse.settlement))
                .collect()
        } else {
            Vec::new()
        };

        // One delayed market report reaches this Master per review. Charming
        // or intelligent people receive a better report, not global truth.
        let remote_markets: Vec<_> = markets
            .iter()
            .filter(|market| company.branches.branch(market.settlement).is_none())
            .collect();
        if !remote_markets.is_empty() {
            let rumor_index = mixed_trade_seed(
                company.id.0 ^ u64::from(day).rotate_left(19) ^ company.master.0.rotate_left(37),
            ) as usize
                % remote_markets.len();
            for truth in remote_markets[rumor_index].observations {
                knowledge.observe(rumor_observation(
                    truth,
                    company.id,
                    company.master,
                    master_attributes,
                    day,
                ));
            }
        }

        if company.account.wage_arrears > 0 || company.account.tax_arrears > 0 {
            if diagnostics && !company_warehouses.is_empty() {
                eprintln!(
                    "MERCHANT REVIEW day={day} company=#{} blocked=arrears wage={} tax={} warehouses={company_warehouses:?}",
                    company.id.0, company.account.wage_arrears, company.account.tax_arrears,
                );
            }
            continue;
        }
        // Payroll for the warehouse porter is already protected above for the
        // company's chosen reserve horizon. Requiring one additional full
        // wage here double-counted labour and permanently blocked modest firms
        // from trying a smaller cash-sized cargo.
        if available_cash == 0 {
            if diagnostics && !company_warehouses.is_empty() {
                eprintln!(
                    "MERCHANT REVIEW day={day} company=#{} blocked=no-free-cash cash={} protected_payroll={} warehouses={company_warehouses:?}",
                    company.id.0, company.account.cash, protected_payroll,
                );
            }
            continue;
        }
        let route_capacity = porter_count.get(&company.id).copied().unwrap_or(0);
        if route_capacity == 0
            || active_routes_per_company
                .get(&company.id)
                .copied()
                .unwrap_or(0)
                >= route_capacity
            || routes_per_company.get(&company.id).copied().unwrap_or(0)
                >= route_capacity.saturating_mul(4)
        {
            if diagnostics && !company_warehouses.is_empty() {
                eprintln!(
                    "MERCHANT REVIEW day={day} company=#{} blocked=route-capacity cash={} free={} porters={} active={} routes={} warehouses={company_warehouses:?}",
                    company.id.0,
                    company.account.cash,
                    available_cash,
                    route_capacity,
                    active_routes_per_company
                        .get(&company.id)
                        .copied()
                        .unwrap_or(0),
                    routes_per_company.get(&company.id).copied().unwrap_or(0),
                );
            }
            continue;
        }

        let attention_budget = 2 + usize::from(master_attributes.intelligence() / 25);
        let mut candidates = Vec::new();
        for warehouse in warehouse_snapshots.iter().filter(|warehouse| {
            warehouse.company == company.id
                && active_routes_by_warehouse
                    .get(&warehouse.id)
                    .copied()
                    .unwrap_or(0)
                    < porter_count_by_warehouse
                        .get(&(company.id, warehouse.id))
                        .copied()
                        .unwrap_or(0)
        }) {
            let Some(source_market) = market_by_settlement.get(&warehouse.settlement) else {
                continue;
            };
            for good in Good::ALL {
                let mut source = source_market.observations[good.index()];
                source.listed_units = source.listed_units.max(warehouse.stock[good.index()]);
                if warehouse.stock[good.index()] == 0 {
                    let purchase_limit =
                        uncertain_purchase_limit(source.asking_price, source.confidence);
                    source.listed_units = source.listed_units.min(
                        (available_cash / purchase_limit.max(1)).min(u64::from(u32::MAX)) as u32,
                    );
                }
                for destination in knowledge
                    .observations
                    .iter()
                    .copied()
                    .filter(|observation| {
                        observation.good == good
                            && observation.settlement != warehouse.settlement
                            && land_access.allows(warehouse.settlement, observation.settlement)
                            && day.saturating_sub(observation.observed_day)
                                <= TRADE_INTEL_MAX_AGE_DAYS
                    })
                {
                    if unsold_consignments.contains(&(warehouse.id, destination.settlement, good)) {
                        continue;
                    }
                    if existing_lanes.contains(&(
                        company.id,
                        warehouse.settlement,
                        destination.settlement,
                        good,
                    )) {
                        continue;
                    }
                    let Some(destination_market) =
                        market_by_settlement.get(&destination.settlement)
                    else {
                        continue;
                    };
                    let committed = incoming
                        .get(&(destination.settlement, good))
                        .copied()
                        .unwrap_or(0);
                    if let Some(opportunity) = evaluate_merchant_opportunity(
                        source,
                        destination,
                        source_market.position,
                        destination_market.position,
                        company.policy.strategy,
                        warehouse.daily_wage,
                        committed,
                    ) {
                        let notice = mixed_trade_seed(
                            company.id.0
                                ^ opportunity.origin.0.rotate_left(11)
                                ^ opportunity.destination.0.rotate_left(23)
                                ^ (opportunity.good.index() as u64).rotate_left(43)
                                ^ u64::from(day),
                        );
                        candidates.push((notice, *warehouse, opportunity));
                    }
                }

                // A local warehouse is also an import base. Its porter may
                // leave home empty, buy at a learned remote source, and sell
                // back into the company's own settlement. Requiring a depot
                // in the source town made trade with small producer towns—or
                // a zero-population test market—impossible by construction.
                let destination = source_market.observations[good.index()];
                if unsold_consignments.contains(&(warehouse.id, warehouse.settlement, good)) {
                    continue;
                }
                for mut source in knowledge
                    .observations
                    .iter()
                    .copied()
                    .filter(|observation| {
                        observation.good == good
                            && observation.settlement != warehouse.settlement
                            && land_access.allows(warehouse.settlement, observation.settlement)
                            && day.saturating_sub(observation.observed_day)
                                <= TRADE_INTEL_MAX_AGE_DAYS
                    })
                {
                    if existing_lanes.contains(&(
                        company.id,
                        source.settlement,
                        warehouse.settlement,
                        good,
                    )) {
                        continue;
                    }
                    let Some(remote_market) = market_by_settlement.get(&source.settlement) else {
                        continue;
                    };
                    // Trial cargo is a decision variable, not an obligation to
                    // fill a cart. Size a purchase to genuinely free company
                    // cash, then let the ordinary opportunity calculation
                    // reject it if fixed wages and travel make that smaller
                    // load uneconomic.
                    let purchase_limit =
                        uncertain_purchase_limit(source.asking_price, source.confidence);
                    source.listed_units = source.listed_units.min(
                        (available_cash / purchase_limit.max(1)).min(u64::from(u32::MAX)) as u32,
                    );
                    let committed = incoming
                        .get(&(warehouse.settlement, good))
                        .copied()
                        .unwrap_or(0);
                    if let Some(opportunity) = evaluate_merchant_opportunity(
                        source,
                        destination,
                        remote_market.position,
                        source_market.position,
                        company.policy.strategy,
                        warehouse.daily_wage,
                        committed,
                    ) {
                        let notice = mixed_trade_seed(
                            company.id.0
                                ^ opportunity.origin.0.rotate_left(11)
                                ^ opportunity.destination.0.rotate_left(23)
                                ^ (opportunity.good.index() as u64).rotate_left(43)
                                ^ u64::from(day),
                        );
                        candidates.push((notice, *warehouse, opportunity));
                    }
                }
            }
        }
        candidates.sort_unstable_by_key(|(notice, warehouse, opportunity)| {
            (*notice, warehouse.id, opportunity.good.index())
        });
        candidates.truncate(attention_budget);
        let Some((_, warehouse, opportunity)) = candidates
            .into_iter()
            .max_by_key(|(_, _, opportunity)| opportunity.expected_profit)
        else {
            if diagnostics && !company_warehouses.is_empty() {
                let bread_knowledge: Vec<_> = knowledge
                    .observations
                    .iter()
                    .filter(|observation| observation.good == Good::Bread)
                    .copied()
                    .collect();
                eprintln!(
                    "MERCHANT REVIEW day={day} company=#{} blocked=no-candidate cash={} free={} porters={} warehouses={company_warehouses:?} bread_knowledge={bread_knowledge:?}",
                    company.id.0, company.account.cash, available_cash, route_capacity,
                );
            }
            continue;
        };
        let load_owned_stock = warehouse.settlement == opportunity.origin
            && warehouse.stock[opportunity.good.index()] > 0;
        let required_cash = if load_owned_stock {
            0
        } else {
            opportunity
                .maximum_purchase_price
                .saturating_mul(u64::from(opportunity.cargo_units))
        };
        if required_cash > available_cash {
            if diagnostics {
                eprintln!(
                    "MERCHANT REVIEW day={day} company=#{} blocked=cargo-cash free={} required={} opportunity={opportunity:?}",
                    company.id.0, available_cash, required_cash,
                );
            }
            continue;
        }
        if diagnostics {
            eprintln!(
                "MERCHANT REVIEW day={day} company=#{} action=open-route free={} required={} warehouse=#{} opportunity={opportunity:?}",
                company.id.0, available_cash, required_cash, warehouse.id.0,
            );
        }
        let schedule = TradeRouteSchedule::new([
            TradeRouteStop {
                settlement: opportunity.origin,
                action: if load_owned_stock {
                    TradeRouteStopAction::Load
                } else {
                    TradeRouteStopAction::Buy
                },
            },
            TradeRouteStop {
                settlement: opportunity.destination,
                action: TradeRouteStopAction::Sell,
            },
        ])
        .expect("two-stop autonomous timetable is valid");
        commands.spawn((
            CompanyTradeRoute {
                company: company.id,
                warehouse: warehouse.id,
                mode: TradeRouteMode::Merchant,
                origin: opportunity.origin,
                destination: opportunity.destination,
                good: opportunity.good,
                cargo_target: opportunity.cargo_units,
                maximum_purchase_price: opportunity.maximum_purchase_price,
                minimum_destination_price: opportunity.minimum_sale_price,
                // One embodied trial circuit at a time. The daily Company
                // Master review decides whether another should depart.
                automatic: false,
                autonomous_management: true,
                expected_trip_profit: opportunity.expected_profit,
                decision_confidence: opportunity.confidence,
                active_contract: None,
                assigned_caravaner: None,
                current_stop: 0,
                status: TradeRouteStatus::WaitingForPorter,
                completed_trips: 0,
                lifetime_units: 0,
                lifetime_delivery_revenue: 0,
                lifetime_purchase_cost: 0,
                lifetime_consigned_value: 0,
            },
            schedule,
            TradeRouteHistory::default(),
            AutonomousMerchantRoute {
                last_review_day: day,
                mothballed_day: None,
                last_completed_trips: 0,
                disappointing_reviews: 0,
                expected_trip_profit: opportunity.expected_profit,
                confidence: opportunity.confidence,
                unsold_since_day: None,
                minimum_cargo_net: opportunity.minimum_cargo_net,
                freight_cost: opportunity.freight_cost,
                minimum_return_bps: opportunity.minimum_return_bps,
                destination_fee_bps: opportunity.destination_fee_bps,
            },
            Replicate::to_clients(NetworkTarget::All),
        ));
        *routes_per_company.entry(company.id).or_default() += 1;
        *active_routes_per_company.entry(company.id).or_default() += 1;
        *active_routes_by_warehouse.entry(warehouse.id).or_default() += 1;
        *incoming
            .entry((opportunity.destination, opportunity.good))
            .or_default() += opportunity.cargo_units;
        existing_lanes.insert((
            company.id,
            opportunity.origin,
            opportunity.destination,
            opportunity.good,
        ));
        info!(
            "Company #{} opened a bounded {} merchant trial from settlement #{} to #{}: {} units, expected {} coin/trip, confidence {}%",
            company.id.0,
            opportunity.good.label(),
            opportunity.origin.0,
            opportunity.destination.0,
            opportunity.cargo_units,
            shared::economy::format_money(opportunity.expected_profit.max(0) as u64),
            opportunity.confidence,
        );
    }
}

/// The Town Works is the buyer. It checks its own exchange first; only the
/// uncovered part becomes an import order. Escrow is removed from treasury at
/// posting, so a town cannot promise the same coin to payroll or two carriers.
#[allow(clippy::type_complexity)]
pub fn post_civic_import_contracts(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    land_networks: Query<(&SettlementId, &FoundingLandNetwork), With<Settlement>>,
    projects: Query<(
        &CivicHallUpgradeWorksite,
        &shared::components::BuildingOf,
        &GoodsInventory,
        &ConstructionSite,
    )>,
    mut halls: Query<(
        &shared::components::SettlementId,
        &mut Settlement,
        &GoodsInventory,
        &MootMarket,
        &PlayerPosition,
    )>,
    warehouses: Query<(
        &shared::components::BuildingOf,
        &SettlementBuilding,
        Option<&BusinessCondition>,
        Option<&BusinessWagePolicy>,
    )>,
    contracts: Query<&CivicTradeContract>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let land_access = LandTradeAccess::from_tags(land_networks.iter());
    let active: HashSet<_> = contracts
        .iter()
        .filter(|contract| civic_review::blocks_new_tender(contract, day))
        .map(|contract| (contract.destination, contract.good))
        .collect();
    if !projects
        .iter()
        .any(|(project, building_of, inventory, site)| {
            !site.raising
                && !active.contains(&(building_of.0, project.material))
                && project.material_required > inventory.amount(project.material)
        })
    {
        return;
    }

    let source_offers: Vec<_> = halls
        .iter()
        .flat_map(|(settlement, _, inventory, market, _)| {
            let regional = market.supports_regional_trade();
            market.listings().iter().filter_map(move |listing| {
                let MarketSeller::Business(_) = listing.seller else {
                    return None;
                };
                let physical = inventory.amount(listing.good);
                (regional && listing.units > 0 && physical > 0).then_some((
                    *settlement,
                    listing.seller,
                    listing.good,
                    listing.unit_price,
                    listing.units.min(physical),
                ))
            })
        })
        .collect();
    let hall_snapshots: HashMap<_, _> = halls
        .iter()
        .filter_map(|(settlement, _, _, market, position)| {
            market.supports_regional_trade().then_some((
                *settlement,
                HallSnapshot {
                    position: position.0,
                    rotation: 0.0,
                },
            ))
        })
        .collect();
    let mut carrier_wages = HashMap::<SettlementId, u64>::new();
    for (settlement, building, condition, wage) in warehouses.iter() {
        if building.kind == SettlementBuildingKind::StorageHall
            && condition.is_none_or(|condition| condition.state.can_operate())
        {
            let wage = wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage);
            carrier_wages
                .entry(settlement.0)
                .and_modify(|current| *current = (*current).min(wage))
                .or_insert(wage);
        }
    }

    for (project, building_of, inventory, site) in projects.iter() {
        if site.raising || active.contains(&(building_of.0, project.material)) {
            continue;
        }
        let remaining = project
            .material_required
            .saturating_sub(inventory.amount(project.material));
        if remaining == 0 {
            continue;
        }
        let Some((_, mut destination, destination_store, destination_market, destination_at)) =
            halls
                .iter_mut()
                .find(|(settlement, ..)| **settlement == building_of.0)
        else {
            continue;
        };
        if !destination_market.supports_regional_trade() {
            continue;
        }
        let local_private_units = destination_market
            .listings()
            .iter()
            .filter(|listing| {
                listing.good == project.material
                    && matches!(listing.seller, MarketSeller::Business(_))
            })
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
            .min(destination_store.amount(project.material));
        let import_units = remaining.saturating_sub(local_private_units);
        if import_units == 0 {
            continue;
        }

        let journey_allowances = civic_review::journey_allowances(
            HallSnapshot {
                position: destination_at.0,
                rotation: 0.0,
            },
            &hall_snapshots,
        );
        let carrier_cost = |origin: SettlementId| {
            carrier_wages
                .get(&origin)
                .copied()
                .unwrap_or(FOUNDING_DAILY_WAGE)
                .saturating_add(journey_allowances.get(&origin).copied().unwrap_or(0))
        };
        let source_offer = source_offers
            .iter()
            .copied()
            .filter(|(origin, _, good, _, _)| {
                *origin != building_of.0
                    && *good == project.material
                    && land_access.allows(*origin, building_of.0)
            })
            .filter_map(|(origin, seller, _, price, units)| {
                civic_review::affordable_quote(
                    project.material,
                    import_units.min(units),
                    price,
                    destination.treasury,
                    carrier_cost(origin),
                )
                .map(|quote| (origin, seller, quote))
            })
            .min_by(civic_review::quote_order);
        // An unbound tender exists to invite production in *another*
        // settlement. With no possible remote origin it would only remove the
        // buyer's money from circulation and suppress repeated local purchase
        // attempts forever. Keep the treasury liquid and let the local market
        // publish its ordinary unmet-demand signal instead.
        if source_offer.is_none()
            && !hall_snapshots.keys().any(|settlement| {
                *settlement != building_of.0 && land_access.allows(*settlement, building_of.0)
            })
        {
            continue;
        }
        let Some(quote) = source_offer.map(|(_, _, quote)| quote).or_else(|| {
            hall_snapshots
                .keys()
                .copied()
                .filter(|origin| {
                    *origin != building_of.0 && land_access.allows(*origin, building_of.0)
                })
                .filter_map(|origin| {
                    civic_review::affordable_quote(
                        project.material,
                        import_units,
                        project.material.base_price(),
                        destination.treasury,
                        carrier_cost(origin),
                    )
                })
                .min_by_key(|quote| {
                    (
                        quote.escrow_cash / u64::from(quote.units),
                        std::cmp::Reverse(quote.units),
                    )
                })
        }) else {
            continue;
        };
        let civic_review::CivicQuote {
            units,
            maximum_unit_price,
            delivery_fee_per_bulk,
            escrow_cash: reserved_cash,
        } = quote;
        destination.treasury -= reserved_cash;
        commands.spawn((
            CivicTradeContract {
                origin: source_offer.map(|(origin, ..)| origin),
                destination: building_of.0,
                good: project.material,
                source_seller: source_offer.map(|(_, seller, _)| seller),
                requested_units: units,
                delivered_units: 0,
                maximum_unit_price,
                delivery_fee_per_bulk,
                reserved_cash,
                escrow_cash: reserved_cash,
                spent_on_goods: 0,
                spent_on_freight: 0,
                created_day: day,
                last_attempt_day: u32::MAX,
                status: TradeContractStatus::Open,
            },
            Replicate::to_clients(NetworkTarget::All),
        ));
        if let Some((origin, ..)) = source_offer {
            info!(
                "Settlement '{}': posted a buyer-funded import contract for {} {} from settlement #{} ({} coin reserved)",
                destination.name,
                units,
                project.material.label(),
                origin.0,
                shared::economy::format_money(reserved_cash),
            );
        } else {
            info!(
                "Settlement '{}': posted the first cash-backed tender for {} {} at up to {} each ({} coin reserved)",
                destination.name,
                units,
                project.material.label(),
                shared::economy::format_money(maximum_unit_price),
                shared::economy::format_money(reserved_cash),
            );
        }
    }
}

/// Translate open contracts into ordinary company assets. A completed and
/// staffed Storage Hall is mandatory, and the route's expected fee must cover
/// one normal wage plus a small distance allowance. No company is tagged as a
/// special merchant type.
#[allow(clippy::type_complexity)]
pub fn manage_company_trade_routes(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut next_review_world_seconds: Local<f64>,
    land_networks: Query<(&SettlementId, &FoundingLandNetwork), With<Settlement>>,
    mut halls: Query<
        (
            Entity,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &GoodsInventory,
            &MootMarket,
            &mut Settlement,
        ),
        With<Settlement>,
    >,
    warehouses: Query<(
        Entity,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &shared::components::OperatedBy,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&BusinessCondition>,
        Option<&BusinessWagePolicy>,
    )>,
    mut contracts: Query<(Entity, &TradeContractId, &mut CivicTradeContract)>,
    mut routes: Query<
        (
            Entity,
            &TradeRouteId,
            &mut CompanyTradeRoute,
            Option<&TradeRouteSchedule>,
        ),
        Without<MaritimeTradeRoute>,
    >,
    porters: Query<
        (
            Entity,
            &shared::components::PersonId,
            &CompanyPorter,
            &GoodsInventory,
            Option<&TradeRouteRoutine>,
            Option<&InternalDeliveryRoutine>,
            Option<&MarketCollectionRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&MootQueueTicket>,
            Option<&MootMealRoutine>,
        ),
        With<CharacterKind>,
    >,
    dispatch_busy: Query<(), super::worker_activity::TransportStartBlocked>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let now = absolute_world_seconds(clock);
    if now + f64::EPSILON < *next_review_world_seconds {
        return;
    }
    *next_review_world_seconds = now + ROUTE_MANAGEMENT_INTERVAL_WORLD_SECONDS;
    let day = clock.day;
    let land_access = LandTradeAccess::from_tags(land_networks.iter());
    let hall_snapshots: HashMap<_, _> = halls
        .iter()
        .map(|(_entity, id, position, rotation, ..)| {
            (
                *id,
                HallSnapshot {
                    position: position.0,
                    rotation: rotation.map_or(0.0, |rotation| rotation.0),
                },
            )
        })
        .collect();
    let regional_markets: HashSet<_> = halls
        .iter()
        .filter_map(|(_, id, _, _, _, market, _)| market.supports_regional_trade().then_some(*id))
        .collect();
    let source_offers: Vec<_> = halls
        .iter()
        .flat_map(|(_, settlement, _, _, inventory, market, _)| {
            let regional = market.supports_regional_trade();
            market.listings().iter().filter_map(move |listing| {
                let MarketSeller::Business(_) = listing.seller else {
                    return None;
                };
                let physical = inventory.amount(listing.good);
                (regional && listing.units > 0 && physical > 0).then_some((
                    *settlement,
                    listing.seller,
                    listing.good,
                    listing.unit_price,
                    listing.units.min(physical),
                ))
            })
        })
        .collect();
    let warehouse_snapshots: Vec<_> = warehouses
        .iter()
        .filter(|(_, _, _, _, building, ..)| building.kind == SettlementBuildingKind::StorageHall)
        .map(
            |(_entity, id, building_of, company, _, position, rotation, condition, _)| {
                WarehouseSnapshot {
                    id: *id,
                    settlement: building_of.0,
                    company: company.0,
                    position: position.0,
                    rotation: rotation.0,
                    can_operate: condition.is_none_or(|condition| condition.state.can_operate()),
                }
            },
        )
        .collect();

    let staffed_warehouses: HashSet<_> = porters
        .iter()
        .map(|(_, _, porter, ..)| (porter.company, porter.storage_hall))
        .collect();
    let warehouse_wages: HashMap<_, _> = warehouses
        .iter()
        .map(|(_, id, _, _, _, _, _, _, wage)| {
            (
                *id,
                wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage),
            )
        })
        .collect();
    let mut carrier_wages = HashMap::<SettlementId, u64>::new();
    for warehouse in &warehouse_snapshots {
        if warehouse.can_operate && staffed_warehouses.contains(&(warehouse.company, warehouse.id))
        {
            let wage = warehouse_wages[&warehouse.id];
            carrier_wages
                .entry(warehouse.settlement)
                .and_modify(|current| *current = (*current).min(wage))
                .or_insert(wage);
        }
    }

    // Repair assignments whose worker disappeared or lost the job. Cargo is
    // protected by the worker's carried inventory while alive; before pickup,
    // an abandoned assignment can safely return to the open board.
    let active_route_ids: HashSet<_> = porters
        .iter()
        .filter_map(|(_, _, _, _, routine, ..)| routine.map(|routine| routine.route))
        .collect();
    for (_, route_id, mut route, _) in routes.iter_mut() {
        if route.assigned_caravaner.is_some() && !active_route_ids.contains(route_id) {
            route.assigned_caravaner = None;
            if let Some(contract_id) = route.active_contract {
                if let Some((_, _, mut contract)) =
                    contracts.iter_mut().find(|(_, id, _)| **id == contract_id)
                {
                    match contract.status {
                        TradeContractStatus::Assigned | TradeContractStatus::Open => {
                            contract.status = TradeContractStatus::Open;
                            route.status = TradeRouteStatus::WaitingForPorter;
                        }
                        TradeContractStatus::InTransit => {
                            // Never purchase a second load merely because an
                            // embodied caravaner disappeared. Cargo recovery
                            // can later resume this route explicitly; until
                            // then both the destination's ownership claim and
                            // the remaining escrow stay intact and auditable.
                            route.status = TradeRouteStatus::Mothballed;
                            warn!(
                                "Company #{} mothballed route #{}: porter vanished with buyer-owned cargo in transit",
                                route.company.0, route_id.0,
                            );
                        }
                        TradeContractStatus::Fulfilled | TradeContractStatus::Cancelled => {
                            route.active_contract = None;
                            route.status = TradeRouteStatus::Idle;
                        }
                    }
                } else {
                    route.active_contract = None;
                    route.status = TradeRouteStatus::Idle;
                }
            } else {
                route.status = TradeRouteStatus::Idle;
            }
        }
    }

    let route_contracts: HashSet<_> = routes
        .iter()
        .filter_map(|(_, _, route, _)| route.active_contract)
        .collect();
    for (_, contract_id, mut contract) in contracts.iter_mut() {
        if contract.status != TradeContractStatus::Open
            || route_contracts.contains(contract_id)
            || contract.last_attempt_day == day
        {
            continue;
        }
        // Route formation is a daily company decision, not a per-frame poll.
        // A missing warehouse or employee can change on a later day without
        // making every Activity tick rescan all companies in the meantime.
        contract.last_attempt_day = day;
        let Some((_, _, _, _, _, _, mut buyer)) = halls
            .iter_mut()
            .find(|(_, id, ..)| **id == contract.destination)
        else {
            // Preserve the liability until the receiving treasury exists.
            continue;
        };
        if civic_review::expire_open_tender(&mut contract, &mut buyer.treasury, day) {
            continue;
        }
        if !regional_markets.contains(&contract.destination) {
            continue;
        }
        let Some(destination) = hall_snapshots.get(&contract.destination).copied() else {
            continue;
        };
        let journey_allowances = civic_review::journey_allowances(destination, &hall_snapshots);
        let available_cash = buyer.treasury.saturating_add(contract.escrow_cash);
        // Offers may change before collection. Once per day compare the
        // current complete landed quotes, including the ordinary employee's
        // actual wage, and move only the additional affordable reservation.
        let Some((origin_id, seller, quote)) = source_offers
            .iter()
            .copied()
            .filter(|(origin, _, good, _, _)| {
                *origin != contract.destination
                    && land_access.allows(*origin, contract.destination)
                    && *good == contract.good
            })
            .filter_map(|(origin, seller, _, price, units)| {
                let journey = journey_allowances.get(&origin)?;
                let wage = carrier_wages
                    .get(&origin)
                    .copied()
                    .unwrap_or(FOUNDING_DAILY_WAGE);
                civic_review::affordable_quote(
                    contract.good,
                    contract.remaining_units().min(units),
                    price,
                    available_cash,
                    wage.saturating_add(*journey),
                )
                .map(|quote| (origin, seller, quote))
            })
            .min_by(|a, b| {
                // A quote with real transport can fulfil the public need
                // now. An unstaffed source remains a production tender only
                // while no affordable staffed alternative exists.
                carrier_wages
                    .contains_key(&b.0)
                    .cmp(&carrier_wages.contains_key(&a.0))
                    .then_with(|| civic_review::quote_order(a, b))
            })
        else {
            continue;
        };
        if !civic_review::apply_quote(&mut contract, &mut buyer.treasury, quote) {
            continue;
        }
        contract.origin = Some(origin_id);
        contract.source_seller = Some(seller);
        let fee = u64::from(contract.remaining_units())
            .saturating_mul(u64::from(contract.good.bulk_per_unit()))
            .saturating_mul(contract.delivery_fee_per_bulk);
        let journey_allowance = journey_allowances[&origin_id];
        let Some(warehouse) = warehouse_snapshots
            .iter()
            .copied()
            .filter(|warehouse| warehouse.can_operate && warehouse.settlement == origin_id)
            .filter(|warehouse| staffed_warehouses.contains(&(warehouse.company, warehouse.id)))
            .filter(|warehouse| {
                fee >= warehouse_wages[&warehouse.id].saturating_add(journey_allowance)
            })
            .min_by_key(|warehouse| (warehouse.company, warehouse.id))
        else {
            continue;
        };

        let reusable = routes
            .iter_mut()
            .find(|(_, _, route, _)| {
                route.mode == TradeRouteMode::ContractCarrier
                    && route.company == warehouse.company
                    && route.warehouse == warehouse.id
                    && route.origin == origin_id
                    && route.destination == contract.destination
                    && route.good == contract.good
                    && route.active_contract.is_none()
                    && matches!(
                        route.status,
                        TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                    )
            })
            .map(|(_, _, route, _)| route);
        if let Some(mut route) = reusable {
            route.cargo_target = contract.remaining_units();
            route.maximum_purchase_price = contract.maximum_unit_price;
            route.active_contract = Some(*contract_id);
            route.status = TradeRouteStatus::WaitingForPorter;
        } else {
            commands.spawn((
                CompanyTradeRoute {
                    company: warehouse.company,
                    warehouse: warehouse.id,
                    mode: TradeRouteMode::ContractCarrier,
                    origin: origin_id,
                    destination: contract.destination,
                    good: contract.good,
                    cargo_target: contract.remaining_units(),
                    maximum_purchase_price: contract.maximum_unit_price,
                    minimum_destination_price: 0,
                    automatic: true,
                    autonomous_management: false,
                    expected_trip_profit: 0,
                    decision_confidence: 100,
                    active_contract: Some(*contract_id),
                    assigned_caravaner: None,
                    current_stop: 0,
                    status: TradeRouteStatus::WaitingForPorter,
                    completed_trips: 0,
                    lifetime_units: 0,
                    lifetime_delivery_revenue: 0,
                    lifetime_purchase_cost: 0,
                    lifetime_consigned_value: 0,
                },
                TradeRouteSchedule::contract(origin_id, contract.destination),
                TradeRouteHistory::default(),
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
        contract.status = TradeContractStatus::Assigned;
    }

    // Dispatch only after a route has received its stable id (normally the
    // update after creation). The same porter remains an ordinary employee of
    // the warehouse and therefore continues through normal payroll.
    for (_, route_id, mut route, _) in routes.iter_mut() {
        if route.status != TradeRouteStatus::WaitingForPorter
            || route.assigned_caravaner.is_some()
            || route.active_contract.is_none()
            || route.mode != TradeRouteMode::ContractCarrier
            || !land_access.allows(route.origin, route.destination)
        {
            continue;
        }
        let Some(origin) = hall_snapshots.get(&route.origin).copied() else {
            continue;
        };
        let candidate = porters
            .iter()
            .filter(
                |(
                    entity,
                    _,
                    porter,
                    inventory,
                    routine,
                    internal,
                    market,
                    home,
                    road,
                    queue,
                    meal,
                )| {
                    !dispatch_busy.contains(*entity)
                        && porter.company == route.company
                        && porter.storage_hall == route.warehouse
                        && inventory.is_empty()
                        && routine.is_none()
                        && internal.is_none()
                        && market.is_none()
                        && home.is_none()
                        && road.is_none()
                        && queue.is_none()
                        && meal.is_none()
                },
            )
            .min_by_key(|(_, person, ..)| **person);
        let Some((porter_entity, person, ..)) = candidate else {
            continue;
        };
        route.assigned_caravaner = Some(*person);
        route.status = TradeRouteStatus::GoingToOrigin;
        commands.entity(porter_entity).insert((
            TradeRouteRoutine {
                route: *route_id,
                mode: TradeRouteMode::ContractCarrier,
                phase: TradeRoutePhase::GoingToOrigin,
                stop_index: 0,
                stops_visited: 0,
                departed_day: day,
                departed_world_seconds: now,
                source_purchase_cost: 0,
                source_market_fees: 0,
                cargo_units: 0,
                consigned_value: 0,
                failed_approaches: 0,
            },
            MoveTarget(hall_entrance(origin)),
            CharacterActivity::Idle,
        ));
        info!(
            "Company #{} dispatched porter #{} on route #{} from settlement #{} to #{}",
            route.company.0, person.0, route_id.0, route.origin.0, route.destination.0,
        );
    }

    // Player-authored merchant routes use the same Storage Hall employees and
    // physical carts, but execute an ordered timetable rather than borrowing
    // civic escrow. `automatic=false` still permits one explicit dispatch;
    // after that circuit the porter returns home and the route remains idle.
    for (_, route_id, mut route, schedule) in routes.iter_mut() {
        if route.mode != TradeRouteMode::Merchant
            || route.status != TradeRouteStatus::WaitingForPorter
            || route.assigned_caravaner.is_some()
            || route.active_contract.is_some()
        {
            continue;
        }
        let Some(first_stop) = schedule
            .and_then(|schedule| schedule.stops().first())
            .copied()
        else {
            route.status = TradeRouteStatus::Mothballed;
            continue;
        };
        if schedule.is_some_and(|schedule| {
            land_access.validate_schedule(schedule.stops()).is_err()
                || schedule
                    .stops()
                    .iter()
                    .any(|stop| !regional_markets.contains(&stop.settlement))
        }) {
            route.status = TradeRouteStatus::Mothballed;
            continue;
        }
        let first_target = if matches!(
            first_stop.action,
            shared::components::TradeRouteStopAction::Load
                | shared::components::TradeRouteStopAction::Unload
        ) {
            let Some(warehouse) = warehouse_snapshots.iter().find(|warehouse| {
                warehouse.id == route.warehouse
                    && warehouse.company == route.company
                    && warehouse.settlement == first_stop.settlement
                    && warehouse.can_operate
            }) else {
                route.status = TradeRouteStatus::Mothballed;
                continue;
            };
            SettlementBuildingKind::StorageHall
                .entrance_position(warehouse.position, warehouse.rotation)
        } else {
            let Some(hall) = hall_snapshots.get(&first_stop.settlement).copied() else {
                route.status = TradeRouteStatus::Mothballed;
                continue;
            };
            hall_entrance(hall)
        };
        let candidate = porters
            .iter()
            .filter(
                |(
                    entity,
                    _,
                    porter,
                    inventory,
                    routine,
                    internal,
                    market,
                    home,
                    road,
                    queue,
                    meal,
                )| {
                    !dispatch_busy.contains(*entity)
                        && porter.company == route.company
                        && porter.storage_hall == route.warehouse
                        && inventory.is_empty()
                        && routine.is_none()
                        && internal.is_none()
                        && market.is_none()
                        && home.is_none()
                        && road.is_none()
                        && queue.is_none()
                        && meal.is_none()
                },
            )
            .min_by_key(|(_, person, ..)| **person);
        let Some((porter_entity, person, ..)) = candidate else {
            continue;
        };
        route.assigned_caravaner = Some(*person);
        route.current_stop = 0;
        route.status = TradeRouteStatus::GoingToOrigin;
        commands.entity(porter_entity).insert((
            TradeRouteRoutine {
                route: *route_id,
                mode: TradeRouteMode::Merchant,
                phase: TradeRoutePhase::MerchantTravellingToStop,
                stop_index: 0,
                stops_visited: 0,
                departed_day: day,
                departed_world_seconds: now,
                source_purchase_cost: 0,
                source_market_fees: 0,
                cargo_units: 0,
                consigned_value: 0,
                failed_approaches: 0,
            },
            MoveTarget(first_target),
            CharacterActivity::Idle,
        ));
        info!(
            "Company #{} dispatched porter #{} on merchant route #{} with {} stops",
            route.company.0,
            person.0,
            route_id.0,
            schedule.map_or(0, |schedule| schedule.stops().len()),
        );
    }
}

/// Advance one embodied collection, outward haul and return journey. The
/// route performs no all-town scans per porter; stable ids join each bounded
/// route/contract set, while the normal navigation queue owns pathfinding.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_company_trade_routes(
    _simulation_time: crate::world::simulation_time::SimulationTime,
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut Settlement,
            &mut GoodsInventory,
            &mut MootMarket,
            Option<&mut shared::economy::CivicAccount>,
        ),
        (
            With<Settlement>,
            Without<SettlementBuilding>,
            Without<CharacterKind>,
        ),
    >,
    mut projects: Query<
        (
            &CivicHallUpgradeWorksite,
            &shared::components::BuildingOf,
            &ConstructionSite,
            &mut GoodsInventory,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    mut warehouses: Query<
        (
            &shared::components::BuildingId,
            &shared::components::OperatedBy,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &mut BusinessAccount,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut contracts: Query<(&TradeContractId, &mut CivicTradeContract)>,
    mut routes: Query<
        (
            &TradeRouteId,
            &mut CompanyTradeRoute,
            &mut TradeRouteHistory,
        ),
        Without<MaritimeTradeRoute>,
    >,
    mut porters: Query<
        (
            Entity,
            &shared::components::PersonId,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            &mut TradeRouteRoutine,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
    mut road_traffic: Option<ResMut<crate::world::regional_roads::RegionalRoadTraffic>>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let now = absolute_world_seconds(clock);
    if porters.is_empty() {
        return;
    }

    for (
        porter_entity,
        person,
        position,
        mut activity,
        mut carrier,
        move_target,
        mut routine,
        route_failed,
    ) in porters.iter_mut()
    {
        let Some((_, mut route, mut history)) =
            routes.iter_mut().find(|(id, ..)| **id == routine.route)
        else {
            commands.entity(porter_entity).remove::<TradeRouteRoutine>();
            continue;
        };
        if routine.mode != TradeRouteMode::ContractCarrier {
            continue;
        }

        let traffic_leg = crate::world::regional_roads::TradeLeg {
            route: routine.route,
            cycle: route.completed_trips,
            stop: 1,
        };
        if routine.phase == TradeRoutePhase::InTransit && carrier.amount(route.good) > 0 {
            if let Some(traffic) = road_traffic.as_deref_mut() {
                traffic.observe(
                    traffic_leg,
                    route.origin,
                    route.destination,
                    position.0.xz(),
                    day,
                );
            }
        }

        // Returning is deliberately independent of the contract entity. A
        // fulfilled/cancelled contract may be archived or removed while the
        // embodied porter is still walking home; that must never strand the
        // employee in a permanent route routine.
        let warehouse =
            warehouses
                .iter_mut()
                .find_map(|(id, company, building, at, rotation, _)| {
                    (*id == route.warehouse
                        && company.0 == route.company
                        && building.kind == SettlementBuildingKind::StorageHall)
                        .then_some(WarehouseSnapshot {
                            id: *id,
                            settlement: route.origin,
                            company: company.0,
                            position: at.0,
                            rotation: rotation.0,
                            can_operate: true,
                        })
                });
        let Some(warehouse) = warehouse else {
            continue;
        };
        if routine.phase == TradeRoutePhase::ReturningToOrigin {
            let Some(origin) = halls.iter().find_map(|(id, at, rotation, ..)| {
                (*id == route.origin).then_some(HallSnapshot {
                    position: at.0,
                    rotation: rotation.map_or(0.0, |rotation| rotation.0),
                })
            }) else {
                continue;
            };
            let target = hall_trade_approach(origin, routine.failed_approaches);
            if route_failed.is_some() {
                retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                    hall_trade_approach(origin, attempt)
                });
                continue;
            }
            if ground_distance(position.0, target) > ROUTE_REACH {
                ensure_move_target(&mut commands, porter_entity, move_target, target);
                continue;
            }
            routine.phase = TradeRoutePhase::ReturningToWarehouse;
            routine.failed_approaches = 0;
            commands
                .entity(porter_entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        }
        if routine.phase == TradeRoutePhase::ReturningToWarehouse {
            let target = warehouse_trade_approach(warehouse, routine.failed_approaches);
            if route_failed.is_some() {
                retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                    warehouse_trade_approach(warehouse, attempt)
                });
                continue;
            }
            if ground_distance(position.0, target) > ROUTE_REACH {
                ensure_move_target(&mut commands, porter_entity, move_target, target);
                continue;
            }
            route.assigned_caravaner = None;
            route.active_contract = None;
            route.status = TradeRouteStatus::Idle;
            activity.set_if_neq(CharacterActivity::Idle);
            commands
                .entity(porter_entity)
                .remove::<TradeRouteRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            debug!(
                "Company #{} route #{} porter #{} returned to its warehouse",
                route.company.0, routine.route.0, person.0,
            );
            continue;
        }
        let Some(contract_id) = route.active_contract else {
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::ReturningToOrigin;
            continue;
        };
        let Some((_, mut contract)) = contracts.iter_mut().find(|(id, _)| **id == contract_id)
        else {
            route.active_contract = None;
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::ReturningToOrigin;
            continue;
        };

        let origin = halls.iter().find_map(|(id, at, rotation, ..)| {
            (*id == route.origin).then_some(HallSnapshot {
                position: at.0,
                rotation: rotation.map_or(0.0, |rotation| rotation.0),
            })
        });
        let destination = halls.iter().find_map(|(id, at, rotation, ..)| {
            (*id == route.destination).then_some(HallSnapshot {
                position: at.0,
                rotation: rotation.map_or(0.0, |rotation| rotation.0),
            })
        });
        let Some((origin, destination)) = origin.zip(destination) else {
            continue;
        };

        if route_failed.is_some() {
            // Like merchant and return legs, public carriers may approach a
            // shared doorway/worksite from another nearby loading bay. The
            // attempt count is finite; once exhausted, shared navigation keeps
            // its exponential backoff for the last goal. Neither retry path
            // changes the transaction or the buyer-owned cargo on the porter.
            if routine.phase == TradeRoutePhase::GoingToOrigin {
                retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                    hall_trade_approach(origin, attempt)
                });
            } else if routine.phase == TradeRoutePhase::InTransit {
                let site = projects.iter().find_map(|(project, building_of, site, _)| {
                    (building_of.0 == route.destination && project.material == route.good)
                        .then_some(site)
                });
                retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                    contract_delivery_approach(destination, site, attempt)
                });
            }
            continue;
        }

        match routine.phase {
            TradeRoutePhase::GoingToOrigin => {
                let regional_endpoints = halls.iter().all(|(id, _, _, _, _, market, _)| {
                    (*id != route.origin && *id != route.destination)
                        || market.supports_regional_trade()
                });
                if !regional_endpoints {
                    if let Some((_, _, _, mut destination_settlement, _, _, _)) =
                        halls.iter_mut().find(|(id, ..)| **id == route.destination)
                    {
                        destination_settlement.treasury = destination_settlement
                            .treasury
                            .saturating_add(contract.escrow_cash);
                        contract.escrow_cash = 0;
                    }
                    contract.status = TradeContractStatus::Cancelled;
                    route.status = TradeRouteStatus::Returning;
                    routine.phase = TradeRoutePhase::ReturningToWarehouse;
                    routine.failed_approaches = 0;
                    warn!(
                        "Company #{} cancelled contract route #{} because a public endpoint has no completed Marketplace",
                        route.company.0, routine.route.0,
                    );
                    ensure_move_target(
                        &mut commands,
                        porter_entity,
                        move_target,
                        SettlementBuildingKind::StorageHall
                            .entrance_position(warehouse.position, warehouse.rotation),
                    );
                    continue;
                }
                let target = hall_trade_approach(origin, routine.failed_approaches);
                if ground_distance(position.0, target) > ROUTE_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, target);
                    continue;
                }
                commands.entity(porter_entity).remove::<MoveTarget>();
                route.status = TradeRouteStatus::Loading;
                route.current_stop = 0;
                let wanted = contract
                    .remaining_units()
                    .min(route.cargo_target)
                    .min(carrier.free_bulk() / route.good.bulk_per_unit().max(1));
                let delivery_reserve = u64::from(wanted)
                    .saturating_mul(u64::from(route.good.bulk_per_unit()))
                    .saturating_mul(contract.delivery_fee_per_bulk);
                let purchase_budget = contract.escrow_cash.saturating_sub(delivery_reserve);
                let Some((source_id, _, _, _, mut source_store, mut source_market, _)) =
                    halls.iter_mut().find(|(id, ..)| **id == route.origin)
                else {
                    continue;
                };
                let requested = wanted.min(source_store.amount(route.good));
                let purchase = source_market.purchase_from_seller_for_resale(
                    contract
                        .source_seller
                        .expect("a dispatched contract must have a bound seller"),
                    route.good,
                    requested,
                    purchase_budget,
                    Some(route.maximum_purchase_price),
                );
                if purchase.trade.units == 0 {
                    // A named offer is not a reservation: local households or
                    // processors may legitimately buy it before the wagon
                    // arrives. Close and refund this stale contract so the
                    // civic buyer can post a fresh order against the current
                    // best real seller on the next planning pass.
                    if let Some((_, _, _, mut destination_settlement, _, _, _)) =
                        halls.iter_mut().find(|(id, ..)| **id == route.destination)
                    {
                        destination_settlement.treasury = destination_settlement
                            .treasury
                            .saturating_add(contract.escrow_cash);
                        contract.escrow_cash = 0;
                    }
                    contract.status = TradeContractStatus::Cancelled;
                    contract.last_attempt_day = day;
                    route.status = TradeRouteStatus::Returning;
                    routine.phase = TradeRoutePhase::ReturningToWarehouse;
                    routine.failed_approaches = 0;
                    ensure_move_target(
                        &mut commands,
                        porter_entity,
                        move_target,
                        SettlementBuildingKind::StorageHall
                            .entrance_position(warehouse.position, warehouse.rotation),
                    );
                    continue;
                }
                let removed = source_store.remove(route.good, purchase.trade.units);
                let loaded = carrier.add(route.good, removed);
                debug_assert_eq!(loaded, purchase.trade.units);
                contract.escrow_cash = contract.escrow_cash.saturating_sub(purchase.trade.pennies);
                contract.spent_on_goods = contract
                    .spent_on_goods
                    .saturating_add(purchase.trade.pennies);
                contract.status = TradeContractStatus::InTransit;
                routine.source_purchase_cost = purchase.trade.pennies;
                routine.source_market_fees = purchase
                    .fills
                    .iter()
                    .map(|fill| fill.market_fee)
                    .fold(0u64, u64::saturating_add);
                routine.cargo_units = loaded;
                business_events.record_market_purchase(day, *source_id, purchase.fills);
                if let Some((_, _, _, _, _, _, Some(mut civic))) =
                    halls.iter_mut().find(|(id, ..)| **id == route.destination)
                {
                    civic.record_material_expense(day.saturating_add(1), purchase.trade.pennies);
                }
                route.status = TradeRouteStatus::InTransit;
                route.current_stop = 1;
                routine.phase = TradeRoutePhase::InTransit;
                routine.failed_approaches = 0;
                activity.set_if_neq(CharacterActivity::Idle);
                let site = projects.iter().find_map(|(project, building_of, site, _)| {
                    (building_of.0 == route.destination && project.material == route.good)
                        .then_some(site)
                });
                let target =
                    contract_delivery_approach(destination, site, routine.failed_approaches);
                ensure_move_target(&mut commands, porter_entity, move_target, target);
            }
            TradeRoutePhase::InTransit => {
                let site = projects.iter().find_map(|(project, building_of, site, _)| {
                    (building_of.0 == route.destination && project.material == route.good)
                        .then_some(site)
                });
                let target =
                    contract_delivery_approach(destination, site, routine.failed_approaches);
                if ground_distance(position.0, target) > ROUTE_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, target);
                    continue;
                }
                let carried = carrier.amount(route.good).min(routine.cargo_units);
                let destination_project =
                    projects.iter_mut().find(|(project, building_of, _, _)| {
                        building_of.0 == route.destination && project.material == route.good
                    });
                let delivered = if let Some((_, _, _, mut store)) = destination_project {
                    store.add(route.good, carried)
                } else {
                    // The destination already owns paid cargo in transit. If
                    // local suppliers happened to finish Town Works before the
                    // wagon arrived, retain the shipment as civic reserve at
                    // the Hall instead of leaving the porter in a permanent
                    // wait for a project which will never respawn.
                    halls
                        .iter_mut()
                        .find(|(id, ..)| **id == route.destination)
                        .map_or(0, |(_, _, _, _, mut store, _, _)| {
                            store.add(route.good, carried)
                        })
                };
                if delivered == 0 {
                    // The destination still owns the load. A genuinely full
                    // worksite/Hall may clear later, so preserve the cargo on
                    // the porter rather than deleting it or converting it into
                    // carrier property.
                    continue;
                }
                carrier.remove(route.good, delivered);
                contract.delivered_units = contract.delivered_units.saturating_add(delivered);
                let freight = u64::from(delivered)
                    .saturating_mul(u64::from(route.good.bulk_per_unit()))
                    .saturating_mul(contract.delivery_fee_per_bulk)
                    .min(contract.escrow_cash);
                contract.escrow_cash -= freight;
                contract.spent_on_freight = contract.spent_on_freight.saturating_add(freight);
                if let Some((_, _, _, mut destination_settlement, _, _, civic)) =
                    halls.iter_mut().find(|(id, ..)| **id == route.destination)
                {
                    if let Some(mut civic) = civic {
                        civic.record_freight_expense(day.saturating_add(1), freight);
                    }
                    if contract.delivered_units >= contract.requested_units {
                        destination_settlement.treasury = destination_settlement
                            .treasury
                            .saturating_add(contract.escrow_cash);
                        contract.escrow_cash = 0;
                        contract.status = TradeContractStatus::Fulfilled;
                    } else {
                        contract.status = TradeContractStatus::Open;
                    }
                }
                if let Some(company_entity) = company_entities
                    .iter()
                    .find_map(|(entity, id)| (*id == route.company).then_some(entity))
                {
                    if let Ok(mut account) = company_accounts.get_mut(company_entity) {
                        account.credit(freight);
                    }
                }
                if let Some((_, _, _, _, _, mut account)) = warehouses
                    .iter_mut()
                    .find(|(id, ..)| **id == route.warehouse)
                {
                    account.record_service_revenue(day, freight);
                }
                if let Some(traffic) = road_traffic.as_deref_mut() {
                    traffic.delivered(
                        traffic_leg,
                        route.origin,
                        route.destination,
                        position.0.xz(),
                        delivered,
                        day,
                    );
                    traffic.finish_leg(traffic_leg);
                }
                route.completed_trips = route.completed_trips.saturating_add(1);
                route.lifetime_units = route.lifetime_units.saturating_add(delivered);
                route.lifetime_delivery_revenue =
                    route.lifetime_delivery_revenue.saturating_add(freight);
                history.record(TradeRouteTrip {
                    departed_day: routine.departed_day,
                    completed_day: day,
                    units: delivered,
                    source_purchase_cost: routine.source_purchase_cost,
                    source_market_fees: routine.source_market_fees,
                    delivery_revenue: freight,
                    consigned_value: 0,
                    stops_visited: 2,
                    travel_world_seconds: (now - routine.departed_world_seconds)
                        .max(0.0)
                        .min(f64::from(u32::MAX)) as u32,
                });
                info!(
                    "Company #{} route #{} delivered {} {} to settlement #{} and earned {} coin",
                    route.company.0,
                    routine.route.0,
                    delivered,
                    route.good.label(),
                    route.destination.0,
                    shared::economy::format_money(freight),
                );
                route.status = TradeRouteStatus::Returning;
                routine.phase = TradeRoutePhase::ReturningToOrigin;
                routine.cargo_units = 0;
                routine.failed_approaches = 0;
                ensure_move_target(
                    &mut commands,
                    porter_entity,
                    move_target,
                    hall_trade_approach(origin, routine.failed_approaches),
                );
            }
            TradeRoutePhase::ReturningToOrigin | TradeRoutePhase::ReturningToWarehouse => {
                unreachable!("returning routes are handled above")
            }
            TradeRoutePhase::MerchantTravellingToStop
            | TradeRoutePhase::MerchantReturningToOrigin
            | TradeRoutePhase::MerchantReturningToWarehouse => {
                unreachable!("merchant routes are handled by run_merchant_trade_routes")
            }
        }
    }
}

/// Execute one player-authored merchant timetable. The company risks its own
/// cash at Buy stops, while Sell stops create ordinary public consignments
/// which pay only when a real local buyer clears them. Load/Unload stops are
/// private branch transfers and therefore require company Storage Halls.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_merchant_trade_routes(
    _simulation_time: crate::world::simulation_time::SimulationTime,
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    intelligence: Option<Res<RegionalTradeIntelligence>>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (With<Settlement>, Without<CharacterKind>),
    >,
    mut warehouses: Query<
        (
            &shared::components::BuildingId,
            &shared::components::OperatedBy,
            &shared::components::BuildingOf,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &mut GoodsInventory,
            &mut BusinessAccount,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut routes: Query<
        (
            &TradeRouteId,
            &mut CompanyTradeRoute,
            &TradeRouteSchedule,
            &mut TradeRouteHistory,
            Option<&AutonomousMerchantRoute>,
        ),
        Without<MaritimeTradeRoute>,
    >,
    mut porters: Query<
        (
            Entity,
            &shared::components::PersonId,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            &mut TradeRouteRoutine,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
    mut road_traffic: Option<ResMut<crate::world::regional_roads::RegionalRoadTraffic>>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let now = absolute_world_seconds(clock);
    let hall_snapshots: HashMap<_, _> = halls
        .iter_mut()
        .map(|(id, position, rotation, ..)| {
            (
                *id,
                HallSnapshot {
                    position: position.0,
                    rotation: rotation.map_or(0.0, |rotation| rotation.0),
                },
            )
        })
        .collect();
    let warehouse_snapshots: Vec<_> = warehouses
        .iter_mut()
        .filter(|(_, _, _, building, ..)| building.kind == SettlementBuildingKind::StorageHall)
        .map(
            |(id, operated_by, building_of, _, position, rotation, ..)| WarehouseSnapshot {
                id: *id,
                settlement: building_of.0,
                company: operated_by.0,
                position: position.0,
                rotation: rotation.0,
                can_operate: true,
            },
        )
        .collect();
    let company_entities: HashMap<_, _> = company_entities
        .iter()
        .map(|(entity, company)| (*company, entity))
        .collect();

    for (
        porter_entity,
        person,
        position,
        mut activity,
        mut carrier,
        move_target,
        mut routine,
        route_failed,
    ) in porters.iter_mut()
    {
        if routine.mode != TradeRouteMode::Merchant {
            continue;
        }
        let Some((_, mut route, schedule, mut history, autonomous)) = routes
            .iter_mut()
            .find(|(route_id, ..)| **route_id == routine.route)
        else {
            commands.entity(porter_entity).remove::<TradeRouteRoutine>();
            continue;
        };
        let Some(home) = warehouse_snapshots
            .iter()
            .find(|warehouse| warehouse.id == route.warehouse && warehouse.company == route.company)
            .copied()
        else {
            route.status = TradeRouteStatus::Mothballed;
            route.assigned_caravaner = None;
            activity.set_if_neq(CharacterActivity::Idle);
            commands
                .entity(porter_entity)
                .remove::<TradeRouteRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            warn!(
                "Company #{} merchant route #{} mothballed because its home Storage Hall is unavailable",
                route.company.0, routine.route.0,
            );
            continue;
        };

        if routine.phase == TradeRoutePhase::MerchantReturningToOrigin {
            let Some(origin) = hall_snapshots.get(&route.origin).copied() else {
                continue;
            };
            let target = hall_trade_approach(origin, routine.failed_approaches);
            if route_failed.is_some() {
                retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                    hall_trade_approach(origin, attempt)
                });
                continue;
            }
            if ground_distance(position.0, target) > ROUTE_REACH {
                ensure_move_target(&mut commands, porter_entity, move_target, target);
                continue;
            }
            routine.phase = TradeRoutePhase::MerchantReturningToWarehouse;
            routine.failed_approaches = 0;
            commands
                .entity(porter_entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        }
        if routine.phase == TradeRoutePhase::MerchantReturningToWarehouse {
            let target = warehouse_trade_approach(home, routine.failed_approaches);
            if route_failed.is_some() {
                retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                    warehouse_trade_approach(home, attempt)
                });
                continue;
            }
            if ground_distance(position.0, target) > ROUTE_REACH {
                ensure_move_target(&mut commands, porter_entity, move_target, target);
                continue;
            }
            // A manual circuit may intentionally finish with cargo aboard.
            // Returning it to the home store prevents pausing a route from
            // trapping company property on an otherwise idle employee.
            let carried = carrier.amount(route.good);
            if carried > 0 {
                let Some((_, _, _, _, _, _, mut store, _)) = warehouses
                    .iter_mut()
                    .find(|(id, ..)| **id == route.warehouse)
                else {
                    continue;
                };
                let deposited = store.add(route.good, carried);
                carrier.remove(route.good, deposited);
                if carrier.amount(route.good) > 0 {
                    continue;
                }
            }
            route.assigned_caravaner = None;
            route.current_stop = 0;
            route.status = TradeRouteStatus::Idle;
            activity.set_if_neq(CharacterActivity::Idle);
            commands
                .entity(porter_entity)
                .remove::<TradeRouteRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            debug!(
                "Company #{} merchant route #{} porter #{} returned home",
                route.company.0, routine.route.0, person.0,
            );
            continue;
        }
        if routine.phase != TradeRoutePhase::MerchantTravellingToStop {
            continue;
        }
        let Some(stop) = schedule
            .stops()
            .get(usize::from(routine.stop_index))
            .copied()
        else {
            route.automatic = false;
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
            continue;
        };
        let traffic_leg = crate::world::regional_roads::TradeLeg {
            route: routine.route,
            cycle: route.completed_trips,
            stop: routine.stop_index,
        };
        let traffic_from = usize::from(routine.stop_index)
            .checked_sub(1)
            .and_then(|index| schedule.stops().get(index))
            .map(|previous| previous.settlement);
        if carrier.amount(route.good) > 0 {
            if let (Some(traffic), Some(from)) = (road_traffic.as_deref_mut(), traffic_from) {
                traffic.observe(traffic_leg, from, stop.settlement, position.0.xz(), day);
            }
        }
        let stop_has_marketplace = halls.iter_mut().any(|(id, _, _, _, market)| {
            *id == stop.settlement && market.supports_regional_trade()
        });
        if !stop_has_marketplace {
            route.automatic = false;
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
            warn!(
                "Company #{} merchant route #{} left service because settlement #{} has no completed Marketplace",
                route.company.0, routine.route.0, stop.settlement.0,
            );
            continue;
        }
        let Some(target) = merchant_stop_target(
            stop,
            route.company,
            &hall_snapshots,
            &warehouse_snapshots,
            routine.failed_approaches,
        ) else {
            // A sold/destroyed destination warehouse invalidates a private
            // stop. Finish safely at home and leave the route idle for edits.
            route.automatic = false;
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
            continue;
        };
        if route_failed.is_some() {
            retry_trade_approach(&mut commands, porter_entity, &mut routine, |attempt| {
                merchant_stop_target(
                    stop,
                    route.company,
                    &hall_snapshots,
                    &warehouse_snapshots,
                    attempt,
                )
                .unwrap_or(target)
            });
            continue;
        }
        if ground_distance(position.0, target) > ROUTE_REACH {
            ensure_move_target(&mut commands, porter_entity, move_target, target);
            continue;
        }
        commands.entity(porter_entity).remove::<MoveTarget>();
        routine.failed_approaches = 0;
        route.status = TradeRouteStatus::Loading;
        route.current_stop = routine.stop_index;

        let mut stop_complete = true;
        match stop.action {
            TradeRouteStopAction::Load => {
                let wanted = route
                    .cargo_target
                    .saturating_sub(carrier.amount(route.good))
                    .min(carrier.free_bulk() / route.good.bulk_per_unit().max(1));
                if wanted > 0 {
                    let Some((_, _, _, _, _, _, mut store, _)) = warehouses.iter_mut().find(
                        |(_, operated_by, building_of, building, ..)| {
                            operated_by.0 == route.company
                                && building_of.0 == stop.settlement
                                && building.kind == SettlementBuildingKind::StorageHall
                        },
                    ) else {
                        continue;
                    };
                    let removed = store.remove(route.good, wanted);
                    let loaded = carrier.add(route.good, removed);
                    debug_assert_eq!(loaded, removed);
                    routine.cargo_units = routine.cargo_units.saturating_add(loaded);
                }
            }
            TradeRouteStopAction::Buy => {
                let wanted = route
                    .cargo_target
                    .saturating_sub(carrier.amount(route.good))
                    .min(carrier.free_bulk() / route.good.bulk_per_unit().max(1));
                if wanted > 0 {
                    let Some(company_entity) = company_entities.get(&route.company).copied() else {
                        continue;
                    };
                    let Ok(mut company_account) = company_accounts.get_mut(company_entity) else {
                        continue;
                    };
                    let Some((market_id, _, _, mut source_store, mut source_market)) =
                        halls.iter_mut().find(|(id, ..)| **id == stop.settlement)
                    else {
                        continue;
                    };
                    if !source_market.supports_regional_trade() {
                        route.automatic = false;
                        route.status = TradeRouteStatus::Returning;
                        routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
                        warn!(
                            "Company #{} merchant route #{} could not buy in settlement #{} because it has no completed Marketplace",
                            route.company.0, routine.route.0, stop.settlement.0,
                        );
                        continue;
                    }
                    let requested = wanted.min(source_store.amount(route.good));
                    let budget = if route.autonomous_management {
                        company_account.cash.saturating_sub(
                            intelligence
                                .as_ref()
                                .and_then(|intel| intel.protected_company_cash.get(&route.company))
                                .copied()
                                .unwrap_or(0),
                        )
                    } else {
                        company_account.cash
                    };
                    if route.autonomous_management {
                        let preview = source_market.preview_purchase(
                            route.good,
                            requested,
                            budget,
                            Some(route.maximum_purchase_price),
                            Some(MarketSeller::Business(route.warehouse)),
                        );
                        if autonomous.is_none_or(|terms| {
                            !viable_pickup(preview, route.minimum_destination_price, terms)
                        }) {
                            // A tiny or newly unaffordable load cannot repay the
                            // fixed trip. Return with cash intact for review.
                            route.completed_trips = route.completed_trips.saturating_add(1);
                            history.record(TradeRouteTrip {
                                departed_day: routine.departed_day,
                                completed_day: day,
                                units: 0,
                                source_purchase_cost: 0,
                                source_market_fees: 0,
                                delivery_revenue: 0,
                                consigned_value: 0,
                                stops_visited: routine.stops_visited.saturating_add(1),
                                travel_world_seconds: (now - routine.departed_world_seconds)
                                    .max(0.0)
                                    .min(f64::from(u32::MAX))
                                    as u32,
                            });
                            route.status = TradeRouteStatus::Returning;
                            routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
                            continue;
                        }
                    }
                    let purchase = source_market.purchase_for_resale(
                        route.good,
                        requested,
                        budget,
                        Some(route.maximum_purchase_price),
                        Some(MarketSeller::Business(route.warehouse)),
                    );
                    if purchase.trade.units > 0 && company_account.debit(purchase.trade.pennies) {
                        let removed = source_store.remove(route.good, purchase.trade.units);
                        let loaded = carrier.add(route.good, removed);
                        debug_assert_eq!(loaded, purchase.trade.units);
                        let fees = purchase
                            .fills
                            .iter()
                            .map(|fill| fill.market_fee)
                            .fold(0u64, u64::saturating_add);
                        routine.source_purchase_cost = routine
                            .source_purchase_cost
                            .saturating_add(purchase.trade.pennies);
                        routine.source_market_fees =
                            routine.source_market_fees.saturating_add(fees);
                        routine.cargo_units = routine.cargo_units.saturating_add(loaded);
                        route.lifetime_purchase_cost = route
                            .lifetime_purchase_cost
                            .saturating_add(purchase.trade.pennies);
                        if let Some((_, _, _, _, _, _, _, mut account)) = warehouses
                            .iter_mut()
                            .find(|(id, ..)| **id == route.warehouse)
                        {
                            account.record_input_purchase(day, purchase.trade.pennies, loaded);
                        }
                        business_events.record_market_purchase(day, *market_id, purchase.fills);
                    }
                }
            }
            TradeRouteStopAction::Unload => {
                let carried = carrier.amount(route.good);
                if carried > 0 {
                    let Some((_, _, _, _, _, _, mut store, _)) = warehouses.iter_mut().find(
                        |(_, operated_by, building_of, building, ..)| {
                            operated_by.0 == route.company
                                && building_of.0 == stop.settlement
                                && building.kind == SettlementBuildingKind::StorageHall
                        },
                    ) else {
                        continue;
                    };
                    let deposited = store.add(route.good, carried);
                    carrier.remove(route.good, deposited);
                    if let (Some(traffic), Some(from)) = (road_traffic.as_deref_mut(), traffic_from)
                    {
                        traffic.delivered(
                            traffic_leg,
                            from,
                            stop.settlement,
                            position.0.xz(),
                            deposited,
                            day,
                        );
                    }
                    stop_complete = carrier.amount(route.good) == 0;
                }
            }
            TradeRouteStopAction::Sell => {
                let carried = carrier.amount(route.good);
                if carried > 0 {
                    let Some((_, _, _, mut destination_store, mut destination_market)) =
                        halls.iter_mut().find(|(id, ..)| **id == stop.settlement)
                    else {
                        continue;
                    };
                    if !destination_market.supports_regional_trade() {
                        route.automatic = false;
                        route.status = TradeRouteStatus::Returning;
                        routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
                        warn!(
                            "Company #{} merchant route #{} could not sell in settlement #{} because it has no completed Marketplace",
                            route.company.0, routine.route.0, stop.settlement.0,
                        );
                        continue;
                    }
                    let deposited = destination_store.add(route.good, carried);
                    if deposited > 0 {
                        carrier.remove(route.good, deposited);
                        if let (Some(traffic), Some(from)) =
                            (road_traffic.as_deref_mut(), traffic_from)
                        {
                            traffic.delivered(
                                traffic_leg,
                                from,
                                stop.settlement,
                                position.0.xz(),
                                deposited,
                                day,
                            );
                        }
                        destination_market.consign(
                            MarketSeller::Business(route.warehouse),
                            route.good,
                            deposited,
                            route.minimum_destination_price,
                        );
                        let value = route
                            .minimum_destination_price
                            .saturating_mul(u64::from(deposited));
                        routine.consigned_value = routine.consigned_value.saturating_add(value);
                        route.lifetime_consigned_value =
                            route.lifetime_consigned_value.saturating_add(value);
                    }
                    stop_complete = carrier.amount(route.good) == 0;
                }
            }
            TradeRouteStopAction::ContractPickup | TradeRouteStopAction::ContractDelivery => {
                route.automatic = false;
                route.status = TradeRouteStatus::Returning;
                routine.phase = TradeRoutePhase::MerchantReturningToOrigin;
                continue;
            }
        }
        if !stop_complete {
            continue;
        }

        if let Some(traffic) = road_traffic.as_deref_mut() {
            traffic.finish_leg(traffic_leg);
        }
        routine.stops_visited = routine.stops_visited.saturating_add(1);
        let next_index = usize::from(routine.stop_index).saturating_add(1);
        if next_index < schedule.stops().len() {
            routine.stop_index = next_index as u8;
            route.current_stop = routine.stop_index;
            route.status = TradeRouteStatus::InTransit;
            let next_stop = schedule.stops()[next_index];
            if let Some(next_target) = merchant_stop_target(
                next_stop,
                route.company,
                &hall_snapshots,
                &warehouse_snapshots,
                routine.failed_approaches,
            ) {
                ensure_move_target(&mut commands, porter_entity, None, next_target);
            }
            continue;
        }

        route.completed_trips = route.completed_trips.saturating_add(1);
        route.lifetime_units = route.lifetime_units.saturating_add(routine.cargo_units);
        history.record(TradeRouteTrip {
            departed_day: routine.departed_day,
            completed_day: day,
            units: routine.cargo_units,
            source_purchase_cost: routine.source_purchase_cost,
            source_market_fees: routine.source_market_fees,
            delivery_revenue: 0,
            consigned_value: routine.consigned_value,
            stops_visited: routine.stops_visited,
            travel_world_seconds: (now - routine.departed_world_seconds)
                .max(0.0)
                .min(f64::from(u32::MAX)) as u32,
        });

        if route.automatic {
            routine.stop_index = 0;
            routine.stops_visited = 0;
            routine.departed_day = day;
            routine.departed_world_seconds = now;
            routine.source_purchase_cost = 0;
            routine.source_market_fees = 0;
            routine.cargo_units = 0;
            routine.consigned_value = 0;
            route.current_stop = 0;
            route.status = TradeRouteStatus::InTransit;
            if let Some(next_target) = schedule.stops().first().copied().and_then(|next_stop| {
                merchant_stop_target(
                    next_stop,
                    route.company,
                    &hall_snapshots,
                    &warehouse_snapshots,
                    routine.failed_approaches,
                )
            }) {
                ensure_move_target(&mut commands, porter_entity, None, next_target);
            }
        } else {
            route.status = TradeRouteStatus::Returning;
            // The Storage Hall is the company's operating base, regardless
            // of whether this was an export or import timetable. An importer
            // that has just sold in its home town must not make a pointless
            // second round trip to the remote source before clocking off.
            routine.phase = TradeRoutePhase::MerchantReturningToWarehouse;
            routine.failed_approaches = 0;
            ensure_move_target(
                &mut commands,
                porter_entity,
                None,
                warehouse_trade_approach(home, routine.failed_approaches),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{
        BuildingId, BuildingOf, CivicHallLevel, CompanyId, OperatedBy, SettlementId, SettlementTier,
    };
    use shared::economy::{BusinessAccount, CivicAccount, CompanyAccount};

    const SOURCE: SettlementId = SettlementId(1);
    const DESTINATION: SettlementId = SettlementId(2);
    const SELLER_COMPANY: CompanyId = CompanyId(10);
    const CARRIER_COMPANY: CompanyId = CompanyId(11);
    const QUARRY: BuildingId = BuildingId(20);
    const WAREHOUSE: BuildingId = BuildingId(21);
    const CONTRACT: TradeContractId = TradeContractId(30);
    const ROUTE: TradeRouteId = TradeRouteId(31);
    const PORTER: shared::components::PersonId = shared::components::PersonId(40);

    fn hall(
        id: SettlementId,
        name: &str,
        at: Vec3,
        treasury: u64,
        market: MootMarket,
        inventory: GoodsInventory,
    ) -> impl Bundle {
        (
            id,
            Settlement {
                name: name.to_string(),
                tier: SettlementTier::Village,
                residents: 30,
                treasury,
            },
            PlayerPosition(at),
            PlayerRotation(0.0),
            market,
            inventory,
            CivicAccount::default(),
        )
    }

    fn regional_market(mut market: MootMarket) -> MootMarket {
        market.unlock_trade_tier(shared::economy::MarketTradeTier::Marketplace);
        market
    }

    #[test]
    fn partial_stone_load_still_covers_the_fixed_carrier_callout() {
        let per_bulk = contract_delivery_pennies_per_bulk(Good::Stone, 3);
        let freight = per_bulk.saturating_mul(u64::from(3 * Good::Stone.bulk_per_unit()));
        assert!(freight >= CONTRACT_MINIMUM_DELIVERY_PENNIES);
        assert!(freight >= FOUNDING_DAILY_WAGE);
    }

    fn observed_market(
        settlement: SettlementId,
        good: Good,
        ask: u64,
        listed: u32,
        sold: u32,
        funded_unmet: u32,
        confidence: u8,
    ) -> CompanyTradeObservation {
        let mut funded_demand = shared::economy::FundedDemandCurve::default();
        funded_demand.add(u64::from(funded_unmet), ask);
        CompanyTradeObservation {
            settlement,
            good,
            observed_day: 4,
            confidence,
            asking_price: ask,
            listed_units: listed,
            recent_units_sold: sold,
            recent_sale_price: ask,
            funded_unmet_units: funded_unmet,
            funded_demand,
            substitute_demand: shared::economy::FundedDemandCurve::default(),
            market_fee_bps: 500,
        }
    }

    #[test]
    fn repeated_independent_rumours_gradually_become_actionable() {
        let mut knowledge = CompanyTradeKnowledge::default();
        let mut first = observed_market(SOURCE, Good::Bread, 10, 20, 0, 0, 43);
        first.observed_day = 2;
        knowledge.observe(first);
        let mut later = first;
        later.observed_day = 5;
        knowledge.observe(later);
        assert_eq!(knowledge.observations[0].confidence, 48);

        for day in [8, 11, 14, 17, 20, 23, 26, 29] {
            later.observed_day = day;
            knowledge.observe(later);
        }
        assert_eq!(
            knowledge.observations[0].confidence, 85,
            "hearsay may become trusted without becoming omniscient"
        );

        let exact = observed_market(SOURCE, Good::Bread, 12, 16, 4, 0, 100);
        knowledge.observe(exact);
        assert_eq!(knowledge.observations[0], exact);
    }

    #[test]
    fn cheap_bread_can_answer_conservative_substitute_food_demand() {
        let mut market = regional_market(MootMarket::founding());
        let missed = market.purchase_recording_demand(Good::Meat, 8, 10_000, None, None);
        assert_eq!(missed.trade.units, 0);
        market.consign(MarketSeller::Business(QUARRY), Good::Food, 8, 10);
        assert_eq!(
            market.purchase(Good::Food, 8, 80, None, None).trade.units,
            8
        );

        let bread = market_observation(DESTINATION, &market, Good::Bread, 4);
        assert_eq!(bread.funded_unmet_units, 2);
        assert_eq!(bread.substitute_demand.total_units(), 8);
        assert_eq!(bread.recent_units_sold, 0);
        assert_eq!(bread.recent_sale_price, 0);
    }

    #[test]
    fn cheap_fish_sales_cannot_multiply_buyers_at_one_expensive_bread_sale_price() {
        let mut market = regional_market(MootMarket::founding());
        market.consign(MarketSeller::Business(QUARRY), Good::Bread, 1, 200);
        market.consign(MarketSeller::Business(QUARRY), Good::Food, 400, 1);
        assert_eq!(
            market.purchase(Good::Bread, 1, 200, None, None).trade.units,
            1
        );
        assert_eq!(
            market
                .purchase(Good::Food, 400, 400, None, None)
                .trade
                .units,
            400
        );

        let bread = market_observation(DESTINATION, &market, Good::Bread, 4);
        assert_eq!((bread.recent_units_sold, bread.recent_sale_price), (1, 200));
        let source = observed_market(SOURCE, Good::Bread, 20, 100, 0, 0, 100);
        assert!(
            evaluate_merchant_opportunity(
                source,
                bread,
                Vec3::ZERO,
                Vec3::X * 40.0,
                shared::economy::BusinessStrategy::Balanced,
                FOUNDING_DAILY_WAGE,
                0,
            )
            .is_none(),
            "one historical Bread buyer cannot cover this trip's minimum profit; Fish volume must not invent 51 Bread buyers"
        );
    }

    #[test]
    fn funded_food_shortage_can_support_a_risky_merchant_opportunity() {
        let source = observed_market(SOURCE, Good::Bread, 100, 20, 4, 0, 100);
        let destination = observed_market(DESTINATION, Good::Bread, 220, 0, 0, 10, 55);
        let opportunity = evaluate_merchant_opportunity(
            source,
            destination,
            Vec3::ZERO,
            Vec3::new(40.0, 0.0, 0.0),
            shared::economy::BusinessStrategy::Balanced,
            FOUNDING_DAILY_WAGE,
            0,
        )
        .expect("funded scarcity and a large price gap should justify one trial cart");
        assert_eq!(opportunity.good, Good::Bread);
        assert!(opportunity.cargo_units > 0);
        assert!(opportunity.expected_profit > 0);
    }

    #[test]
    fn profitable_uncertain_quote_has_a_bounded_limit_price() {
        let source = observed_market(SOURCE, Good::Bread, 9, 3, 0, 0, 57);
        let destination = observed_market(DESTINATION, Good::Bread, 180, 0, 0, 3, 100);
        let uncertain = evaluate_merchant_opportunity(
            source,
            destination,
            Vec3::ZERO,
            Vec3::new(360.0, 0.0, 0.0),
            shared::economy::BusinessStrategy::Balanced,
            FOUNDING_DAILY_WAGE,
            0,
        )
        .expect("the large margin should tolerate a one-penny rumor error");
        assert_eq!(uncertain.maximum_purchase_price, 10);

        let exact = evaluate_merchant_opportunity(
            CompanyTradeObservation {
                confidence: 100,
                ..source
            },
            destination,
            Vec3::ZERO,
            Vec3::new(360.0, 0.0, 0.0),
            shared::economy::BusinessStrategy::Balanced,
            FOUNDING_DAILY_WAGE,
            0,
        )
        .expect("the exact quote remains profitable");
        assert_eq!(exact.maximum_purchase_price, source.asking_price);
    }

    #[test]
    fn hunger_without_funded_demand_is_not_guaranteed_merchant_revenue() {
        let source = observed_market(SOURCE, Good::Bread, 100, 20, 4, 0, 100);
        let destination = observed_market(DESTINATION, Good::Bread, 220, 0, 0, 0, 55);
        assert!(
            evaluate_merchant_opportunity(
                source,
                destination,
                Vec3::ZERO,
                Vec3::new(40.0, 0.0, 0.0),
                shared::economy::BusinessStrategy::Balanced,
                FOUNDING_DAILY_WAGE,
                0,
            )
            .is_none()
        );
    }

    #[test]
    fn autonomous_importer_uses_delayed_intel_to_open_one_physical_food_trial() {
        assert_autonomous_importer_land_access(None, true);
    }

    #[test]
    fn autonomous_importer_rejects_separate_land_networks_but_keeps_connected_trials() {
        assert_autonomous_importer_land_access(Some((7, 9)), false);
        assert_autonomous_importer_land_access(Some((7, 7)), true);
    }

    fn assert_autonomous_importer_land_access(groups: Option<(u64, u64)>, expected: bool) {
        let mut app = App::new();
        app.init_resource::<RegionalTradeIntelligence>()
            .init_resource::<RegionalMerchantDemand>()
            .add_systems(Update, review_autonomous_merchant_trade);
        app.world_mut().spawn(WorldTime::new_default());

        let mut source_market = regional_market(MootMarket::founding());
        source_market.consign(MarketSeller::Business(QUARRY), Good::Bread, 20, 10);
        let mut source_store = GoodsInventory::new(shared::economy::capacity::HALL);
        source_store.add(Good::Bread, 20);
        source_market.refresh_all(&source_store);
        app.world_mut().spawn(hall(
            SOURCE,
            "Breadfield",
            Vec3::ZERO,
            0,
            source_market,
            source_store,
        ));
        let mut hungry_market = regional_market(MootMarket::founding());
        let empty_store = GoodsInventory::new(shared::economy::capacity::HALL);
        let purchase = hungry_market.purchase_recording_demand(Good::Bread, 10, 3_000, None, None);
        assert_eq!(purchase.trade.units, 0);
        let destination_hall = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Hungry Market",
                Vec3::new(40.0, 0.0, 0.0),
                0,
                hungry_market,
                empty_store,
            ))
            .id();

        let company = CompanyId(3);
        let master = shared::components::PersonId(300);
        let mut branches = shared::economy::CompanyBranchPolicies::default();
        branches.ensure_branch(DESTINATION);
        app.world_mut().spawn((
            company,
            shared::components::CompanyLeadership { master },
            CompanyAccount {
                // Three one-coin payroll days are protected. The remaining
                // 0.50 coin can fund five cheap loaves, not the full cart.
                cash: 350,
                ..default()
            },
            shared::economy::CompanyManagementPolicy::default(),
            branches,
        ));
        app.world_mut()
            .spawn((master, CharacterAttributes::new(20, 20, 20)));

        let warehouse_stock = GoodsInventory::new(shared::economy::capacity::STORAGE_HALL);
        app.world_mut().spawn((
            WAREHOUSE,
            OperatedBy(company),
            BuildingOf(DESTINATION),
            SettlementBuilding {
                kind: SettlementBuildingKind::StorageHall,
                settlement: "Hungry Market".to_string(),
                owner: Some("Merchant".to_string()),
                quality: 1.0,
                workers: vec!["Porter".to_string()],
            },
            warehouse_stock,
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(1),
        ));
        app.world_mut().spawn(CompanyPorter {
            settlement: destination_hall,
            settlement_id: DESTINATION,
            company,
            storage_hall: WAREHOUSE,
        });
        // A richer-looking second depot cannot borrow the first depot's
        // employee merely because both belong to the same company.
        app.world_mut().spawn((
            BuildingId(WAREHOUSE.0 + 1),
            OperatedBy(company),
            BuildingOf(DESTINATION),
            SettlementBuilding {
                kind: SettlementBuildingKind::StorageHall,
                settlement: "Hungry Market".into(),
                owner: Some("Merchant".into()),
                quality: 1.0,
                workers: vec![],
            },
            GoodsInventory::new(shared::economy::capacity::STORAGE_HALL),
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(0),
        ));
        app.world_mut().spawn((
            CompanyTradeRoute {
                company,
                warehouse: WAREHOUSE,
                mode: TradeRouteMode::ContractCarrier,
                origin: SOURCE,
                destination: DESTINATION,
                good: Good::Stone,
                cargo_target: 0,
                maximum_purchase_price: 0,
                minimum_destination_price: 0,
                automatic: true,
                autonomous_management: false,
                expected_trip_profit: 0,
                decision_confidence: 100,
                active_contract: None,
                assigned_caravaner: None,
                current_stop: 0,
                status: TradeRouteStatus::Idle,
                completed_trips: 1,
                lifetime_units: 4,
                lifetime_delivery_revenue: 200,
                lifetime_purchase_cost: 0,
                lifetime_consigned_value: 0,
            },
            TradeRouteSchedule::contract(SOURCE, DESTINATION),
            TradeRouteHistory::default(),
        ));

        if let Some((source, destination)) = groups {
            let halls: Vec<_> = app
                .world_mut()
                .query::<(Entity, &SettlementId)>()
                .iter(app.world())
                .map(|(entity, id)| (entity, *id))
                .collect();
            for (entity, id) in halls {
                app.world_mut()
                    .entity_mut(entity)
                    .insert(FoundingLandNetwork(if id == SOURCE {
                        source
                    } else {
                        destination
                    }));
            }
        }
        app.update();
        let advertised = app.world().resource::<RegionalMerchantDemand>();
        assert_eq!(
            advertised.units(SOURCE, Good::Bread) > 0,
            expected,
            "export permits must only see demand that a land route can actually reach"
        );
        assert_eq!(
            advertised.bulk(DESTINATION) > 0,
            expected,
            "an unreachable import cannot justify local warehouse construction"
        );
        if !expected {
            assert_eq!(
                app.world_mut()
                    .query::<&CompanyTradeRoute>()
                    .iter(app.world())
                    .count(),
                1,
                "the existing contract lane is preserved but no impossible merchant trial is created"
            );
            assert_eq!(
                app.world_mut()
                    .query::<&CompanyAccount>()
                    .single(app.world())
                    .unwrap()
                    .cash,
                350,
                "rejecting a separated market must not spend the company's cash"
            );
            assert!(
                app.world_mut()
                    .query::<&AutonomousMerchantRoute>()
                    .iter(app.world())
                    .next()
                    .is_none()
            );
            return;
        }

        let (route_entity, _, route, schedule) = app
            .world_mut()
            .query::<(
                Entity,
                &AutonomousMerchantRoute,
                &CompanyTradeRoute,
                &TradeRouteSchedule,
            )>()
            .single(app.world())
            .expect("one bounded autonomous food route should be founded");
        assert_eq!(route.company, company);
        assert_eq!(route.warehouse, WAREHOUSE);
        assert_eq!(route.good, Good::Bread);
        assert_eq!(route.origin, SOURCE);
        assert_eq!(route.destination, DESTINATION);
        assert!((1..=5).contains(&route.cargo_target));
        assert!(
            route
                .maximum_purchase_price
                .saturating_mul(u64::from(route.cargo_target))
                <= 50,
            "the trial must fit the 0.50 coin left after payroll protection"
        );
        assert!(!route.automatic, "NPC trials must return for review");
        assert_eq!(schedule.stops()[0].action, TradeRouteStopAction::Buy);
        assert_eq!(schedule.stops()[1].action, TradeRouteStopAction::Sell);
        let delivered_units = route.cargo_target;
        let posted_price = route.minimum_destination_price;
        assert_eq!(
            app.world_mut()
                .query::<&CompanyTradeRoute>()
                .iter(app.world())
                .count(),
            2,
            "the reusable idle contract lane must not consume the company's only porter slot"
        );
        // A delivered but unsold trial cannot silently buy a second cart.
        // Its local consignment is marked down by the daily merchant review.
        {
            let mut route = app
                .world_mut()
                .get_mut::<CompanyTradeRoute>(route_entity)
                .unwrap();
            route.completed_trips = 1;
            route.status = TradeRouteStatus::Idle;
        }
        app.world_mut()
            .get_mut::<TradeRouteHistory>(route_entity)
            .unwrap()
            .record(TradeRouteTrip {
                departed_day: 0,
                completed_day: 0,
                units: delivered_units,
                source_purchase_cost: 10 * u64::from(delivered_units),
                source_market_fees: 0,
                delivery_revenue: 0,
                consigned_value: posted_price * u64::from(delivered_units),
                stops_visited: 2,
                travel_world_seconds: 20,
            });
        app.world_mut()
            .get_mut::<MootMarket>(destination_hall)
            .unwrap()
            .consign(
                MarketSeller::Business(WAREHOUSE),
                Good::Bread,
                delivered_units,
                posted_price,
            );
        app.world_mut()
            .get_mut::<GoodsInventory>(destination_hall)
            .unwrap()
            .add(Good::Bread, delivered_units);
        let clock_entity = app
            .world_mut()
            .query_filtered::<Entity, With<WorldTime>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .day = 3;
        app.update();
        let route = app.world().get::<CompanyTradeRoute>(route_entity).unwrap();
        assert_eq!(route.status, TradeRouteStatus::Mothballed);
        let marked_down = app
            .world()
            .get::<MootMarket>(destination_hall)
            .unwrap()
            .suggested_price(Good::Bread);
        assert!(marked_down < posted_price);
        assert_eq!(
            app.world_mut()
                .query::<&CompanyAccount>()
                .single(app.world())
                .unwrap()
                .cash,
            350
        );

        // Taking explicit control disables both repricing and route changes.
        {
            let mut route = app
                .world_mut()
                .get_mut::<CompanyTradeRoute>(route_entity)
                .unwrap();
            route.autonomous_management = false;
            route.minimum_destination_price = 777;
            route.status = TradeRouteStatus::Idle;
        }
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .day = 6;
        app.update();
        let route = app.world().get::<CompanyTradeRoute>(route_entity).unwrap();
        assert_eq!(route.status, TradeRouteStatus::Idle);
        assert_eq!(route.minimum_destination_price, 777);
        assert_eq!(
            app.world()
                .get::<MootMarket>(destination_hall)
                .unwrap()
                .suggested_price(Good::Bread),
            marked_down
        );
    }

    #[test]
    fn town_works_posts_an_escrowed_order_against_real_remote_stock() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());

        let mut source_market = regional_market(MootMarket::founding());
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        app.world_mut().spawn(hall(
            SOURCE,
            "Stonefield",
            Vec3::ZERO,
            0,
            source_market,
            source_stock,
        ));
        let destination = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Meadowford",
                Vec3::new(40.0, 0.0, 0.0),
                10_000,
                regional_market(MootMarket::founding()),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Town,
                material: Good::Stone,
                material_required: 8,
            },
            BuildingOf(DESTINATION),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Meadowford".to_string(),
                raising: false,
                stand: Vec3::new(40.0, 0.0, -5.2),
                rotation: 0.0,
            },
            GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
        ));

        app.update();

        let contract = app
            .world_mut()
            .query::<&CivicTradeContract>()
            .single(app.world())
            .expect("Town Works should be the first remote Stone buyer");
        assert_eq!(contract.origin, Some(SOURCE));
        assert_eq!(contract.destination, DESTINATION);
        assert_eq!(contract.source_seller, Some(MarketSeller::Business(QUARRY)));
        assert_eq!(contract.requested_units, 8);
        assert_eq!(contract.status, TradeContractStatus::Open);
        assert_eq!(contract.delivery_fee_per_bulk, 5);
        assert_eq!(contract.reserved_cash, 2_240);
        assert_eq!(contract.escrow_cash, 2_240);
        assert_eq!(
            app.world().get::<Settlement>(destination).unwrap().treasury,
            7_760,
            "contract cash must leave the spendable treasury immediately"
        );
    }

    #[test]
    fn separate_land_networks_cannot_lock_civic_cash_with_stock_or_an_unbound_tender() {
        for source_units in [0, 8] {
            let mut app = App::new();
            app.add_systems(Update, post_civic_import_contracts);
            app.world_mut().spawn(WorldTime::new_default());
            let mut market = regional_market(MootMarket::founding());
            let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
            if source_units > 0 {
                market.consign(
                    MarketSeller::Business(QUARRY),
                    Good::Stone,
                    source_units,
                    250,
                );
                stock.add(Good::Stone, source_units);
            }
            let source = app
                .world_mut()
                .spawn((
                    hall(SOURCE, "Other coast", Vec3::ZERO, 0, market, stock),
                    FoundingLandNetwork(2),
                ))
                .id();
            let destination = app
                .world_mut()
                .spawn((
                    hall(
                        DESTINATION,
                        "Local town",
                        Vec3::X * 40.0,
                        10_000,
                        regional_market(MootMarket::founding()),
                        GoodsInventory::new(shared::economy::capacity::HALL),
                    ),
                    FoundingLandNetwork(1),
                ))
                .id();
            app.world_mut().spawn((
                CivicHallUpgradeWorksite {
                    target: CivicHallLevel::Town,
                    material: Good::Stone,
                    material_required: 8,
                },
                BuildingOf(DESTINATION),
                ConstructionSite {
                    kind: SettlementBuildingKind::Hall,
                    settlement: "Local town".into(),
                    raising: false,
                    stand: Vec3::new(40.0, 0.0, -5.2),
                    rotation: 0.0,
                },
                GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
            ));
            app.update();
            assert!(
                app.world_mut()
                    .query::<&CivicTradeContract>()
                    .iter(app.world())
                    .next()
                    .is_none()
            );
            assert_eq!(
                app.world().get::<Settlement>(destination).unwrap().treasury,
                10_000
            );

            // The identical economic opportunity becomes valid when the
            // source belongs to the actual connected group.
            app.world_mut()
                .entity_mut(source)
                .insert(FoundingLandNetwork(1));
            app.update();
            let contract = app
                .world_mut()
                .query::<&CivicTradeContract>()
                .single(app.world())
                .expect("a connected source permits ordinary civic trade");
            assert_eq!(contract.origin, (source_units > 0).then_some(SOURCE));
            assert!(contract.escrow_cash > 0);
            assert_eq!(
                app.world().get::<Settlement>(destination).unwrap().treasury + contract.escrow_cash,
                10_000
            );
        }
    }

    #[test]
    fn town_works_posts_a_cash_backed_tender_before_any_supplier_exists() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());
        // A tender may precede a supplier, but it still needs a real remote
        // settlement that could eventually host one. The separate isolated
        // control below proves a one-settlement world keeps this cash local.
        app.world_mut().spawn(hall(
            SOURCE,
            "Empty Stonefield",
            Vec3::ZERO,
            0,
            regional_market(MootMarket::founding()),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ));

        let destination = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Meadowford",
                Vec3::new(40.0, 0.0, 0.0),
                10_000,
                regional_market(MootMarket::founding()),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Town,
                material: Good::Stone,
                material_required: 8,
            },
            BuildingOf(DESTINATION),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Meadowford".to_string(),
                raising: false,
                stand: Vec3::new(40.0, 0.0, -5.2),
                rotation: 0.0,
            },
            GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
        ));

        app.update();

        let contract = app
            .world_mut()
            .query::<&CivicTradeContract>()
            .single(app.world())
            .expect("Town Works must advertise demand before a quarry exists");
        assert_eq!(contract.origin, None);
        assert_eq!(contract.source_seller, None);
        assert_eq!(contract.requested_units, 8);
        assert_eq!(contract.maximum_unit_price, Good::Stone.base_price());
        assert_eq!(contract.delivery_fee_per_bulk, 5);
        assert_eq!(contract.escrow_cash, 2_240);
        assert_eq!(
            app.world().get::<Settlement>(destination).unwrap().treasury,
            7_760
        );
    }

    #[test]
    fn isolated_hall_project_keeps_cash_local_instead_of_posting_impossible_import() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());

        let destination = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Solo Meadow",
                Vec3::ZERO,
                10_000,
                MootMarket::founding(),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Village,
                material: Good::Wood,
                material_required: 12,
            },
            BuildingOf(DESTINATION),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Solo Meadow".to_string(),
                raising: false,
                stand: Vec3::new(0.0, 0.0, -5.2),
                rotation: 0.0,
            },
            GoodsInventory::new(12 * Good::Wood.bulk_per_unit()),
        ));

        app.update();

        assert!(
            app.world_mut()
                .query::<&CivicTradeContract>()
                .iter(app.world())
                .next()
                .is_none(),
            "a one-settlement world has no remote origin for an import tender"
        );
        assert_eq!(
            app.world().get::<Settlement>(destination).unwrap().treasury,
            10_000,
            "impossible freight must not strand the civic treasury in escrow"
        );
    }

    #[test]
    fn two_moots_remain_local_and_cannot_post_a_regional_tender() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());
        app.world_mut().spawn(hall(
            SOURCE,
            "Local Stone Moot",
            Vec3::ZERO,
            0,
            MootMarket::founding(),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ));
        let destination = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Local Meadow Moot",
                Vec3::new(40.0, 0.0, 0.0),
                10_000,
                MootMarket::founding(),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Town,
                material: Good::Stone,
                material_required: 8,
            },
            BuildingOf(DESTINATION),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Local Meadow Moot".to_string(),
                raising: false,
                stand: Vec3::new(40.0, 0.0, -5.2),
                rotation: 0.0,
            },
            GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
        ));

        app.update();

        assert_eq!(
            app.world_mut()
                .query::<&CivicTradeContract>()
                .iter(app.world())
                .count(),
            0,
            "two local Moot exchanges must not silently become a regional market"
        );
        assert_eq!(
            app.world().get::<Settlement>(destination).unwrap().treasury,
            10_000,
            "a Moot-only town must keep its cash out of impossible regional escrow"
        );
    }

    #[test]
    fn ai_route_waits_for_a_staffed_warehouse_and_reviews_only_daily() {
        let mut app = App::new();
        app.add_systems(Update, manage_company_trade_routes);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut source_market = regional_market(MootMarket::founding());
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        let source_hall = app
            .world_mut()
            .spawn(hall(
                SOURCE,
                "Stonefield",
                Vec3::ZERO,
                0,
                source_market,
                source_stock,
            ))
            .id();
        app.world_mut().spawn(hall(
            DESTINATION,
            "Meadowford",
            Vec3::new(40.0, 0.0, 0.0),
            0,
            regional_market(MootMarket::founding()),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ));
        let contract_entity = app
            .world_mut()
            .spawn((
                CONTRACT,
                CivicTradeContract {
                    origin: None,
                    destination: DESTINATION,
                    good: Good::Stone,
                    source_seller: None,
                    requested_units: 8,
                    delivered_units: 0,
                    maximum_unit_price: 250,
                    delivery_fee_per_bulk: CONTRACT_DELIVERY_PENNIES_PER_BULK,
                    reserved_cash: 2_144,
                    escrow_cash: 2_144,
                    spent_on_goods: 0,
                    spent_on_freight: 0,
                    created_day: 0,
                    last_attempt_day: u32::MAX,
                    status: TradeContractStatus::Open,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .get::<CivicTradeContract>(contract_entity)
                .unwrap()
                .last_attempt_day,
            0
        );
        assert_eq!(
            app.world()
                .get::<CivicTradeContract>(contract_entity)
                .unwrap()
                .origin,
            Some(SOURCE),
            "the cash-backed tender should bind to the first complete real offer"
        );
        assert_eq!(
            app.world_mut()
                .query::<&CompanyTradeRoute>()
                .iter(app.world())
                .count(),
            0
        );

        app.world_mut().spawn((
            WAREHOUSE,
            BuildingOf(SOURCE),
            OperatedBy(CARRIER_COMPANY),
            SettlementBuilding {
                kind: SettlementBuildingKind::StorageHall,
                settlement: "Stonefield".to_string(),
                owner: Some("Carrier".to_string()),
                quality: 1.0,
                workers: vec!["Caravaner".to_string()],
            },
            PlayerPosition(Vec3::new(-12.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ));
        let porter = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PORTER,
                CompanyPorter {
                    settlement: source_hall,
                    settlement_id: SOURCE,
                    company: CARRIER_COMPANY,
                    storage_hall: WAREHOUSE,
                },
                GoodsInventory::new(shared::economy::capacity::PORTER),
                CharacterActivity::Indoors,
            ))
            .id();

        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&CompanyTradeRoute>()
                .iter(app.world())
                .count(),
            0,
            "adding capacity later in the same day must not create a per-frame decision loop"
        );
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        let route = app
            .world_mut()
            .query::<&CompanyTradeRoute>()
            .single(app.world())
            .expect("the staffed warehouse should accept the viable contract next day");
        assert_eq!(route.company, CARRIER_COMPANY);
        assert_eq!(route.warehouse, WAREHOUSE);
        assert_eq!(route.good, Good::Stone);
        assert_eq!(route.active_contract, Some(CONTRACT));

        let route_entity = app
            .world_mut()
            .query_filtered::<Entity, With<CompanyTradeRoute>>()
            .single(app.world())
            .unwrap();
        app.world_mut().entity_mut(route_entity).insert(ROUTE);
        app.world_mut()
            .entity_mut(porter)
            .insert(WorkplaceDoorTransit {
                building: Vec3::ZERO,
                door: Vec3::ZERO,
                inside: Vec3::Z,
                direction: WorkplaceDoorDirection::Entering,
                phase: WorkplaceDoorPhase::Crossing,
                destination_after_exit: None,
            });
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
        app.update();
        assert!(
            app.world().get::<TradeRouteRoutine>(porter).is_none(),
            "dispatch must not steal the destination during a door crossing"
        );
        app.world_mut()
            .entity_mut(porter)
            .remove::<WorkplaceDoorTransit>();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 3;
        app.update();
        assert!(app.world().get::<TradeRouteRoutine>(porter).is_some());
        assert_eq!(
            app.world().get::<CharacterActivity>(porter),
            Some(&CharacterActivity::Idle),
            "a new freight journey must wake an indoor porter"
        );
        assert_eq!(
            app.world().get::<MoveTarget>(porter).unwrap().0,
            SettlementBuildingKind::Hall.entrance_position(Vec3::ZERO, 0.0)
        );
    }

    #[test]
    fn physical_contract_trip_pays_seller_then_carrier_and_returns_porter() {
        assert_physical_contract_trip(false);
    }

    #[test]
    fn public_carrier_retries_loading_bays_without_losing_paid_cargo_or_repaying_sellers() {
        assert_physical_contract_trip(true);
    }

    fn assert_physical_contract_trip(fail_approaches: bool) {
        let mut app = App::new();
        app.init_resource::<BusinessEventQueue>().add_systems(
            Update,
            (run_company_trade_routes, apply_business_events).chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());

        let source_at = Vec3::ZERO;
        let destination_at = Vec3::new(40.0, 0.0, 0.0);
        let warehouse_at = Vec3::new(-12.0, 0.0, 0.0);
        let project_stand = Vec3::new(40.0, 0.0, -5.2);
        let source_entrance = SettlementBuildingKind::Hall.entrance_position(source_at, 0.0);
        let warehouse_entrance =
            SettlementBuildingKind::StorageHall.entrance_position(warehouse_at, 0.0);

        let mut source_market = regional_market(MootMarket::founding());
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        let source_hall = app
            .world_mut()
            .spawn(hall(
                SOURCE,
                "Stonefield",
                source_at,
                0,
                source_market,
                source_stock,
            ))
            .id();
        app.world_mut().spawn(hall(
            DESTINATION,
            "Meadowford",
            destination_at,
            1_000,
            regional_market(MootMarket::founding()),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ));
        let project = app
            .world_mut()
            .spawn((
                CivicHallUpgradeWorksite {
                    target: CivicHallLevel::Town,
                    material: Good::Stone,
                    material_required: 8,
                },
                BuildingOf(DESTINATION),
                ConstructionSite {
                    kind: SettlementBuildingKind::Hall,
                    settlement: "Meadowford".to_string(),
                    raising: false,
                    stand: project_stand,
                    rotation: 0.0,
                },
                GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
            ))
            .id();

        let seller_company = app
            .world_mut()
            .spawn((SELLER_COMPANY, CompanyAccount::default()))
            .id();
        let carrier_company = app
            .world_mut()
            .spawn((CARRIER_COMPANY, CompanyAccount::default()))
            .id();
        let quarry = app
            .world_mut()
            .spawn((
                QUARRY,
                OperatedBy(SELLER_COMPANY),
                BusinessAccount::default(),
            ))
            .id();
        let warehouse = app
            .world_mut()
            .spawn((
                WAREHOUSE,
                BuildingOf(SOURCE),
                OperatedBy(CARRIER_COMPANY),
                SettlementBuilding {
                    kind: SettlementBuildingKind::StorageHall,
                    settlement: "Stonefield".to_string(),
                    owner: Some("Carrier".to_string()),
                    quality: 1.0,
                    workers: vec!["Caravaner".to_string()],
                },
                PlayerPosition(warehouse_at),
                PlayerRotation(0.0),
                BusinessAccount::default(),
            ))
            .id();
        let contract_entity = app
            .world_mut()
            .spawn((
                CONTRACT,
                CivicTradeContract {
                    origin: Some(SOURCE),
                    destination: DESTINATION,
                    good: Good::Stone,
                    source_seller: Some(MarketSeller::Business(QUARRY)),
                    requested_units: 8,
                    delivered_units: 0,
                    maximum_unit_price: 250,
                    delivery_fee_per_bulk: CONTRACT_DELIVERY_PENNIES_PER_BULK,
                    reserved_cash: 2_144,
                    escrow_cash: 2_144,
                    spent_on_goods: 0,
                    spent_on_freight: 0,
                    created_day: 0,
                    last_attempt_day: 0,
                    status: TradeContractStatus::Assigned,
                },
            ))
            .id();
        let route = app
            .world_mut()
            .spawn((
                ROUTE,
                CompanyTradeRoute {
                    company: CARRIER_COMPANY,
                    warehouse: WAREHOUSE,
                    mode: TradeRouteMode::ContractCarrier,
                    origin: SOURCE,
                    destination: DESTINATION,
                    good: Good::Stone,
                    cargo_target: 8,
                    maximum_purchase_price: 250,
                    minimum_destination_price: 0,
                    automatic: true,
                    autonomous_management: false,
                    expected_trip_profit: 0,
                    decision_confidence: 100,
                    active_contract: Some(CONTRACT),
                    assigned_caravaner: Some(PORTER),
                    current_stop: 0,
                    status: TradeRouteStatus::GoingToOrigin,
                    completed_trips: 0,
                    lifetime_units: 0,
                    lifetime_delivery_revenue: 0,
                    lifetime_purchase_cost: 0,
                    lifetime_consigned_value: 0,
                },
                TradeRouteSchedule::contract(SOURCE, DESTINATION),
                TradeRouteHistory::default(),
            ))
            .id();
        let porter = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PORTER,
                PlayerPosition(source_entrance),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::PORTER),
                TradeRouteRoutine {
                    route: ROUTE,
                    mode: TradeRouteMode::ContractCarrier,
                    phase: TradeRoutePhase::GoingToOrigin,
                    stop_index: 0,
                    stops_visited: 0,
                    departed_day: 0,
                    departed_world_seconds: 0.0,
                    source_purchase_cost: 0,
                    source_market_fees: 0,
                    cargo_units: 0,
                    consigned_value: 0,
                    failed_approaches: 0,
                },
            ))
            .id();

        if fail_approaches {
            app.world_mut()
                .entity_mut(porter)
                .insert(NavigationRouteFailed {
                    goal: source_entrance,
                });
            app.update();
            let alternate_origin = hall_trade_approach(
                HallSnapshot {
                    position: source_at,
                    rotation: 0.0,
                },
                1,
            );
            assert_eq!(
                app.world().get::<MoveTarget>(porter).unwrap().0,
                alternate_origin
            );
            assert_eq!(
                app.world().get::<PlayerPosition>(porter).unwrap().0,
                source_entrance
            );
            assert!(app.world().get::<NavigationRouteFailed>(porter).is_none());
            app.update();
            assert_eq!(
                app.world().get::<MoveTarget>(porter).unwrap().0,
                alternate_origin
            );
            assert_eq!(
                app.world()
                    .get::<CivicTradeContract>(contract_entity)
                    .unwrap()
                    .escrow_cash,
                2_144,
                "changing pickup approach cannot purchase before the porter arrives"
            );
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(porter)
                    .unwrap()
                    .amount(Good::Stone),
                0
            );
            app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = alternate_origin;
        }

        app.update();

        assert_eq!(
            app.world()
                .get::<GoodsInventory>(source_hall)
                .unwrap()
                .amount(Good::Stone),
            0
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(porter)
                .unwrap()
                .amount(Good::Stone),
            8
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(seller_company)
                .unwrap()
                .cash,
            1_900,
            "source firm receives the listing gross less its ordinary 5% market fee"
        );
        assert_eq!(
            app.world().get::<Settlement>(source_hall).unwrap().treasury,
            100
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(quarry)
                .unwrap()
                .gross_revenue,
            2_000
        );

        assert_eq!(
            app.world()
                .get::<TradeRouteRoutine>(porter)
                .unwrap()
                .failed_approaches,
            0
        );
        let mut delivery_target = project_stand;
        if fail_approaches {
            for attempt in 1..TRADE_STOP_APPROACH_OFFSETS.len() {
                app.world_mut()
                    .entity_mut(porter)
                    .insert(NavigationRouteFailed {
                        goal: delivery_target,
                    });
                app.update();
                delivery_target = offset_trade_approach(project_stand, 0.0, attempt as u8);
                assert_eq!(
                    app.world().get::<MoveTarget>(porter).unwrap().0,
                    delivery_target
                );
                assert!(app.world().get::<NavigationRouteFailed>(porter).is_none());
                app.update();
                assert_eq!(
                    app.world().get::<MoveTarget>(porter).unwrap().0,
                    delivery_target,
                    "the next update must retain the alternate service bay"
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(porter)
                        .unwrap()
                        .amount(Good::Stone),
                    8
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(project)
                        .unwrap()
                        .amount(Good::Stone),
                    0
                );
                let contract = app
                    .world()
                    .get::<CivicTradeContract>(contract_entity)
                    .unwrap();
                assert_eq!(contract.status, TradeContractStatus::InTransit);
                assert_eq!(contract.escrow_cash, 144);
                assert_eq!(contract.spent_on_goods, 2_000);
                assert_eq!(contract.spent_on_freight, 0);
                assert_eq!(
                    app.world()
                        .get::<CompanyAccount>(seller_company)
                        .unwrap()
                        .cash,
                    1_900
                );
                assert_eq!(
                    app.world()
                        .get::<CompanyAccount>(carrier_company)
                        .unwrap()
                        .cash,
                    0
                );
                assert!(
                    app.world()
                        .get::<TradeRouteHistory>(route)
                        .unwrap()
                        .trips()
                        .is_empty()
                );
            }
            app.world_mut()
                .entity_mut(porter)
                .insert(NavigationRouteFailed {
                    goal: delivery_target,
                });
            for _ in 0..3 {
                app.update();
                assert_eq!(
                    app.world().get::<MoveTarget>(porter).unwrap().0,
                    delivery_target
                );
                assert_eq!(
                    app.world()
                        .get::<TradeRouteRoutine>(porter)
                        .unwrap()
                        .failed_approaches,
                    (TRADE_STOP_APPROACH_OFFSETS.len() - 1) as u8
                );
                assert!(
                    app.world().get::<NavigationRouteFailed>(porter).is_some(),
                    "exhausted candidates must retain the shared navigation failure/backoff instead of restarting searches every update"
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(porter)
                        .unwrap()
                        .amount(Good::Stone),
                    8
                );
            }
            // Stand in for navigation eventually reaching the retained bay;
            // the trade system must still wait for physical arrival to settle.
            app.world_mut()
                .entity_mut(porter)
                .remove::<NavigationRouteFailed>();
        }
        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = delivery_target;
        app.update();

        assert_eq!(
            app.world()
                .get::<GoodsInventory>(project)
                .unwrap()
                .amount(Good::Stone),
            8
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(carrier_company)
                .unwrap()
                .cash,
            144
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(warehouse)
                .unwrap()
                .gross_revenue,
            144
        );
        assert_eq!(
            app.world()
                .get::<CivicTradeContract>(contract_entity)
                .unwrap()
                .status,
            TradeContractStatus::Fulfilled
        );
        let route_state = app.world().get::<CompanyTradeRoute>(route).unwrap();
        assert_eq!(route_state.status, TradeRouteStatus::Returning);
        assert_eq!(route_state.completed_trips, 1);
        assert_eq!(
            app.world()
                .get::<TradeRouteHistory>(route)
                .unwrap()
                .trips()
                .len(),
            1
        );

        app.world_mut()
            .entity_mut(porter)
            .insert(NavigationRouteFailed {
                goal: source_entrance,
            });
        app.update();
        let alternate_origin = hall_trade_approach(
            HallSnapshot {
                position: source_at,
                rotation: 0.0,
            },
            1,
        );
        assert_eq!(
            app.world().get::<MoveTarget>(porter).unwrap().0,
            alternate_origin,
            "a returning caravan should retry at a nearby loading bay instead of waiting forever at one blocked doorway"
        );
        assert!(
            app.world().get::<NavigationRouteFailed>(porter).is_none(),
            "the alternate approach must become a genuinely new bounded route request"
        );

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = alternate_origin;
        app.update();
        assert_eq!(
            app.world().get::<TradeRouteRoutine>(porter).unwrap().phase,
            TradeRoutePhase::ReturningToWarehouse,
        );

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = warehouse_entrance;
        app.update();

        assert!(app.world().get::<TradeRouteRoutine>(porter).is_none());
        let route_state = app.world().get::<CompanyTradeRoute>(route).unwrap();
        assert_eq!(route_state.status, TradeRouteStatus::Idle);
        assert_eq!(route_state.active_contract, None);
        assert_eq!(route_state.assigned_caravaner, None);
    }

    #[test]
    fn merchant_timetable_buys_carries_and_consigns_at_its_ordered_stops() {
        let mut app = App::new();
        app.init_resource::<BusinessEventQueue>().add_systems(
            Update,
            (run_merchant_trade_routes, apply_business_events).chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());

        let source_at = Vec3::ZERO;
        let destination_at = Vec3::new(40.0, 0.0, 0.0);
        let warehouse_at = Vec3::new(-12.0, 0.0, 0.0);
        let source_entrance = SettlementBuildingKind::Hall.entrance_position(source_at, 0.0);
        let destination_entrance =
            SettlementBuildingKind::Hall.entrance_position(destination_at, 0.0);

        let mut source_market = regional_market(MootMarket::founding());
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        let source_hall = app
            .world_mut()
            .spawn(hall(
                SOURCE,
                "Stonefield",
                source_at,
                0,
                source_market,
                source_stock,
            ))
            .id();
        let destination_hall = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Meadowford",
                destination_at,
                0,
                regional_market(MootMarket::founding()),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();

        let seller_company = app
            .world_mut()
            .spawn((SELLER_COMPANY, CompanyAccount::default()))
            .id();
        let carrier_company = app
            .world_mut()
            .spawn((
                CARRIER_COMPANY,
                CompanyAccount {
                    cash: 5_000,
                    ..default()
                },
            ))
            .id();
        app.world_mut().spawn((
            QUARRY,
            OperatedBy(SELLER_COMPANY),
            BusinessAccount::default(),
        ));
        let warehouse = app
            .world_mut()
            .spawn((
                WAREHOUSE,
                BuildingOf(SOURCE),
                OperatedBy(CARRIER_COMPANY),
                SettlementBuilding {
                    kind: SettlementBuildingKind::StorageHall,
                    settlement: "Stonefield".to_string(),
                    owner: Some("Carrier".to_string()),
                    quality: 1.0,
                    workers: vec!["Caravaner".to_string()],
                },
                PlayerPosition(warehouse_at),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::STORAGE_HALL),
                BusinessAccount::default(),
            ))
            .id();
        let schedule = TradeRouteSchedule::new([
            TradeRouteStop {
                settlement: SOURCE,
                action: TradeRouteStopAction::Buy,
            },
            TradeRouteStop {
                settlement: DESTINATION,
                action: TradeRouteStopAction::Sell,
            },
        ])
        .unwrap();
        let route = app
            .world_mut()
            .spawn((
                ROUTE,
                CompanyTradeRoute {
                    company: CARRIER_COMPANY,
                    warehouse: WAREHOUSE,
                    mode: TradeRouteMode::Merchant,
                    origin: SOURCE,
                    destination: DESTINATION,
                    good: Good::Stone,
                    cargo_target: 8,
                    maximum_purchase_price: 250,
                    minimum_destination_price: 310,
                    automatic: false,
                    autonomous_management: false,
                    expected_trip_profit: 0,
                    decision_confidence: 100,
                    active_contract: None,
                    assigned_caravaner: Some(PORTER),
                    current_stop: 0,
                    status: TradeRouteStatus::GoingToOrigin,
                    completed_trips: 0,
                    lifetime_units: 0,
                    lifetime_delivery_revenue: 0,
                    lifetime_purchase_cost: 0,
                    lifetime_consigned_value: 0,
                },
                schedule,
                TradeRouteHistory::default(),
            ))
            .id();
        let porter = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PORTER,
                PlayerPosition(source_entrance),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::PORTER),
                TradeRouteRoutine {
                    route: ROUTE,
                    mode: TradeRouteMode::Merchant,
                    phase: TradeRoutePhase::MerchantTravellingToStop,
                    stop_index: 0,
                    stops_visited: 0,
                    departed_day: 0,
                    departed_world_seconds: 0.0,
                    source_purchase_cost: 0,
                    source_market_fees: 0,
                    cargo_units: 0,
                    consigned_value: 0,
                    failed_approaches: 0,
                },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .get::<GoodsInventory>(source_hall)
                .unwrap()
                .amount(Good::Stone),
            0
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(porter)
                .unwrap()
                .amount(Good::Stone),
            8
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(carrier_company)
                .unwrap()
                .cash,
            3_000
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(seller_company)
                .unwrap()
                .cash,
            1_900
        );

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = destination_entrance;
        app.update();

        assert_eq!(
            app.world()
                .get::<GoodsInventory>(destination_hall)
                .unwrap()
                .amount(Good::Stone),
            8,
            "the destination Hall must physically hold every consigned unit"
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(porter)
                .unwrap()
                .amount(Good::Stone),
            0
        );
        let destination_market = app.world().get::<MootMarket>(destination_hall).unwrap();
        assert!(destination_market.listings().iter().any(|listing| {
            listing.seller == MarketSeller::Business(WAREHOUSE)
                && listing.good == Good::Stone
                && listing.units == 8
                && listing.unit_price == 310
        }));
        let route_state = app.world().get::<CompanyTradeRoute>(route).unwrap();
        assert_eq!(route_state.completed_trips, 1);
        assert_eq!(route_state.lifetime_purchase_cost, 2_000);
        assert_eq!(route_state.lifetime_consigned_value, 2_480);
        assert_eq!(route_state.status, TradeRouteStatus::Returning);
        let trip = app.world().get::<TradeRouteHistory>(route).unwrap().trips()[0];
        assert_eq!(trip.units, 8);
        assert_eq!(trip.stops_visited, 2);
        assert_eq!(trip.consigned_value, 2_480);
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(warehouse)
                .unwrap()
                .current_day
                .input_expense,
            2_000,
            "merchant cargo purchases remain visible in the route base cost centre"
        );
    }
}
