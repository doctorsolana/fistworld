//! Tactical farm, fishing and lumber production loops.
//!
//! Output exists only after embodied workers perform and deposit a physical
//! batch. Employment and workplace attachment are joined through durable IDs.

use super::*;

pub(super) fn workplace_road_is_ready(
    building: Entity,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    settlement_id: shared::components::SettlementId,
    hall_position: Vec3,
    hall_rotation: f32,
    roads: &Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: &Query<(), With<RoadRequest>>,
) -> bool {
    if road_requests.get(building).is_ok() {
        return false;
    }
    let settlement_roads: Vec<_> = roads
        .iter()
        .filter_map(|(road, road_of)| (road_of.0 == settlement_id).then_some(road))
        .collect();
    // Focused unit fixtures predating physical roads retain their lightweight
    // seam. A live settlement publishes a RoadRequest before the first road,
    // so this fallback never opens a real unconnected workplace.
    settlement_roads.is_empty()
        || crate::world::village_roads::building_has_connected_road(
            kind,
            position,
            rotation,
            hall_position,
            hall_rotation,
            &settlement_roads,
        )
}

/// Plant the two authored wheat fields beside every completed Farmstead.
pub fn ensure_farm_fields(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    farms: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
    )>,
    fields: Query<(&FarmField, &shared::components::AttachedTo)>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let planted: HashSet<(shared::components::BuildingId, u8)> = fields
        .iter()
        .map(|(field, attached_to)| (attached_to.0, field.plot_index))
        .collect();
    for (farm, position, rotation, building_id) in farms.iter() {
        if farm.kind != SettlementBuildingKind::Farmstead {
            continue;
        }
        for plot_index in 0..shared::components::FARM_FIELDS_PER_FARMSTEAD {
            if planted.contains(&(*building_id, plot_index)) {
                continue;
            }
            let Some(mut field_position) = farm
                .kind
                .field_position_at(position.0, rotation.0, plot_index)
            else {
                continue;
            };
            field_position.y = terrain.get_height(field_position.x, field_position.z);
            let field = commands
                .spawn((
                    FarmField {
                        settlement: farm.settlement.clone(),
                        farmstead: position.0,
                        plot_index,
                        quality: farm.quality,
                    },
                    PlayerPosition(field_position),
                    PlayerRotation(rotation.0),
                    Replicate::to_clients(NetworkTarget::All),
                ))
                .id();
            commands
                .entity(field)
                .insert(shared::components::AttachedTo(*building_id));
            info!(
                "Village '{}': planted wheat field {}/{} beside its Farmstead",
                farm.settlement,
                plot_index + 1,
                shared::components::FARM_FIELDS_PER_FARMSTEAD,
            );
        }
    }
}

/// Place the separate collider-free pier behind every completed Fisherman's
/// Hut. The asset origin is its landward end; the authored piles extend below
/// that origin, so water level is the stable placement plane on every coast.
pub fn ensure_fishing_piers(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    huts: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
    )>,
    piers: Query<&shared::components::AttachedTo, With<FishingPier>>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let Some(water) = terrain.water_level() else {
        return;
    };
    for (hut, position, rotation, building_id) in huts.iter() {
        if hut.kind != SettlementBuildingKind::FishermansHut
            || piers
                .iter()
                .any(|attached_to| attached_to.0 == *building_id)
        {
            continue;
        }
        let Some(mut pier_position) = hut.kind.pier_position(position.0, rotation.0) else {
            continue;
        };
        pier_position.y = water;
        let pier = commands
            .spawn((
                FishingPier {
                    settlement: hut.settlement.clone(),
                    fishermans_hut: position.0,
                    quality: hut.quality,
                },
                PlayerPosition(pier_position),
                PlayerRotation(rotation.0),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        commands
            .entity(pier)
            .insert(shared::components::AttachedTo(*building_id));
        info!(
            "Village '{}': set a fishing pier behind its Fisherman's Hut",
            hut.settlement
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn assign_farmer_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    farms: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    fields: Query<(
        Entity,
        &FarmField,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::AttachedTo,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&FarmerHarvestProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<ProcessingRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
            Without<strategic::StrategicPerson>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !ordinary_workday(clock) {
        return;
    }
    let mut eligible_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, _, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            eligible_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
    }
    if eligible_by_building.is_empty() {
        return;
    }
    let mut fields_by_farm: HashMap<shared::components::BuildingId, Vec<(Entity, u8, Vec3, f32)>> =
        HashMap::new();
    for (field_entity, field, position, rotation, attached_to) in fields.iter() {
        fields_by_farm.entry(attached_to.0).or_default().push((
            field_entity,
            field.plot_index,
            position.0,
            rotation.0,
        ));
    }
    for farm_fields in fields_by_farm.values_mut() {
        farm_fields.sort_by_key(|(_, plot_index, _, _)| *plot_index);
    }
    let mut claimed = HashSet::new();
    for (farmstead, farm, position, rotation, building_id, building_of) in farms.iter() {
        if farm.kind != SettlementBuildingKind::Farmstead {
            continue;
        }
        let Some(employees) = eligible_by_building.get(building_id) else {
            continue;
        };
        let Some(farm_fields) = fields_by_farm.get(building_id) else {
            continue;
        };
        let Some((hall, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, _, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        if !workplace_road_is_ready(
            farmstead,
            farm.kind,
            position.0,
            rotation.0,
            building_of.0,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            defer_shift_until_workplace_access(&mut commands, employees, clock.day);
            continue;
        }
        for (worker_index, employee) in employees
            .iter()
            .take(farm.kind.positions() as usize)
            .enumerate()
        {
            let (field, _, field_position, field_rotation) =
                farm_fields[worker_index % farm_fields.len()];
            let Ok((worker, name, intent, _, off_duty, progress, employment)) =
                villagers.get(*employee)
            else {
                continue;
            };
            if claimed.contains(&worker)
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let harvest_seconds = progress
                .filter(|progress| progress.farmstead == farmstead && progress.field == field)
                .map_or(0.0, |progress| progress.seconds);
            let worker_salt = stable_name_hash(&name.0);
            let work_stand = if let Some(terrain) = terrain.as_deref() {
                crate::world::village_roads::reachable_farm_work_stand(
                    terrain,
                    position.0,
                    rotation.0,
                    field_position,
                    worker_salt,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                )
            } else {
                farm_work_stand(
                    field_position,
                    field_rotation,
                    worker_salt,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                )
            };
            let Some(work_stand) = work_stand else {
                debug!(
                    "Farmer {} cannot reach a certified standing point for Farmstead {:?}",
                    name.0, building_id
                );
                continue;
            };
            claimed.insert(worker);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<FarmerHarvestProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    FarmerRoutine {
                        farmstead,
                        field,
                        hall,
                        work_stand,
                        harvest_seconds,
                        failed_workplace_routes: 0,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: FarmerPhase::GoingToFarmstead,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(farm.kind.entrance_position(position.0, rotation.0)),
                ));
        }
    }
}

/// Run the producer-owned part of the physical wheat loop: field to Farmstead.
/// Farmers never sell or haul to the Moot Hall. The market porter independently
/// evaluates the Farmstead's sale policy and collects only approved surplus.
#[allow(clippy::too_many_arguments)]
pub fn run_farmer_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    _economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    farms: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    fields: Query<(&FarmField, &shared::components::AttachedTo), Without<CharacterKind>>,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    moot_service_busy: Query<(), Or<(With<MootQueueTicket>, With<MootMealRoutine>)>>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut CharacterActivity,
            Option<&mut CharacterAttributes>,
            &mut FarmerRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = ordinary_workday(clock);

    for (
        worker,
        name,
        employment,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut activity,
        mut attributes,
        mut routine,
        move_target,
        route_failed,
    ) in workers.iter_mut()
    {
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || moot_service_busy.get(worker).is_ok()
            || road_builder.is_some()
            || door_transit.is_some()
        {
            continue;
        }
        if !intent.is_settled() {
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        }
        let Ok((farm, farm_position, farm_rotation, building_id, building_of)) =
            farms.get(routine.farmstead)
        else {
            commands
                .entity(worker)
                .remove::<FarmerRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if farm.kind != SettlementBuildingKind::Farmstead || employment.0 != *building_id {
            commands
                .entity(worker)
                .remove::<FarmerRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((field, attached_to)) = fields.get(routine.field) else {
            continue;
        };
        let Ok(settlement_id) = halls.get(routine.hall) else {
            continue;
        };
        if attached_to.0 != *building_id || *settlement_id != building_of.0 {
            continue;
        }
        let farm_entrance = farm
            .kind
            .entrance_position(farm_position.0, farm_rotation.0);
        let farm_inside = farm
            .kind
            .interior_door_position(farm_position.0, farm_rotation.0);

        // A repeatedly impossible certified route must not become an
        // unbounded A* loop. Preserve the same finite-inventory handoff, but
        // collapse only this failed last leg after the retry budget is spent.
        if routine.failed_workplace_routes >= MAX_WORKPLACE_ROUTE_FAILURES
            && inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
        {
            commands
                .entity(worker)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            *activity = CharacterActivity::Idle;
            if unload_worker_output(&mut inventories, worker, routine.farmstead, Good::Wheat) {
                warn!(
                    "Farmer {} completed a loaded workplace handoff abstractly after {} failed routes",
                    name.0, routine.failed_workplace_routes,
                );
                finish_farmer_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
            continue;
        }

        if let Some(failed) = route_failed {
            let retry_target = match routine.phase {
                FarmerPhase::WalkingToField { stand } => Some(stand),
                FarmerPhase::GoingToFarmstead | FarmerPhase::ReturningToFarmstead => {
                    Some(farm_entrance)
                }
                FarmerPhase::Inside { .. } | FarmerPhase::Farming | FarmerPhase::EndingShift => {
                    None
                }
            };
            routine.failed_workplace_routes = routine.failed_workplace_routes.saturating_add(1);
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            let carrying_wheat = inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0);
            if carrying_wheat {
                if routine.failed_workplace_routes == MAX_WORKPLACE_ROUTE_FAILURES {
                    warn!(
                        "Farmer {} still carries Wheat after {} failed workplace routes; retaining the load for a bounded last-leg handoff before going off duty",
                        name.0, routine.failed_workplace_routes,
                    );
                }
                if routine.failed_workplace_routes < MAX_WORKPLACE_ROUTE_FAILURES {
                    commands.entity(worker).insert(MoveTarget(farm_entrance));
                }
                routine.phase = FarmerPhase::ReturningToFarmstead;
                *activity = CharacterActivity::Idle;
                continue;
            }
            if routine.failed_workplace_routes >= MAX_WORKPLACE_ROUTE_FAILURES || !workday_active {
                warn!(
                    "Farmer {} could not reach the workplace at {:.1},{:.1} after {} routes; ending the empty-handed shift",
                    name.0,
                    failed.goal.x,
                    failed.goal.z,
                    routine.failed_workplace_routes,
                );
                finish_farmer_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
                continue;
            }
            if let Some(target) = retry_target {
                // One failed commute must not turn an employed farmer into an
                // ambient idler for the rest of the day. Keep the exact work
                // phase (and any carried Wheat), then let the route planner
                // try a different certified corridor.
                commands.entity(worker).insert(MoveTarget(target));
                *activity = CharacterActivity::Idle;
            } else {
                warn!(
                    "Farmer {} discarded stale route failure for {:.1},{:.1} while {:?}",
                    name.0, failed.goal.x, failed.goal.z, routine.phase
                );
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                FarmerPhase::Inside { .. } => {
                    *activity = CharacterActivity::Idle;
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        farm_position.0,
                        farm_entrance,
                        farm_inside,
                        farm_entrance,
                    );
                    routine.phase = FarmerPhase::EndingShift;
                    continue;
                }
                FarmerPhase::WalkingToField { .. } | FarmerPhase::Farming => {
                    *activity = CharacterActivity::Idle;
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                FarmerPhase::GoingToFarmstead
                    if ground_distance(position.0, farm_entrance) <= DOOR_REACH =>
                {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                    {
                        routine.phase = FarmerPhase::ReturningToFarmstead;
                        continue;
                    }
                    finish_farmer_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FarmerPhase::EndingShift => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                    {
                        ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                        routine.phase = FarmerPhase::ReturningToFarmstead;
                        continue;
                    }
                    finish_farmer_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FarmerPhase::GoingToFarmstead | FarmerPhase::ReturningToFarmstead => {}
            }
        }

        match routine.phase {
            FarmerPhase::GoingToFarmstead => {
                if ground_distance(position.0, farm_entrance) <= DOOR_REACH {
                    routine.failed_workplace_routes = 0;
                    if !workday_active {
                        if inventories
                            .get_mut(worker)
                            .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                        {
                            routine.phase = FarmerPhase::ReturningToFarmstead;
                            continue;
                        }
                        finish_farmer_shift(
                            &mut commands,
                            worker,
                            production_day,
                            &routine,
                            &mut activity,
                        );
                        continue;
                    }
                    *activity = CharacterActivity::Idle;
                    begin_workplace_entry(
                        &mut commands,
                        worker,
                        farm_position.0,
                        farm_entrance,
                        farm_inside,
                    );
                    routine.phase = FarmerPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                }
            }
            FarmerPhase::Inside { seconds_left } => {
                *activity = CharacterActivity::Indoors;
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = FarmerPhase::Inside { seconds_left: left };
                    continue;
                }
                let stand = routine.work_stand;
                *activity = CharacterActivity::Idle;
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    farm_position.0,
                    farm_entrance,
                    farm_inside,
                    stand,
                );
                routine.phase = FarmerPhase::WalkingToField { stand };
            }
            FarmerPhase::WalkingToField { stand } => {
                if ground_distance(position.0, stand) <= WORK_REACH {
                    routine.failed_workplace_routes = 0;
                    commands.entity(worker).remove::<MoveTarget>();
                    *activity = CharacterActivity::Farming;
                    routine.phase = FarmerPhase::Farming;
                } else {
                    ensure_move_target(&mut commands, worker, move_target, stand);
                }
            }
            FarmerPhase::Farming => {
                *activity = CharacterActivity::Farming;
                routine.harvest_seconds += dt;
                let seconds_per_wheat = farmer_seconds_per_wheat(field.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    // The carried bundle is a completed field basket, not a
                    // frame-by-frame progress meter. Publishing the first of
                    // two Wheat immediately made the client replace the
                    // harvest animation with a stationary carry pose for the
                    // entire second production interval. Retain continuous
                    // labour internally, then materialise the full batch and
                    // leave the field on the same tick.
                    let carried = carrier.amount(Good::Wheat);
                    let needed = FARM_CARRY_BATCH_UNITS.saturating_sub(carried);
                    let batch_seconds = seconds_per_wheat * needed as f32;
                    if needed > 0 && routine.harvest_seconds < batch_seconds {
                        continue;
                    }
                    let first_harvest_today = routine.produced_today == 0;
                    let produced = carrier.add(Good::Wheat, needed);
                    if produced > 0 {
                        routine.harvest_seconds = (routine.harvest_seconds
                            - seconds_per_wheat * produced as f32)
                            .max(0.0);
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    business_events.record_production(production_day, *building_id, produced);
                    // Wheat is agricultural output, not a ration. The mill
                    // records food production only when this becomes Flour.
                    // Attribute progression remains once per productive day
                    // even though production itself has no daily cap.
                    if produced > 0 && first_harvest_today {
                        if let Some(attributes) = attributes.as_deref_mut() {
                            attributes.train_physique(1);
                        }
                    }
                    let batch_ready = carrier.amount(Good::Wheat) >= FARM_CARRY_BATCH_UNITS;
                    let can_keep_harvesting =
                        !batch_ready && carrier.free_bulk() >= Good::Wheat.bulk_per_unit();
                    if can_keep_harvesting {
                        continue;
                    }
                }
                *activity = CharacterActivity::Idle;
                commands.entity(worker).insert(MoveTarget(farm_entrance));
                routine.phase = FarmerPhase::ReturningToFarmstead;
            }
            FarmerPhase::ReturningToFarmstead => {
                if ground_distance(position.0, farm_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    continue;
                }
                routine.failed_workplace_routes = 0;
                commands.entity(worker).remove::<MoveTarget>();
                let unloaded =
                    unload_worker_output(&mut inventories, worker, routine.farmstead, Good::Wheat);
                if !unloaded {
                    // A full store is backpressure, not a licence to take
                    // company stock home. Wait at the Farmstead until a porter
                    // frees space, preserving every unit in personal cargo.
                    *activity = CharacterActivity::Idle;
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                if !workday_active {
                    finish_farmer_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                *activity = CharacterActivity::Idle;
                begin_workplace_entry(
                    &mut commands,
                    worker,
                    farm_position.0,
                    farm_entrance,
                    farm_inside,
                );
                routine.phase = FarmerPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            FarmerPhase::EndingShift => {
                if inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                finish_farmer_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Attach the physical hut-to-pier loop to each named fisher.
pub fn assign_fishing_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    huts: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    piers: Query<(Entity, &FishingPier, &shared::components::AttachedTo)>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&FishingWorkProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<ProcessingRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
            Without<strategic::StrategicPerson>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !ordinary_workday(clock) {
        return;
    }
    let mut eligible_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, _, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            eligible_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
    }
    if eligible_by_building.is_empty() {
        return;
    }
    let mut claimed = HashSet::new();
    for (hut_entity, hut, hut_position, hut_rotation, building_id, building_of) in huts.iter() {
        if hut.kind != SettlementBuildingKind::FishermansHut {
            continue;
        }
        let Some(employees) = eligible_by_building.get(building_id) else {
            continue;
        };
        let Some((pier, _, _)) = piers
            .iter()
            .find(|(_, _, attached_to)| attached_to.0 == *building_id)
        else {
            continue;
        };
        let Some((hall, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, _, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        if !workplace_road_is_ready(
            hut_entity,
            hut.kind,
            hut_position.0,
            hut_rotation.0,
            building_of.0,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            defer_shift_until_workplace_access(&mut commands, employees, clock.day);
            continue;
        }
        for employee in employees.iter().take(hut.kind.positions() as usize) {
            let Ok((worker, worker_display_name, intent, _, off_duty, progress, employment)) =
                villagers.get(*employee)
            else {
                continue;
            };
            if claimed.contains(&worker)
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let catch_seconds = progress
                .filter(|progress| progress.hut == hut_entity && progress.pier == pier)
                .map_or(0.0, |progress| progress.seconds);
            claimed.insert(worker);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<FishingWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    FishingRoutine {
                        hut: hut_entity,
                        pier,
                        hall,
                        catch_seconds,
                        failed_workplace_routes: 0,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: FishingPhase::GoingToHut,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(hut.kind.entrance_position(hut_position.0, hut_rotation.0)),
                ));
            info!(
                "Village '{}': {} began fishing from the new pier",
                hut.settlement, worker_display_name.0,
            );
        }
    }
}

fn fishing_land_point(terrain: &WorldTerrain, hut: Vec3, rotation: f32, local: Vec2) -> Vec3 {
    let offset = shared::rotation::local_to_world_xz(local, rotation);
    let x = hut.x + offset.x;
    let z = hut.z + offset.y;
    Vec3::new(x, terrain.get_height(x, z), z)
}

pub(super) fn fishing_deck_points(pier: Vec3, rotation: f32) -> (Vec3, Vec3) {
    const DECK_HEIGHT: f32 = 0.52;
    let deck_start = Vec3::new(pier.x, pier.y + DECK_HEIGHT, pier.z);
    let offset = shared::rotation::local_to_world_xz(Vec2::new(0.0, 6.25), rotation);
    let fish_spot = Vec3::new(pier.x + offset.x, pier.y + DECK_HEIGHT, pier.z + offset.y);
    (deck_start, fish_spot)
}

fn install_fishing_route(
    commands: &mut Commands,
    worker: Entity,
    goal: Vec3,
    waypoints: impl IntoIterator<Item = Vec3>,
    traversal: PierTraversal,
) {
    commands
        .entity(worker)
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            MoveTarget(goal),
            TravelRoute {
                goal,
                waypoints: waypoints
                    .into_iter()
                    .map(|position| RouteWaypoint {
                        position,
                        on_road: false,
                    })
                    .collect(),
                next: 0,
            },
            traversal,
        ));
}

/// Run the visible direct-food loop:
/// hut -> safe side route -> pier -> fish until a carry batch is full -> hut.
/// The fisher deposits only into the private hut store; the Moot porter later
/// collects policy-approved surplus as a separate job.
#[allow(clippy::too_many_arguments)]
pub fn run_fishing_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    world_time: Query<&WorldTime>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    huts: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    piers: Query<
        (
            &FishingPier,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::AttachedTo,
        ),
        Without<CharacterKind>,
    >,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    moot_service_busy: Query<(), Or<(With<MootQueueTicket>, With<MootMealRoutine>)>>,
    mut workers: Query<
        (
            Entity,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut FishingRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = ordinary_workday(clock);

    for (
        worker,
        employment,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_failed,
    ) in workers.iter_mut()
    {
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || moot_service_busy.get(worker).is_ok()
            || road_builder.is_some()
            || door_transit.is_some()
        {
            continue;
        }
        if !intent.is_settled() {
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((hut, hut_position, hut_rotation, building_id, building_of)) = huts.get(routine.hut)
        else {
            commands
                .entity(worker)
                .remove::<FishingRoutine>()
                .remove::<PierTraversal>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if hut.kind != SettlementBuildingKind::FishermansHut || employment.0 != *building_id {
            commands
                .entity(worker)
                .remove::<FishingRoutine>()
                .remove::<PierTraversal>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((pier, pier_position, pier_rotation, attached_to)) = piers.get(routine.pier) else {
            continue;
        };
        let Ok(settlement_id) = halls.get(routine.hall) else {
            continue;
        };
        if attached_to.0 != *building_id || *settlement_id != building_of.0 {
            continue;
        }

        let entrance = hut.kind.entrance_position(hut_position.0, hut_rotation.0);
        let inside = hut
            .kind
            .interior_door_position(hut_position.0, hut_rotation.0);
        let staging = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, -4.45),
        );
        let nets = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, -0.35),
        );
        let rear = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, 2.85),
        );
        let (deck_start, fish_spot) = fishing_deck_points(pier_position.0, pier_rotation.0);
        let traversal = PierTraversal {
            deck_start,
            deck_end: fish_spot,
        };

        if routine.failed_workplace_routes >= MAX_WORKPLACE_ROUTE_FAILURES
            && inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
        {
            commands
                .entity(worker)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .remove::<PierTraversal>();
            *activity = CharacterActivity::Idle;
            if unload_worker_output(&mut inventories, worker, routine.hut, Good::Food) {
                warn!(
                    "Fisher completed a loaded workplace handoff abstractly after {} failed routes",
                    routine.failed_workplace_routes,
                );
                finish_fishing_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
            continue;
        }

        if let Some(failed) = route_failed {
            routine.failed_workplace_routes = routine.failed_workplace_routes.saturating_add(1);
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            let carrying_catch = inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Food) > 0);
            if carrying_catch {
                if routine.failed_workplace_routes == MAX_WORKPLACE_ROUTE_FAILURES {
                    warn!(
                        "Fisher still carries a catch after {} failed workplace routes; retaining it for a bounded last-leg handoff before going off duty",
                        routine.failed_workplace_routes,
                    );
                }
                if routine.failed_workplace_routes < MAX_WORKPLACE_ROUTE_FAILURES {
                    commands.entity(worker).insert(MoveTarget(entrance));
                }
                routine.phase = FishingPhase::ReturningToHut;
                *activity = CharacterActivity::Idle;
            } else if routine.failed_workplace_routes >= MAX_WORKPLACE_ROUTE_FAILURES
                || !workday_active
            {
                warn!(
                    "Fisher could not reach the hut at {:.1},{:.1} after {} routes; ending the empty-handed shift",
                    failed.goal.x,
                    failed.goal.z,
                    routine.failed_workplace_routes,
                );
                finish_fishing_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            } else {
                commands.entity(worker).insert(MoveTarget(entrance));
                routine.phase = FishingPhase::ReturningToHut;
                *activity = CharacterActivity::Idle;
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                FishingPhase::Inside { .. } => {
                    *activity = CharacterActivity::Idle;
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        hut_position.0,
                        entrance,
                        inside,
                        entrance,
                    );
                    routine.phase = FishingPhase::EndingShift;
                    continue;
                }
                FishingPhase::Fishing | FishingPhase::WalkingToPier => {
                    *activity = CharacterActivity::Idle;
                    install_fishing_route(
                        &mut commands,
                        worker,
                        staging,
                        [deck_start, rear, nets, staging],
                        traversal,
                    );
                    routine.phase = FishingPhase::ReturningFromPier { staging };
                    continue;
                }
                FishingPhase::StagingForPier { .. } => {
                    commands
                        .entity(worker)
                        .remove::<PierTraversal>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>();
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                FishingPhase::GoingToHut if ground_distance(position.0, entrance) <= DOOR_REACH => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                    {
                        routine.phase = FishingPhase::ReturningToHut;
                        continue;
                    }
                    finish_fishing_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FishingPhase::EndingShift => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                    {
                        ensure_move_target(&mut commands, worker, move_target, entrance);
                        routine.phase = FishingPhase::ReturningToHut;
                        continue;
                    }
                    finish_fishing_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FishingPhase::GoingToHut
                | FishingPhase::ReturningFromPier { .. }
                | FishingPhase::ReturningToHut => {}
            }
        }

        match routine.phase {
            FishingPhase::GoingToHut => {
                if ground_distance(position.0, entrance) <= DOOR_REACH {
                    routine.failed_workplace_routes = 0;
                    if !workday_active {
                        if inventories
                            .get_mut(worker)
                            .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                        {
                            routine.phase = FishingPhase::ReturningToHut;
                            continue;
                        }
                        finish_fishing_shift(
                            &mut commands,
                            worker,
                            production_day,
                            &routine,
                            &mut activity,
                        );
                        continue;
                    }
                    *activity = CharacterActivity::Idle;
                    begin_workplace_entry(&mut commands, worker, hut_position.0, entrance, inside);
                    routine.phase = FishingPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                }
            }
            FishingPhase::Inside { seconds_left } => {
                *activity = CharacterActivity::Indoors;
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = FishingPhase::Inside { seconds_left: left };
                    continue;
                }
                *activity = CharacterActivity::Idle;
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    hut_position.0,
                    entrance,
                    inside,
                    staging,
                );
                routine.phase = FishingPhase::StagingForPier { staging };
            }
            FishingPhase::StagingForPier { staging } => {
                if ground_distance(position.0, staging) > WORK_REACH {
                    ensure_move_target(&mut commands, worker, move_target, staging);
                    continue;
                }
                routine.failed_workplace_routes = 0;
                install_fishing_route(
                    &mut commands,
                    worker,
                    fish_spot,
                    [nets, rear, deck_start, fish_spot],
                    traversal,
                );
                routine.phase = FishingPhase::WalkingToPier;
            }
            FishingPhase::WalkingToPier => {
                if ground_distance(position.0, fish_spot) <= WORK_REACH {
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<PierTraversal>();
                    let outward = Vec2::new(fish_spot.x - deck_start.x, fish_spot.z - deck_start.z);
                    if outward.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-outward.x, -outward.y);
                    }
                    *activity = CharacterActivity::Fishing;
                    routine.phase = FishingPhase::Fishing;
                }
            }
            FishingPhase::Fishing => {
                *activity = CharacterActivity::Fishing;
                routine.catch_seconds += dt;
                let seconds_per_food = fisher_seconds_per_food(pier.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    let carried = carrier.amount(Good::Food);
                    let needed = FISH_CARRY_BATCH_UNITS.saturating_sub(carried);
                    let batch_seconds = seconds_per_food * needed as f32;
                    if needed > 0 && routine.catch_seconds < batch_seconds {
                        continue;
                    }
                    let produced = carrier.add(Good::Food, needed);
                    if produced > 0 {
                        routine.catch_seconds =
                            (routine.catch_seconds - seconds_per_food * produced as f32).max(0.0);
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    business_events.record_production(production_day, *building_id, produced);
                    economy_runtime.record_food_production(routine.hall, produced);
                    if carrier.amount(Good::Food) < FISH_CARRY_BATCH_UNITS
                        && carrier.free_bulk() >= Good::Food.bulk_per_unit()
                    {
                        continue;
                    }
                }
                *activity = CharacterActivity::Idle;
                install_fishing_route(
                    &mut commands,
                    worker,
                    staging,
                    [deck_start, rear, nets, staging],
                    traversal,
                );
                routine.phase = FishingPhase::ReturningFromPier { staging };
            }
            FishingPhase::ReturningFromPier { staging } => {
                if ground_distance(position.0, staging) > WORK_REACH {
                    continue;
                }
                commands
                    .entity(worker)
                    .remove::<PierTraversal>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .insert(MoveTarget(entrance));
                routine.phase = FishingPhase::ReturningToHut;
            }
            FishingPhase::ReturningToHut => {
                if ground_distance(position.0, entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    continue;
                }
                routine.failed_workplace_routes = 0;
                commands.entity(worker).remove::<MoveTarget>();
                let unloaded =
                    unload_worker_output(&mut inventories, worker, routine.hut, Good::Food);
                if !unloaded {
                    *activity = CharacterActivity::Idle;
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                if !workday_active {
                    finish_fishing_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                begin_workplace_entry(&mut commands, worker, hut_position.0, entrance, inside);
                routine.phase = FishingPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            FishingPhase::EndingShift => {
                if inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                finish_fishing_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Attach a physical work routine to every staffed lumberjack hut.
///
/// `EmployedAt(BuildingId)` is authoritative. The readable name roster is only
/// a compatibility path for buildings loaded before durable relationships.
pub fn assign_lumberjack_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    huts: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&LumberjackWorkProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<ProcessingRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
            Without<strategic::StrategicPerson>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !ordinary_workday(clock) {
        return;
    }
    let mut eligible_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, _, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            eligible_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
    }
    if eligible_by_building.is_empty() {
        return;
    }
    let mut claimed = HashSet::new();
    for (hut_entity, building, hut_position, hut_rotation, building_id, building_of) in huts.iter()
    {
        if building.kind != SettlementBuildingKind::LumberjackHut {
            continue;
        }
        let Some(employees) = eligible_by_building.get(building_id) else {
            continue;
        };
        let Some((hall, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, _, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        if !workplace_road_is_ready(
            hut_entity,
            building.kind,
            hut_position.0,
            hut_rotation.0,
            building_of.0,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            defer_shift_until_workplace_access(&mut commands, employees, clock.day);
            continue;
        }
        for employee in employees.iter().take(building.kind.positions() as usize) {
            let Ok((worker, worker_display_name, intent, _, off_duty, progress, employment)) =
                villagers.get(*employee)
            else {
                continue;
            };
            if claimed.contains(&worker)
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let (cycle, chop_seconds) = progress
                .filter(|progress| progress.hut == hut_entity)
                .map_or((0, 0.0), |progress| (progress.cycle, progress.chop_seconds));
            claimed.insert(worker);
            let entrance = building
                .kind
                .entrance_position(hut_position.0, hut_rotation.0);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<LumberjackWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    LumberjackRoutine {
                        hut: hut_entity,
                        hall,
                        cycle,
                        failed_tree_routes: 0,
                        failed_hut_routes: 0,
                        chop_seconds,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: LumberjackPhase::GoingToHut,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(entrance),
                ));
            info!(
                "Village '{}': {} began working from the lumberjack hut",
                building.settlement, worker_display_name.0
            );
        }
    }
}

/// Run the observed woodcutting loop:
/// hut -> tree -> chop -> carry -> hut, with periodic hauling to the hall.
///
/// Every transfer uses bounded inventories. A full destination leaves the
/// remainder in the source, so congestion is visible as a worker who cannot
/// complete the next leg rather than as deleted resources.
#[allow(clippy::too_many_arguments)]
pub fn run_lumberjack_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    huts: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut tree_candidates: Local<TreeWorkCandidateCache>,
    moot_service_busy: Query<(), Or<(With<MootQueueTicket>, With<MootMealRoutine>)>>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut LumberjackRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = ordinary_workday(clock);

    for (
        worker,
        name,
        employment,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_pending,
        route_failed,
    ) in workers.iter_mut()
    {
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || moot_service_busy.get(worker).is_ok()
            || road_builder.is_some()
            || door_transit.is_some()
        {
            continue;
        }
        // A permit temporarily takes this person's builder time. Construction
        // owns their destination until it releases them back to residency;
        // their ordinary job then resumes from the same physical phase.
        if !intent.is_settled() {
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        }
        let Ok((hut, hut_position, hut_rotation, building_id, building_of)) = huts.get(routine.hut)
        else {
            commands
                .entity(worker)
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        };
        if hut.kind != SettlementBuildingKind::LumberjackHut || employment.0 != *building_id {
            commands
                .entity(worker)
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        }
        let Ok(settlement_id) = halls.get(routine.hall) else {
            continue;
        };
        if *settlement_id != building_of.0 {
            continue;
        }

        let hut_entrance = hut.kind.entrance_position(hut_position.0, hut_rotation.0);
        let hut_inside = hut
            .kind
            .interior_door_position(hut_position.0, hut_rotation.0);

        if routine.failed_hut_routes >= MAX_WORKPLACE_ROUTE_FAILURES
            && inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
        {
            commands
                .entity(worker)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            *activity = CharacterActivity::Idle;
            if unload_worker_output(&mut inventories, worker, routine.hut, Good::Wood) {
                warn!(
                    "Woodcutter {} completed a loaded workplace handoff abstractly after {} failed routes",
                    name.0, routine.failed_hut_routes,
                );
                finish_lumberjack_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
            continue;
        }

        if route_failed.is_some()
            && matches!(
                routine.phase,
                LumberjackPhase::GoingToHut | LumberjackPhase::ReturningToHut
            )
        {
            let failed = route_failed.expect("checked above");
            // A failed route used to be handled only while walking to a tree.
            // A woodcutter carrying Wood home could therefore retain the
            // terminal failure forever and silently disable the business.
            // Clear the terminal navigation state and submit the hut entrance
            // again; the planner can then use updated obstacle geometry or a
            // different A* corridor without losing the carried goods.
            routine.failed_hut_routes = routine.failed_hut_routes.saturating_add(1);
            let carrying_wood = inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0);
            if carrying_wood {
                if routine.failed_hut_routes == MAX_WORKPLACE_ROUTE_FAILURES {
                    warn!(
                        "Woodcutter {} still carries Wood after {} failed hut routes; retaining the load for a bounded last-leg handoff before going off duty",
                        name.0, routine.failed_hut_routes,
                    );
                }
                commands
                    .entity(worker)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>();
                if routine.failed_hut_routes < MAX_WORKPLACE_ROUTE_FAILURES {
                    commands.entity(worker).insert(MoveTarget(hut_entrance));
                }
                routine.phase = LumberjackPhase::ReturningToHut;
                *activity = CharacterActivity::Idle;
            } else if routine.failed_hut_routes >= MAX_WORKPLACE_ROUTE_FAILURES || !workday_active {
                warn!(
                    "Woodcutter {} could not reach the hut at {:.1},{:.1} after {} routes; ending the empty-handed shift",
                    name.0,
                    failed.goal.x,
                    failed.goal.z,
                    routine.failed_hut_routes,
                );
                finish_lumberjack_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            } else {
                commands
                    .entity(worker)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .insert(MoveTarget(hut_entrance));
                *activity = CharacterActivity::Idle;
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                LumberjackPhase::Inside { .. } => {
                    *activity = CharacterActivity::Idle;
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        hut_position.0,
                        hut_entrance,
                        hut_inside,
                        hut_entrance,
                    );
                    routine.phase = LumberjackPhase::EndingShift;
                    continue;
                }
                LumberjackPhase::WalkingToTree { .. } | LumberjackPhase::Chopping => {
                    commands
                        .entity(worker)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    *activity = CharacterActivity::Idle;
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                LumberjackPhase::GoingToHut
                    if ground_distance(position.0, hut_entrance) <= DOOR_REACH =>
                {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                    {
                        routine.phase = LumberjackPhase::ReturningToHut;
                        continue;
                    }
                    finish_lumberjack_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                LumberjackPhase::EndingShift => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                    {
                        ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                        routine.phase = LumberjackPhase::ReturningToHut;
                        continue;
                    }
                    finish_lumberjack_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                LumberjackPhase::GoingToHut | LumberjackPhase::ReturningToHut => {}
            }
        }

        match routine.phase {
            LumberjackPhase::GoingToHut => {
                if ground_distance(position.0, hut_entrance) <= DOOR_REACH {
                    routine.failed_hut_routes = 0;
                    if !workday_active {
                        if inventories
                            .get_mut(worker)
                            .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                        {
                            routine.phase = LumberjackPhase::ReturningToHut;
                            continue;
                        }
                        finish_lumberjack_shift(
                            &mut commands,
                            worker,
                            production_day,
                            &routine,
                            &mut activity,
                        );
                        continue;
                    }
                    *activity = CharacterActivity::Idle;
                    begin_workplace_entry(
                        &mut commands,
                        worker,
                        hut_position.0,
                        hut_entrance,
                        hut_inside,
                    );
                    routine.phase = LumberjackPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                }
            }
            LumberjackPhase::Inside { seconds_left } => {
                if *activity != CharacterActivity::Indoors {
                    *activity = CharacterActivity::Indoors;
                }
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = LumberjackPhase::Inside { seconds_left: left };
                    continue;
                }
                let salt = stable_name_hash(&name.0);
                let (tree, stand) = match find_tree_for_cycle_cached(
                    &mut tree_candidates,
                    &terrain,
                    derived.as_deref(),
                    obstacles.as_deref(),
                    hut_position.0,
                    routine.cycle,
                    salt,
                ) {
                    TreeCandidateLookup::Pending => {
                        // Prop generation is deliberately spread across ticks
                        // to avoid a first-search frame hitch. Remain ready to
                        // leave as soon as the bounded cache finishes.
                        routine.phase = LumberjackPhase::Inside { seconds_left: 0.0 };
                        continue;
                    }
                    TreeCandidateLookup::Unavailable => {
                        // Move through the deterministic pool when a tree has
                        // no collision-free interaction point. Retrying the
                        // same cycle here previously created an indoor loop.
                        routine.cycle = routine.cycle.wrapping_add(1);
                        routine.phase = LumberjackPhase::Inside {
                            seconds_left: INDOOR_REST_SECONDS,
                        };
                        continue;
                    }
                    TreeCandidateLookup::Found { tree, stand } => (tree, stand),
                };
                *activity = CharacterActivity::Idle;
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    hut_position.0,
                    hut_entrance,
                    hut_inside,
                    stand,
                );
                routine.phase = LumberjackPhase::WalkingToTree { tree, stand };
            }
            LumberjackPhase::WalkingToTree { tree, stand } => {
                if let Some(failed) = route_failed {
                    let (cycle, failed_routes, widened) =
                        advance_failed_tree_candidate(routine.cycle, routine.failed_tree_routes);
                    routine.cycle = cycle;
                    routine.failed_tree_routes = failed_routes;
                    if widened {
                        warn!(
                            "Woodcutter {} exhausted twelve tree approaches after {:.1},{:.1}; widening the search instead of stopping",
                            name.0, failed.goal.x, failed.goal.z
                        );
                    }
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>()
                        .insert(MoveTarget(hut_entrance));
                    routine.phase = LumberjackPhase::GoingToHut;
                    *activity = CharacterActivity::Idle;
                    continue;
                }
                if route_pending.is_some_and(NavigationRoutePending::exhausted) {
                    // A coastline or cliff can put the deterministic nearest
                    // tree on a different landmass. Abandon that target and
                    // advance the stable search salt instead of staring at an
                    // exhausted path request forever.
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>();
                    routine.cycle = routine.cycle.wrapping_add(1);
                    routine.phase = LumberjackPhase::GoingToHut;
                    commands.entity(worker).insert(MoveTarget(hut_entrance));
                    *activity = CharacterActivity::Idle;
                    continue;
                }
                if ground_distance(position.0, stand) <= WORK_REACH {
                    routine.failed_tree_routes = 0;
                    commands.entity(worker).remove::<MoveTarget>();
                    let to_tree = tree - position.0;
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.z);
                    }
                    *activity = CharacterActivity::Chopping;
                    routine.phase = LumberjackPhase::Chopping;
                } else {
                    ensure_move_target(&mut commands, worker, move_target, stand);
                }
            }
            LumberjackPhase::Chopping => {
                if *activity != CharacterActivity::Chopping {
                    *activity = CharacterActivity::Chopping;
                }
                routine.chop_seconds += dt;
                let required_seconds = lumber_seconds_per_tree(hut.quality);
                if routine.chop_seconds < required_seconds {
                    continue;
                }
                let yield_units = lumber_tree_yield(hut.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    let produced = carrier.add(Good::Wood, yield_units);
                    if produced > 0 {
                        routine.chop_seconds = (routine.chop_seconds - required_seconds).max(0.0);
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    business_events.record_production(production_day, *building_id, produced);
                }
                routine.cycle = routine.cycle.wrapping_add(1);
                *activity = CharacterActivity::Idle;
                commands.entity(worker).insert(MoveTarget(hut_entrance));
                routine.phase = LumberjackPhase::ReturningToHut;
            }
            LumberjackPhase::ReturningToHut => {
                if ground_distance(position.0, hut_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    continue;
                }
                routine.failed_hut_routes = 0;
                commands.entity(worker).remove::<MoveTarget>();
                let unloaded =
                    unload_worker_output(&mut inventories, worker, routine.hut, Good::Wood);
                if !unloaded {
                    *activity = CharacterActivity::Idle;
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                if !workday_active {
                    finish_lumberjack_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                *activity = CharacterActivity::Idle;
                begin_workplace_entry(
                    &mut commands,
                    worker,
                    hut_position.0,
                    hut_entrance,
                    hut_inside,
                );
                routine.phase = LumberjackPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            LumberjackPhase::EndingShift => {
                if inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                finish_lumberjack_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Keep the small replicated carried-load view in sync with private inventory.
pub fn sync_carried_load(
    mut carriers: Query<(&GoodsInventory, &mut CarriedLoad), With<CharacterKind>>,
) {
    for (inventory, mut carried) in carriers.iter_mut() {
        let next = CarriedLoad::from_inventory(inventory);
        if *carried != next {
            *carried = next;
        }
    }
}

/// Pick a real deterministic tree prop and a collision-free chopping point.
///
/// Tree removal/regrowth needs stable resource-node ids and a sparse depletion
/// map. Until that layer exists, the worker uses the same authored tree the
/// client already draws but does not yet remove it after harvesting.
pub(super) fn advance_failed_tree_candidate(
    mut cycle: u32,
    mut failed_routes: u8,
) -> (u32, u8, bool) {
    failed_routes = failed_routes.saturating_add(1);
    cycle = cycle.wrapping_add(1);
    if failed_routes < 12 {
        return (cycle, failed_routes, false);
    }
    // `cycle` is the deterministic search salt. Jump farther after a whole
    // failed batch so the next search does not immediately revisit the same
    // local candidate set. Resetting the diagnostic counter keeps failure
    // bounded without turning twelve bad trees into a terminal AI state.
    cycle = cycle.wrapping_add(12);
    failed_routes = 0;
    (cycle, failed_routes, true)
}

fn timber_retry_delay(failures: u8) -> f64 {
    let exponent = u32::from(failures.saturating_sub(1)).min(8);
    (TIMBER_RETRY_BASE_SECONDS * 2_f64.powi(exponent as i32)).min(TIMBER_RETRY_MAX_SECONDS)
}

pub(super) fn postpone_construction_tree_search(
    routine: &mut ConstructionMaterialRoutine,
    now: f64,
) -> bool {
    let retry_failures = routine.failed_tree_routes.saturating_add(1);
    let (cycle, failed_routes, widened) =
        advance_failed_tree_candidate(routine.cycle, routine.failed_tree_routes);
    routine.cycle = cycle;
    routine.failed_tree_routes = failed_routes;
    routine.tree_retry_after = now + timber_retry_delay(retry_failures);
    routine.phase = ConstructionMaterialPhase::Seeking;
    widened
}

pub(super) fn postpone_construction_store_route(
    routine: &mut ConstructionMaterialRoutine,
    now: f64,
) {
    routine.failed_store_routes = routine.failed_store_routes.saturating_add(1);
    routine.store_retry_after = now + timber_retry_delay(routine.failed_store_routes);
    routine.phase = ConstructionMaterialPhase::Seeking;
}

pub(super) const TREE_APPROACH_ANGLES: [f32; 8] = [
    0.0,
    std::f32::consts::FRAC_PI_4,
    -std::f32::consts::FRAC_PI_4,
    std::f32::consts::FRAC_PI_2,
    -std::f32::consts::FRAC_PI_2,
    3.0 * std::f32::consts::FRAC_PI_4,
    -3.0 * std::f32::consts::FRAC_PI_4,
    std::f32::consts::PI,
];

/// A failed route must not recreate the same interaction goal on the next
/// visit. `cycle` advances once per failed goal; after the routine has tried
/// every candidate tree, the preferred side of each trunk rotates as well.
pub(super) fn tree_approach_start(cycle: u32, choice_count: usize) -> usize {
    (cycle as usize / choice_count.max(1)) % TREE_APPROACH_ANGLES.len()
}

#[derive(Default)]
struct TreeWorkCandidates {
    spawns: Vec<shared::props::PropSpawn>,
    trees: Vec<usize>,
    blockers: Vec<TreeWorkBlocker>,
    blocker_cells: HashMap<(i32, i32), Vec<usize>>,
    max_blocker_radius: f32,
    pending_chunks: Vec<ChunkCoord>,
    complete: bool,
}

#[derive(Clone, Copy)]
struct TreeWorkBlocker {
    spawn_index: usize,
    center: Vec2,
    radius: f32,
}

const TREE_BLOCKER_CELL_SIZE: f32 = 8.0;

fn tree_blocker_cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / TREE_BLOCKER_CELL_SIZE).floor() as i32,
        (point.y / TREE_BLOCKER_CELL_SIZE).floor() as i32,
    )
}

impl TreeWorkCandidates {
    fn pending(hut: Vec3) -> Self {
        Self {
            pending_chunks: ChunkCoord::from_world_pos(hut).chunks_in_radius(2),
            ..default()
        }
    }

    fn collect(
        terrain: &WorldTerrain,
        hut: Vec3,
        derived: Option<&DerivedColliderLibrary>,
    ) -> Self {
        let mut candidates = Self::pending(hut);
        candidates.advance(terrain, hut, derived, usize::MAX);
        candidates
    }

    fn advance(
        &mut self,
        terrain: &WorldTerrain,
        hut: Vec3,
        derived: Option<&DerivedColliderLibrary>,
        chunk_budget: usize,
    ) {
        if self.complete {
            return;
        }
        for _ in 0..chunk_budget {
            let Some(chunk) = self.pending_chunks.pop() else {
                break;
            };
            self.spawns
                .extend(shared::props::generate_chunk_prop_spawns(
                    &terrain.generator,
                    chunk,
                ));
        }
        if !self.pending_chunks.is_empty() {
            return;
        }

        self.trees = self
            .spawns
            .iter()
            .enumerate()
            .filter(|(_, spawn)| spawn.kind.is_some_and(|kind| kind.is_tree()))
            .filter(|(_, spawn)| {
                let distance =
                    Vec2::new(spawn.position.x - hut.x, spawn.position.z - hut.z).length();
                (TREE_MIN_DISTANCE..=TREE_MAX_DISTANCE).contains(&distance)
            })
            .map(|(index, _)| index)
            .collect();
        self.trees.sort_by(|a, b| {
            let a = self.spawns[*a].position;
            let b = self.spawns[*b].position;
            a.distance_squared(hut)
                .total_cmp(&b.distance_squared(hut))
                .then_with(|| a.x.total_cmp(&b.x))
                .then_with(|| a.z.total_cmp(&b.z))
        });

        let mut blockers = Vec::new();
        let mut blocker_cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut max_blocker_radius = 0.0_f32;
        for (spawn_index, spawn) in self.spawns.iter().enumerate() {
            let Some(kind) = spawn.kind.filter(|kind| kind.blocks_village_road()) else {
                continue;
            };
            let radius = derived
                .and_then(|library| library.by_kind.get(&kind))
                .map_or(0.75, |shape| shape.horizontal_radius)
                * spawn.scale
                + VILLAGER_PROP_RADIUS;
            let blocker_index = blockers.len();
            let center = Vec2::new(spawn.position.x, spawn.position.z);
            blockers.push(TreeWorkBlocker {
                spawn_index,
                center,
                radius,
            });
            blocker_cells
                .entry(tree_blocker_cell(center))
                .or_default()
                .push(blocker_index);
            max_blocker_radius = max_blocker_radius.max(radius);
        }
        self.blockers = blockers;
        self.blocker_cells = blocker_cells;
        self.max_blocker_radius = max_blocker_radius;
        self.complete = true;
    }

    fn stand_overlaps_prop(&self, stand: Vec2, tree_index: usize) -> bool {
        let origin = tree_blocker_cell(stand);
        let cell_radius = (self.max_blocker_radius / TREE_BLOCKER_CELL_SIZE).ceil() as i32 + 1;
        for cell_x in (origin.0 - cell_radius)..=(origin.0 + cell_radius) {
            for cell_z in (origin.1 - cell_radius)..=(origin.1 + cell_radius) {
                let Some(indices) = self.blocker_cells.get(&(cell_x, cell_z)) else {
                    continue;
                };
                for index in indices {
                    let blocker = self.blockers[*index];
                    if blocker.spawn_index != tree_index
                        && blocker.center.distance_squared(stand) < blocker.radius * blocker.radius
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Generated prop candidates are immutable until the future depletion layer
/// exists. Keep the expensive 5x5-chunk enumeration per workplace origin
/// instead of regenerating every tree in those chunks for every retry tick.
#[derive(Default)]
pub(crate) struct TreeWorkCandidateCache {
    by_origin: HashMap<ChunkCoord, TreeWorkCandidates>,
}

impl TreeWorkCandidateCache {
    fn candidates<'a>(
        &'a mut self,
        terrain: &WorldTerrain,
        derived: Option<&DerivedColliderLibrary>,
        hut: Vec3,
    ) -> Option<&'a TreeWorkCandidates> {
        const CHUNKS_PER_SEARCH_TICK: usize = 1;
        const MAX_CACHED_ORIGINS: usize = 512;
        let origin = ChunkCoord::from_world_pos(hut);
        if self.by_origin.len() >= MAX_CACHED_ORIGINS && !self.by_origin.contains_key(&origin) {
            self.by_origin.clear();
        }
        let candidates = self
            .by_origin
            .entry(origin)
            .or_insert_with(|| TreeWorkCandidates::pending(hut));
        candidates.advance(terrain, hut, derived, CHUNKS_PER_SEARCH_TICK);
        candidates.complete.then_some(candidates)
    }
}

pub(super) enum TreeCandidateLookup {
    Pending,
    Unavailable,
    Found { tree: Vec3, stand: Vec3 },
}

fn find_tree_in_candidates(
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    salt: u32,
    candidates: &TreeWorkCandidates,
) -> Option<(Vec3, Vec3)> {
    // Keep a comfortably large deterministic candidate pool. Sparse coastal
    // groves may only contain two usable trees, while a dense forest should
    // not trap all workers on the same nearest twelve trunks.
    let choice_count = candidates.trees.len().min(48);
    if choice_count == 0 {
        return None;
    }
    let tree_index = candidates.trees[(cycle.wrapping_add(salt) as usize) % choice_count];
    let tree_spawn = &candidates.spawns[tree_index];
    let tree = tree_spawn.position;
    let toward_hut = Vec2::new(hut.x - tree.x, hut.z - tree.z).normalize_or(Vec2::Y);
    let tree_radius = tree_spawn
        .kind
        .and_then(|kind| derived.and_then(|library| library.by_kind.get(&kind)))
        .map_or(0.75, |shape| shape.horizontal_radius)
        * tree_spawn.scale;
    let stand_distance = (tree_radius + VILLAGER_PROP_RADIUS + 0.25).max(2.4);

    // The natural first choice faces back toward the workplace. If its route
    // fails, the next pass starts on a different side of the trunk instead of
    // returning the identical locally-clear but globally-unreachable goal.
    // The ordering remains deterministic for reproducible simulation runs.
    let approach_start = tree_approach_start(cycle, choice_count);
    for step in 0..TREE_APPROACH_ANGLES.len() {
        let angle = TREE_APPROACH_ANGLES[(approach_start + step) % TREE_APPROACH_ANGLES.len()];
        let (sin, cos) = angle.sin_cos();
        let direction = Vec2::new(
            toward_hut.x * cos - toward_hut.y * sin,
            toward_hut.x * sin + toward_hut.y * cos,
        );
        let stand_xz = Vec2::new(tree.x, tree.z) + direction * stand_distance;
        if obstacles.is_some_and(|grid| grid.point_blocked(stand_xz)) {
            continue;
        }
        if candidates.stand_overlaps_prop(stand_xz, tree_index) {
            continue;
        }
        let stand = Vec3::new(
            stand_xz.x,
            terrain.get_height(stand_xz.x, stand_xz.y),
            stand_xz.y,
        );
        return Some((tree, stand));
    }
    None
}

pub(super) fn find_tree_for_cycle_cached(
    cache: &mut TreeWorkCandidateCache,
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    salt: u32,
) -> TreeCandidateLookup {
    let Some(candidates) = cache.candidates(terrain, derived, hut) else {
        return TreeCandidateLookup::Pending;
    };
    match find_tree_in_candidates(terrain, derived, obstacles, hut, cycle, salt, candidates) {
        Some((tree, stand)) => TreeCandidateLookup::Found { tree, stand },
        None => TreeCandidateLookup::Unavailable,
    }
}

/// Permit-time proof that a lumber workplace has at least one usable tree on
/// its own walkable landmass. Resource density alone is insufficient on a
/// coast: a tree 30 metres away across a channel is not timber supply.
///
/// Candidate props are generated once, and only the nearest bounded set pays a
/// terrain-only route check. This runs when the plot decision changes, not per
/// villager or simulation tick.
pub(crate) fn lumber_plot_has_reachable_tree(terrain: &WorldTerrain, hut: Vec3) -> bool {
    const PERMIT_TREE_CANDIDATES: usize = 12;
    let candidates = TreeWorkCandidates::collect(terrain, hut, None);
    let attempts = candidates.trees.len().min(PERMIT_TREE_CANDIDATES);
    (0..attempts).any(|cycle| {
        find_tree_in_candidates(terrain, None, None, hut, cycle as u32, 0, &candidates).is_some_and(
            |(_, stand)| {
                crate::world::village_roads::embodied_land_route_exists(terrain, hut, stand)
            },
        )
    })
}

pub(super) fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

fn farm_work_stand(
    field: Vec3,
    rotation: f32,
    worker_salt: u32,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<Vec3> {
    let side = if worker_salt & 1 == 0 { -1.5 } else { 1.5 };
    let candidates = [
        Vec2::new(side, 0.0),
        Vec2::new(-side, 0.0),
        Vec2::new(0.0, 2.0),
        Vec2::new(0.0, -2.0),
        Vec2::new(side, 2.5),
        Vec2::new(-side, 2.5),
        Vec2::new(side, -2.5),
        Vec2::new(-side, -2.5),
    ];
    candidates.into_iter().find_map(|local| {
        let offset = shared::rotation::local_to_world_xz(local, rotation);
        let point = Vec2::new(field.x + offset.x, field.z + offset.y);
        if obstacles.is_some_and(|obstacles| obstacles.point_blocked(point)) {
            return None;
        }
        if colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::navigation_point_is_clear_of_props(
                point, colliders, derived,
            )
        }) {
            return None;
        }
        Some(Vec3::new(point.x, field.y, point.y))
    })
}

pub(super) fn build_clip_facing(to_work: Vec3) -> f32 {
    f32::atan2(-to_work.x, -to_work.z)
}

/// A collision-safe point beyond an authored threshold.
///
/// The door anchor sits only a few centimetres outside some inflated building
/// footprints. Stopping merely within `DOOR_REACH` of that anchor can therefore
/// leave an actor inside the blocker forever. Door-controlled movement may
/// cross the wall, so finish the crossing a short distance beyond the anchor
/// before ordinary navigation takes ownership again.
pub(super) fn exterior_door_clearance_position(building: Vec3, door: Vec3) -> Vec3 {
    let outward = Vec2::new(door.x - building.x, door.z - building.z).normalize_or_zero();
    Vec3::new(
        door.x + outward.x * (DOOR_REACH + 0.35),
        door.y,
        door.z + outward.y * (DOOR_REACH + 0.35),
    )
}

pub(super) fn ordinary_workday(clock: &WorldTime) -> bool {
    clock.is_ordinary_work_time()
}

/// Move one trade's physical output from its worker into the owning workplace.
/// `false` means cargo remains (normally because finite workplace storage is
/// full), so callers must retain the work routine rather than clocking off.
fn unload_worker_output(
    inventories: &mut Query<&mut GoodsInventory>,
    worker: Entity,
    workplace: Entity,
    good: Good,
) -> bool {
    let Ok([mut carrier, mut store]) = inventories.get_many_mut([worker, workplace]) else {
        return false;
    };
    carrier.transfer_to(&mut store, good, u32::MAX);
    carrier.amount(good) == 0
}

/// A completed workplace can advertise positions one schedule pass before its
/// connector joins the public road graph. Those employees still have a real
/// job, but they cannot begin an embodied shift yet. Release them to ordinary
/// household/ambient behaviour for this day instead of leaving the whole
/// roster motionless outside their homes until the connector finishes.
fn defer_shift_until_workplace_access(commands: &mut Commands, employees: &[Entity], day: u32) {
    for employee in employees {
        commands.entity(*employee).insert(WorkerOffDuty { day });
    }
}

fn finish_farmer_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &FarmerRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<FarmerRoutine>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            FarmerHarvestProgress {
                farmstead: routine.farmstead,
                field: routine.field,
                seconds: routine.harvest_seconds,
            },
        ));
}

fn finish_fishing_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &FishingRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<FishingRoutine>()
        .remove::<PierTraversal>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            FishingWorkProgress {
                hut: routine.hut,
                pier: routine.pier,
                seconds: routine.catch_seconds,
            },
        ));
}

fn finish_lumberjack_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &LumberjackRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<LumberjackRoutine>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            LumberjackWorkProgress {
                hut: routine.hut,
                cycle: routine.cycle,
                chop_seconds: routine.chop_seconds,
            },
        ));
}
