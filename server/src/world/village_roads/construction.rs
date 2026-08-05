//! Door-to-network connectivity, connector requests and embodied road building.

use super::*;

#[derive(Default)]
pub(super) struct HallRoadNetwork {
    pub(super) connected_keys: HashSet<(i32, i32)>,
    connected_points: Vec<Vec2>,
    pub(super) disconnected_components: usize,
}

/// Build the completed road component that can actually reach the Moot Hall.
///
/// Planned and half-built roads are deliberately excluded. Paths are raised
/// from a building outward, so their visible prefix is not public network yet;
/// letting another house connect to it is how detached road islands formed.
/// Build connectivity for a caller-provided stable SettlementId partition.
/// The replicated road name is deliberately ignored here.
pub(super) fn hall_road_network(hall_door: Vec2, roads: &[&VillageRoad]) -> HallRoadNetwork {
    let mut points = HashMap::<(i32, i32), Vec2>::new();
    let mut edges = HashMap::<(i32, i32), HashSet<(i32, i32)>>::new();
    for road in roads.iter().copied().filter(|road| road.is_complete()) {
        let mut previous = None;
        for point in road.built_points().iter().copied() {
            let key = graph_key(point);
            points.entry(key).or_insert(point);
            edges.entry(key).or_default();
            if let Some(previous) = previous {
                edges.entry(previous).or_default().insert(key);
                edges.entry(key).or_default().insert(previous);
            }
            previous = Some(key);
        }
    }

    let mut queue = VecDeque::new();
    let mut connected_keys = HashSet::new();
    for (key, point) in &points {
        if point.distance_squared(hall_door) <= 0.5_f32.powi(2) {
            connected_keys.insert(*key);
            queue.push_back(*key);
        }
    }
    while let Some(node) = queue.pop_front() {
        for next in edges.get(&node).into_iter().flatten() {
            if connected_keys.insert(*next) {
                queue.push_back(*next);
            }
        }
    }

    let mut connected_points: Vec<_> = connected_keys
        .iter()
        .filter_map(|key| points.get(key).copied())
        .collect();
    connected_points.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y)));

    let mut unseen: HashSet<_> = points
        .keys()
        .copied()
        .filter(|key| !connected_keys.contains(key))
        .collect();
    let mut disconnected_components = 0usize;
    while let Some(start) = unseen.iter().next().copied() {
        disconnected_components += 1;
        unseen.remove(&start);
        queue.push_back(start);
        while let Some(node) = queue.pop_front() {
            for next in edges.get(&node).into_iter().flatten() {
                if unseen.remove(next) {
                    queue.push_back(*next);
                }
            }
        }
    }

    HallRoadNetwork {
        connected_keys,
        connected_points,
        disconnected_components,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingRoadStatus {
    Connected,
    Pending,
    Disconnected,
    Roadless,
}

fn road_starts_at(road: &VillageRoad, door: Vec2) -> bool {
    road.points
        .first()
        .is_some_and(|point| point.distance_squared(door) <= 0.75_f32.powi(2))
}

pub(super) fn absolute_world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

pub(super) fn building_road_status(
    door: Vec2,
    roads: &[&VillageRoad],
    network: &HallRoadNetwork,
    has_request: bool,
) -> BuildingRoadStatus {
    let touching: Vec<_> = roads
        .iter()
        .copied()
        .filter(|road| road_starts_at(road, door))
        .collect();
    if touching.iter().any(|road| {
        road.is_complete()
            && road
                .built_points()
                .iter()
                .any(|point| network.connected_keys.contains(&graph_key(*point)))
    }) {
        return BuildingRoadStatus::Connected;
    }
    if has_request || touching.iter().any(|road| !road.is_complete()) {
        return BuildingRoadStatus::Pending;
    }
    if touching.iter().any(|road| road.is_complete()) {
        BuildingRoadStatus::Disconnected
    } else {
        BuildingRoadStatus::Roadless
    }
}

#[allow(clippy::too_many_arguments)]
pub fn plan_requested_roads(
    mut commands: Commands,
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    requests: Query<(
        Entity,
        &RoadRequest,
        &SettlementBuilding,
        &shared::components::BuildingId,
        &PlayerPosition,
        &PlayerRotation,
        Option<&RoadSurveyBackoff>,
    )>,
    placed_buildings: Query<(
        &shared::building::PlacedBuilding,
        &shared::building::BuildingPosition,
    )>,
    fields: Query<(
        &FarmField,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::AttachedTo,
    )>,
    building_scopes: Query<(
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    settlements: Query<(
        &Settlement,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    mut builders: Query<
        (
            &CharacterName,
            &mut VillagerIntent,
            &mut CharacterActivity,
            Option<&RoadSteward>,
        ),
        With<CharacterKind>,
    >,
    mut prop_cache: Local<RoutePropChunkCache>,
    mut survey_scratch: Local<SurveyScratch>,
) {
    let Some(terrain) = terrain else { return };
    let now = simulation_time.elapsed_real_seconds_f64();
    let building_settlements: HashMap<_, _> = building_scopes
        .iter()
        .map(|(building_id, building_of)| (*building_id, building_of.0))
        .collect();
    for (building_entity, request, building, building_id, position, rotation, backoff) in
        requests.iter()
    {
        if backoff.is_some_and(|backoff| now < backoff.retry_after) {
            continue;
        }
        // A Farmstead and its two separate authored crop plots are published in
        // consecutive systems. Never survey in the one-frame seam between
        // them: a road planned without both field blockers remains physically
        // wrong after the missing field appears. Retaining the request lets the
        // same builder adopt it once the complete field layout is authoritative.
        if building.kind == SettlementBuildingKind::Farmstead
            && fields
                .iter()
                .filter(|(_, _, _, attached_to)| attached_to.0 == *building_id)
                .count()
                < shared::components::FARM_FIELDS_PER_FARMSTEAD as usize
        {
            continue;
        }
        let Ok((builder_name, mut intent, mut activity, steward)) =
            builders.get_mut(request.builder)
        else {
            commands
                .entity(building_entity)
                .remove::<RoadRequest>()
                .remove::<RoadSurveyBackoff>();
            continue;
        };
        let may_claim_builder = matches!(
            *intent,
            VillagerIntent::Resident { settlement }
                if settlement == request.settlement
        ) || matches!(
            *intent,
            VillagerIntent::Building { settlement, site }
                if settlement == request.settlement && site == request.completed_site
        );
        if !may_claim_builder {
            // A failed survey intentionally retains its request for retry.
            // During that wait the resident may receive another permit; the
            // old request cannot overwrite that newer Building intent.
            continue;
        }
        let Ok((settlement, settlement_id, hall_position, hall_rotation)) =
            settlements.get(request.settlement)
        else {
            commands
                .entity(building_entity)
                .remove::<RoadRequest>()
                .remove::<RoadSurveyBackoff>();
            continue;
        };
        let (start, survey_start) = doorway_approach(building.kind, position.0, rotation.0);
        if !road_segment_is_dry(&terrain, start, survey_start) {
            let next_backoff = RoadSurveyBackoff::after_failure(backoff.copied(), now);
            if next_backoff.should_warn() {
                warn!(
                    "Village '{}': the {} doorway has no dry road apron; retry {} in {:.1}s",
                    settlement.name,
                    building.kind.label(),
                    next_backoff.failures,
                    next_backoff.retry_after - now,
                );
            }
            commands.entity(building_entity).insert(next_backoff);
            *intent = VillagerIntent::Resident {
                settlement: request.settlement,
            };
            *activity = CharacterActivity::Idle;
            continue;
        }

        let (hall_door, hall_approach) = doorway_approach(
            SettlementBuildingKind::Hall,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        let settlement_roads: Vec<_> = roads
            .iter()
            .filter_map(|(road, road_of)| (road_of.0 == *settlement_id).then_some(road))
            .collect();
        let network = hall_road_network(hall_door, &settlement_roads);
        let mut existing_goals = network.connected_points;
        existing_goals.sort_by(|a, b| {
            a.distance_squared(start)
                .total_cmp(&b.distance_squared(start))
        });
        existing_goals.dedup_by(|a, b| a.distance_squared(*b) <= 0.01);
        // A nearest point can sit in the wide door apron of the hall or
        // another building. Try several existing network points before the
        // hall fallback; abandoning the whole request after one bad candidate
        // is how completed houses silently ended up roadless.
        let mut candidates: Vec<_> = existing_goals
            .into_iter()
            .take(12)
            .map(|goal| (goal, goal, None))
            .collect();
        candidates.push((hall_door, hall_approach, Some(hall_door)));

        let survey_failure = backoff.map_or(0, |backoff| backoff.failures);
        let seed = stable_road_seed(&settlement.name, start)
            ^ u32::from(request.attempt).wrapping_mul(0x9E37_79B9)
            ^ u32::from(survey_failure).wrapping_mul(0x85EB_CA6B);
        let main_count = settlement_roads
            .iter()
            .filter(|road| road.class == RoadClass::Main)
            .count();
        let selected = candidates
            .into_iter()
            .find_map(|(goal, survey_goal, hall_door_goal)| {
                let class = if hall_door_goal.is_some() && main_count < 2 {
                    RoadClass::Main
                } else {
                    RoadClass::Lane
                };
                let width = surface_width_for_tier(settlement.tier, class);
                let reserved_width = class.initial_reserved_width();
                if hall_door_goal
                    .is_some_and(|door| !road_segment_is_dry(&terrain, survey_goal, door))
                {
                    return None;
                }
                let route_min = survey_start.min(survey_goal) - Vec2::splat(SURVEY_PADDING);
                let route_max = survey_start.max(survey_goal) + Vec2::splat(SURVEY_PADDING);
                let mut building_blockers: Vec<_> = placed_buildings
                    .iter()
                    .filter(|(_, other_position)| {
                        let point = Vec2::new(other_position.0.x, other_position.0.z);
                        point.cmpge(route_min).all() && point.cmple(route_max).all()
                    })
                    .map(|(other, other_position)| {
                        let footprint = other.building_type.definition().footprint;
                        let other_center = Vec2::new(other_position.0.x, other_position.0.z);
                        // The road is intentionally allowed to enter the front
                        // apron of its source building and the Moot Hall. Every
                        // unrelated structure receives the full future corridor.
                        let is_endpoint = other_center
                            .distance_squared(Vec2::new(position.0.x, position.0.z))
                            <= 0.01
                            || other_center
                                .distance_squared(Vec2::new(hall_position.0.x, hall_position.0.z))
                                <= 0.01;
                        let protected_width = if is_endpoint { width } else { reserved_width };
                        BuildingBlocker {
                            center: other_center,
                            half: footprint * 0.5 + Vec2::splat(protected_width * 0.5 + 0.45),
                            rotation: other.rotation,
                        }
                    })
                    .collect();
                building_blockers.extend(
                    fields
                        .iter()
                        .filter(|(_, _, _, attached_to)| {
                            building_settlements.get(&attached_to.0) == Some(settlement_id)
                        })
                        .map(|(_, field_position, field_rotation, _)| BuildingBlocker {
                            center: Vec2::new(field_position.0.x, field_position.0.z),
                            half: SettlementBuildingKind::Farmstead
                                .field_half_extents()
                                .expect("Farmstead has an authored wheat-field footprint")
                                + Vec2::splat(
                                    reserved_width * 0.5
                                        + shared::components::FARM_FIELD_EDGE_CLEARANCE
                                        + ROAD_SURVEY_FIELD_EPSILON,
                                ),
                            rotation: field_rotation.0,
                        }),
                );
                if building_blockers
                    .iter()
                    .any(|blocker| blocker.contains(survey_goal))
                {
                    return None;
                }
                let prop_blockers = blockers_for_route(
                    &terrain,
                    survey_start,
                    survey_goal,
                    &building_blockers,
                    reserved_width * 0.5,
                    colliders.as_deref(),
                    derived.as_deref(),
                    &mut prop_cache,
                );
                let surveyed = survey_village_road(
                    &terrain,
                    survey_start,
                    survey_goal,
                    &building_blockers,
                    &prop_blockers,
                    seed,
                    &mut survey_scratch,
                );
                if surveyed.len() < 2 {
                    return None;
                }
                // Sampling and simplification are deliberately fast, but the
                // shared continuous ribbon test is the final authority. Run
                // it before accepting a candidate so a tangent grid route can
                // never become a permanent crop overlap.
                let mut certified_points = Vec::with_capacity(surveyed.len() + 2);
                certified_points.push(start);
                certified_points.extend(surveyed.iter().copied());
                if let Some(hall_door) = hall_door_goal {
                    certified_points.push(hall_door);
                }
                let certified = VillageRoad {
                    settlement: settlement.name.clone(),
                    builder: builder_name.0.clone(),
                    built_through: certified_points.len() as u16,
                    points: certified_points,
                    width,
                    reserved_width,
                    surface: RoadSurface::Dirt,
                    class,
                    stone_committed: 0,
                };
                let crosses_field = fields
                    .iter()
                    .filter(|(_, _, _, attached_to)| {
                        building_settlements.get(&attached_to.0) == Some(settlement_id)
                    })
                    .any(|(_, field_position, field_rotation, _)| {
                        certified.intersects_rotated_rect(
                            Vec2::new(field_position.0.x, field_position.0.z),
                            SettlementBuildingKind::Farmstead
                                .field_half_extents()
                                .expect("Farmstead has an authored wheat-field footprint"),
                            field_rotation.0,
                            shared::components::FARM_FIELD_EDGE_CLEARANCE,
                        )
                    });
                let dry = road_corridor_is_dry(
                    &terrain,
                    &certified.points,
                    certified.reservation_width(),
                );
                (!crosses_field && dry).then_some((
                    goal,
                    surveyed,
                    hall_door_goal,
                    class,
                    width,
                    reserved_width,
                ))
            });

        let Some((_goal, surveyed, hall_door_goal, class, width, reserved_width)) = selected else {
            let next_backoff = RoadSurveyBackoff::after_failure(backoff.copied(), now);
            if next_backoff.should_warn() {
                warn!(
                    "Village '{}': {} could not survey a clear path from the {}; retry {} in {:.1}s",
                    settlement.name,
                    builder_name.0,
                    building.kind.label(),
                    next_backoff.failures,
                    next_backoff.retry_after - now,
                );
            }
            if next_backoff.failures >= MAX_ROAD_SURVEY_ATTEMPTS && steward.is_none() {
                warn!(
                    "Village '{}': {} released the blocked {} connector after {} surveys for Road Steward repair",
                    settlement.name,
                    builder_name.0,
                    building.kind.label(),
                    next_backoff.failures,
                );
                commands
                    .entity(building_entity)
                    .remove::<RoadRequest>()
                    .remove::<RoadSurveyBackoff>();
            } else {
                commands.entity(building_entity).insert(next_backoff);
            }
            *intent = VillagerIntent::Resident {
                settlement: request.settlement,
            };
            *activity = CharacterActivity::Idle;
            continue;
        };

        // The only portion allowed through an inflated building blocker is
        // this authored, straight front apron. The expensive survey starts
        // outside it, so the remaining path cannot choose a rear shortcut.
        let mut points = Vec::with_capacity(surveyed.len() + 2);
        points.push(start);
        points.extend(surveyed);
        if let Some(hall_door) = hall_door_goal {
            points.push(hall_door);
        }

        let road = commands
            .spawn((
                VillageRoad {
                    settlement: settlement.name.clone(),
                    builder: builder_name.0.clone(),
                    points,
                    built_through: 1,
                    width,
                    reserved_width,
                    surface: RoadSurface::Dirt,
                    class,
                    stone_committed: 0,
                },
                shared::components::RoadOf(*settlement_id),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        *intent = VillagerIntent::RoadBuilding {
            settlement: request.settlement,
            road,
        };
        *activity = CharacterActivity::Idle;
        // The connector is a new navigation owner. A failed household,
        // ambient, or construction destination left on the actor makes
        // movement deliberately sleep, so adopting the road must clear every
        // route artifact before its first point is published next tick.
        commands
            .entity(request.builder)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .insert(RoadBuilderRoutine {
                road,
                settlement: request.settlement,
                attempt: request.attempt,
                phase: RoadBuildPhase::GoingTo { point: 0 },
            });
        commands
            .entity(building_entity)
            .remove::<RoadRequest>()
            .remove::<RoadSurveyBackoff>();
        info!(
            "Village '{}': {} began the path from the {}",
            settlement.name,
            builder_name.0,
            building.kind.label()
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_village_roads(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    mut commands: Commands,
    mut roads: Query<(&mut VillageRoad, &shared::components::RoadOf)>,
    buildings: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    mut builders: Query<
        (
            Entity,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut VillagerIntent,
            Option<&MoveTarget>,
            &mut RoadBuilderRoutine,
            Option<&HomeRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else { return };
    let dt = simulation_time.world_seconds();
    for (
        builder,
        position,
        mut facing,
        mut activity,
        mut intent,
        move_target,
        mut routine,
        home,
        route_failed,
    ) in builders.iter_mut()
    {
        if home.is_some() {
            continue;
        }
        let Ok((mut road, road_of)) = roads.get_mut(routine.road) else {
            *activity = CharacterActivity::Idle;
            *intent = VillagerIntent::Resident {
                settlement: routine.settlement,
            };
            commands
                .entity(builder)
                .remove::<RoadBuilderRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };
        if road.points.len() < 2 {
            warn!(
                "Village '{}': discarded a malformed road with fewer than two points",
                road.settlement
            );
            let road_entity = routine.road;
            let settlement = routine.settlement;
            commands.entity(road_entity).despawn();
            finish_road_builder(
                &mut commands,
                builder,
                &mut intent,
                &mut activity,
                settlement,
            );
            continue;
        }

        if let (RoadBuildPhase::GoingTo { point }, Some(failed)) = (routine.phase, route_failed) {
            let point = point.min(road.points.len() - 1);
            let xz = road.points[point];
            let target = Vec3::new(xz.x, terrain.get_height(xz.x, xz.y), xz.y);
            if failed.goal.distance_squared(target) <= 0.01 {
                commands
                    .entity(builder)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();

                if point < usize::from(road.built_through) {
                    // The first point is the already-built doorway anchor. It
                    // need not be revisited merely to advance to point one;
                    // doing so stranded Aud's connector when the exact door
                    // target was rejected despite the surveyed apron beyond it
                    // being reachable.
                    let next = point + 1;
                    if next >= road.points.len() {
                        finish_road_builder(
                            &mut commands,
                            builder,
                            &mut intent,
                            &mut activity,
                            routine.settlement,
                        );
                    } else {
                        routine.phase = RoadBuildPhase::GoingTo { point: next };
                        let xz = road.points[next];
                        commands.entity(builder).insert(MoveTarget(Vec3::new(
                            xz.x,
                            terrain.get_height(xz.x, xz.y),
                            xz.y,
                        )));
                    }
                    continue;
                }

                let start = road.points[0];
                let road_entity = routine.road;
                let next_attempt = routine.attempt.saturating_add(1);
                let building = buildings
                    .iter()
                    .find(|(_, building, position, rotation, building_of)| {
                        if building_of.0 != road_of.0 {
                            return false;
                        }
                        let door = building.kind.entrance_position(position.0, rotation.0);
                        start.distance_squared(Vec2::new(door.x, door.z)) <= 0.75_f32.powi(2)
                    })
                    .map(|(entity, ..)| entity);
                warn!(
                    "Village '{}': {} could not reach road point {} on survey attempt {}; resurveying",
                    road.settlement,
                    road.builder,
                    point,
                    routine.attempt.saturating_add(1)
                );
                commands.entity(road_entity).despawn();
                *intent = VillagerIntent::Resident {
                    settlement: routine.settlement,
                };
                *activity = CharacterActivity::Idle;
                commands.entity(builder).remove::<RoadBuilderRoutine>();
                if next_attempt < MAX_ROAD_SURVEY_ATTEMPTS {
                    if let Some(building) = building {
                        commands.entity(building).insert(RoadRequest {
                            builder,
                            settlement: routine.settlement,
                            completed_site: building,
                            attempt: next_attempt,
                        });
                    }
                } else {
                    warn!(
                        "Road connector exhausted {} surveys; releasing it to the Road Steward audit",
                        MAX_ROAD_SURVEY_ATTEMPTS
                    );
                }
                continue;
            }
        }

        if route_failed.is_some() {
            // This failure belongs to an older household, ambient, or road
            // point destination. Movement intentionally pauses while any
            // NavigationRouteFailed exists, so ignoring a mismatched one
            // strands an otherwise healthy connector forever.
            commands
                .entity(builder)
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            if let RoadBuildPhase::GoingTo { point } = routine.phase {
                let xz = road.points[point.min(road.points.len() - 1)];
                commands.entity(builder).insert(MoveTarget(Vec3::new(
                    xz.x,
                    terrain.get_height(xz.x, xz.y),
                    xz.y,
                )));
            }
            continue;
        }

        match routine.phase {
            RoadBuildPhase::GoingTo { point } => {
                *activity = CharacterActivity::Idle;
                let point = point.min(road.points.len() - 1);
                let xz = road.points[point];
                let target = Vec3::new(xz.x, terrain.get_height(xz.x, xz.y), xz.y);
                if ground_distance(position.0, target) > ROAD_REACH {
                    ensure_move_target(&mut commands, builder, move_target, target);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                if point < usize::from(road.built_through) {
                    let next = point + 1;
                    if next >= road.points.len() {
                        finish_road_builder(
                            &mut commands,
                            builder,
                            &mut intent,
                            &mut activity,
                            routine.settlement,
                        );
                    } else {
                        routine.phase = RoadBuildPhase::GoingTo { point: next };
                    }
                } else {
                    let direction = if point > 0 {
                        // Face the section being packed, not the untouched
                        // ground ahead. The authored build clip works in front
                        // of the character, so this makes the action meet the
                        // newly appearing ribbon instead of pointing away.
                        road.points[point - 1] - road.points[point]
                    } else {
                        road.points[0] - road.points[1]
                    };
                    if direction.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-direction.x, -direction.y);
                    }
                    *activity = CharacterActivity::Building;
                    // Arrival and the first work slice happen in the same
                    // fixed tick. Deferring the subtraction until next tick
                    // discarded almost the entire time slice at 100x-1000x,
                    // making roads perversely slower in simulated time as warp
                    // increased. Travel to the next point remains physical.
                    let left = ROAD_BUILD_SECONDS - dt;
                    if left > 0.0 {
                        routine.phase = RoadBuildPhase::Working {
                            point,
                            seconds_left: left,
                        };
                    } else {
                        road.built_through = road
                            .built_through
                            .max((point + 1).min(road.points.len()) as u16);
                        let next = point + 1;
                        if next >= road.points.len() {
                            info!(
                                "Village '{}': {} completed {:.0} m of path",
                                road.settlement,
                                road.builder,
                                road.total_length()
                            );
                            finish_road_builder(
                                &mut commands,
                                builder,
                                &mut intent,
                                &mut activity,
                                routine.settlement,
                            );
                        } else {
                            // Keep the packing pose observable for this server
                            // frame even when warp consumed the whole timer.
                            // The next GoingTo tick returns to Idle before the
                            // villager starts toward the following point.
                            routine.phase = RoadBuildPhase::GoingTo { point: next };
                            let xz = road.points[next];
                            commands.entity(builder).insert(MoveTarget(Vec3::new(
                                xz.x,
                                terrain.get_height(xz.x, xz.y),
                                xz.y,
                            )));
                        }
                    }
                }
            }
            RoadBuildPhase::Working {
                point,
                seconds_left,
            } => {
                *activity = CharacterActivity::Building;
                commands.entity(builder).remove::<MoveTarget>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = RoadBuildPhase::Working {
                        point,
                        seconds_left: left,
                    };
                    continue;
                }
                road.built_through = road
                    .built_through
                    .max((point + 1).min(road.points.len()) as u16);
                let next = point + 1;
                if next >= road.points.len() {
                    info!(
                        "Village '{}': {} completed {:.0} m of path",
                        road.settlement,
                        road.builder,
                        road.total_length()
                    );
                    finish_road_builder(
                        &mut commands,
                        builder,
                        &mut intent,
                        &mut activity,
                        routine.settlement,
                    );
                } else {
                    *activity = CharacterActivity::Idle;
                    routine.phase = RoadBuildPhase::GoingTo { point: next };
                    // Movement runs later in the fixed schedule, so publish
                    // the next physical destination now. Waiting for another
                    // server tick merely to create this target made extreme
                    // time warp lose one whole tick per road point.
                    let xz = road.points[next];
                    commands.entity(builder).insert(MoveTarget(Vec3::new(
                        xz.x,
                        terrain.get_height(xz.x, xz.y),
                        xz.y,
                    )));
                }
            }
        }
    }
}

fn finish_road_builder(
    commands: &mut Commands,
    builder: Entity,
    intent: &mut VillagerIntent,
    activity: &mut CharacterActivity,
    settlement: Entity,
) {
    *intent = VillagerIntent::Resident { settlement };
    *activity = CharacterActivity::Idle;
    commands
        .entity(builder)
        .remove::<RoadBuilderRoutine>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
}

fn ensure_move_target(
    commands: &mut Commands,
    entity: Entity,
    current: Option<&MoveTarget>,
    expected: Vec3,
) {
    if current.is_none_or(|target| ground_distance(target.0, expected) > 0.05) {
        commands.entity(entity).insert(MoveTarget(expected));
    }
}

fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

fn stable_road_seed(settlement: &str, start: Vec2) -> u32 {
    settlement.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16_777_619)
    }) ^ start.x.to_bits().rotate_left(7)
        ^ start.y.to_bits().rotate_left(19)
}
