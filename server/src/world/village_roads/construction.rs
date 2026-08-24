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

/// Stable point keys in the physically completed component that reaches the
/// Moot Hall. Permit planning uses this same authority as the Road Steward so
/// a new plot cannot reserve frontage on a detached road island.
pub(crate) fn hall_connected_road_keys(
    hall_door: Vec2,
    roads: &[&VillageRoad],
) -> HashSet<(i32, i32)> {
    hall_road_network(hall_door, roads).connected_keys
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

/// Recover the permit-approved part that a live connector survey normally
/// produces (approach through network join, excluding the authored door and
/// the separately appended Hall threshold).
pub(super) fn planned_access_survey_points(
    points: &[Vec2],
    door: Vec2,
    survey_goal: Vec2,
    hall_door_goal: Option<Vec2>,
) -> Option<Vec<Vec2>> {
    if points.len() < 2 || points[0].distance_squared(door) > 0.01 {
        return None;
    }
    let end = if let Some(hall_door) = hall_door_goal {
        if points.len() < 3
            || points.last()?.distance_squared(hall_door) > 0.01
            || points[points.len() - 2].distance_squared(survey_goal) > 0.01
        {
            return None;
        }
        points.len() - 1
    } else {
        if points.last()?.distance_squared(survey_goal) > 0.01 {
            return None;
        }
        points.len()
    };
    let surveyed = points[1..end].to_vec();
    (!surveyed.is_empty()).then_some(surveyed)
}

#[allow(clippy::too_many_arguments)]
pub fn plan_requested_roads(
    mut commands: Commands,
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    world_time: Query<&WorldTime>,
    requests: Query<(
        Entity,
        &RoadRequest,
        &SettlementBuilding,
        &shared::components::BuildingId,
        &PlayerPosition,
        &PlayerRotation,
        Option<&PlannedRoadAccess>,
        Option<&RoadSurveyBackoff>,
    )>,
    placed_buildings: Query<(
        &shared::building::PlacedBuilding,
        &shared::building::BuildingPosition,
    )>,
    worksites: Query<&UnderConstruction>,
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
            Option<&MootSteward>,
            Has<shared::components::EmployedAt>,
            Has<FarmerRoutine>,
            Has<FishingRoutine>,
            Has<LumberjackRoutine>,
            Has<MarketCollectionRoutine>,
            Has<HouseholdShoppingRoutine>,
            Has<MootQueueTicket>,
            Has<HomeRoutine>,
            Has<InternalDeliveryRoutine>,
            Has<TradeRouteRoutine>,
            Has<crate::world::settlement_development::CivicHallBuilderRoutine>,
        ),
        With<CharacterKind>,
    >,
    mut prop_cache: Local<RoutePropChunkCache>,
    mut survey_scratch: Local<SurveyScratch>,
) {
    let Some(terrain) = terrain else { return };
    let now = world_time.iter().next().map_or_else(
        || simulation_time.elapsed_real_seconds_f64() * f64::from(simulation_time.factor()),
        absolute_world_seconds,
    );
    // A connector survey may generate several prop chunks and run bounded A*
    // alternatives. Population bursts can publish dozens of requests at once;
    // preserve that queue but admit only one expensive survey per server tick.
    // This is a CPU budget, not a gameplay throttle: at 60 Hz even 30 new
    // cabins receive their deterministic survey within half a real second.
    let mut survey_used = false;
    let mut prop_generation_used = false;
    let building_settlements: HashMap<_, _> = building_scopes
        .iter()
        .map(|(building_id, building_of)| (*building_id, building_of.0))
        .collect();
    // A civic worker should normally have only one retained request. Older
    // saves and a former night-time audit bug can still leave several on the
    // same person, though. Select the oldest stable BuildingId explicitly so
    // ECS archetype order can never starve an early cabin behind newer work.
    let preferred_request_by_builder: HashMap<_, _> = requests.iter().fold(
        HashMap::new(),
        |mut preferred, (entity, request, _, id, ..)| {
            preferred
                .entry(request.builder)
                .and_modify(|current: &mut (shared::components::BuildingId, Entity)| {
                    if (*id, entity.to_bits()) < (current.0, current.1.to_bits()) {
                        *current = (*id, entity);
                    }
                })
                .or_insert((*id, entity));
            preferred
        },
    );
    for (
        building_entity,
        request,
        building,
        building_id,
        position,
        rotation,
        planned_access,
        backoff,
    ) in requests.iter()
    {
        if preferred_request_by_builder
            .get(&request.builder)
            .is_some_and(|(_, preferred)| *preferred != building_entity)
        {
            continue;
        }
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
        let Ok((
            builder_name,
            mut intent,
            mut activity,
            steward,
            privately_employed,
            farming,
            fishing,
            lumbering,
            collecting,
            shopping,
            queueing,
            at_home,
            internal_delivery,
            trade_route,
            civic_hall_building,
        )) = builders.get_mut(request.builder)
        else {
            commands
                .entity(building_entity)
                .remove::<RoadRequest>()
                .remove::<RoadSurveyBackoff>();
            continue;
        };
        // The original owner remains responsible for the connector, but road
        // duty is not a second simultaneous job. Wait for the active shift,
        // household errand, Moot queue, or home choreography to release them;
        // the retained RoadRequest is then claimed on a later tick.
        if (privately_employed && steward.is_none())
            || farming
            || fishing
            || lumbering
            || collecting
            || shopping
            || queueing
            || at_home
            || internal_delivery
            || trade_route
            || civic_hall_building
        {
            continue;
        }
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
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        if survey_used {
            continue;
        }
        survey_used = true;

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
        let mut candidate_goals: Vec<_> = existing_goals.iter().copied().take(12).collect();
        // Dense neighbourhoods can surround all twelve nearest samples with
        // the same cabin or field blockers even though another branch of the
        // connected network is reachable. Add a bounded, evenly distributed
        // survey of the whole component rather than increasing an unbounded
        // nearest-neighbour search as the town grows.
        if existing_goals.len() > 12 {
            for slot in 1..=12 {
                let index = slot * (existing_goals.len() - 1) / 12;
                candidate_goals.push(existing_goals[index]);
            }
        }
        // Permit approval already proved and reserved one route to the
        // then-completed public network. In a large town that join can fall
        // outside both bounded samples above, causing the completed shell to
        // retry forever even though later plots kept its corridor open. Keep
        // it as an additional goal when it still belongs to the connected
        // component, then retain normal nearest-first ordering. The fresh
        // survey remains authoritative for terrain, props and buildings.
        let reserved_goal = planned_access
            .filter(|access| access.settlement_id == *settlement_id)
            .and_then(|access| access.points.last().copied())
            .filter(|goal| network.connected_keys.contains(&graph_key(*goal)));
        if let Some(reserved_goal) = reserved_goal {
            candidate_goals.push(reserved_goal);
        }
        candidate_goals.sort_by(|a, b| {
            a.distance_squared(start)
                .total_cmp(&b.distance_squared(start))
        });
        candidate_goals.dedup_by(|a, b| a.distance_squared(*b) <= 0.01);
        let mut candidates: Vec<_> = candidate_goals
            .into_iter()
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
        let mut prop_generation_pending = false;
        let mut reserved_rejection = None;
        // PlannedRoadAccess reserves a corridor while the shell is pending.
        // Prefer that exact polyline when it still passes the live terrain,
        // building, field and permanent-prop checks below. Re-surveying from
        // scratch could reject the very network join that the permit kept
        // clear (most often because the join lies in an older road's doorway
        // apron), leaving a completed building permanently roadless. Changed
        // geometry still invalidates the reservation and falls through to the
        // normal obstacle survey.
        let selected = candidates
            .into_iter()
            .find_map(|(goal, survey_goal, hall_door_goal)| {
                let class = if hall_door_goal.is_some() && main_count < 2 {
                    RoadClass::Main
                } else {
                    RoadClass::Lane
                };
                let width = surface_width_for_tier(settlement.tier, class);
                // A new connector first protects its full future corridor.
                // If that repeatedly makes an otherwise legitimate dense
                // cabin unreachable, accept a permanently narrow local lane:
                // connectivity matters more than hypothetical widening, and
                // larger arterials remain separate settlement projects.
                let reserved_width = if survey_failure >= 4 {
                    width
                } else {
                    class.initial_reserved_width()
                };
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
                        let root = Vec2::new(other_position.0.x, other_position.0.z);
                        // The visible Hall may still be a Moot Hall, but no
                        // connector may claim ground occupied by its future
                        // Village/Town Hall shell. Other buildings reserve
                        // only their actual authored footprint.
                        let definition = if other.building_type.is_civic_hall() {
                            shared::components::CivicHallLevel::largest_supported()
                                .building_type()
                                .definition()
                        } else {
                            other.building_type.definition()
                        };
                        let other_center =
                            definition.world_footprint_center(other_position.0, other.rotation);
                        // The road is intentionally allowed to enter the front
                        // apron of its source building and the Moot Hall. Every
                        // unrelated structure receives the full future corridor.
                        let is_endpoint = root
                            .distance_squared(Vec2::new(position.0.x, position.0.z))
                            <= 0.01
                            || root
                                .distance_squared(Vec2::new(hall_position.0.x, hall_position.0.z))
                                <= 0.01;
                        let protected_width = if is_endpoint { width } else { reserved_width };
                        BuildingBlocker {
                            center: other_center,
                            half: definition.footprint * 0.5
                                + Vec2::splat(protected_width * 0.5 + 0.45),
                            rotation: other.rotation,
                        }
                    })
                    .collect();
                // A permitted plot owns this ground before its shell exists.
                // Without reserving it here, a road surveyed during a
                // population burst can cross a worksite, then become
                // permanently impassable when that cabin or Farmstead is
                // completed. Site placement already avoids existing roads;
                // this closes the opposite half of that ordering race.
                for site in worksites
                    .iter()
                    .filter(|site| site.settlement_id == *settlement_id)
                {
                    let site_center = Vec2::new(site.position.x, site.position.z);
                    if site_center.cmpge(route_min).all() && site_center.cmple(route_max).all() {
                        let definition = site.kind.art().definition();
                        building_blockers.push(BuildingBlocker {
                            center: definition.world_footprint_center(site.position, site.rotation),
                            half: definition.footprint * 0.5
                                + Vec2::splat(reserved_width * 0.5 + 0.45),
                            rotation: site.rotation,
                        });
                    }
                    if let (Some(field_positions), Some(field_half)) = (
                        site.kind.field_positions(site.position, site.rotation),
                        site.kind.field_half_extents(),
                    ) {
                        for field in field_positions {
                            let field_center = Vec2::new(field.x, field.z);
                            if field_center.cmpge(route_min).all()
                                && field_center.cmple(route_max).all()
                            {
                                building_blockers.push(BuildingBlocker {
                                    center: field_center,
                                    half: field_half
                                        + Vec2::splat(
                                            reserved_width * 0.5
                                                + shared::components::FARM_FIELD_TERRACE_MARGIN
                                                + ROAD_SURVEY_FIELD_EPSILON,
                                        ),
                                    rotation: site.rotation,
                                });
                            }
                        }
                    }
                }
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
                                        + shared::components::FARM_FIELD_TERRACE_MARGIN
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
                let matching_reserved = planned_access
                    .filter(|access| access.settlement_id == *settlement_id)
                    .and_then(|access| {
                        planned_access_survey_points(
                            &access.points,
                            start,
                            survey_goal,
                            hall_door_goal,
                        )
                    });
                let reserved_survey = matching_reserved.and_then(|survey| {
                    let wide_enough = planned_access
                        .is_some_and(|access| access.half_width * 2.0 + 0.01 >= reserved_width);
                    if !wide_enough {
                        reserved_rejection.get_or_insert("reserved corridor is too narrow");
                    }
                    wide_enough.then_some(survey)
                });
                let mut reserved_is_clear = reserved_survey.is_some();
                if let Some(reserved) = reserved_survey.as_deref() {
                    let mut route = Vec::with_capacity(reserved.len() + 2);
                    route.push(start);
                    route.extend_from_slice(reserved);
                    if let Some(hall_door) = hall_door_goal {
                        route.push(hall_door);
                    }
                    reserved_is_clear &= road_corridor_is_dry(&terrain, &route, reserved_width);
                    if !reserved_is_clear {
                        reserved_rejection.get_or_insert("reserved corridor is wet");
                    }
                    // The first door-to-approach segment is intentionally
                    // inside the source shell's inflated blocker. The build
                    // zone has already cleared that authored apron; every
                    // segment after it must satisfy the current live world.
                    for segment in route[1..].windows(2) {
                        if !reserved_is_clear {
                            break;
                        }
                        if building_blockers
                            .iter()
                            .any(|blocker| blocker.blocks_segment(segment[0], segment[1]))
                        {
                            reserved_is_clear = false;
                            reserved_rejection.get_or_insert("reserved corridor meets a building");
                            break;
                        }
                        let Some(permanent_props) = blockers_for_route(
                            &terrain,
                            segment[0],
                            segment[1],
                            &building_blockers,
                            reserved_width * 0.5,
                            colliders.as_deref(),
                            derived.as_deref(),
                            &mut prop_cache,
                            &mut prop_generation_used,
                            false,
                        ) else {
                            prop_generation_pending = true;
                            return None;
                        };
                        let steps = (segment[0].distance(segment[1]) / NAVIGATION_SAMPLE_STEP)
                            .ceil()
                            .max(1.0) as usize;
                        let mut previous_height = None;
                        for step in 0..=steps {
                            let point = segment[0].lerp(segment[1], step as f32 / steps as f32);
                            let height = terrain.get_height(point.x, point.y);
                            if permanent_props.blocks(point)
                                || previous_height
                                    .is_some_and(|previous: f32| (height - previous).abs() > 0.47)
                            {
                                reserved_is_clear = false;
                                reserved_rejection.get_or_insert(
                                    "reserved corridor meets a permanent prop or steep step",
                                );
                                break;
                            }
                            previous_height = Some(height);
                        }
                    }
                }
                let Some(prop_blockers) = blockers_for_route(
                    &terrain,
                    survey_start,
                    survey_goal,
                    &building_blockers,
                    reserved_width * 0.5,
                    colliders.as_deref(),
                    derived.as_deref(),
                    &mut prop_cache,
                    &mut prop_generation_used,
                    true,
                ) else {
                    prop_generation_pending = true;
                    return None;
                };
                let mut surveyed = if reserved_is_clear {
                    reserved_survey.expect("a clear reserved survey exists")
                } else {
                    survey_village_road(
                        &terrain,
                        survey_start,
                        survey_goal,
                        &building_blockers,
                        &prop_blockers,
                        seed,
                        &mut survey_scratch,
                    )
                };
                if surveyed.len() < 2 {
                    // Prefer a small natural detour around mature scenery.
                    // If trees alone seal the connector, retry once with
                    // timber treated as an embodied clearance job. Rocks,
                    // buildings, fields and water remain hard constraints.
                    let Some(permanent_prop_blockers) = blockers_for_route(
                        &terrain,
                        survey_start,
                        survey_goal,
                        &building_blockers,
                        reserved_width * 0.5,
                        colliders.as_deref(),
                        derived.as_deref(),
                        &mut prop_cache,
                        &mut prop_generation_used,
                        false,
                    ) else {
                        prop_generation_pending = true;
                        return None;
                    };
                    surveyed = survey_village_road(
                        &terrain,
                        survey_start,
                        survey_goal,
                        &building_blockers,
                        &permanent_prop_blockers,
                        seed ^ 0xA511_E9B3,
                        &mut survey_scratch,
                    );
                    if surveyed.len() < 2 {
                        return None;
                    }
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
                            shared::components::FARM_FIELD_TERRACE_MARGIN,
                        )
                    });
                let dry = road_corridor_is_dry(
                    &terrain,
                    &certified.points,
                    certified.reservation_width(),
                );
                // The obstacle survey begins outside the authored door apron.
                // Collect against the complete final ribbon even when the
                // main route bent around every tree, so a trunk between the
                // door and that survey start still becomes physical work.
                let mut trees = clearable_trees_intersecting_road(
                    &terrain,
                    &certified.points,
                    width,
                    derived.as_deref(),
                    &mut prop_cache,
                );
                // Deterministic prop generation still contains scenery that
                // a completed building's authored build zone already erased.
                // Do not make the road worker chop those invisible ghosts.
                trees.retain(|tree| {
                    !placed_buildings.iter().any(|(placed, placed_position)| {
                        let definition = placed.building_type.definition();
                        BuildingBlocker {
                            center: definition
                                .world_footprint_center(placed_position.0, placed.rotation),
                            half: definition.footprint * 0.5,
                            rotation: placed.rotation,
                        }
                        .contains(tree.point)
                    })
                });
                (!crosses_field && dry).then_some((
                    goal,
                    surveyed,
                    hall_door_goal,
                    class,
                    width,
                    reserved_width,
                    trees,
                ))
            });

        let Some((_goal, surveyed, hall_door_goal, class, width, reserved_width, trees)) = selected
        else {
            if prop_generation_pending {
                // The route is waiting for its deterministic prop corridor to
                // finish warming, not failing a survey. Preserve its attempt
                // counter and let the next server tick resume it.
                continue;
            }
            let next_backoff = RoadSurveyBackoff::after_failure(backoff.copied(), now);
            if next_backoff.should_warn() {
                warn!(
                    "Village '{}': {} could not survey a clear path from the {}; retry {} in {:.1}s ({})",
                    settlement.name,
                    builder_name.0,
                    building.kind.label(),
                    next_backoff.failures,
                    next_backoff.retry_after - now,
                    reserved_rejection.unwrap_or("no compatible permit reservation"),
                );
                #[cfg(test)]
                eprintln!(
                    "LAB road survey rejection: village={} building={} builder={} attempt={} reason={} access={:?}",
                    settlement.name,
                    building.kind.label(),
                    builder_name.0,
                    next_backoff.failures,
                    reserved_rejection.unwrap_or("no compatible permit reservation"),
                    planned_access.map(|access| &access.points),
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
                    .insert(RoadRepairBacklog)
                    .remove::<RoadRequest>()
                    .remove::<RoadSurveyBackoff>();
            } else {
                commands.entity(building_entity).insert(next_backoff);
            }
            *intent = VillagerIntent::Resident {
                settlement: request.settlement,
            };
            activity.set_if_neq(CharacterActivity::Idle);
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
                RoadConnectorFor {
                    building: building_entity,
                },
                RoadTreeClearancePlan { trees },
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        *intent = VillagerIntent::RoadBuilding {
            settlement: request.settlement,
            road,
        };
        activity.set_if_neq(CharacterActivity::Idle);
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
            .remove::<RoadSurveyBackoff>()
            .remove::<RoadRepairBacklog>();
        info!(
            "Village '{}': {} began the path from the {}",
            settlement.name,
            builder_name.0,
            building.kind.label()
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn obstruction_on_road_segment(
    road: &VillageRoad,
    point: usize,
    clearance: Option<&RoadTreeClearancePlan>,
) -> Option<RoadTreeObstruction> {
    let clearance = clearance?;
    if point == 0 || point >= road.points.len() {
        return None;
    }
    let start = road.points[point - 1];
    let end = road.points[point];
    let segment = end - start;
    let length_squared = segment.length_squared();
    clearance
        .trees
        .iter()
        .copied()
        .filter_map(|tree| {
            let t = if length_squared <= f32::EPSILON {
                0.0
            } else {
                ((tree.point - start).dot(segment) / length_squared).clamp(0.0, 1.0)
            };
            let clearance = tree.radius + road.width * 0.5 + 0.15;
            (tree.point.distance_squared(start + segment * t) <= clearance * clearance)
                .then_some((t, tree))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, tree)| tree)
}

fn road_tree_stand(
    terrain: &WorldTerrain,
    tree: RoadTreeObstruction,
    approach_from: Vec2,
    approach: u8,
) -> Vec3 {
    const ANGLES: [f32; 8] = [
        0.0,
        std::f32::consts::FRAC_PI_4,
        -std::f32::consts::FRAC_PI_4,
        std::f32::consts::FRAC_PI_2,
        -std::f32::consts::FRAC_PI_2,
        3.0 * std::f32::consts::FRAC_PI_4,
        -3.0 * std::f32::consts::FRAC_PI_4,
        std::f32::consts::PI,
    ];
    let base = (approach_from - tree.point).normalize_or(Vec2::X);
    let angle = ANGLES[usize::from(approach) % ANGLES.len()];
    let (sin, cos) = angle.sin_cos();
    let direction = Vec2::new(base.x * cos - base.y * sin, base.x * sin + base.y * cos);
    let point = tree.point + direction * (tree.radius + VILLAGER_PROP_RADIUS + 0.3);
    Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y)
}

fn remove_streamed_road_tree(colliders: &mut StaticColliders, tree: Vec2) -> bool {
    colliders.mark_road_tree_cleared(tree);
    let id = colliders
        .instances
        .iter()
        .filter(|(_, instance)| instance.kind.is_road_clearable())
        .filter_map(|(id, instance)| {
            let point = Vec2::new(instance.position.x, instance.position.z);
            (point.distance_squared(tree) <= 0.35_f32.powi(2))
                .then_some((*id, point.distance_squared(tree)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id);
    id.is_some_and(|id| colliders.remove_instance(id).is_some())
}

#[allow(clippy::too_many_arguments)]
pub fn build_village_roads(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    mut colliders: Option<ResMut<StaticColliders>>,
    mut commands: Commands,
    mut roads: Query<(
        &mut VillageRoad,
        &shared::components::RoadOf,
        Option<&RoadConnectorFor>,
        Option<&mut RoadTreeClearancePlan>,
    )>,
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
        let Ok((mut road, road_of, connector, mut clearance)) = roads.get_mut(routine.road) else {
            activity.set_if_neq(CharacterActivity::Idle);
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
                        release_completed_access_reservation(&mut commands, connector);
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

                if point == usize::from(road.built_through)
                    && ground_distance(position.0, target) <= ROAD_FAILED_WAYPOINT_WORK_REACH
                {
                    // The route planner has already brought the worker within
                    // one resampled road segment. Exact goals at the inflated
                    // edge of a building apron can be rejected even though
                    // the worker is plainly close enough to pack the dirt.
                    // Treat this as an embodied arrival; resurveying the same
                    // certified connector every tick cannot improve it.
                    let direction = road.points[point - 1] - road.points[point];
                    if direction.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-direction.x, -direction.y);
                    }
                    activity.set_if_neq(CharacterActivity::Building);
                    routine.phase = RoadBuildPhase::Working {
                        point,
                        seconds_left: ROAD_BUILD_SECONDS,
                    };
                    continue;
                }

                let start = road.points[0];
                let road_entity = routine.road;
                let next_attempt = routine.attempt.saturating_add(1);
                let building = connector.map(|connector| connector.building).or_else(|| {
                    buildings
                        .iter()
                        .find(|(_, building, position, rotation, building_of)| {
                            if building_of.0 != road_of.0 {
                                return false;
                            }
                            let door = building.kind.entrance_position(position.0, rotation.0);
                            start.distance_squared(Vec2::new(door.x, door.z)) <= 0.75_f32.powi(2)
                        })
                        .map(|(entity, ..)| entity)
                });
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
                activity.set_if_neq(CharacterActivity::Idle);
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
                    if let Some(building) = building {
                        // The physical road disappears in this same update,
                        // after the periodic audit has already run. Publish the
                        // civic obligation immediately so the building cannot
                        // spend the interval until the next audit looking
                        // falsely healthy or vanish from every lifecycle.
                        commands.entity(building).insert(RoadRepairBacklog);
                    }
                }
                continue;
            }
        }

        if let (
            RoadBuildPhase::GoingToTree {
                point,
                tree,
                stand,
                radius,
                approach,
            },
            Some(failed),
        ) = (routine.phase, route_failed)
        {
            let stand3 = Vec3::new(stand.x, terrain.get_height(stand.x, stand.y), stand.y);
            if failed.goal.distance_squared(stand3) <= 0.01 {
                commands
                    .entity(builder)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
                if Vec2::new(position.0.x, position.0.z).distance(tree) <= radius + 2.0 {
                    let to_tree = tree - Vec2::new(position.0.x, position.0.z);
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.y);
                    }
                    activity.set_if_neq(CharacterActivity::Chopping);
                    routine.phase = RoadBuildPhase::ChoppingTree {
                        point,
                        tree,
                        radius,
                        seconds_left: CHOP_SECONDS,
                    };
                } else {
                    let next_approach = approach.wrapping_add(1);
                    let next = road_tree_stand(
                        &terrain,
                        RoadTreeObstruction {
                            point: tree,
                            radius,
                        },
                        road.points[point.saturating_sub(1)],
                        next_approach,
                    );
                    routine.phase = RoadBuildPhase::GoingToTree {
                        point,
                        tree,
                        stand: Vec2::new(next.x, next.z),
                        radius,
                        approach: next_approach,
                    };
                    commands.entity(builder).insert(MoveTarget(next));
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
            match routine.phase {
                RoadBuildPhase::GoingTo { point } => {
                    let xz = road.points[point.min(road.points.len() - 1)];
                    commands.entity(builder).insert(MoveTarget(Vec3::new(
                        xz.x,
                        terrain.get_height(xz.x, xz.y),
                        xz.y,
                    )));
                }
                RoadBuildPhase::GoingToTree { stand, .. } => {
                    commands.entity(builder).insert(MoveTarget(Vec3::new(
                        stand.x,
                        terrain.get_height(stand.x, stand.y),
                        stand.y,
                    )));
                }
                RoadBuildPhase::ChoppingTree { .. } | RoadBuildPhase::Working { .. } => {}
            }
            continue;
        }

        match routine.phase {
            RoadBuildPhase::GoingTo { point } => {
                activity.set_if_neq(CharacterActivity::Idle);
                let point = point.min(road.points.len() - 1);
                let xz = road.points[point];
                if point >= usize::from(road.built_through) {
                    if let Some(tree) =
                        obstruction_on_road_segment(&road, point, clearance.as_deref())
                    {
                        let stand = road_tree_stand(
                            &terrain,
                            tree,
                            Vec2::new(position.0.x, position.0.z),
                            0,
                        );
                        routine.phase = RoadBuildPhase::GoingToTree {
                            point,
                            tree: tree.point,
                            stand: Vec2::new(stand.x, stand.z),
                            radius: tree.radius,
                            approach: 0,
                        };
                        ensure_move_target(&mut commands, builder, move_target, stand);
                        continue;
                    }
                }
                let target = Vec3::new(xz.x, terrain.get_height(xz.x, xz.y), xz.y);
                if ground_distance(position.0, target) > ROAD_REACH {
                    ensure_move_target(&mut commands, builder, move_target, target);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                if point < usize::from(road.built_through) {
                    let next = point + 1;
                    if next >= road.points.len() {
                        release_completed_access_reservation(&mut commands, connector);
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
                    activity.set_if_neq(CharacterActivity::Building);
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
                            release_completed_access_reservation(&mut commands, connector);
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
            RoadBuildPhase::GoingToTree {
                point,
                tree,
                stand,
                radius,
                ..
            } => {
                activity.set_if_neq(CharacterActivity::Idle);
                let target = Vec3::new(stand.x, terrain.get_height(stand.x, stand.y), stand.y);
                if ground_distance(position.0, target) > ROAD_REACH {
                    ensure_move_target(&mut commands, builder, move_target, target);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                let to_tree = tree - Vec2::new(position.0.x, position.0.z);
                if to_tree.length_squared() > 1e-4 {
                    facing.0 = f32::atan2(-to_tree.x, -to_tree.y);
                }
                activity.set_if_neq(CharacterActivity::Chopping);
                routine.phase = RoadBuildPhase::ChoppingTree {
                    point,
                    tree,
                    radius,
                    seconds_left: CHOP_SECONDS,
                };
            }
            RoadBuildPhase::ChoppingTree {
                point,
                tree,
                radius,
                seconds_left,
            } => {
                activity.set_if_neq(CharacterActivity::Chopping);
                commands.entity(builder).remove::<MoveTarget>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = RoadBuildPhase::ChoppingTree {
                        point,
                        tree,
                        radius,
                        seconds_left: left,
                    };
                    continue;
                }
                if let Some(clearance) = clearance.as_deref_mut() {
                    clearance
                        .trees
                        .retain(|candidate| candidate.point.distance_squared(tree) > 0.01);
                }
                let removed_live_collider = colliders
                    .as_deref_mut()
                    .is_some_and(|colliders| remove_streamed_road_tree(colliders, tree));
                debug!(
                    "Village '{}': {} cleared a tree at {:.1},{:.1} for the road (live collider removed: {})",
                    road.settlement, road.builder, tree.x, tree.y, removed_live_collider
                );
                activity.set_if_neq(CharacterActivity::Idle);
                routine.phase = RoadBuildPhase::GoingTo { point };
                let xz = road.points[point.min(road.points.len() - 1)];
                commands.entity(builder).insert(MoveTarget(Vec3::new(
                    xz.x,
                    terrain.get_height(xz.x, xz.y),
                    xz.y,
                )));
            }
            RoadBuildPhase::Working {
                point,
                seconds_left,
            } => {
                activity.set_if_neq(CharacterActivity::Building);
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
                    release_completed_access_reservation(&mut commands, connector);
                    finish_road_builder(
                        &mut commands,
                        builder,
                        &mut intent,
                        &mut activity,
                        routine.settlement,
                    );
                } else {
                    activity.set_if_neq(CharacterActivity::Idle);
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

fn release_completed_access_reservation(
    commands: &mut Commands,
    connector: Option<&RoadConnectorFor>,
) {
    if let Some(connector) = connector {
        commands
            .entity(connector.building)
            .remove::<PlannedRoadAccess>()
            .remove::<RoadRepairBacklog>();
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
    if *activity != CharacterActivity::Idle {
        *activity = CharacterActivity::Idle;
    }
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
