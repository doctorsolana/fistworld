//! Civic road staffing, inspection and repair adoption.
//!
//! CivicEmployment and settlement IDs own the office; names are maintained
//! only as readable panel data and log labels.

use super::*;

/// A second cart is worthwhile at 240 bulk of saleable workplace stock. Keep
/// this economic threshold independent of personal cargo tuning: increasing
/// everyone's carrying capacity should improve clearance time, not silently
/// postpone municipal hiring during the same production backlog.
const SECOND_STEWARD_HIRE_BACKLOG_BULK: u32 = 240;
/// Once hired for a temporary surge, retain the second steward until the
/// backlog is below 72 bulk so staffing does not oscillate every collection.
const SECOND_STEWARD_RELEASE_BACKLOG_BULK: u32 = 72;
/// A Hamlet this large benefits from two permanent carts even during a brief
/// quiet market interval.
const SECOND_STEWARD_RESIDENTS: u32 = 24;

/// Add the Moot Hall's first public office without bloating the founding path.
pub fn ensure_moot_administrations(
    mut commands: Commands,
    halls: Query<
        (
            Entity,
            Option<&MootAdministration>,
            Option<&MootAdministrationRuntime>,
        ),
        With<Settlement>,
    >,
) {
    for (hall, administration, runtime) in halls.iter() {
        let mut entity = commands.entity(hall);
        if administration.is_none() {
            entity.insert(MootAdministration::default());
        }
        if runtime.is_none() {
            entity.insert(MootAdministrationRuntime {
                last_audit_at: None,
                road_progress: HashMap::new(),
            });
        }
    }
}

/// Reserve the first resident for a combined Moot Steward position. Public
/// staffing may add a second budget-gated steward; each collects business
/// output, audits roads and builds/adopts missing connectors.
pub fn staff_moot_stewards(
    mut commands: Commands,
    mut halls: Query<(
        Entity,
        &Settlement,
        &mut MootAdministration,
        &mut MootAdministrationRuntime,
        &shared::components::SettlementId,
        Option<&shared::components::SettlementPolicies>,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        Option<&MootSteward>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
    )>,
) {
    for (hall, settlement, mut administration, mut runtime, settlement_id, policies) in
        halls.iter_mut()
    {
        let policies = policies.copied().unwrap_or_default();
        let current = villagers
            .iter()
            .filter(|(_, _, _, intent, _, _, _, civic_job)| {
                intent.settlement() == Some(hall)
                    && intent.counts_as_resident()
                    && civic_job.is_some_and(|job| {
                        job.settlement == *settlement_id
                            && job.role == shared::components::CivicRole::MootSteward
                    })
            })
            .min_by_key(|(_, person_id, ..)| **person_id)
            .map(|(entity, ..)| entity);

        if current.is_none() {
            administration.lead_steward = None;
        }

        let worker = if let Some(worker) = current {
            worker
        } else {
            if !crate::world::village::civic::can_afford_new_civic_hire(
                settlement,
                &administration,
                &policies,
            ) {
                continue;
            }
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, steward, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && steward.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, ..)| entity);
            let Some(candidate) = candidate else {
                continue;
            };
            let Ok((_, _, name, _, mut occupation, _, _, _)) = villagers.get_mut(candidate) else {
                continue;
            };
            occupation.0 = Some("Moot Steward".to_string());
            administration.lead_steward = Some(name.0.clone());
            administration.steward_daily_salary = MOOT_STEWARD_DAILY_SALARY;
            runtime.last_audit_at = None;
            commands.entity(candidate).insert((
                MootSteward { settlement: hall },
                WorkStatus::Employed,
                shared::components::CivicEmployment {
                    settlement: *settlement_id,
                    role: shared::components::CivicRole::MootSteward,
                },
            ));
            info!(
                "Village '{}': {} took the combined Moot Steward position",
                settlement.name, name.0
            );
            candidate
        };

        if let Ok((_, _, name, _, mut occupation, steward, _, civic_job)) =
            villagers.get_mut(worker)
        {
            administration.lead_steward = Some(name.0.clone());
            if occupation.0.as_deref() != Some("Moot Steward") {
                occupation.0 = Some("Moot Steward".to_string());
            }
            if steward.is_none_or(|steward| steward.settlement != hall) {
                commands
                    .entity(worker)
                    .insert(MootSteward { settlement: hall });
            }
            if civic_job.is_none_or(|job| {
                job.settlement != *settlement_id
                    || job.role != shared::components::CivicRole::MootSteward
            }) {
                commands
                    .entity(worker)
                    .insert(shared::components::CivicEmployment {
                        settlement: *settlement_id,
                        role: shared::components::CivicRole::MootSteward,
                    });
            }
        }
    }
}

/// Fill the tier-bounded public roster with named residents.
///
/// Guards are intentionally jobs before they are combat AI: the vacancy is
/// visible, it consumes one person's time, and later patrol behaviour can be
/// attached without changing the settlement model. Every founding public
/// worker is a combined Moot Steward, so a solvent Hamlet can put two real
/// people onto market collections and road repairs without inventing a second
/// job for either duty.
pub fn staff_public_positions(
    mut commands: Commands,
    mut halls: Query<(
        Entity,
        &Settlement,
        &mut MootAdministration,
        &shared::components::SettlementId,
        Option<&shared::components::SettlementPolicies>,
    )>,
    businesses: Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        &shared::economy::GoodsInventory,
        &shared::economy::BusinessSalePolicy,
        Option<&shared::economy::BusinessCondition>,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        &mut WorkStatus,
        Option<&MootSteward>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        Option<&crate::world::village::MarketCollectionRoutine>,
        Option<&RoadBuilderRoutine>,
    )>,
) {
    // Aggregate once for all halls. Scanning every business separately for
    // every settlement would turn civic staffing into O(settlements × firms)
    // in the future 5,000-person world.
    let mut uncollected_bulk_by_settlement: HashMap<shared::components::SettlementId, u32> =
        HashMap::new();
    for (building, building_of, inventory, policy, condition) in businesses.iter() {
        if !policy.collection_enabled
            || condition
                .is_some_and(|condition| condition.state == shared::economy::BusinessState::Closed)
        {
            continue;
        }
        let Some(good) = crate::world::village::business_output(building.kind) else {
            continue;
        };
        let backlog = inventory
            .amount(good)
            .saturating_sub(policy.company_reserve_units)
            .saturating_mul(good.bulk_per_unit());
        let total = uncollected_bulk_by_settlement
            .entry(building_of.0)
            .or_default();
        *total = total.saturating_add(backlog);
    }

    for (hall, settlement, mut administration, settlement_id, policies) in halls.iter_mut() {
        let policies = policies.copied().unwrap_or_default();
        let mut city_workers: Vec<_> = villagers
            .iter()
            .filter_map(
                |(entity, person_id, name, intent, _, _, _, _, civic_job, _, _)| {
                    let job = civic_job?;
                    (intent.settlement() == Some(hall)
                        && intent.counts_as_resident()
                        && job.settlement == *settlement_id
                        && matches!(
                            job.role,
                            shared::components::CivicRole::MootSteward
                                | shared::components::CivicRole::CityWorker
                        ))
                    .then_some((
                        entity,
                        *person_id,
                        name.0.clone(),
                        matches!(job.role, shared::components::CivicRole::MootSteward),
                    ))
                },
            )
            .collect();
        city_workers.sort_by_key(|(_, person_id, _, is_steward)| (!*is_steward, *person_id));
        let mut guards: Vec<_> = villagers
            .iter()
            .filter_map(
                |(entity, person_id, name, intent, _, _, _, _, civic_job, _, _)| {
                    let job = civic_job?;
                    (intent.settlement() == Some(hall)
                        && intent.counts_as_resident()
                        && job.settlement == *settlement_id
                        && job.role == shared::components::CivicRole::Guard)
                        .then_some((entity, *person_id, name.0.clone()))
                },
            )
            .collect();
        guards.sort_by_key(|(_, person_id, _)| *person_id);

        let (desired_workers, desired_guards) =
            crate::world::village::civic::civic_staffing_targets(
                settlement.tier,
                policies.staffing_posture,
            );
        let uncollected_bulk = uncollected_bulk_by_settlement
            .get(settlement_id)
            .copied()
            .unwrap_or_default();
        let second_steward_threshold = if city_workers.len() >= 2 {
            SECOND_STEWARD_RELEASE_BACKLOG_BULK
        } else {
            SECOND_STEWARD_HIRE_BACKLOG_BULK
        };
        let second_steward_needed = settlement.residents >= SECOND_STEWARD_RESIDENTS
            || uncollected_bulk >= second_steward_threshold;
        let desired_workers = desired_workers.min(1 + usize::from(second_steward_needed));

        // Old saves and earlier builds called the second slot a City Worker.
        // It now has the same bounded physical responsibilities as the first
        // worker. Normalise the durable role and both behaviour markers rather
        // than leaving a paid name in the roster that cannot haul anything.
        for (entity, _, name, is_steward) in &mut city_workers {
            if *is_steward {
                continue;
            }
            if let Ok((_, _, _, _, mut occupation, mut status, steward, _, _, _, _)) =
                villagers.get_mut(*entity)
            {
                occupation.0 = Some("Moot Steward".to_string());
                *status = WorkStatus::Employed;
                let mut worker = commands.entity(*entity);
                if steward.is_none_or(|steward| steward.settlement != hall) {
                    worker.insert(MootSteward { settlement: hall });
                }
                worker.insert(shared::components::CivicEmployment {
                    settlement: *settlement_id,
                    role: shared::components::CivicRole::MootSteward,
                });
                *is_steward = true;
                info!(
                    "Village '{}': {} became a combined Moot Steward",
                    settlement.name, name
                );
            }
        }
        city_workers.sort_by_key(|(_, person_id, _, _)| *person_id);

        // A settlement may advertise jobs it cannot yet fill. Do not add
        // further civic hires when the complete civic roster would consume the
        // final resident; vacancies are safer than eliminating private work.
        let staffing_budget = settlement.residents.saturating_sub(1) as usize;
        let mut retained_busy_workers = Vec::new();
        for (entity, ..) in city_workers.drain(desired_workers.min(city_workers.len())..) {
            if let Ok((
                _,
                person_id,
                name,
                _,
                mut occupation,
                mut status,
                _,
                _,
                _,
                collection,
                road_work,
            )) = villagers.get_mut(entity)
            {
                // A staffing-policy change must not erase privately owned
                // cargo or abandon a half-built connector. Keep paying the
                // excess worker until their current physical duty completes;
                // the next staffing pass will then release them cleanly.
                if collection.is_some() || road_work.is_some() {
                    retained_busy_workers.push((entity, *person_id, name.0.clone(), true));
                    continue;
                }
                occupation.0 = None;
                *status = WorkStatus::LookingForWork;
            }
            commands
                .entity(entity)
                .remove::<shared::components::CivicEmployment>()
                .remove::<MootSteward>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
        }
        city_workers.extend(retained_busy_workers);
        for (entity, ..) in guards.drain(desired_guards.min(guards.len())..) {
            if let Ok((_, _, _, _, mut occupation, mut status, _, _, _, _, _)) =
                villagers.get_mut(entity)
            {
                occupation.0 = None;
                *status = WorkStatus::LookingForWork;
            }
            commands
                .entity(entity)
                .remove::<shared::components::CivicEmployment>();
        }

        while city_workers.len() < desired_workers
            && usize::from(administration.reeve.is_some()) + city_workers.len() + guards.len()
                < staffing_budget
        {
            let projected =
                usize::from(administration.reeve.is_some()) + city_workers.len() + guards.len() + 1;
            if !crate::world::village::civic::can_afford_civic_positions(
                settlement,
                &administration,
                &policies,
                projected,
            ) {
                break;
            }
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, _, steward, employed_at, civic_job, _, _)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && steward.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, person_id, name, ..)| (entity, *person_id, name.0.clone()));
            let Some((entity, person_id, name)) = candidate else {
                break;
            };
            if let Ok((_, _, _, _, mut occupation, mut status, steward, _, _, _, _)) =
                villagers.get_mut(entity)
            {
                if steward.is_none() {
                    occupation.0 = Some("Moot Steward".to_string());
                    *status = WorkStatus::Employed;
                    commands.entity(entity).insert((
                        MootSteward { settlement: hall },
                        shared::components::CivicEmployment {
                            settlement: *settlement_id,
                            role: shared::components::CivicRole::MootSteward,
                        },
                    ));
                }
            }
            info!(
                "Village '{}': {} took an additional combined Moot Steward position",
                settlement.name, name
            );
            city_workers.push((entity, person_id, name, true));
        }

        while guards.len() < desired_guards
            && usize::from(administration.reeve.is_some()) + city_workers.len() + guards.len()
                < staffing_budget
        {
            let projected =
                usize::from(administration.reeve.is_some()) + city_workers.len() + guards.len() + 1;
            if !crate::world::village::civic::can_afford_civic_positions(
                settlement,
                &administration,
                &policies,
                projected,
            ) {
                break;
            }
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, _, _, employed_at, civic_job, _, _)| {
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
            if let Ok((_, _, _, _, mut occupation, mut status, _, _, _, _, _)) =
                villagers.get_mut(entity)
            {
                occupation.0 = Some("Town Guard".to_string());
                *status = WorkStatus::Employed;
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
        &shared::components::BuildingId,
        Option<&RoadRequest>,
    )>,
    roads: Query<(
        Entity,
        &VillageRoad,
        &shared::components::RoadOf,
        Option<&RoadConnectorFor>,
    )>,
    road_workers: Query<(
        Entity,
        &VillagerIntent,
        Option<&RoadBuilderRoutine>,
        &PlayerPosition,
        Has<HomeRoutine>,
        Has<shared::components::EmployedAt>,
    )>,
    stewards: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &MootSteward,
        &shared::components::CivicEmployment,
        Option<&RoadBuilderRoutine>,
        Option<&crate::world::village::MarketCollectionRoutine>,
        Has<crate::world::village::strategic::StrategicPerson>,
    )>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let now = absolute_world_seconds(clock);
    let road_owners: HashMap<_, _> = road_workers
        .iter()
        .filter_map(|(builder, intent, routine, position, at_home, _)| {
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
    let steward_request_owners: HashSet<_> = buildings
        .iter()
        .filter_map(|(_, _, _, _, _, _, request)| request.map(|request| request.builder))
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
        let Some((
            steward,
            steward_name,
            intent,
            _,
            _,
            road_work,
            collection,
            steward_is_strategic,
        )) = stewards
            .iter()
            .filter(|(_, _, intent, steward, civic_job, _, _, _)| {
                steward.settlement == hall
                    && intent.settlement() == Some(hall)
                    && civic_job.settlement == *settlement_id
                    && civic_job.role == shared::components::CivicRole::MootSteward
            })
            // Prefer an actually idle steward. With two workers, selecting the
            // oldest one unconditionally could leave the second idle while a
            // collection or connector kept the primary occupied for hours.
            .min_by_key(|(entity, _, intent, _, _, road_work, collection, _)| {
                (
                    !intent.is_settled()
                        || road_work.is_some()
                        || collection.is_some()
                        || steward_request_owners.contains(entity),
                    entity.to_bits(),
                )
            })
        else {
            continue;
        };
        // Continue inspecting an in-progress connector so the steward can
        // reclaim their own stalled work. Whether this person may accept a
        // *new* repair is decided after the full audit.
        let steward_owns_request = steward_request_owners.contains(&steward);
        let steward_available_for_repair = road_work.is_none()
            && collection.is_none()
            && intent.is_settled()
            && !steward_owns_request;

        let mut abandoned_roads = 0usize;
        let mut stalled_roads = 0usize;
        let mut observed_roads = HashSet::new();
        let mut settlement_roads = Vec::new();
        let mut connector_buildings = HashSet::new();
        for (entity, road, road_of, connector) in roads.iter() {
            if road_of.0 != *settlement_id {
                continue;
            }
            if road.is_complete() {
                runtime.road_progress.remove(&entity);
                settlement_roads.push(road);
                if let Some(connector) = connector {
                    connector_buildings.insert(connector.building);
                }
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
            if let Some(connector) = connector {
                connector_buildings.insert(connector.building);
            }
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
        for (building_entity, building, position, rotation, _building_of, building_id, request) in
            buildings
                .iter()
                .filter(|(_, _, _, _, building_of, _, _)| building_of.0 == *settlement_id)
        {
            let door3 = building.kind.entrance_position(position.0, rotation.0);
            let request_is_active = request.is_some_and(|request| {
                road_workers.get(request.builder).is_ok_and(
                    |(_, intent, routine, _, _, privately_employed)| {
                        routine.is_none()
                            && !privately_employed
                            && (matches!(
                                intent,
                                VillagerIntent::Resident { settlement }
                                    if *settlement == hall
                            ) || matches!(
                                intent,
                                VillagerIntent::Building { settlement, site }
                                    if *settlement == hall && *site == request.completed_site
                            ))
                    },
                )
            });
            if let Some(stale_request) = request.filter(|_| !request_is_active) {
                // A request whose named builder took another job or lost its
                // accountable intent is no longer a second repair owner. Turn
                // it into civic backlog atomically. Previously the audit added
                // `RoadRepairBacklog` but retained this stale component when
                // both stewards were busy, leaving one building in two road
                // lifecycles until a later audit happened to clean it up.
                commands.entity(building_entity).remove::<RoadRequest>();
                if road_workers.get(stale_request.builder).is_ok_and(
                    |(_, intent, routine, _, _, _)| {
                        routine.is_none()
                            && matches!(
                                intent,
                                VillagerIntent::Building { settlement, site }
                                    if *settlement == hall
                                        && *site == stale_request.completed_site
                            )
                    },
                ) {
                    commands
                        .entity(stale_request.builder)
                        .insert(VillagerIntent::Resident { settlement: hall });
                }
            }
            // A neighbouring connector may cross this doorway on its way to
            // the public network. Proximity is useful for topology but is not
            // ownership: every completed building must retain its own short
            // `RoadConnectorFor`, even when that connector only joins a lane a
            // few metres away. Otherwise despawning or repairing the neighbour
            // silently strands this building and the steward never notices.
            let status = if connector_buildings.contains(&building_entity) {
                building_road_status(
                    Vec2::new(door3.x, door3.z),
                    &settlement_roads,
                    &network,
                    request_is_active,
                )
            } else if request_is_active {
                BuildingRoadStatus::Pending
            } else {
                BuildingRoadStatus::Roadless
            };
            match status {
                BuildingRoadStatus::Disconnected => {
                    disconnected += 1;
                    commands.entity(building_entity).insert(RoadRepairBacklog);
                    repairs.push((
                        0u8,
                        *building_id,
                        building_entity,
                        building.kind,
                        request.copied(),
                    ));
                }
                BuildingRoadStatus::Roadless => {
                    roadless += 1;
                    commands.entity(building_entity).insert(RoadRepairBacklog);
                    repairs.push((
                        1u8,
                        *building_id,
                        building_entity,
                        building.kind,
                        request.copied(),
                    ));
                }
                BuildingRoadStatus::Pending => {
                    pending += 1;
                    commands
                        .entity(building_entity)
                        .remove::<RoadRepairBacklog>();
                }
                BuildingRoadStatus::Connected => {
                    commands
                        .entity(building_entity)
                        .remove::<RoadRepairBacklog>();
                }
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

        repairs.sort_unstable_by_key(|(priority, id, entity, _, _)| {
            (*priority, *id, entity.to_bits())
        });
        if let Some((_, _, building, kind, previous_request)) = repairs
            .first()
            .copied()
            .filter(|_| steward_available_for_repair)
        {
            if steward_is_strategic {
                // Road inspection is cheap settlement bookkeeping and must
                // continue while a town is off screen. Physical repair is a
                // different matter: wake only the selected accountable steward,
                // discard any abstract leisure journey, and keep them
                // tactical until the connector routine reaches a safe end.
                commands
                    .entity(steward)
                    .insert((
                        crate::world::village::strategic::PendingStrategicDemotion,
                        shared::components::CharacterMotion::STATIONARY,
                    ))
                    .remove::<crate::world::village::strategic::StrategicPerson>()
                    .remove::<crate::world::village::strategic::StrategicTravel>();
            }
            // Completing a building deliberately leaves its builder in the
            // Building intent until that person adopts the connector. If the
            // person was hired before the road turn, the steward must take
            // over without leaving the former builder permanently committed
            // to a construction site that has already despawned. That stale
            // intent excluded employed residents from every work routine and
            // presented as crowds standing at their cabin doors.
            if let Some(previous_request) = previous_request.filter(|request| {
                request.builder != steward
                    && road_workers
                        .get(request.builder)
                        .is_ok_and(|(_, intent, ..)| {
                            matches!(
                                intent,
                                VillagerIntent::Building { settlement, site }
                                    if *settlement == hall && *site == request.completed_site
                            )
                        })
            }) {
                commands
                    .entity(previous_request.builder)
                    .insert(VillagerIntent::Resident { settlement: hall });
            }
            commands
                .entity(building)
                .insert(RoadRequest {
                    builder: steward,
                    settlement: hall,
                    completed_site: building,
                    attempt: 0,
                })
                .remove::<RoadRepairBacklog>();
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
        } else if pending > 0 || (!repairs.is_empty() && !steward_available_for_repair) {
            info!(
                "Village '{}': Road Steward {} found {} connector(s) pending and {} repair(s) queued behind their current assignment",
                settlement.name,
                steward_name.0,
                pending,
                repairs.len(),
            );
        } else {
            info!(
                "Village '{}': Road Steward {} completed the road audit; every building reaches the Moot Hall",
                settlement.name, steward_name.0
            );
        }
    }
}
