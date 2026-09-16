//! Shared physical workplace occupancy, entry, exit and door presentation.

use super::super::*;

/// Physical workplace occupancy survives the inbound doorway animation, so
/// unrelated routines cannot route an indoor worker straight through a wall.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WorkplaceInterior {
    pub(crate) building: Vec3,
    pub(crate) door: Vec3,
    pub(crate) inside: Vec3,
}

/// The currently moving portion of an occupied workplace's doorway. The
/// interior marker survives an inbound crossing until a real exit finishes.
#[derive(Component, Debug, Clone, Copy)]
pub struct WorkplaceDoorTransit {
    pub(crate) building: Vec3,
    pub(crate) door: Vec3,
    pub(crate) inside: Vec3,
    pub(crate) direction: WorkplaceDoorDirection,
    pub(crate) phase: WorkplaceDoorPhase,
    pub(crate) destination_after_exit: Option<Vec3>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkplaceDoorDirection {
    Entering,
    Leaving,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WorkplaceDoorPhase {
    Opening { seconds_left: f32 },
    Crossing,
}

pub(crate) fn begin_workplace_entry(
    commands: &mut Commands,
    worker: Entity,
    building: Vec3,
    door: Vec3,
    inside: Vec3,
) {
    commands
        .entity(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert(WorkplaceInterior {
            building,
            door,
            inside,
        })
        .insert(WorkplaceDoorTransit {
            building,
            door,
            inside,
            direction: WorkplaceDoorDirection::Entering,
            phase: WorkplaceDoorPhase::Opening {
                seconds_left: DOOR_OPEN_SECONDS,
            },
            destination_after_exit: None,
        });
}

pub(crate) fn begin_workplace_exit(
    commands: &mut Commands,
    worker: Entity,
    building: Vec3,
    door: Vec3,
    inside: Vec3,
    destination: Vec3,
) {
    commands
        .entity(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert(WorkplaceInterior {
            building,
            door,
            inside,
        })
        .insert(WorkplaceDoorTransit {
            building,
            door,
            inside,
            direction: WorkplaceDoorDirection::Leaving,
            phase: WorkplaceDoorPhase::Opening {
                seconds_left: DOOR_OPEN_SECONDS,
            },
            destination_after_exit: Some(destination),
        });
}

pub(crate) fn begin_workplace_interior_exit(
    commands: &mut Commands,
    worker: Entity,
    interior: &WorkplaceInterior,
    destination: Vec3,
) {
    begin_workplace_exit(
        commands,
        worker,
        interior.building,
        interior.door,
        interior.inside,
        destination,
    );
}

/// Paid/essential services may reserve a place while somebody works indoors.
/// The common doorway owner must make the actor safe before the queue is
/// allowed to install a land route. Production can remain paused throughout.
pub(crate) fn run_workplace_service_handoffs(
    mut commands: Commands,
    workers: Query<
        (Entity, &WorkplaceInterior),
        (With<MootQueueTicket>, Without<WorkplaceDoorTransit>),
    >,
) {
    for (worker, interior) in &workers {
        // The queue chooses its own counter after the exit, so first finish
        // at the existing exterior entrance instead of reusing a work target.
        begin_workplace_interior_exit(&mut commands, worker, interior, interior.door);
    }
}

/// Keep the job system paused while the worker crosses the threshold, so its
/// destination cannot drag them through the wall while the door is opening.
pub fn run_workplace_door_transits(
    simulation_time: crate::world::simulation_time::SimulationTime,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    mut commands: Commands,
    mut workers: Query<
        (
            Entity,
            &PlayerPosition,
            &VillagerIntent,
            Option<&mut HomeRoutine>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut WorkplaceDoorTransit,
            Option<&MoveTarget>,
        ),
        (With<CharacterKind>,),
    >,
) {
    let dt = simulation_time.world_seconds();
    for (worker, position, intent, mut home, mut facing, mut activity, mut transit, move_target) in
        workers.iter_mut()
    {
        let leaving_for_home = home
            .as_deref()
            .is_some_and(|home| home.phase == HomePhase::LeavingWorkplace);
        // A resident can receive a construction or road task while still
        // crossing a workplace threshold. Cancelling the paired transit just
        // because their intent is no longer the idle Resident variant leaves
        // HomeRoutine::LeavingWorkplace with no system able to finish it.
        let invalid_commitment = if leaving_for_home {
            !intent.counts_as_resident()
        } else {
            !intent.is_settled()
        };
        if (home.is_some() && !leaving_for_home) || invalid_commitment {
            commands
                .entity(worker)
                .remove::<WorkplaceDoorTransit>()
                .remove::<WorkplaceInterior>()
                .remove::<BuildingDoorUse>();
            continue;
        }

        let door_use = BuildingDoorUse {
            building: transit.building,
        };
        commands.entity(worker).insert(door_use);
        match transit.phase {
            WorkplaceDoorPhase::Opening { seconds_left } => {
                let target = match transit.direction {
                    WorkplaceDoorDirection::Entering => transit.inside,
                    WorkplaceDoorDirection::Leaving => {
                        exterior_door_clearance_position(transit.building, transit.door)
                    }
                };
                let to_target = target - position.0;
                if to_target.length_squared() > 1e-4 {
                    facing.0 = build_clip_facing(to_target);
                }
                let left = seconds_left - dt;
                if left > 0.0 {
                    transit.phase = WorkplaceDoorPhase::Opening { seconds_left: left };
                    continue;
                }
                activity.set_if_neq(CharacterActivity::Idle);
                commands.entity(worker).insert(MoveTarget(target));
                transit.phase = WorkplaceDoorPhase::Crossing;
            }
            WorkplaceDoorPhase::Crossing => {
                let target = match transit.direction {
                    WorkplaceDoorDirection::Entering => transit.inside,
                    WorkplaceDoorDirection::Leaving => {
                        exterior_door_clearance_position(transit.building, transit.door)
                    }
                };
                activity.set_if_neq(CharacterActivity::Idle);
                // The entrance marker itself is outside the inflated building
                // blocker, but DOOR_REACH extends slightly back through the
                // wall. Do not release collision immunity from a leaving
                // worker merely because they are close to the door: at that
                // point their next ordinary route would begin inside the
                // blocker and can never certify. Walk the final few
                // centimetres until their actual position is outside too.
                let still_inside_blocker = transit.direction == WorkplaceDoorDirection::Leaving
                    && obstacles.as_deref().is_some_and(|grid| {
                        grid.point_blocked(Vec2::new(position.0.x, position.0.z))
                    });
                if ground_distance(position.0, target) > DOOR_REACH || still_inside_blocker {
                    ensure_move_target(&mut commands, worker, move_target, target);
                    continue;
                }

                let destination = transit.destination_after_exit;
                if transit.direction == WorkplaceDoorDirection::Entering {
                    activity.set_if_neq(CharacterActivity::Indoors);
                }
                let mut worker_commands = commands.entity(worker);
                if transit.direction == WorkplaceDoorDirection::Leaving {
                    worker_commands.remove::<WorkplaceInterior>();
                }
                worker_commands
                    .remove::<WorkplaceDoorTransit>()
                    .remove::<BuildingDoorUse>()
                    .remove::<MoveTarget>();
                if let Some(destination) = destination {
                    worker_commands.insert(MoveTarget(destination));
                }
                if leaving_for_home {
                    home.as_deref_mut().expect("checked above").phase = HomePhase::GoingToDoor;
                }
            }
        }
    }
}

/// Fold short-lived actor requests into one stable replicated value per door.
///
/// Replicating only [`BuildingDoorUse`] made the visual depend on the client
/// observing a transient component on an actor at exactly the right network
/// snapshot. A door belongs to its building, so its aggregate demand lives on
/// that stable entity and remains present even while its boolean is false.
pub fn sync_building_door_demands(
    mut commands: Commands,
    uses: Query<&BuildingDoorUse>,
    mut buildings: Query<
        (Entity, &PlayerPosition, Option<&mut BuildingDoorDemand>),
        Or<(With<Settlement>, With<SettlementBuilding>)>,
    >,
) {
    let requested: HashSet<[u32; 3]> = uses
        .iter()
        .map(|request| request.building.to_array().map(f32::to_bits))
        .collect();
    for (entity, position, demand) in buildings.iter_mut() {
        let open = requested.contains(&position.0.to_array().map(f32::to_bits));
        if let Some(mut demand) = demand {
            if demand.open != open {
                debug!(
                    "Building door at {:.1},{:.1} demand {}",
                    position.0.x,
                    position.0.z,
                    if open { "OPEN" } else { "CLOSED" }
                );
                demand.open = open;
            }
        } else {
            if open {
                debug!(
                    "Building door at {:.1},{:.1} demand OPEN",
                    position.0.x, position.0.z
                );
            }
            commands.entity(entity).insert(BuildingDoorDemand { open });
        }
    }
}
