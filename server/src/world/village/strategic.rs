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
    CharacterMotion, CivicEmployment, CivicRole, EmployedAt, FarmField, PlayerPosition,
    PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    WorldTime,
};
use shared::economy::{
    BusinessAccount, BusinessProcurementPolicy, BusinessSalePolicy, BusinessWagePolicy, Good,
    GoodsInventory, MootMarket,
};
use shared::region::{RegionCoord, SimLevel};

use crate::player::hero::MoveTarget;
use crate::world::regions::{RegionRegistry, StrategicStep};
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, RouteWaypoint, TravelRoute,
    ROAD_SPEED_MULTIPLIER,
};

use super::{
    ambient, business_output, farmer_seconds_per_wheat, fisher_seconds_per_food,
    lumber_seconds_per_tree, lumber_tree_yield, process_available_cycles, processing_recipe,
    viable_processing_input_purchase, BusinessEventQueue, ConstructionMaterialRoutine,
    FarmerHarvestProgress, FarmerRoutine, FishingRoutine, FishingWorkProgress, HomeRoutine,
    HouseholdShoppingRoutine, LumberjackRoutine, LumberjackWorkProgress, MarketCollectionRoutine,
    MootMealRoutine, MootQueueTicket, PierTraversal, ProcessingRoutine, ProcessorWorkProgress,
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

/// A transaction or public work already in motion is allowed to reach a safe
/// boundary before the person's expensive tactical state is stripped.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PendingStrategicDemotion;

#[derive(Resource, Default)]
pub struct StrategicProductionProgress {
    seconds: HashMap<BuildingId, f64>,
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
            Has<ConstructionMaterialRoutine>,
            Has<RoadBuilderRoutine>,
            Has<MarketCollectionRoutine>,
            Has<HouseholdShoppingRoutine>,
            Has<MootQueueTicket>,
            Has<MootMealRoutine>,
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
            Has<ConstructionMaterialRoutine>,
            Has<RoadBuilderRoutine>,
            Has<MarketCollectionRoutine>,
            Has<HouseholdShoppingRoutine>,
            Has<MootQueueTicket>,
            Has<MootMealRoutine>,
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
            Has<ConstructionMaterialRoutine>,
            Has<RoadBuilderRoutine>,
            Has<MarketCollectionRoutine>,
            Has<HouseholdShoppingRoutine>,
            Has<MootQueueTicket>,
            Has<MootMealRoutine>,
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
            construction,
            road,
            market,
            shopping,
            queue,
            meal,
            intent,
        ) in people.iter()
        {
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
                    || shopping
                    || queue
                    || meal
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
            construction,
            road,
            market,
            shopping,
            queue,
            meal,
            intent,
        ) in changed_people.iter()
        {
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
                    || shopping
                    || queue
                    || meal
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
        construction,
        road,
        market,
        shopping,
        queue,
        meal,
        intent,
    ) in pending.iter()
    {
        let critical = construction
            || road
            || market
            || shopping
            || queue
            || meal
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
    workers: Query<(&EmployedAt, Option<&StrategicPerson>)>,
    civic_workers: Query<(&CivicEmployment, Option<&StrategicPerson>)>,
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
    let mut fields_by_building: HashMap<BuildingId, u32> = HashMap::new();
    for (_, attached) in fields.iter() {
        *fields_by_building.entry(attached.0).or_default() += 1;
    }
    let mut live_buildings = HashSet::new();

    for (id, building_of, building, mut store, policy, condition, procurement, mut account, wage) in
        businesses.iter_mut()
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
                    let budget = account
                        .cash
                        .saturating_sub(account.wage_arrears)
                        .saturating_sub(account.tax_arrears)
                        .saturating_sub(payroll_reserve);
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
                            if preview.units != viable_units
                                || !account.buy_inputs(clock.day, preview.pennies, preview.units)
                            {
                                continue;
                            }
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
        if !strategic_porters.contains(&building_of.0) || !policy.collection_enabled {
            continue;
        }
        let offered = store
            .amount(good)
            .saturating_sub(policy.keep_units)
            .min(policy.max_units_per_collection)
            .min(hall_store.free_bulk() / good.bulk_per_unit());
        let moved = store.transfer_to(&mut hall_store, good, offered);
        if moved > 0 {
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
    progress
        .seconds
        .retain(|building, _| live_buildings.contains(building));
}

#[cfg(test)]
mod tests {
    use super::*;
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

        app.update();

        assert!(app.world().get::<StrategicPerson>(resident).is_some());
        assert!(app.world().get::<StrategicTravel>(resident).is_some());
        assert!(app.world().get::<MoveTarget>(resident).is_none());
        assert!(app.world().get::<StrategicPerson>(migrant).is_none());
        assert!(app.world().get::<MoveTarget>(migrant).is_some());

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
        let farm_position = Vec3::new(10.0, 0.0, 10.0);
        let farm = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(settlement_id),
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
        let processor = |kind, id, input, target| {
            (
                id,
                BuildingOf(settlement_id),
                SettlementBuilding {
                    kind,
                    settlement: "Strategic Bread".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![format!("{kind:?} worker")],
                },
                GoodsInventory::new(100),
                BusinessSalePolicy {
                    keep_units: 0,
                    ..BusinessSalePolicy::for_good(business_output(kind).expect("processor output"))
                },
                BusinessProcurementPolicy::none().with_rule(
                    input,
                    BusinessInputRule {
                        enabled: true,
                        reorder_below: 2,
                        target_units: target,
                        maximum_unit_price: 10 * PENNIES_PER_COIN,
                    },
                ),
                BusinessAccount::with_capital(100 * PENNIES_PER_COIN),
                BusinessWagePolicy::default(),
            )
        };
        app.world_mut().spawn(processor(
            SettlementBuildingKind::Windmill,
            mill_id,
            Good::Wheat,
            4,
        ));
        app.world_mut().spawn(processor(
            SettlementBuildingKind::Bakery,
            bakery_id,
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
        let mut stock = GoodsInventory::new(100);
        stock.add(Good::Wood, 10);
        let business = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(settlement_id),
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
            8,
            "the strategic porter must consign the saleable stock for its owner"
        );
        assert_eq!(
            app.world().get::<BusinessAccount>(business).unwrap().cash,
            0,
            "delivery is not a sale; only a real buyer creates revenue"
        );
    }
}
