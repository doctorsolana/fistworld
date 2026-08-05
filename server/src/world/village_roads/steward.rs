//! Civic road staffing, inspection and repair adoption.
//!
//! CivicEmployment and settlement IDs own the office; names are maintained
//! only as readable panel data and log labels.

use super::*;

/// Add the Moot Hall's first public office without bloating the founding path.
pub fn ensure_moot_administrations(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    halls: Query<
        (
            Entity,
            Option<&MootAdministration>,
            Option<&MootAdministrationRuntime>,
        ),
        With<Settlement>,
    >,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (hall, administration, runtime) in halls.iter() {
        let mut entity = commands.entity(hall);
        if administration.is_none() {
            entity.insert(MootAdministration::default());
        }
        if runtime.is_none() {
            entity.insert(MootAdministrationRuntime {
                last_paid_day: day,
                last_audit_at: None,
                road_progress: HashMap::new(),
            });
        }
    }
}

/// Reserve one resident for civic road work and pay their daily public wage.
///
/// Unpaid wages remain explicit arrears when the treasury is short. Coin is
/// transferred directly from the public treasury to the worker's wallet, so
/// the salary neither creates money nor drains a commodity market pool.
pub fn staff_and_pay_road_stewards(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut halls: Query<(
        Entity,
        &mut Settlement,
        &mut MootAdministration,
        &mut MootAdministrationRuntime,
        &shared::components::SettlementId,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        &mut Wallet,
        Option<&RoadSteward>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
    )>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    for (hall, mut settlement, mut administration, mut runtime, settlement_id) in halls.iter_mut() {
        let current = villagers
            .iter()
            .filter(|(_, _, _, intent, _, _, _, _, civic_job)| {
                intent.settlement() == Some(hall)
                    && intent.counts_as_resident()
                    && civic_job.is_some_and(|job| {
                        job.settlement == *settlement_id
                            && job.role == shared::components::CivicRole::RoadSteward
                    })
            })
            .min_by_key(|(_, person_id, ..)| **person_id)
            .map(|(entity, ..)| entity);

        if current.is_none() {
            administration.road_steward = None;
            administration.wage_arrears = 0;
            runtime.last_paid_day = day;
        }

        let worker = if let Some(worker) = current {
            worker
        } else {
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, _, steward, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && steward.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, ..)| entity);
            let Some(candidate) = candidate else {
                runtime.last_paid_day = day;
                continue;
            };
            let Ok((_, _, name, _, mut occupation, _, _, _, _)) = villagers.get_mut(candidate)
            else {
                continue;
            };
            occupation.0 = Some("Road Steward".to_string());
            administration.road_steward = Some(name.0.clone());
            administration.road_steward_daily_salary = ROAD_STEWARD_DAILY_SALARY;
            administration.wage_arrears = 0;
            runtime.last_paid_day = day;
            runtime.last_audit_at = None;
            commands
                .entity(candidate)
                .insert(RoadSteward { settlement: hall });
            commands
                .entity(candidate)
                .insert(shared::components::CivicEmployment {
                    settlement: *settlement_id,
                    role: shared::components::CivicRole::RoadSteward,
                });
            info!(
                "Village '{}': {} took the public Road Steward position at the Moot Hall",
                settlement.name, name.0
            );
            candidate
        };

        if let Ok((_, _, name, _, mut occupation, _, steward, _, civic_job)) =
            villagers.get_mut(worker)
        {
            administration.road_steward = Some(name.0.clone());
            if occupation.0.as_deref() != Some("Road Steward") {
                occupation.0 = Some("Road Steward".to_string());
            }
            if steward.is_none_or(|steward| steward.settlement != hall) {
                commands
                    .entity(worker)
                    .insert(RoadSteward { settlement: hall });
            }
            if civic_job.is_none_or(|job| {
                job.settlement != *settlement_id
                    || job.role != shared::components::CivicRole::RoadSteward
            }) {
                commands
                    .entity(worker)
                    .insert(shared::components::CivicEmployment {
                        settlement: *settlement_id,
                        role: shared::components::CivicRole::RoadSteward,
                    });
            }
        }

        let elapsed_days = day.saturating_sub(runtime.last_paid_day);
        if elapsed_days > 0 {
            administration.wage_arrears = administration.wage_arrears.saturating_add(
                administration
                    .road_steward_daily_salary
                    .saturating_mul(u64::from(elapsed_days)),
            );
            runtime.last_paid_day = day;
        }
        let payment = settlement.treasury.min(administration.wage_arrears);
        if payment > 0 {
            if let Ok((_, _, name, _, _, mut wallet, _, _, _)) = villagers.get_mut(worker) {
                settlement.treasury -= payment;
                administration.wage_arrears -= payment;
                wallet.credit(payment);
                info!(
                    "Village '{}': paid {} {:.2} coin in Road Steward wages{}",
                    settlement.name,
                    name.0,
                    payment as f64 / 100.0,
                    if administration.wage_arrears > 0 {
                        " (some wages remain in arrears)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
}

/// Fill the tier-bounded public roster with named residents.
///
/// Guards are intentionally jobs before they are combat AI: the vacancy is
/// visible, it consumes one person's time, and later patrol behaviour can be
/// attached without changing the settlement model. The first city worker is
/// the existing accountable Road Steward.
pub fn staff_public_positions(
    mut commands: Commands,
    mut halls: Query<(
        Entity,
        &Settlement,
        &mut MootAdministration,
        &shared::components::SettlementId,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        Option<&RoadSteward>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
    )>,
) {
    for (hall, settlement, mut administration, settlement_id) in halls.iter_mut() {
        let mut city_workers: Vec<_> = villagers
            .iter()
            .filter_map(|(entity, person_id, name, intent, _, _, _, civic_job)| {
                let job = civic_job?;
                (intent.settlement() == Some(hall)
                    && intent.counts_as_resident()
                    && job.settlement == *settlement_id
                    && matches!(
                        job.role,
                        shared::components::CivicRole::RoadSteward
                            | shared::components::CivicRole::CityWorker
                    ))
                .then_some((
                    entity,
                    *person_id,
                    name.0.clone(),
                    job.role == shared::components::CivicRole::RoadSteward,
                ))
            })
            .collect();
        city_workers.sort_by_key(|(_, person_id, _, is_steward)| (!*is_steward, *person_id));
        let mut guards: Vec<_> = villagers
            .iter()
            .filter_map(|(entity, person_id, name, intent, _, _, _, civic_job)| {
                let job = civic_job?;
                (intent.settlement() == Some(hall)
                    && intent.counts_as_resident()
                    && job.settlement == *settlement_id
                    && job.role == shared::components::CivicRole::Guard)
                    .then_some((entity, *person_id, name.0.clone()))
            })
            .collect();
        guards.sort_by_key(|(_, person_id, _)| *person_id);

        let desired_workers = usize::from(settlement.tier.public_worker_positions());
        let desired_guards = usize::from(settlement.tier.public_guard_positions());
        // A settlement may advertise jobs it cannot yet fill. Do not add
        // further civic hires past population minus one; vacancies are safer
        // than letting a tiny foundation consume every new arrival at the hall.
        let staffing_budget = settlement.residents.saturating_sub(1) as usize;
        for (entity, ..) in city_workers.drain(desired_workers.min(city_workers.len())..) {
            commands
                .entity(entity)
                .remove::<shared::components::CivicEmployment>();
        }
        for (entity, ..) in guards.drain(desired_guards.min(guards.len())..) {
            commands
                .entity(entity)
                .remove::<shared::components::CivicEmployment>();
        }

        while city_workers.len() < desired_workers
            && city_workers.len() + guards.len() < staffing_budget
        {
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, _, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, person_id, name, ..)| (entity, *person_id, name.0.clone()));
            let Some((entity, person_id, name)) = candidate else {
                break;
            };
            if let Ok((_, _, _, _, mut occupation, steward, _, _)) = villagers.get_mut(entity) {
                if steward.is_none() {
                    occupation.0 = Some("City Worker".to_string());
                    commands.entity(entity).insert(WorkStatus::Employed);
                    commands
                        .entity(entity)
                        .insert(shared::components::CivicEmployment {
                            settlement: *settlement_id,
                            role: shared::components::CivicRole::CityWorker,
                        });
                }
            }
            city_workers.push((entity, person_id, name, false));
        }

        while guards.len() < desired_guards && city_workers.len() + guards.len() < staffing_budget {
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, _, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, person_id, name, ..)| (entity, *person_id, name.0.clone()));
            let Some((entity, person_id, name)) = candidate else {
                break;
            };
            if let Ok((_, _, _, _, mut occupation, _, _, _)) = villagers.get_mut(entity) {
                occupation.0 = Some("Town Guard".to_string());
                commands.entity(entity).insert(WorkStatus::Employed);
                commands
                    .entity(entity)
                    .insert(shared::components::CivicEmployment {
                        settlement: *settlement_id,
                        role: shared::components::CivicRole::Guard,
                    });
            }
            guards.push((entity, person_id, name));
        }

        let city_worker_names: Vec<_> = city_workers
            .into_iter()
            .map(|(_, _, name, _)| name)
            .collect();
        let guard_names: Vec<_> = guards.into_iter().map(|(_, _, name)| name).collect();
        if administration.city_workers != city_worker_names {
            administration.city_workers = city_worker_names;
        }
        if administration.guards != guard_names {
            administration.guards = guard_names;
        }
    }
}

/// Periodically audit every completed building against the hall-connected road
/// component and adopt one repair at a time.
///
/// Wages remain daily, but civic safety is not a once-per-day lottery. An
/// unfinished road only counts as active pending work while a live builder
/// still owns its routine; abandoned ribbons are removed before classification
/// so they can never hide a roadless building forever.
#[allow(clippy::too_many_arguments)]
pub fn audit_village_roads(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut halls: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        &shared::components::SettlementId,
        &mut MootAdministration,
        &mut MootAdministrationRuntime,
    )>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingOf,
        Option<&RoadRequest>,
    )>,
    roads: Query<(Entity, &VillageRoad, &shared::components::RoadOf)>,
    road_workers: Query<(
        Entity,
        &VillagerIntent,
        Option<&RoadBuilderRoutine>,
        &PlayerPosition,
        Has<HomeRoutine>,
    )>,
    stewards: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &RoadSteward,
            &shared::components::CivicEmployment,
            Option<&RoadBuilderRoutine>,
        ),
        Without<crate::world::village::strategic::StrategicPerson>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let now = absolute_world_seconds(clock);
    let road_owners: HashMap<_, _> = road_workers
        .iter()
        .filter_map(|(builder, intent, routine, position, at_home)| {
            let routine = routine?;
            let owns_current_intent = matches!(
                intent,
                VillagerIntent::RoadBuilding { road, .. } if *road == routine.road
            );
            Some((
                routine.road,
                (
                    builder,
                    Vec2::new(position.0.x, position.0.z),
                    at_home,
                    owns_current_intent,
                ),
            ))
        })
        .collect();

    for (
        hall,
        settlement,
        hall_position,
        hall_rotation,
        settlement_id,
        mut administration,
        mut runtime,
    ) in halls.iter_mut()
    {
        if runtime
            .last_audit_at
            .is_some_and(|last| now - last < ROAD_AUDIT_INTERVAL_SECONDS)
        {
            continue;
        }
        let Some((steward, steward_name, intent, _, _, road_work)) =
            stewards
                .iter()
                .find(|(_, _, intent, steward, civic_job, _)| {
                    steward.settlement == hall
                        && intent.settlement() == Some(hall)
                        && civic_job.settlement == *settlement_id
                        && civic_job.role == shared::components::CivicRole::RoadSteward
                })
        else {
            continue;
        };
        if road_work.is_some() || !intent.is_settled() {
            continue;
        }

        let mut abandoned_roads = 0usize;
        let mut stalled_roads = 0usize;
        let mut observed_roads = HashSet::new();
        let mut settlement_roads = Vec::new();
        for (entity, road, road_of) in roads.iter() {
            if road_of.0 != *settlement_id {
                continue;
            }
            if road.is_complete() {
                runtime.road_progress.remove(&entity);
                settlement_roads.push(road);
                continue;
            }

            let Some((builder, builder_position, at_home, owns_current_intent)) =
                road_owners.get(&entity).copied()
            else {
                commands.entity(entity).despawn();
                runtime.road_progress.remove(&entity);
                abandoned_roads += 1;
                continue;
            };
            if !owns_current_intent {
                // An old road task must never survive after another system has
                // legitimately given the actor a newer commitment.
                commands.entity(entity).despawn();
                commands.entity(builder).remove::<RoadBuilderRoutine>();
                runtime.road_progress.remove(&entity);
                abandoned_roads += 1;
                continue;
            }

            let observation =
                runtime
                    .road_progress
                    .entry(entity)
                    .or_insert(RoadProgressObservation {
                        built_through: road.built_through,
                        builder_position,
                        last_progress_at: now,
                    });
            let made_progress = observation.built_through != road.built_through
                || observation
                    .builder_position
                    .distance_squared(builder_position)
                    > 0.5f32.powi(2);
            if made_progress || !clock.is_day() {
                *observation = RoadProgressObservation {
                    built_through: road.built_through,
                    builder_position,
                    last_progress_at: now,
                };
            }
            let stalled =
                clock.is_day() && now - observation.last_progress_at >= ROAD_BUILDER_STALL_SECONDS;
            if stalled {
                commands.entity(entity).despawn();
                let mut builder_commands = commands.entity(builder);
                builder_commands
                    .insert(VillagerIntent::Resident { settlement: hall })
                    .remove::<RoadBuilderRoutine>();
                if !at_home {
                    builder_commands
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                runtime.road_progress.remove(&entity);
                stalled_roads += 1;
                continue;
            }
            observed_roads.insert(entity);
            settlement_roads.push(road);
        }
        runtime
            .road_progress
            .retain(|road, _| observed_roads.contains(road));
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
        let network = hall_road_network(hall_door, &settlement_roads);
        let mut roadless = 0usize;
        let mut disconnected = 0usize;
        let mut pending = 0usize;
        let mut repairs = Vec::new();
        for (building_entity, building, position, rotation, _building_of, request) in buildings
            .iter()
            .filter(|(_, _, _, _, building_of, _)| building_of.0 == *settlement_id)
        {
            let door3 = building.kind.entrance_position(position.0, rotation.0);
            let request_is_active = request.is_some_and(|request| {
                road_workers
                    .get(request.builder)
                    .is_ok_and(|(_, intent, routine, _, _)| {
                        routine.is_none()
                            && (matches!(
                                intent,
                                VillagerIntent::Resident { settlement }
                                    if *settlement == hall
                            ) || matches!(
                                intent,
                                VillagerIntent::Building { settlement, site }
                                    if *settlement == hall && *site == request.completed_site
                            ))
                    })
            });
            let status = building_road_status(
                Vec2::new(door3.x, door3.z),
                &settlement_roads,
                &network,
                request_is_active,
            );
            match status {
                BuildingRoadStatus::Disconnected => {
                    disconnected += 1;
                    repairs.push((0u8, building_entity, building.kind));
                }
                BuildingRoadStatus::Roadless => {
                    roadless += 1;
                    repairs.push((1u8, building_entity, building.kind));
                }
                BuildingRoadStatus::Pending => pending += 1,
                BuildingRoadStatus::Connected => {}
            }
        }
        administration.roadless_buildings = roadless.min(u16::MAX as usize) as u16;
        administration.disconnected_buildings = disconnected.min(u16::MAX as usize) as u16;
        administration.pending_road_buildings = pending.min(u16::MAX as usize) as u16;
        administration.last_road_audit_day = day;
        runtime.last_audit_at = Some(now);

        if abandoned_roads > 0 {
            warn!(
                "Village '{}': Road Steward {} cleared {} abandoned unfinished road(s)",
                settlement.name, steward_name.0, abandoned_roads
            );
        }
        if stalled_roads > 0 {
            warn!(
                "Village '{}': Road Steward {} reclaimed {} connector(s) with no daylight progress for {:.0} world seconds",
                settlement.name,
                steward_name.0,
                stalled_roads,
                ROAD_BUILDER_STALL_SECONDS,
            );
        }

        repairs.sort_unstable_by_key(|(priority, entity, _)| (*priority, entity.to_bits()));
        if let Some((_, building, kind)) = repairs.first().copied() {
            commands.entity(building).insert(RoadRequest {
                builder: steward,
                settlement: hall,
                completed_site: building,
                attempt: 0,
            });
            info!(
                "Village '{}': Road Steward {} found {} roadless, {} disconnected and {} actively pending building(s) across {} detached road component(s); repairing the {}",
                settlement.name,
                steward_name.0,
                roadless,
                disconnected,
                pending,
                network.disconnected_components,
                kind.label(),
            );
        } else if pending > 0 {
            info!(
                "Village '{}': Road Steward {} found every completed connector healthy, with {} building connector(s) still actively under construction",
                settlement.name, steward_name.0, pending
            );
        } else {
            info!(
                "Village '{}': Road Steward {} completed the road audit; every building reaches the Moot Hall",
                settlement.name, steward_name.0
            );
        }
    }
}
