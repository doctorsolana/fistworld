//! Cheap off-screen village simulation.
//!
//! People remain durable ECS records for identity, money and households, but
//! strategic residents carry no routes, door choreography or work phases.
//! Productive labour is integrated once per strategic tick by workplace.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
#[cfg(test)]
use shared::components::MootAdministration;
use shared::components::{
    AttachedTo, BuildingDoorUse, BuildingId, BuildingOf, CharacterActivity, CharacterKind,
    CharacterMotion, CivicEmployment, CivicRole, EmployedAt, FarmField, OperatedBy, PlayerPosition,
    PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    WorldTime,
};
use shared::economy::{
    BusinessAccount, BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy,
    BusinessSourcingMode, BusinessSupplyPolicy, BusinessWagePolicy, CivicAccount, Good,
    GoodsInventory, MootMarket,
};
use shared::region::{RegionCoord, SimLevel};

use crate::player::hero::MoveTarget;
use crate::world::regions::{RegionRegistry, StrategicStep};
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, RouteWaypoint, TravelRoute,
    ROAD_SPEED_MULTIPLIER,
};

use super::commerce::{internal_transfer_unit_value, MUNICIPAL_DELIVERY_PENNIES_PER_BULK};
use super::{
    ambient, business_output, farmer_seconds_per_wheat, fisher_seconds_per_food,
    lumber_seconds_per_tree, lumber_tree_yield, process_available_cycles, processing_recipe,
    viable_processing_input_purchase, BusinessEventQueue, CompanyPorter,
    ConstructionMaterialRoutine, FarmerHarvestProgress, FarmerRoutine, FishingRoutine,
    FishingWorkProgress, HomeRoutine, HouseholdShoppingRoutine, InternalDeliveryRoutine,
    LumberjackRoutine, LumberjackWorkProgress, MarketCollectionRoutine, MootMealRoutine,
    MootQueueTicket, PierTraversal, ProcessingRoutine, ProcessorWorkProgress,
    SettlementEconomyRuntime, WorkerOffDuty, WorkplaceDoorTransit, WORKDAY_END_DAY_T,
};

#[derive(Component, Debug, Clone, Copy)]
pub struct StrategicPerson;

/// A durable, per-person journey while its region is not rendered tactically.
///
/// The route and progress remain individual; only collision checks and 60 Hz
/// body stepping are collapsed. Promotion advances the plan to the exact
/// current world time, restores the remaining tactical route, and therefore
/// never teleports a resident merely because the camera approached.
#[derive(Component, Debug, Clone)]
pub struct StrategicTravel {
    goal: Vec3,
    waypoints: Vec<RouteWaypoint>,
    next: usize,
    last_world_seconds: f64,
}

impl StrategicTravel {
    #[cfg(test)]
    pub(crate) fn for_test(goal: Vec3, last_world_seconds: f64) -> Self {
        Self::from_tactical(goal, None, last_world_seconds)
    }

    fn from_tactical(target: Vec3, route: Option<&TravelRoute>, last_world_seconds: f64) -> Self {
        let mut waypoints = route
            .filter(|route| route.goal.distance_squared(target) <= 0.01)
            .map(|route| route.waypoints.iter().skip(route.next).copied().collect())
            .unwrap_or_else(Vec::new);
        if waypoints.last().is_none_or(|waypoint: &RouteWaypoint| {
            waypoint.position.distance_squared(target) > 0.01
        }) {
            waypoints.push(RouteWaypoint {
                position: target,
                on_road: false,
            });
        }
        Self {
            goal: target,
            waypoints,
            next: 0,
            last_world_seconds,
        }
    }

    fn advance(&mut self, position: &mut Vec3, rotation: &mut f32, elapsed_seconds: f64) -> bool {
        let mut remaining = elapsed_seconds.max(0.0) as f32;
        while remaining > 1.0e-5 && self.next < self.waypoints.len() {
            let waypoint = self.waypoints[self.next];
            let from = Vec2::new(position.x, position.z);
            let to = Vec2::new(waypoint.position.x, waypoint.position.z);
            let offset = to - from;
            let distance = offset.length();
            if distance <= shared::player::HERO_ARRIVE_EPSILON {
                *position = waypoint.position;
                self.next += 1;
                continue;
            }
            let direction = offset / distance;
            let speed = shared::player::HERO_MOVE_SPEED
                * if waypoint.on_road {
                    ROAD_SPEED_MULTIPLIER
                } else {
                    1.0
                };
            let travel = (speed * remaining).min(distance);
            let fraction = travel / distance;
            position.x += direction.x * travel;
            position.z += direction.y * travel;
            position.y += (waypoint.position.y - position.y) * fraction;
            *rotation = f32::atan2(-direction.x, -direction.y);
            remaining -= travel / speed;
            if travel + shared::player::HERO_ARRIVE_EPSILON >= distance {
                *position = waypoint.position;
                self.next += 1;
            }
        }
        self.next >= self.waypoints.len()
    }

    fn remaining_route(&self) -> TravelRoute {
        TravelRoute {
            goal: self.goal,
            waypoints: self.waypoints.iter().skip(self.next).copied().collect(),
            next: 0,
        }
    }
}

fn absolute_world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

#[derive(Clone, Copy)]
struct StrategicInternalTransfer {
    supplier: Entity,
    receiver: Entity,
    settlement: SettlementId,
    company: shared::components::CompanyId,
    good: Good,
    units: u32,
    unit_value: u64,
    preferred: bool,
    receiver_id: BuildingId,
    supplier_id: BuildingId,
    municipal_fee: bool,
    overflow: bool,
}

#[derive(Clone)]
struct StrategicCompanySite {
    entity: Entity,
    id: BuildingId,
    settlement: SettlementId,
    company: shared::components::CompanyId,
    kind: SettlementBuildingKind,
    store: GoodsInventory,
    procurement: BusinessProcurementPolicy,
    supply: BusinessSupplyPolicy,
    management: BusinessManagementPolicy,
    account: BusinessAccount,
    can_operate: bool,
}

/// Off-screen parity for the embodied company porter. One strategic civic
/// worker completes at most one direct trip per bounded strategic step, so
/// abstraction removes pathfinding and animation rather than creating
/// infinite freight throughput.
#[allow(clippy::type_complexity)]
pub fn advance_strategic_company_deliveries(
    step: Res<StrategicStep>,
    world_time: Query<&WorldTime>,
    mut last_serial: Local<u64>,
    civic_workers: Query<(&CivicEmployment, Option<&StrategicPerson>)>,
    private_porters: Query<(&CompanyPorter, Option<&StrategicPerson>)>,
    hall_index: Query<(Entity, &SettlementId, &MootMarket), With<Settlement>>,
    mut halls: Query<(&mut Settlement, &mut CivicAccount)>,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut sites: ParamSet<(
        Query<(
            Entity,
            &BuildingId,
            &BuildingOf,
            &OperatedBy,
            &SettlementBuilding,
            &GoodsInventory,
            &BusinessSalePolicy,
            &BusinessProcurementPolicy,
            &BusinessSupplyPolicy,
            &BusinessManagementPolicy,
            &BusinessAccount,
            Option<&shared::economy::BusinessCondition>,
        )>,
        Query<(&mut GoodsInventory, &mut BusinessAccount)>,
    )>,
) {
    if step.serial == 0 || *last_serial == step.serial {
        return;
    }
    *last_serial = step.serial;
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let companies_by_id: HashMap<shared::components::CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let strategic_porters: HashSet<SettlementId> = civic_workers
        .iter()
        .filter_map(|(employment, strategic)| {
            (strategic.is_some()
                && matches!(
                    employment.role,
                    CivicRole::MootSteward | CivicRole::MarketPorter
                ))
            .then_some(employment.settlement)
        })
        .collect();
    let strategic_private_porters: HashSet<(SettlementId, shared::components::CompanyId)> =
        private_porters
            .iter()
            .filter_map(|(porter, strategic)| {
                strategic
                    .is_some()
                    .then_some((porter.settlement_id, porter.company))
            })
            .collect();
    if strategic_porters.is_empty() && strategic_private_porters.is_empty() {
        return;
    }
    let halls_by_settlement: HashMap<SettlementId, (Entity, [u64; Good::COUNT])> = hall_index
        .iter()
        .map(|(entity, id, market)| {
            let mut prices = [0; Good::COUNT];
            for good in Good::ALL {
                prices[good.index()] = market.suggested_price(good);
            }
            (*id, (entity, prices))
        })
        .collect();

    let site_snapshots: Vec<StrategicCompanySite> = {
        let site_query = sites.p0();
        site_query
            .iter()
            .map(
                |(
                    entity,
                    id,
                    building_of,
                    operated_by,
                    building,
                    store,
                    _sale,
                    procurement,
                    supply,
                    management,
                    account,
                    condition,
                )| StrategicCompanySite {
                    entity,
                    id: *id,
                    settlement: building_of.0,
                    company: operated_by.0,
                    kind: building.kind,
                    store: store.clone(),
                    procurement: *procurement,
                    supply: *supply,
                    management: *management,
                    account: *account,
                    can_operate: condition.is_none_or(|condition| condition.state.can_operate()),
                },
            )
            .collect()
    };
    let mut candidates = Vec::new();
    for receiver in &site_snapshots {
        let municipal_fee =
            !strategic_private_porters.contains(&(receiver.settlement, receiver.company));
        if (municipal_fee && !strategic_porters.contains(&receiver.settlement))
            || !receiver.supply.automatic
            || !receiver.can_operate
        {
            continue;
        }
        for good in Good::ALL {
            let public_rule = receiver.procurement.rule(good);
            let private_rule = receiver.supply.rule(good);
            let held = receiver.store.amount(good);
            if !public_rule.enabled || !private_rule.enabled || held >= public_rule.reorder_below {
                continue;
            }
            let wanted = public_rule
                .target_units
                .saturating_sub(held)
                .min(receiver.store.free_bulk() / good.bulk_per_unit());
            for supplier in &site_snapshots {
                if supplier.entity == receiver.entity
                    || supplier.settlement != receiver.settlement
                    || supplier.company != receiver.company
                    || (super::business_output(supplier.kind) != Some(good)
                        && !(supplier.kind == SettlementBuildingKind::StorageHall
                            && supplier.store.amount(good) > 0))
                    || !supplier.can_operate
                {
                    continue;
                }
                let units = wanted
                    .min(supplier.store.amount(good))
                    .min(shared::economy::capacity::PORTER / good.bulk_per_unit());
                if units == 0 {
                    continue;
                }
                let unit_value =
                    internal_transfer_unit_value(good, &supplier.account, &supplier.management);
                let landed = unit_value.saturating_add(if municipal_fee {
                    u64::from(good.bulk_per_unit())
                        .saturating_mul(MUNICIPAL_DELIVERY_PENNIES_PER_BULK)
                } else {
                    0
                });
                let market_price = halls_by_settlement
                    .get(&receiver.settlement)
                    .map_or(u64::MAX, |(_, prices)| prices[good.index()]);
                if landed > public_rule.maximum_unit_price
                    || (private_rule.sourcing == BusinessSourcingMode::CheapestAvailable
                        && landed > market_price)
                {
                    continue;
                }
                let fee = if municipal_fee {
                    u64::from(units)
                        .saturating_mul(u64::from(good.bulk_per_unit()))
                        .saturating_mul(MUNICIPAL_DELIVERY_PENNIES_PER_BULK)
                } else {
                    0
                };
                let company_free = companies_by_id
                    .get(&receiver.company)
                    .and_then(|entity| company_accounts.get(*entity).ok())
                    .map_or(0, |company| {
                        company
                            .cash
                            .saturating_sub(company.wage_arrears)
                            .saturating_sub(company.tax_arrears)
                    });
                if company_free < fee {
                    continue;
                }
                candidates.push(StrategicInternalTransfer {
                    supplier: supplier.entity,
                    receiver: receiver.entity,
                    settlement: receiver.settlement,
                    company: receiver.company,
                    good,
                    units,
                    unit_value,
                    preferred: private_rule.preferred_supplier == Some(supplier.id),
                    receiver_id: receiver.id,
                    supplier_id: supplier.id,
                    municipal_fee,
                    overflow: false,
                });
            }
        }
    }
    for warehouse in site_snapshots
        .iter()
        .filter(|site| site.kind == SettlementBuildingKind::StorageHall && site.can_operate)
    {
        let municipal_fee =
            !strategic_private_porters.contains(&(warehouse.settlement, warehouse.company));
        if municipal_fee && !strategic_porters.contains(&warehouse.settlement) {
            continue;
        }
        for supplier in site_snapshots.iter().filter(|site| {
            site.settlement == warehouse.settlement
                && site.company == warehouse.company
                && site.entity != warehouse.entity
                && site.can_operate
                && super::business_output(site.kind).is_some()
                && site.store.used_bulk().saturating_mul(100)
                    >= site.store.bulk_capacity().saturating_mul(80)
        }) {
            let Some(good) = super::business_output(supplier.kind) else {
                continue;
            };
            let floor = (supplier.store.bulk_capacity() / good.bulk_per_unit().max(1)) / 2;
            let units = supplier
                .store
                .amount(good)
                .saturating_sub(floor)
                .min(warehouse.store.free_bulk() / good.bulk_per_unit())
                .min(shared::economy::capacity::PORTER / good.bulk_per_unit());
            if units == 0 {
                continue;
            }
            candidates.push(StrategicInternalTransfer {
                supplier: supplier.entity,
                receiver: warehouse.entity,
                settlement: warehouse.settlement,
                company: warehouse.company,
                good,
                units,
                unit_value: internal_transfer_unit_value(
                    good,
                    &supplier.account,
                    &supplier.management,
                ),
                preferred: false,
                receiver_id: warehouse.id,
                supplier_id: supplier.id,
                municipal_fee,
                overflow: true,
            });
        }
    }
    candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.settlement,
            candidate.overflow,
            std::cmp::Reverse(candidate.preferred),
            candidate.receiver_id,
            candidate.supplier_id,
        )
    });
    let mut completed = HashSet::new();
    for candidate in candidates {
        let porter_key = (
            candidate.settlement,
            (!candidate.municipal_fee).then_some(candidate.company),
        );
        if !completed.insert(porter_key) {
            continue;
        }
        let mut site_accounts = sites.p1();
        let Ok([supplier, receiver]) =
            site_accounts.get_many_mut([candidate.supplier, candidate.receiver])
        else {
            continue;
        };
        let (mut supplier_store, mut supplier_account) = supplier;
        let (mut receiver_store, mut receiver_account) = receiver;
        let transferable = candidate
            .units
            .min(supplier_store.amount(candidate.good))
            .min(receiver_store.free_bulk() / candidate.good.bulk_per_unit());
        if transferable == 0 {
            continue;
        }
        let fee = if candidate.municipal_fee {
            u64::from(transferable)
                .saturating_mul(u64::from(candidate.good.bulk_per_unit()))
                .saturating_mul(MUNICIPAL_DELIVERY_PENNIES_PER_BULK)
        } else {
            0
        };
        let Some(company_entity) = companies_by_id.get(&candidate.company).copied() else {
            continue;
        };
        let Ok(mut company_account) = company_accounts.get_mut(company_entity) else {
            continue;
        };
        let protected = company_account
            .wage_arrears
            .saturating_add(company_account.tax_arrears);
        if company_account.cash.saturating_sub(protected) < fee || !company_account.debit(fee) {
            continue;
        }
        if fee > 0 {
            receiver_account.record_delivery_fee(day, fee);
        }
        let delivered =
            supplier_store.transfer_to(&mut receiver_store, candidate.good, transferable);
        debug_assert_eq!(delivered, transferable);
        let value = candidate.unit_value.saturating_mul(u64::from(delivered));
        supplier_account.record_internal_output(day, value, delivered);
        receiver_account.record_internal_input(day, value, delivered);
        if fee > 0 {
            if let Some((hall, _)) = halls_by_settlement.get(&candidate.settlement) {
                if let Ok((mut settlement, mut civic)) = halls.get_mut(*hall) {
                    settlement.treasury = settlement.treasury.saturating_add(fee);
                    civic.record_delivery_fee_income(day, fee);
                }
            }
        }
    }
}

/// A transaction or public work already in motion is allowed to reach a safe
/// boundary before the person's expensive tactical state is stripped.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PendingStrategicDemotion;

#[derive(Resource, Default)]
pub struct StrategicProductionProgress {
    seconds: HashMap<BuildingId, f64>,
}

fn has_loaded_trade_handoff(
    inventories: &Query<&GoodsInventory, With<CharacterKind>>,
    person: Entity,
    farmer: bool,
    fisher: bool,
    lumberjack: bool,
) -> bool {
    inventories.get(person).is_ok_and(|inventory| {
        (farmer && inventory.amount(Good::Wheat) > 0)
            || (fisher && inventory.amount(Good::Food) > 0)
            || (lumberjack && inventory.amount(Good::Wood) > 0)
    })
}

/// Exact overlap between the elapsed strategic interval and ordinary shifts.
/// This keeps one 100x integration step equivalent to many smaller 1x steps at
/// dawn, shift end and across whole day/night cycles.
fn productive_seconds_ending_at(clock: &WorldTime, elapsed: f64) -> f64 {
    let cycle = f64::from(clock.cycle_duration());
    if cycle <= 0.0 || elapsed <= 0.0 {
        return 0.0;
    }
    let work_end = f64::from(clock.day_duration * WORKDAY_END_DAY_T).clamp(0.0, cycle);
    let end = f64::from(clock.day) * cycle + f64::from(clock.seconds_in_cycle);
    let start = (end - elapsed).max(0.0);
    let cumulative = |seconds: f64| {
        let cycles = (seconds / cycle).floor();
        let within = seconds.rem_euclid(cycle);
        cycles * work_end + within.min(work_end)
    };
    (cumulative(end) - cumulative(start)).max(0.0)
}

#[allow(clippy::type_complexity)]
pub fn update_person_simulation_lod(
    mut commands: Commands,
    registry: Res<RegionRegistry>,
    world_time: Query<&WorldTime>,
    inventories: Query<&GoodsInventory, With<CharacterKind>>,
    people: Query<
        (
            Entity,
            &RegionCoord,
            &PlayerPosition,
            Option<&PlayerRotation>,
            Option<&MoveTarget>,
            Option<&TravelRoute>,
            Option<&StrategicTravel>,
            Option<&StrategicPerson>,
            (
                Has<ConstructionMaterialRoutine>,
                Has<RoadBuilderRoutine>,
                Has<MarketCollectionRoutine>,
                Has<InternalDeliveryRoutine>,
                Has<HouseholdShoppingRoutine>,
                Has<MootQueueTicket>,
                Has<MootMealRoutine>,
                Has<FarmerRoutine>,
                Has<FishingRoutine>,
                Has<LumberjackRoutine>,
            ),
            &super::VillagerIntent,
        ),
        (With<CharacterKind>, Without<shared::components::Hero>),
    >,
    changed_people: Query<
        (
            Entity,
            &RegionCoord,
            &PlayerPosition,
            Option<&PlayerRotation>,
            Option<&MoveTarget>,
            Option<&TravelRoute>,
            Option<&StrategicTravel>,
            Option<&StrategicPerson>,
            (
                Has<ConstructionMaterialRoutine>,
                Has<RoadBuilderRoutine>,
                Has<MarketCollectionRoutine>,
                Has<InternalDeliveryRoutine>,
                Has<HouseholdShoppingRoutine>,
                Has<MootQueueTicket>,
                Has<MootMealRoutine>,
                Has<FarmerRoutine>,
                Has<FishingRoutine>,
                Has<LumberjackRoutine>,
            ),
            &super::VillagerIntent,
        ),
        (
            With<CharacterKind>,
            Without<shared::components::Hero>,
            Changed<RegionCoord>,
        ),
    >,
    pending: Query<
        (
            Entity,
            &RegionCoord,
            &PlayerPosition,
            Option<&PlayerRotation>,
            Option<&MoveTarget>,
            Option<&TravelRoute>,
            Option<&StrategicTravel>,
            (
                Has<ConstructionMaterialRoutine>,
                Has<RoadBuilderRoutine>,
                Has<MarketCollectionRoutine>,
                Has<InternalDeliveryRoutine>,
                Has<HouseholdShoppingRoutine>,
                Has<MootQueueTicket>,
                Has<MootMealRoutine>,
                Has<FarmerRoutine>,
                Has<FishingRoutine>,
                Has<LumberjackRoutine>,
            ),
            &super::VillagerIntent,
        ),
        (
            With<CharacterKind>,
            Without<shared::components::Hero>,
            With<PendingStrategicDemotion>,
        ),
    >,
    mut last_revision: Local<Option<u64>>,
) {
    let now = world_time.iter().next().map_or(0.0, absolute_world_seconds);
    let revision_changed = *last_revision != Some(registry.revision());
    if revision_changed {
        *last_revision = Some(registry.revision());
        for (
            entity,
            region,
            position,
            rotation,
            target,
            route,
            strategic_travel,
            strategic,
            (
                construction,
                road,
                market,
                internal,
                shopping,
                queue,
                meal,
                farmer,
                fisher,
                lumberjack,
            ),
            intent,
        ) in people.iter()
        {
            let loaded_trade =
                has_loaded_trade_handoff(&inventories, entity, farmer, fisher, lumberjack);
            apply_person_lod(
                &mut commands,
                &registry,
                entity,
                *region,
                position.0,
                rotation.map_or(0.0, |rotation| rotation.0),
                target,
                route,
                strategic_travel,
                strategic.is_some(),
                construction
                    || road
                    || market
                    || internal
                    || shopping
                    || queue
                    || meal
                    || loaded_trade
                    || matches!(intent, super::VillagerIntent::Travelling { .. }),
                now,
            );
        }
    } else {
        for (
            entity,
            region,
            position,
            rotation,
            target,
            route,
            strategic_travel,
            strategic,
            (
                construction,
                road,
                market,
                internal,
                shopping,
                queue,
                meal,
                farmer,
                fisher,
                lumberjack,
            ),
            intent,
        ) in changed_people.iter()
        {
            let loaded_trade =
                has_loaded_trade_handoff(&inventories, entity, farmer, fisher, lumberjack);
            apply_person_lod(
                &mut commands,
                &registry,
                entity,
                *region,
                position.0,
                rotation.map_or(0.0, |rotation| rotation.0),
                target,
                route,
                strategic_travel,
                strategic.is_some(),
                construction
                    || road
                    || market
                    || internal
                    || shopping
                    || queue
                    || meal
                    || loaded_trade
                    || matches!(intent, super::VillagerIntent::Travelling { .. }),
                now,
            );
        }
    }

    for (
        entity,
        region,
        position,
        rotation,
        target,
        route,
        strategic_travel,
        (construction, road, market, internal, shopping, queue, meal, farmer, fisher, lumberjack),
        intent,
    ) in pending.iter()
    {
        let loaded_trade =
            has_loaded_trade_handoff(&inventories, entity, farmer, fisher, lumberjack);
        let critical = construction
            || road
            || market
            || internal
            || shopping
            || queue
            || meal
            || loaded_trade
            || matches!(intent, super::VillagerIntent::Travelling { .. });
        if !critical {
            apply_person_lod(
                &mut commands,
                &registry,
                entity,
                *region,
                position.0,
                rotation.map_or(0.0, |rotation| rotation.0),
                target,
                route,
                strategic_travel,
                false,
                false,
                now,
            );
        }
    }
}

fn apply_person_lod(
    commands: &mut Commands,
    registry: &RegionRegistry,
    entity: Entity,
    region: RegionCoord,
    position: Vec3,
    rotation: f32,
    target: Option<&MoveTarget>,
    route: Option<&TravelRoute>,
    strategic_travel: Option<&StrategicTravel>,
    is_strategic: bool,
    critical_work: bool,
    now: f64,
) {
    let level = registry
        .get(region)
        .map_or(SimLevel::Strategic, |state| state.sim_level);
    if level == SimLevel::Tactical {
        if is_strategic {
            if let Some(strategic_travel) = strategic_travel {
                let mut travel = strategic_travel.clone();
                let mut promoted_position = position;
                let mut promoted_rotation = rotation;
                let finished = travel.advance(
                    &mut promoted_position,
                    &mut promoted_rotation,
                    now - travel.last_world_seconds,
                );
                let mut entity_commands = commands.entity(entity);
                entity_commands.insert((
                    PlayerPosition(promoted_position),
                    PlayerRotation(promoted_rotation),
                    RegionCoord::from_world_pos(promoted_position),
                    CharacterMotion::STATIONARY,
                ));
                if finished {
                    entity_commands.remove::<StrategicTravel>();
                } else {
                    let remaining_route = travel.remaining_route();
                    entity_commands.insert((MoveTarget(travel.goal), remaining_route));
                    entity_commands.remove::<StrategicTravel>();
                }
            }
            commands.entity(entity).remove::<StrategicPerson>();
        }
        commands.entity(entity).remove::<PendingStrategicDemotion>();
        return;
    }
    if critical_work {
        commands.entity(entity).insert(PendingStrategicDemotion);
        return;
    }
    if let Some(target) = target {
        commands
            .entity(entity)
            .insert(StrategicTravel::from_tactical(target.0, route, now));
    }
    commands
        .entity(entity)
        .insert((
            StrategicPerson,
            CharacterActivity::Idle,
            CharacterMotion::STATIONARY,
        ))
        .remove::<PendingStrategicDemotion>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<BuildingDoorUse>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<PierTraversal>()
        .remove::<HomeRoutine>()
        .remove::<HouseholdShoppingRoutine>()
        .remove::<MootQueueTicket>()
        .remove::<MootMealRoutine>()
        .remove::<super::moot_services::PermitPickupRoutine>()
        .remove::<FarmerRoutine>()
        .remove::<FishingRoutine>()
        .remove::<LumberjackRoutine>()
        .remove::<ProcessingRoutine>()
        .remove::<FarmerHarvestProgress>()
        .remove::<FishingWorkProgress>()
        .remove::<LumberjackWorkProgress>()
        .remove::<ProcessorWorkProgress>()
        .remove::<WorkerOffDuty>()
        .remove::<ambient::AmbientRoutine>();
}

/// Advance individual off-screen journeys at the bounded strategic cadence.
/// Promotion performs the fractional catch-up since this pass, so the 1 Hz
/// cadence is not visible when a camera enters the region.
pub fn advance_strategic_travel(
    mut commands: Commands,
    step: Res<StrategicStep>,
    mut last_serial: Local<u64>,
    mut travellers: Query<
        (
            Entity,
            &mut StrategicTravel,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
        ),
        With<StrategicPerson>,
    >,
) {
    if step.serial == 0 || *last_serial == step.serial {
        return;
    }
    *last_serial = step.serial;
    for (entity, mut travel, mut position, mut rotation, mut region) in travellers.iter_mut() {
        let finished = travel.advance(&mut position.0, &mut rotation.0, step.elapsed_world_seconds);
        travel.last_world_seconds += step.elapsed_world_seconds;
        let next_region = RegionCoord::from_world_pos(position.0);
        if *region != next_region {
            *region = next_region;
        }
        if finished {
            commands.entity(entity).remove::<StrategicTravel>();
        }
    }
}

/// Integrate physical output and porter collection for unobserved workers.
/// Money, storage limits, sale policy and Moot prices remain the exact live
/// systems; only walking and animation are collapsed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn advance_strategic_villages(
    step: Res<StrategicStep>,
    world_time: Query<&WorldTime>,
    mut last_serial: Local<u64>,
    mut progress: ResMut<StrategicProductionProgress>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    company_branch_policies: Query<(
        &shared::components::CompanyId,
        &shared::economy::CompanyBranchPolicies,
    )>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    workers: Query<(&EmployedAt, Option<&StrategicPerson>)>,
    civic_workers: Query<(&CivicEmployment, Option<&StrategicPerson>)>,
    private_porters: Query<(&CompanyPorter, Option<&StrategicPerson>)>,
    fields: Query<(&FarmField, &AttachedTo)>,
    hall_index: Query<(Entity, &SettlementId), With<Settlement>>,
    mut halls: Query<
        (&mut GoodsInventory, &mut MootMarket),
        (With<Settlement>, Without<SettlementBuilding>),
    >,
    mut businesses: Query<
        (
            &BuildingId,
            &BuildingOf,
            &SettlementBuilding,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            Option<&shared::economy::BusinessCondition>,
            Option<&BusinessProcurementPolicy>,
            Option<&mut BusinessAccount>,
            Option<&BusinessWagePolicy>,
            &OperatedBy,
        ),
        (With<SettlementBuilding>, Without<Settlement>),
    >,
) {
    if step.serial == 0 || *last_serial == step.serial {
        return;
    }
    *last_serial = step.serial;
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let companies_by_id: HashMap<shared::components::CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let productive_seconds = productive_seconds_ending_at(clock, step.elapsed_world_seconds);
    if productive_seconds <= 0.0 {
        return;
    }

    let mut worker_counts: HashMap<BuildingId, u32> = HashMap::new();
    for (employment, strategic) in workers.iter() {
        if strategic.is_some() {
            *worker_counts.entry(employment.0).or_default() += 1;
        }
    }
    let halls_by_id: HashMap<SettlementId, Entity> = hall_index
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let strategic_porters: HashSet<SettlementId> = civic_workers
        .iter()
        .filter_map(|(employment, strategic)| {
            (matches!(
                employment.role,
                CivicRole::MootSteward | CivicRole::MarketPorter
            ) && strategic.is_some())
            .then_some(employment.settlement)
        })
        .collect();
    let strategic_private_porters: HashSet<(SettlementId, shared::components::CompanyId)> =
        private_porters
            .iter()
            .filter_map(|(porter, strategic)| {
                strategic
                    .is_some()
                    .then_some((porter.settlement_id, porter.company))
            })
            .collect();
    let mut fields_by_building: HashMap<BuildingId, u32> = HashMap::new();
    for (_, attached) in fields.iter() {
        *fields_by_building.entry(attached.0).or_default() += 1;
    }
    let mut live_buildings = HashSet::new();
    let branch_policies: HashMap<_, _> = company_branch_policies
        .iter()
        .flat_map(|(company, policies)| {
            policies.branches().iter().flat_map(move |branch| {
                Good::ALL
                    .into_iter()
                    .map(move |good| ((*company, branch.settlement, good), branch.resource(good)))
            })
        })
        .collect();
    let mut branch_public_remaining = HashMap::<_, u32>::new();
    let mut branch_asks = HashMap::<_, u64>::new();
    for (_, building_of, building, inventory, sale, condition, _, _, _, operated_by) in
        businesses.iter()
    {
        if condition.is_some_and(|condition| !condition.state.can_operate()) {
            continue;
        }
        for good in Good::ALL {
            *branch_public_remaining
                .entry((operated_by.0, building_of.0, good))
                .or_default() += inventory.amount(good);
        }
        if let Some(good) = business_output(building.kind) {
            let ask = sale.asking_unit_price.max(sale.minimum_unit_price).max(1);
            branch_asks
                .entry((operated_by.0, building_of.0, good))
                .and_modify(|current| *current = (*current).min(ask))
                .or_insert(ask);
        }
    }
    for (key, total) in branch_public_remaining.iter_mut() {
        let policy = branch_policies.get(key).copied().unwrap_or_default();
        *total = policy.public_surplus(*total, 0);
    }

    for (
        id,
        building_of,
        building,
        mut store,
        policy,
        condition,
        procurement,
        mut account,
        wage,
        operated_by,
    ) in businesses.iter_mut()
    {
        live_buildings.insert(*id);
        if condition.is_some_and(|condition| !condition.state.can_operate()) {
            continue;
        }
        let worker_count = worker_counts.get(id).copied().unwrap_or(0);
        if worker_count == 0 {
            continue;
        }
        let Some(good) = business_output(building.kind) else {
            continue;
        };
        let hall_entity = halls_by_id.get(&building_of.0).copied();

        // Collapse the same porter purchase used tactically. This is still a
        // physical market transfer with an exact buyer account and seller
        // fills; only the walk between the Moot and workplace is abstracted.
        if strategic_porters.contains(&building_of.0) {
            if let (Some(procurement), Some(account), Some(hall_entity)) = (
                procurement.filter(|policy| policy.needs_anything()),
                account.as_deref_mut(),
                hall_entity,
            ) {
                if let Ok((mut hall_store, mut market)) = halls.get_mut(hall_entity) {
                    let payroll_reserve = if account.gross_revenue == 0
                        && account.current_day.produced_units == 0
                        && account.previous_day.produced_units == 0
                    {
                        0
                    } else {
                        wage.map_or(0, |wage| wage.daily_wage)
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
                    if budget > 0 {
                        for input in Good::ALL {
                            let rule = procurement.rule(input);
                            let held = store.amount(input);
                            if !rule.enabled || held >= rule.reorder_below {
                                continue;
                            }
                            let wanted = rule
                                .target_units
                                .saturating_sub(held)
                                .min(store.free_bulk() / input.bulk_per_unit())
                                .min(hall_store.amount(input));
                            let preview = market.preview_purchase(
                                input,
                                wanted,
                                budget,
                                Some(rule.maximum_unit_price),
                                Some(shared::economy::MarketSeller::Business(*id)),
                            );
                            let viable_units = viable_processing_input_purchase(
                                building.kind,
                                input,
                                held,
                                preview.units,
                            );
                            if viable_units == 0 {
                                continue;
                            }
                            let preview = market.preview_purchase(
                                input,
                                viable_units,
                                budget,
                                Some(rule.maximum_unit_price),
                                Some(shared::economy::MarketSeller::Business(*id)),
                            );
                            if preview.units != viable_units {
                                continue;
                            }
                            let Some(company_entity) = companies_by_id.get(&operated_by.0).copied()
                            else {
                                continue;
                            };
                            let Ok(mut company) = company_accounts.get_mut(company_entity) else {
                                continue;
                            };
                            let protected =
                                company.wage_arrears.saturating_add(company.tax_arrears);
                            if company.cash.saturating_sub(protected) < preview.pennies
                                || !company.debit(preview.pennies)
                            {
                                continue;
                            }
                            account.record_input_purchase(
                                clock.day,
                                preview.pennies,
                                preview.units,
                            );
                            let purchase = market.purchase(
                                input,
                                preview.units,
                                preview.pennies,
                                Some(rule.maximum_unit_price),
                                Some(shared::economy::MarketSeller::Business(*id)),
                            );
                            let moved =
                                hall_store.transfer_to(&mut store, input, purchase.trade.units);
                            debug_assert_eq!(moved, purchase.trade.units);
                            business_events.record_market_purchase(
                                clock.day,
                                building_of.0,
                                purchase.fills,
                            );
                            break;
                        }
                    }
                }
            }
        }

        if let Some(recipe) = processing_recipe(building.kind) {
            let accumulated = progress.seconds.entry(*id).or_default();
            let reclaimed = recipe
                .input_units
                .saturating_mul(recipe.input.bulk_per_unit());
            let output_bulk = recipe
                .output_units
                .saturating_mul(recipe.output.bulk_per_unit());
            if store.amount(recipe.input) < recipe.input_units
                || store.free_bulk().saturating_add(reclaimed) < output_bulk
            {
                *accumulated = 0.0;
            } else {
                *accumulated += productive_seconds * f64::from(worker_count);
                let requested = (*accumulated / f64::from(recipe.work_seconds)).floor() as u32;
                let (cycles, produced) = process_available_cycles(&mut store, recipe, requested);
                if cycles > 0 {
                    *accumulated -= f64::from(cycles) * f64::from(recipe.work_seconds);
                    if cycles < requested {
                        *accumulated = 0.0;
                    }
                    business_events.record_production(clock.day, *id, produced);
                    if let Some(hall) = hall_entity {
                        economy_runtime.record_food_production(
                            hall,
                            cycles.saturating_mul(recipe.net_food_units()),
                        );
                    }
                }
            }
        } else {
            let field_factor = if building.kind == SettlementBuildingKind::Farmstead {
                fields_by_building.get(id).copied().unwrap_or(0).min(2) as f64 / 2.0
            } else {
                1.0
            };
            let seconds_per_unit = match building.kind {
                SettlementBuildingKind::Farmstead => farmer_seconds_per_wheat(building.quality),
                SettlementBuildingKind::FishermansHut => fisher_seconds_per_food(building.quality),
                SettlementBuildingKind::LumberjackHut => lumber_seconds_per_tree(building.quality),
                _ => continue,
            } as f64;
            let accumulated = progress.seconds.entry(*id).or_default();
            *accumulated += productive_seconds * f64::from(worker_count) * field_factor.max(0.0);
            let cycles = (*accumulated / seconds_per_unit).floor() as u32;
            if cycles > 0 {
                *accumulated -= f64::from(cycles) * seconds_per_unit;
                let units = if good == Good::Wood {
                    cycles.saturating_mul(lumber_tree_yield(building.quality))
                } else {
                    cycles
                };
                let produced = store.add(good, units);
                business_events.record_production(clock.day, *id, produced);
                if good.is_edible() {
                    if let Some(hall) = halls_by_id.get(&building_of.0) {
                        economy_runtime.record_food_production(*hall, produced);
                    }
                }
            }
        }

        let Some(hall_entity) = hall_entity else {
            continue;
        };
        let Ok((mut hall_store, mut market)) = halls.get_mut(hall_entity) else {
            continue;
        };
        // A porter finishing a real delivery is transition-critical and has
        // not demoted yet. Do not simultaneously execute its abstract haul.
        if (!strategic_porters.contains(&building_of.0)
            && !strategic_private_porters.contains(&(building_of.0, operated_by.0)))
            || !policy.collection_enabled
        {
            continue;
        }
        let branch_key = (operated_by.0, building_of.0, good);
        let offered = store
            .amount(good)
            .min(
                branch_public_remaining
                    .get(&branch_key)
                    .copied()
                    .unwrap_or_default(),
            )
            .min(policy.max_units_per_collection)
            .min(shared::economy::capacity::PORTER / good.bulk_per_unit())
            .min(hall_store.free_bulk() / good.bulk_per_unit());
        let moved = store.transfer_to(&mut hall_store, good, offered);
        if moved > 0 {
            if let Some(remaining) = branch_public_remaining.get_mut(&branch_key) {
                *remaining = remaining.saturating_sub(moved);
            }
            market.consign(
                shared::economy::MarketSeller::Business(*id),
                good,
                moved,
                policy
                    .asking_unit_price
                    .max(policy.minimum_unit_price)
                    .max(1),
            );
        }
    }
    // Depots can also release branch surplus while off-screen. Their listing
    // uses the lowest current ask from an owned local producer of that good;
    // storage itself does not invent a second pricing strategy.
    for (id, building_of, building, mut store, _, condition, _, _, _, operated_by) in
        businesses.iter_mut()
    {
        if building.kind != SettlementBuildingKind::StorageHall
            || condition.is_some_and(|condition| !condition.state.can_operate())
            || (!strategic_porters.contains(&building_of.0)
                && !strategic_private_porters.contains(&(building_of.0, operated_by.0)))
        {
            continue;
        }
        let Some(hall_entity) = halls_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let Ok((mut hall_store, mut market)) = halls.get_mut(hall_entity) else {
            continue;
        };
        for good in Good::ALL {
            let key = (operated_by.0, building_of.0, good);
            let offered = store
                .amount(good)
                .min(
                    branch_public_remaining
                        .get(&key)
                        .copied()
                        .unwrap_or_default(),
                )
                .min(16)
                .min(hall_store.free_bulk() / good.bulk_per_unit());
            let moved = store.transfer_to(&mut hall_store, good, offered);
            if moved == 0 {
                continue;
            }
            if let Some(remaining) = branch_public_remaining.get_mut(&key) {
                *remaining = remaining.saturating_sub(moved);
            }
            market.consign(
                shared::economy::MarketSeller::Business(*id),
                good,
                moved,
                branch_asks
                    .get(&key)
                    .copied()
                    .unwrap_or_else(|| good.base_price()),
            );
        }
    }
    progress
        .seconds
        .retain(|building, _| live_buildings.contains(building));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::village::FarmerPhase;
    use shared::components::{CharacterName, SettlementTier};
    use shared::economy::BusinessAccount;

    #[test]
    fn strategic_work_overlap_is_warp_step_invariant() {
        let mut clock = WorldTime::new(1_440.0, 240.0, 200.0);
        clock.day = 1;
        assert!((productive_seconds_ending_at(&clock, 1_680.0) - 1_080.0).abs() < 0.01);
        // The 800 seconds ending 200 seconds into the new day contain 600
        // seconds of night and exactly 200 seconds of the new shift.
        assert!((productive_seconds_ending_at(&clock, 800.0) - 200.0).abs() < 0.01);
    }

    #[test]
    fn strategic_company_delivery_matches_tactical_goods_fee_and_ledgers() {
        let mut app = App::new();
        app.init_resource::<StrategicStep>();
        app.add_systems(Update, advance_strategic_company_deliveries);
        app.world_mut().spawn(WorldTime::new_default());

        let settlement_id = SettlementId(90);
        let company = shared::components::CompanyId(91);
        app.world_mut().spawn((
            company,
            shared::economy::CompanyAccount {
                cash: shared::economy::PENNIES_PER_COIN,
                ..default()
            },
        ));
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Strategic Chain".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 3,
                    treasury: 0,
                },
                CivicAccount::default(),
                MootMarket::founding(),
            ))
            .id();
        app.world_mut().spawn((
            CivicEmployment {
                settlement: settlement_id,
                role: CivicRole::MootSteward,
            },
            StrategicPerson,
        ));

        let farm_id = BuildingId(92);
        let mut farm_stock = GoodsInventory::new(100);
        assert_eq!(farm_stock.add(Good::Wheat, 6), 6);
        let farm = app
            .world_mut()
            .spawn((
                farm_id,
                BuildingOf(settlement_id),
                OperatedBy(company),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Strategic Chain".into(),
                    owner: Some("Alda".into()),
                    quality: 1.0,
                    workers: vec!["Alda".into()],
                },
                farm_stock,
                BusinessSalePolicy {
                    company_reserve_units: 2,
                    ..BusinessSalePolicy::for_good(Good::Wheat)
                },
                BusinessProcurementPolicy::none(),
                BusinessSupplyPolicy::none(),
                BusinessManagementPolicy::default(),
                BusinessAccount {
                    estimated_unit_cost: Good::Wheat.base_price(),
                    ..BusinessAccount::default()
                },
            ))
            .id();

        let mut procurement = BusinessProcurementPolicy::none();
        procurement.set_rule(
            Good::Wheat,
            shared::economy::BusinessInputRule {
                enabled: true,
                coverage_days: 2,
                reorder_below: 2,
                target_units: 4,
                maximum_unit_price: 2 * shared::economy::PENNIES_PER_COIN,
            },
        );
        let supply = BusinessSupplyPolicy::none().with_rule(
            Good::Wheat,
            shared::economy::BusinessPrivateInputRule {
                enabled: true,
                sourcing: BusinessSourcingMode::OwnedOnly,
                preferred_supplier: Some(farm_id),
            },
        );
        let mill = app
            .world_mut()
            .spawn((
                BuildingId(93),
                BuildingOf(settlement_id),
                OperatedBy(company),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Windmill,
                    settlement: "Strategic Chain".into(),
                    owner: Some("Alda".into()),
                    quality: 1.0,
                    workers: vec!["Bera".into()],
                },
                GoodsInventory::new(100),
                BusinessSalePolicy::for_good(Good::Flour),
                procurement,
                supply,
                BusinessManagementPolicy::default(),
                BusinessAccount::default(),
            ))
            .id();

        app.world_mut().resource_mut::<StrategicStep>().serial = 1;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 60.0;
        app.update();

        let expected_fee = 4 * u64::from(Good::Wheat.bulk_per_unit());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Wheat),
            2
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(mill)
                .unwrap()
                .amount(Good::Wheat),
            4
        );
        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().treasury,
            expected_fee
        );
        assert_eq!(
            app.world()
                .get::<MootMarket>(hall)
                .unwrap()
                .listed_units(Good::Wheat),
            0
        );
        let supplier = app.world().get::<BusinessAccount>(farm).unwrap();
        let receiver = app.world().get::<BusinessAccount>(mill).unwrap();
        assert_eq!(
            supplier.current_day.internal_revenue,
            receiver.current_day.internal_input_expense
        );
        assert_eq!(receiver.current_day.delivery_fees, expected_fee);
    }

    #[test]
    fn offscreen_people_drop_routes_but_migrants_finish_their_journey() {
        let mut app = App::new();
        app.init_resource::<RegionRegistry>();
        app.init_resource::<shared::terrain::WorldTerrain>();
        app.add_systems(
            Update,
            (
                crate::world::regions::build_region_registry,
                update_person_simulation_lod,
            )
                .chain(),
        );
        let resident = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Resident".into()),
                RegionCoord::new(0, 0),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                super::super::VillagerIntent::Resident {
                    settlement: Entity::PLACEHOLDER,
                },
                MoveTarget(Vec3::X),
            ))
            .id();
        let migrant = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Migrant".into()),
                RegionCoord::new(0, 0),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                super::super::VillagerIntent::Travelling {
                    settlement: Entity::PLACEHOLDER,
                },
                MoveTarget(Vec3::X),
            ))
            .id();
        let mut last_basket = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        assert_eq!(last_basket.add(Good::Wheat, 1), 1);
        let loaded_farmer = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Loaded farmer".into()),
                RegionCoord::new(0, 0),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                super::super::VillagerIntent::Resident {
                    settlement: Entity::PLACEHOLDER,
                },
                last_basket,
                FarmerRoutine {
                    farmstead: Entity::PLACEHOLDER,
                    field: Entity::PLACEHOLDER,
                    hall: Entity::PLACEHOLDER,
                    work_stand: Vec3::X,
                    harvest_seconds: 0.0,
                    failed_workplace_routes: 0,
                    production_day: 0,
                    produced_today: 1,
                    phase: FarmerPhase::ReturningToFarmstead,
                },
                MoveTarget(Vec3::X),
            ))
            .id();

        app.update();

        assert!(app.world().get::<StrategicPerson>(resident).is_some());
        assert!(app.world().get::<StrategicTravel>(resident).is_some());
        assert!(app.world().get::<MoveTarget>(resident).is_none());
        assert!(app.world().get::<StrategicPerson>(migrant).is_none());
        assert!(app.world().get::<MoveTarget>(migrant).is_some());
        assert!(app
            .world()
            .get::<PendingStrategicDemotion>(loaded_farmer)
            .is_some());
        assert!(app.world().get::<StrategicPerson>(loaded_farmer).is_none());
        assert!(app.world().get::<FarmerRoutine>(loaded_farmer).is_some());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(loaded_farmer)
                .unwrap()
                .amount(Good::Wheat),
            1,
            "LOD must preserve both the last load and its embodied return routine"
        );

        assert_eq!(
            app.world_mut()
                .get_mut::<GoodsInventory>(loaded_farmer)
                .unwrap()
                .remove(Good::Wheat, 1),
            1
        );
        app.update();
        assert!(app.world().get::<StrategicPerson>(loaded_farmer).is_some());
        assert!(app.world().get::<FarmerRoutine>(loaded_farmer).is_none());

        app.world_mut()
            .resource_mut::<RegionRegistry>()
            .set_level_for_test(RegionCoord::new(0, 0), SimLevel::Tactical);
        app.update();
        assert!(app.world().get::<StrategicPerson>(resident).is_none());
        assert!(app.world().get::<StrategicTravel>(resident).is_none());
        assert!(app.world().get::<MoveTarget>(resident).is_some());
    }

    #[test]
    fn abstract_travel_preserves_route_distance_and_resumes_from_progress() {
        let goal = Vec3::new(20.0, 0.0, 0.0);
        let tactical = TravelRoute {
            goal,
            waypoints: vec![
                RouteWaypoint {
                    position: Vec3::new(10.0, 0.0, 0.0),
                    on_road: true,
                },
                RouteWaypoint {
                    position: goal,
                    on_road: false,
                },
            ],
            next: 0,
        };
        let mut travel = StrategicTravel::from_tactical(goal, Some(&tactical), 0.0);
        let mut position = Vec3::ZERO;
        let mut rotation = 0.0;
        let elapsed = 2.0;
        assert!(!travel.advance(&mut position, &mut rotation, elapsed));
        assert!(
            (position.x - shared::player::HERO_MOVE_SPEED * ROAD_SPEED_MULTIPLIER * elapsed as f32)
                .abs()
                < 0.001
        );
        let remaining = travel.remaining_route();
        assert_eq!(remaining.goal, goal);
        assert_eq!(remaining.next, 0);
        assert_eq!(remaining.waypoints.len(), 2);

        assert!(travel.advance(&mut position, &mut rotation, 10.0));
        assert_eq!(position, goal);
    }

    #[test]
    fn two_fields_give_a_strategic_farm_full_output_and_one_field_gives_half() {
        let mut app = App::new();
        app.init_resource::<StrategicStep>();
        app.init_resource::<StrategicProductionProgress>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.init_resource::<BusinessEventQueue>();
        app.add_systems(Update, advance_strategic_villages);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();

        let settlement_id = SettlementId(1);
        app.world_mut().spawn((
            settlement_id,
            Settlement {
                name: "Test".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            MootAdministration {
                market_porter: Some("Porter".into()),
                ..default()
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ));
        let building_id = BuildingId(7);
        let company = shared::components::CompanyId(8);
        app.world_mut()
            .spawn((company, shared::economy::CompanyAccount::default()));
        let farm_position = Vec3::new(10.0, 0.0, 10.0);
        let farm = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(settlement_id),
                OperatedBy(company),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Test".into(),
                    owner: Some("Farmer".into()),
                    quality: 1.0,
                    workers: vec!["Farmer".into()],
                },
                PlayerPosition(farm_position),
                GoodsInventory::new(100),
                BusinessSalePolicy {
                    collection_enabled: false,
                    ..default()
                },
                BusinessAccount::default(),
            ))
            .id();
        app.world_mut().spawn((
            FarmField {
                settlement: "Test".into(),
                farmstead: farm_position,
                plot_index: 0,
                quality: 1.0,
            },
            AttachedTo(building_id),
        ));
        let second_field = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Test".into(),
                    farmstead: farm_position,
                    plot_index: 1,
                    quality: 1.0,
                },
                AttachedTo(building_id),
            ))
            .id();
        app.world_mut()
            .spawn((EmployedAt(building_id), StrategicPerson));

        app.world_mut().resource_mut::<StrategicStep>().serial = 1;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 340.0;
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .advance(340.0, 0.0);
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Wheat),
            2
        );

        app.world_mut().despawn(second_field);
        app.world_mut().resource_mut::<StrategicStep>().serial = 2;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 340.0;
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .advance(340.0, 0.0);
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Wheat),
            3
        );
    }

    #[test]
    fn offscreen_porter_and_processors_complete_the_wheat_to_bread_chain() {
        use shared::economy::{
            BusinessInputRule, BusinessWagePolicy, MarketSeller, PENNIES_PER_COIN,
        };

        let mut app = App::new();
        app.init_resource::<StrategicStep>();
        app.init_resource::<StrategicProductionProgress>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.init_resource::<BusinessEventQueue>();
        app.add_systems(Update, advance_strategic_villages);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();

        let settlement_id = SettlementId(20);
        let wheat_seller = BuildingId(200);
        let mut hall_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(hall_stock.add(Good::Wheat, 4), 4);
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(wheat_seller),
            Good::Wheat,
            4,
            Good::Wheat.base_price(),
        );
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Strategic Bread".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 3,
                    treasury: 0,
                },
                GoodsInventory::new(shared::economy::capacity::HALL),
                market,
            ))
            .id();
        *app.world_mut().get_mut::<GoodsInventory>(hall).unwrap() = hall_stock;

        let mill_id = BuildingId(201);
        let bakery_id = BuildingId(202);
        let mill_company = shared::components::CompanyId(203);
        let bakery_company = shared::components::CompanyId(204);
        for company in [mill_company, bakery_company] {
            app.world_mut().spawn((
                company,
                shared::economy::CompanyAccount {
                    cash: 100 * PENNIES_PER_COIN,
                    ..default()
                },
            ));
        }
        let processor = |kind, id, company, input, target| {
            (
                id,
                BuildingOf(settlement_id),
                OperatedBy(company),
                SettlementBuilding {
                    kind,
                    settlement: "Strategic Bread".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![format!("{kind:?} worker")],
                },
                GoodsInventory::new(100),
                BusinessSalePolicy {
                    company_reserve_units: 0,
                    ..BusinessSalePolicy::for_good(business_output(kind).expect("processor output"))
                },
                BusinessProcurementPolicy::none().with_rule(
                    input,
                    BusinessInputRule {
                        enabled: true,
                        coverage_days: 2,
                        reorder_below: 2,
                        target_units: target,
                        maximum_unit_price: 10 * PENNIES_PER_COIN,
                    },
                ),
                BusinessAccount::default(),
                BusinessWagePolicy::default(),
            )
        };
        app.world_mut().spawn(processor(
            SettlementBuildingKind::Windmill,
            mill_id,
            mill_company,
            Good::Wheat,
            4,
        ));
        app.world_mut().spawn(processor(
            SettlementBuildingKind::Bakery,
            bakery_id,
            bakery_company,
            Good::Flour,
            4,
        ));
        app.world_mut()
            .spawn((EmployedAt(mill_id), StrategicPerson));
        app.world_mut()
            .spawn((EmployedAt(bakery_id), StrategicPerson));
        app.world_mut().spawn((
            CivicEmployment {
                settlement: settlement_id,
                role: CivicRole::MootSteward,
            },
            StrategicPerson,
        ));

        for serial in 1..=3 {
            app.world_mut().resource_mut::<StrategicStep>().serial = serial;
            app.world_mut()
                .resource_mut::<StrategicStep>()
                .elapsed_world_seconds = 400.0;
            app.world_mut()
                .get_mut::<WorldTime>(clock)
                .unwrap()
                .advance(400.0, 0.0);
            app.update();
        }

        let stock = app.world().get::<GoodsInventory>(hall).unwrap();
        assert!(
            stock.amount(Good::Bread) >= 3,
            "the off-screen chain must create and consign physical Bread"
        );
        assert_eq!(Good::Wheat.food_tier(), 0);
        assert_eq!(Good::Bread.food_tier(), 2);
        assert_eq!(
            app.world()
                .get::<MootMarket>(hall)
                .unwrap()
                .seller_listed_units(MarketSeller::Business(bakery_id), Good::Bread),
            stock.amount(Good::Bread),
        );
    }

    #[test]
    fn abstract_collection_waits_until_the_market_porter_is_strategic() {
        let mut app = App::new();
        app.init_resource::<StrategicStep>();
        app.init_resource::<StrategicProductionProgress>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.init_resource::<BusinessEventQueue>();
        app.add_systems(Update, advance_strategic_villages);
        app.world_mut().spawn(WorldTime::new_default());

        let settlement_id = SettlementId(3);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Porter Test".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 0,
                },
                MootAdministration {
                    market_porter: Some("Porter".into()),
                    ..default()
                },
                GoodsInventory::new(shared::economy::capacity::HALL),
                MootMarket::founding(),
            ))
            .id();
        let building_id = BuildingId(11);
        let company = shared::components::CompanyId(12);
        app.world_mut()
            .spawn((company, shared::economy::CompanyAccount::default()));
        let mut stock = GoodsInventory::new(100);
        stock.add(Good::Wood, 10);
        let business = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(settlement_id),
                OperatedBy(company),
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Porter Test".into(),
                    owner: Some("Worker".into()),
                    quality: 1.0,
                    workers: vec!["Worker".into()],
                },
                PlayerPosition(Vec3::ZERO),
                stock,
                BusinessSalePolicy::default(),
                BusinessAccount::default(),
            ))
            .id();
        app.world_mut()
            .spawn((EmployedAt(building_id), StrategicPerson));
        let porter = app
            .world_mut()
            .spawn(CivicEmployment {
                settlement: settlement_id,
                role: CivicRole::MootSteward,
            })
            .id();

        app.world_mut().resource_mut::<StrategicStep>().serial = 1;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 1.0;
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood),
            0
        );

        app.world_mut().entity_mut(porter).insert(StrategicPerson);
        app.world_mut().resource_mut::<StrategicStep>().serial = 2;
        app.update();
        assert!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood)
                > 0
        );
        assert_eq!(
            app.world()
                .get::<MootMarket>(hall)
                .unwrap()
                .seller_listed_units(
                    shared::economy::MarketSeller::Business(building_id),
                    Good::Wood,
                ),
            10,
            "the strategic porter must consign the saleable stock for its owner"
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(business)
                .unwrap()
                .unposted_company_capital,
            0,
            "delivery is not a sale; only a real buyer creates revenue"
        );
    }
}
