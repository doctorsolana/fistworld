//! Company-owned inter-settlement routes and buyer-funded civic contracts.
//!
//! The first controlled loop moves a real civic construction order from one
//! public market to another. A completed private Storage Hall (the warehouse)
//! and one of its ordinary Company Porters are mandatory. The carrier never
//! owns the Stone: destination escrow pays the source seller at collection,
//! the destination owns the in-transit cargo, and the carrier earns its
//! freight fee only after physical delivery.

use super::*;

use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::{
    CharacterObjective, CivicHallUpgradeWorksite, CivicTradeContract, CompanyTradeRoute,
    ConstructionSite, TradeContractId, TradeContractStatus, TradeRouteHistory, TradeRouteId,
    TradeRouteMode, TradeRouteSchedule, TradeRouteStatus, TradeRouteStop, TradeRouteStopAction,
    TradeRouteTrip,
};
use shared::economy::{MarketSeller, FOUNDING_DAILY_WAGE};

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
    target_stock: u32,
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
}

#[derive(Debug, Clone, Copy)]
struct PublicMarketSnapshot {
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
    // Households choose among substitutable foods and record their residual
    // ration demand only against the final attempted good. A merchant does
    // not know every preference, but ordinary food sales are evidence that a
    // much cheaper edible alternative can find buyers. Share only one quarter
    // of aggregate demand with each edible to avoid cloning one hungry
    // household into four simultaneous full-strength markets.
    let (units_sold, funded_unmet) = if good.is_edible() {
        let aggregate_units_sold = Good::ALL
            .into_iter()
            .filter(|candidate| candidate.is_edible())
            .map(|candidate| {
                let flow = market.pool(candidate);
                flow.day
                    .consumer_units
                    .saturating_add(flow.previous_day.consumer_units)
            })
            .fold(0u64, u64::saturating_add);
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
        (
            direct_units_sold.max(aggregate_units_sold.div_ceil(4)),
            direct_funded_unmet.max(aggregate_funded_unmet.div_ceil(4)),
        )
    } else {
        (direct_units_sold, direct_funded_unmet)
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
        recent_units_sold: units_sold.min(u64::from(u32::MAX)) as u32,
        recent_sale_price: sale_coin.checked_div(units_sold).unwrap_or(0),
        funded_unmet_units: funded_unmet.min(u64::from(u32::MAX)) as u32,
        target_stock: pool.target_stock,
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
        BusinessStrategy::Growth => (800, 35, 60),
        BusinessStrategy::Balanced => (1_500, 45, 100),
        BusinessStrategy::HighMargin => (2_800, 50, 150),
        BusinessStrategy::Cautious => (2_000, 65, 140),
        BusinessStrategy::Opportunistic => (1_100, 30, 80),
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

fn evaluate_merchant_opportunity(
    source: CompanyTradeObservation,
    destination: CompanyTradeObservation,
    source_position: Vec3,
    destination_position: Vec3,
    strategy: shared::economy::BusinessStrategy,
    incoming_units: u32,
) -> Option<MerchantOpportunity> {
    if source.settlement == destination.settlement
        || source.good != destination.good
        || source.listed_units == 0
    {
        return None;
    }
    let (minimum_return_bps, minimum_confidence, minimum_profit) = strategy_trade_terms(strategy);
    let confidence = source.confidence.min(destination.confidence);
    if confidence < minimum_confidence {
        return None;
    }

    let recent_demand = destination
        .funded_unmet_units
        .saturating_add(destination.recent_units_sold.div_ceil(2));
    if recent_demand == 0 {
        return None;
    }
    let shelf_gap = destination
        .target_stock
        .saturating_sub(destination.listed_units);
    let demand_units = recent_demand
        .saturating_add(shelf_gap.min(destination.recent_units_sold))
        .saturating_sub(incoming_units);
    if demand_units == 0 {
        return None;
    }

    let unit_capacity =
        (shared::economy::capacity::PORTER / source.good.bulk_per_unit().max(1)).max(1);
    let cargo_units = source.listed_units.min(demand_units).min(unit_capacity);
    if cargo_units == 0 {
        return None;
    }

    let scarcity_price =
        source
            .good
            .base_price()
            .saturating_mul(if destination.funded_unmet_units > 0 {
                125
            } else {
                100
            })
            / 100;
    let expected_sale_price = if destination.listed_units > 0 {
        destination.asking_price.saturating_sub(1).max(1)
    } else {
        destination
            .recent_sale_price
            .max(destination.asking_price)
            .max(scarcity_price)
    };
    if expected_sale_price <= source.asking_price {
        return None;
    }

    let gross_revenue = expected_sale_price.saturating_mul(u64::from(cargo_units));
    let destination_fee = gross_revenue
        .saturating_mul(u64::from(destination.market_fee_bps))
        .div_ceil(BASIS_POINTS);
    let distance = Vec2::new(
        destination_position.x - source_position.x,
        destination_position.z - source_position.z,
    )
    .length();
    let journey_cost = FOUNDING_DAILY_WAGE.saturating_add(
        ((distance * 2.0 / 100.0).ceil() as u64).saturating_mul(JOURNEY_PENNIES_PER_100_METRES),
    );
    let economics = |unit_purchase_price: u64| {
        let purchase_cost = unit_purchase_price.saturating_mul(u64::from(cargo_units));
        let uncertainty_cost =
            purchase_cost.saturating_mul(u64::from(100_u8.saturating_sub(confidence))) / 500;
        let total_cost = purchase_cost
            .saturating_add(destination_fee)
            .saturating_add(journey_cost)
            .saturating_add(uncertainty_cost);
        let profit = i128::from(gross_revenue) - i128::from(total_cost);
        let return_bps = if profit <= 0 {
            0
        } else {
            (profit as u128).saturating_mul(u128::from(BASIS_POINTS))
                / u128::from(purchase_cost.max(1))
        };
        (profit, return_bps)
    };
    let (expected_profit, return_bps) = economics(source.asking_price);
    let acceptable = |profit: i128, return_bps: u128| {
        profit >= i128::from(minimum_profit) && return_bps >= u128::from(minimum_return_bps)
    };
    if !acceptable(expected_profit, return_bps) {
        return None;
    }

    // Find the highest small quote miss this trip can tolerate without
    // violating the Master's own decision rule. This is a limit order, not a
    // forced price: the market still fills at the seller's actual ask.
    let mut maximum_purchase_price = source.asking_price;
    let mut high = uncertain_purchase_limit(source.asking_price, confidence);
    while maximum_purchase_price < high {
        let candidate = maximum_purchase_price + (high - maximum_purchase_price).div_ceil(2);
        let (profit, candidate_return) = economics(candidate);
        if acceptable(profit, candidate_return) {
            maximum_purchase_price = candidate;
        } else {
            high = candidate.saturating_sub(1);
        }
    }

    Some(MerchantOpportunity {
        origin: source.settlement,
        destination: destination.settlement,
        good: source.good,
        cargo_units,
        maximum_purchase_price,
        // A merchant tries to undercut the observed destination without
        // pricing below the return which justified dispatching the wagon.
        minimum_sale_price: expected_sale_price,
        expected_profit: expected_profit.min(i128::from(i64::MAX)) as i64,
        confidence,
    })
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
    halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            &GoodsInventory,
            &MootMarket,
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
    )>,
    porters: Query<&CompanyPorter>,
    mut routes: Query<(
        Entity,
        Option<&TradeRouteId>,
        &mut CompanyTradeRoute,
        &TradeRouteSchedule,
        &TradeRouteHistory,
        Option<&mut AutonomousMerchantRoute>,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    if intelligence.processed_day == Some(day) {
        return;
    }
    intelligence.processed_day = Some(day);

    let mut markets: Vec<_> = halls
        .iter()
        .filter(|(_, _, _, market)| market.supports_regional_trade())
        .map(
            |(settlement, position, _inventory, market)| PublicMarketSnapshot {
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
    let autopilot_companies: HashSet<_> = company_snapshots
        .iter()
        .filter_map(|company| company.policy.autopilot.then_some(company.id))
        .collect();

    #[derive(Clone, Copy)]
    struct AiWarehouse {
        id: shared::components::BuildingId,
        company: shared::components::CompanyId,
        settlement: shared::components::SettlementId,
        stock: [u32; Good::COUNT],
    }
    let mut warehouse_snapshots: Vec<_> = warehouses
        .iter()
        .filter(|(_, _, _, building, _, condition)| {
            building.kind == SettlementBuildingKind::StorageHall
                && condition.is_none_or(|condition| condition.state.can_operate())
        })
        .map(|(id, company, building_of, _, inventory, _)| AiWarehouse {
            id: *id,
            company: company.0,
            settlement: building_of.0,
            stock: std::array::from_fn(|index| inventory.amount(Good::ALL[index])),
        })
        .collect();
    warehouse_snapshots.sort_unstable_by_key(|warehouse| warehouse.id);

    let porter_count: HashMap<_, usize> =
        porters.iter().fold(HashMap::new(), |mut counts, porter| {
            *counts.entry(porter.company).or_default() += 1;
            counts
        });
    let mut routes_per_company = HashMap::<shared::components::CompanyId, usize>::new();
    let mut active_routes_per_company = HashMap::<shared::components::CompanyId, usize>::new();
    let mut incoming = HashMap::<(shared::components::SettlementId, Good), u32>::new();
    let mut existing_lanes = HashSet::new();
    let mut recent_arrival_reports =
        HashMap::<shared::components::CompanyId, Vec<CompanyTradeObservation>>::new();
    for (_, _, route, schedule, history, _) in routes.iter_mut() {
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
        }
        existing_lanes.insert((route.company, route.origin, route.destination, route.good));
        if route.mode == TradeRouteMode::Merchant && route.status != TradeRouteStatus::Mothballed {
            let units = incoming.entry((route.destination, route.good)).or_default();
            *units = units.saturating_add(route.cargo_target);
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
            if source.settlement == destination.settlement {
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

    // Learn from embodied routes and decide whether each NPC trial deserves
    // another circuit. Unsold cargo is intentionally visible as failure; a
    // consignment is not revenue merely because a porter reached town.
    for (_, route_id, mut route, schedule, history, autonomous) in routes.iter_mut() {
        let Some(mut autonomous) = autonomous else {
            continue;
        };
        if !autopilot_companies.contains(&route.company) {
            continue;
        }
        if autonomous.last_review_day == day {
            continue;
        }
        autonomous.last_review_day = day;
        let completed_now = route.completed_trips > autonomous.last_completed_trips;
        if completed_now {
            let last_trip = history.trips().last().copied();
            let unsold = halls
                .iter()
                .find(|(settlement, ..)| **settlement == route.destination)
                .map_or(0, |(_, _, _, market)| {
                    market.seller_listed_units(MarketSeller::Business(route.warehouse), route.good)
                });
            let disappointing =
                last_trip.is_none_or(|trip| trip.units == 0) || unsold >= route.cargo_target.max(1);
            autonomous.disappointing_reviews = if disappointing {
                autonomous.disappointing_reviews.saturating_add(1)
            } else {
                0
            };
            autonomous.last_completed_trips = route.completed_trips;
        }

        if autonomous.disappointing_reviews >= 2 {
            let mothballed_day = *autonomous.mothballed_day.get_or_insert(day);
            if day.saturating_sub(mothballed_day) >= AUTONOMOUS_ROUTE_RETRY_DAYS
                && route.assigned_caravaner.is_none()
                && active_routes_per_company
                    .get(&route.company)
                    .copied()
                    .unwrap_or(0)
                    < porter_count.get(&route.company).copied().unwrap_or(0)
            {
                autonomous.disappointing_reviews = 1;
                autonomous.mothballed_day = None;
                route.status = TradeRouteStatus::WaitingForPorter;
                *active_routes_per_company.entry(route.company).or_default() += 1;
                info!(
                    "Company #{} reopened autonomous {} route #{} after a seven-day market pause",
                    route.company.0,
                    route.good.label(),
                    route_id.map_or(0, |id| id.0),
                );
                continue;
            }
            if route.status != TradeRouteStatus::Mothballed {
                info!(
                    "Company #{} mothballed autonomous {} route #{} after repeated empty or stranded cargo (expected {} coin/trip, confidence {}%)",
                    route.company.0,
                    route.good.label(),
                    route_id.map_or(0, |id| id.0),
                    shared::economy::format_money(autonomous.expected_trip_profit.max(0) as u64),
                    autonomous.confidence,
                );
            }
            route.automatic = false;
            if route.assigned_caravaner.is_none() {
                route.status = TradeRouteStatus::Mothballed;
            }
            continue;
        }
        if route.status == TradeRouteStatus::Idle {
            route.status = TradeRouteStatus::WaitingForPorter;
            *active_routes_per_company.entry(route.company).or_default() += 1;
        }
        debug_assert!(schedule.stops().len() >= 2);
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
        let protected_payroll = payroll_by_company
            .get(&company.id)
            .copied()
            .unwrap_or(0)
            .saturating_mul(u64::from(company.policy.payroll_reserve_days));
        let available_cash = company
            .account
            .cash
            .saturating_sub(protected_payroll)
            .saturating_sub(company.account.wage_arrears)
            .saturating_sub(company.account.tax_arrears);
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
                    active_routes_per_company.get(&company.id).copied().unwrap_or(0),
                    routes_per_company.get(&company.id).copied().unwrap_or(0),
                );
            }
            continue;
        }

        let attention_budget = 2 + usize::from(master_attributes.intelligence() / 25);
        let mut candidates = Vec::new();
        for warehouse in warehouse_snapshots
            .iter()
            .filter(|warehouse| warehouse.company == company.id)
        {
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
                            && day.saturating_sub(observation.observed_day)
                                <= TRADE_INTEL_MAX_AGE_DAYS
                    })
                {
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
                for mut source in knowledge
                    .observations
                    .iter()
                    .copied()
                    .filter(|observation| {
                        observation.good == good
                            && observation.settlement != warehouse.settlement
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
            },
            Replicate::to_clients(NetworkTarget::All),
        ));
        *routes_per_company.entry(company.id).or_default() += 1;
        *active_routes_per_company.entry(company.id).or_default() += 1;
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
    )>,
    contracts: Query<&CivicTradeContract>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let active: HashSet<_> = contracts
        .iter()
        .filter(|contract| contract.status.is_active())
        .map(|contract| (contract.destination, contract.good))
        .collect();

    let source_offers: Vec<_> = halls
        .iter()
        .flat_map(|(settlement, _, inventory, market)| {
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
    let settlement_ids: HashSet<_> = halls
        .iter()
        .filter_map(|(settlement, _, _, market)| {
            market.supports_regional_trade().then_some(*settlement)
        })
        .collect();

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
        let Some((_, mut destination, destination_store, destination_market)) = halls
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

        let source_offer = source_offers
            .iter()
            .copied()
            .filter(|(origin, _, good, _, _)| *origin != building_of.0 && *good == project.material)
            .min_by_key(|(origin, seller, _, price, _)| (*price, *origin, *seller));
        // An unbound tender exists to invite production in *another*
        // settlement. With no possible remote origin it would only remove the
        // buyer's money from circulation and suppress repeated local purchase
        // attempts forever. Keep the treasury liquid and let the local market
        // publish its ordinary unmet-demand signal instead.
        if source_offer.is_none()
            && !settlement_ids
                .iter()
                .any(|settlement| *settlement != building_of.0)
        {
            continue;
        }
        let maximum_unit_price = source_offer
            .map(|(_, _, _, price, _)| price)
            .unwrap_or_else(|| project.material.base_price());
        let offered_units = source_offer
            .map(|(_, _, _, _, units)| units)
            .unwrap_or(import_units);
        let carry_units =
            (shared::economy::capacity::PORTER / project.material.bulk_per_unit()).max(1);
        let candidate_units = import_units.min(offered_units).min(carry_units);
        let Some((units, delivery_fee_per_bulk, reserved_cash)) =
            (1..=candidate_units).rev().find_map(|units| {
                let delivery_fee_per_bulk =
                    contract_delivery_pennies_per_bulk(project.material, units);
                let per_unit = maximum_unit_price.saturating_add(
                    delivery_fee_per_bulk
                        .saturating_mul(u64::from(project.material.bulk_per_unit())),
                );
                let reserved_cash = per_unit.saturating_mul(u64::from(units));
                (reserved_cash <= destination.treasury).then_some((
                    units,
                    delivery_fee_per_bulk,
                    reserved_cash,
                ))
            })
        else {
            continue;
        };
        destination.treasury -= reserved_cash;
        commands.spawn((
            CivicTradeContract {
                origin: source_offer.map(|(origin, ..)| origin),
                destination: building_of.0,
                good: project.material,
                source_seller: source_offer.map(|(_, seller, ..)| seller),
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
    halls: Query<
        (
            Entity,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &GoodsInventory,
            &MootMarket,
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
    )>,
    mut contracts: Query<(Entity, &TradeContractId, &mut CivicTradeContract)>,
    mut routes: Query<(
        Entity,
        &TradeRouteId,
        &mut CompanyTradeRoute,
        Option<&TradeRouteSchedule>,
    )>,
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
            Option<&strategic::StrategicPerson>,
            Option<&strategic::StrategicTravel>,
        ),
        With<CharacterKind>,
    >,
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
    let hall_snapshots: HashMap<_, _> = halls
        .iter()
        .map(|(_entity, id, position, rotation, _, _)| {
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
        .filter_map(|(_, id, _, _, _, market)| market.supports_regional_trade().then_some(*id))
        .collect();
    let source_offers: Vec<_> = halls
        .iter()
        .flat_map(|(_, settlement, _, _, inventory, market)| {
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
            |(_entity, id, building_of, company, _, position, rotation, condition)| {
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
        if contract.origin.is_none() || contract.source_seller.is_none() {
            let Some((origin, seller, _, _, _)) = source_offers
                .iter()
                .copied()
                .filter(|(origin, _, good, price, units)| {
                    *origin != contract.destination
                        && *good == contract.good
                        && *price <= contract.maximum_unit_price
                        && *units >= contract.remaining_units()
                })
                .min_by_key(|(origin, seller, _, price, _)| (*price, *origin, *seller))
            else {
                continue;
            };
            contract.origin = Some(origin);
            contract.source_seller = Some(seller);
            info!(
                "Civic trade contract #{} bound its {} tender to seller {:?} in settlement #{}",
                contract_id.0,
                contract.good.label(),
                seller,
                origin.0,
            );
        }
        let Some(origin_id) = contract.origin else {
            continue;
        };
        if !regional_markets.contains(&origin_id)
            || !regional_markets.contains(&contract.destination)
        {
            continue;
        }
        let (Some(origin), Some(destination)) = (
            hall_snapshots.get(&origin_id),
            hall_snapshots.get(&contract.destination),
        ) else {
            continue;
        };
        let fee = u64::from(contract.remaining_units())
            .saturating_mul(u64::from(contract.good.bulk_per_unit()))
            .saturating_mul(contract.delivery_fee_per_bulk);
        let distance = Vec2::new(
            destination.position.x - origin.position.x,
            destination.position.z - origin.position.z,
        )
        .length();
        let journey_allowance =
            ((distance / 100.0).ceil() as u64).saturating_mul(JOURNEY_PENNIES_PER_100_METRES);
        let Some(warehouse) = warehouse_snapshots
            .iter()
            .copied()
            .filter(|warehouse| warehouse.can_operate && warehouse.settlement == origin_id)
            .filter(|warehouse| {
                porters.iter().any(|(_, _, porter, _, ..)| {
                    porter.company == warehouse.company && porter.storage_hall == warehouse.id
                })
            })
            .filter(|_| fee >= FOUNDING_DAILY_WAGE.saturating_add(journey_allowance))
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
                    _,
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
                    _,
                    strategic_travel,
                )| {
                    porter.company == route.company
                        && porter.storage_hall == route.warehouse
                        && inventory.is_empty()
                        && routine.is_none()
                        && internal.is_none()
                        && market.is_none()
                        && home.is_none()
                        && road.is_none()
                        && queue.is_none()
                        && meal.is_none()
                        && strategic_travel.is_none()
                },
            )
            .min_by_key(|(_, person, ..)| **person);
        let Some((porter_entity, person, ..)) = candidate else {
            continue;
        };
        route.assigned_caravaner = Some(*person);
        route.status = TradeRouteStatus::GoingToOrigin;
        commands
            .entity(porter_entity)
            .remove::<strategic::StrategicPerson>()
            .remove::<strategic::PendingStrategicDemotion>()
            .insert((
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
            schedule
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
                    _,
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
                    _,
                    strategic_travel,
                )| {
                    porter.company == route.company
                        && porter.storage_hall == route.warehouse
                        && inventory.is_empty()
                        && routine.is_none()
                        && internal.is_none()
                        && market.is_none()
                        && home.is_none()
                        && road.is_none()
                        && queue.is_none()
                        && meal.is_none()
                        && strategic_travel.is_none()
                },
            )
            .min_by_key(|(_, person, ..)| **person);
        let Some((porter_entity, person, ..)) = candidate else {
            continue;
        };
        route.assigned_caravaner = Some(*person);
        route.current_stop = 0;
        route.status = TradeRouteStatus::GoingToOrigin;
        commands
            .entity(porter_entity)
            .remove::<strategic::StrategicPerson>()
            .remove::<strategic::PendingStrategicDemotion>()
            .insert((
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
    mut routes: Query<(
        &TradeRouteId,
        &mut CompanyTradeRoute,
        &mut TradeRouteHistory,
    )>,
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
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
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
            *activity = CharacterActivity::Idle;
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
            // The shared navigation retry system owns its exponential
            // backoff and wakes this exact target after geometry changes or
            // the retry deadline. Clearing all route state here made an
            // impossible overland journey launch another full search every
            // update. Keep the transaction and buyer-owned cargo intact while
            // the caravan waits; future bridges/ships can make it reachable.
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
                let target = hall_entrance(origin);
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
                *activity = CharacterActivity::Idle;
                let target = projects
                    .iter_mut()
                    .find(|(project, building_of, _, _)| {
                        building_of.0 == route.destination && project.material == route.good
                    })
                    .map_or_else(|| hall_entrance(destination), |(_, _, site, _)| site.stand);
                ensure_move_target(&mut commands, porter_entity, move_target, target);
            }
            TradeRoutePhase::InTransit => {
                let target = projects
                    .iter_mut()
                    .find(|(project, building_of, _, _)| {
                        building_of.0 == route.destination && project.material == route.good
                    })
                    .map_or_else(|| hall_entrance(destination), |(_, _, site, _)| site.stand);
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
    mut routes: Query<(
        &TradeRouteId,
        &mut CompanyTradeRoute,
        &TradeRouteSchedule,
        &mut TradeRouteHistory,
    )>,
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
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
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
        let Some((_, mut route, schedule, mut history)) = routes
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
            *activity = CharacterActivity::Idle;
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
            *activity = CharacterActivity::Idle;
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
                    let purchase = source_market.purchase_for_resale(
                        route.good,
                        requested,
                        company_account.cash,
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
            target_stock: 24,
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

        let bread = market_observation(DESTINATION, &market, Good::Bread, 4);
        assert_eq!(bread.funded_unmet_units, 2);
        assert_eq!(bread.recent_units_sold, 0);
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
            0,
        )
        .expect("the exact quote remains profitable");
        assert_eq!(exact.maximum_purchase_price, source.asking_price);
    }

    #[test]
    fn hunger_without_funded_demand_is_not_guaranteed_merchant_revenue() {
        let source = observed_market(SOURCE, Good::Bread, 100, 20, 4, 0, 100);
        let destination = observed_market(DESTINATION, Good::Bread, 220, 0, 0, 0, 55);
        assert!(evaluate_merchant_opportunity(
            source,
            destination,
            Vec3::ZERO,
            Vec3::new(40.0, 0.0, 0.0),
            shared::economy::BusinessStrategy::Balanced,
            0,
        )
        .is_none());
    }

    #[test]
    fn autonomous_importer_uses_delayed_intel_to_open_one_physical_food_trial() {
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

        app.update();

        let (_, route, schedule) = app
            .world_mut()
            .query::<(
                &AutonomousMerchantRoute,
                &CompanyTradeRoute,
                &TradeRouteSchedule,
            )>()
            .single(app.world())
            .expect("one bounded autonomous food route should be founded");
        assert_eq!(route.company, company);
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
        assert_eq!(
            app.world_mut()
                .query::<&CompanyTradeRoute>()
                .iter(app.world())
                .count(),
            2,
            "the reusable idle contract lane must not consume the company's only porter slot"
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
        app.world_mut().spawn((
            CharacterKind::Villager,
            PORTER,
            CompanyPorter {
                settlement: source_hall,
                settlement_id: SOURCE,
                company: CARRIER_COMPANY,
                storage_hall: WAREHOUSE,
            },
            GoodsInventory::new(shared::economy::capacity::PORTER),
        ));

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
    }

    #[test]
    fn physical_contract_trip_pays_seller_then_carrier_and_returns_porter() {
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

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = project_stand;
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
