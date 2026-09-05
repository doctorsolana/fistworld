//! Door clip state, animation wiring recovery and windmill motion.

use bevy::animation::AnimatedBy;
use bevy::gltf::Gltf;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use shared::building::BuildingType;
use shared::components::{
    BuildingDoorDemand, CloudSeed, PlayerPosition, PlayerRotation, SettlementBuilding,
    SettlementBuildingKind, TimeWarp, WorldTime,
};
use shared::economy::{BusinessCondition, BusinessState, Good, GoodsInventory};

#[derive(Component)]
pub(super) struct DoorVisualSource {
    pub(super) kind: SettlementBuildingKind,
    pub(super) building_type: BuildingType,
    pub(super) gltf: Handle<Gltf>,
}

#[derive(Clone)]
pub(super) struct DoorGraph {
    pub(super) handle: Handle<AnimationGraph>,
    pub(super) open: AnimationNodeIndex,
    pub(super) close: AnimationNodeIndex,
    pub(super) sails: Option<AnimationNodeIndex>,
}

#[derive(Resource, Default)]
pub(super) struct BuildingDoorAssets {
    pub(super) graphs: HashMap<BuildingType, DoorGraph>,
}

#[derive(Component)]
pub(super) struct DoorPlayerWired;

/// Marks the animated mesh node whose [`AnimatedBy`] relationship selected the
/// player for a building door. Looking up from the target is important: some
/// glTF scenes can contain more than one `AnimationPlayer`, and merely taking
/// the first descendant can wire a valid graph to a player that does not own
/// the visible door.
#[derive(Component)]
pub(super) struct DoorTargetWired;

/// The windmill's permanent mechanical animation wiring. The cap continuously
/// follows the local wind bearing; the sails' playback speed follows the
/// deterministic wind swell and the master time warp.
#[derive(Component)]
pub(super) struct WindmillMotion {
    pub(super) player: Entity,
    pub(super) sails: AnimationNodeIndex,
    pub(super) cap: Entity,
    pub(super) cap_rest_rotation: Quat,
}

#[derive(Component)]
pub(super) struct BuildingDoorAnimation {
    pub(super) player: Entity,
    pub(super) open: AnimationNodeIndex,
    pub(super) close: AnimationNodeIndex,
    pub(super) state: DoorState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum DoorState {
    Shut,
    Opening { elapsed: f32 },
    Open { clear_for: f32 },
    Closing { elapsed: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum DoorCommand {
    Open { seek_seconds: f32 },
    Close,
}

pub(super) const DOOR_OPEN_SECONDS: f32 = 0.667;

pub(super) const DOOR_CLOSE_SECONDS: f32 = 0.917;

pub(super) const DOOR_CLEAR_HOLD_SECONDS: f32 = 0.5;

/// Scene streaming may replace every glTF descendant while retaining the
/// replicated building root. Never let that root's cached player entity turn
/// into a permanent "wired" lie: clear the stale relationship and the target
/// marker so the ordinary discovery system can bind the replacement scene on
/// this same frame boundary.
pub(super) fn recover_stale_building_animation_wiring(
    mut commands: Commands,
    roots: Query<(Entity, &BuildingDoorAnimation, Option<&WindmillMotion>)>,
    players: Query<(), With<AnimationPlayer>>,
    transforms: Query<(), With<Transform>>,
    children: Query<&Children>,
    wired_targets: Query<(), With<DoorTargetWired>>,
) {
    for (root, door, windmill) in roots.iter() {
        let door_missing = players.get(door.player).is_err();
        let mechanism_missing = windmill.is_some_and(|motion| {
            players.get(motion.player).is_err() || transforms.get(motion.cap).is_err()
        });
        if !door_missing && !mechanism_missing {
            continue;
        }

        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if wired_targets.get(entity).is_ok() {
                commands.entity(entity).remove::<DoorTargetWired>();
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        commands
            .entity(root)
            .remove::<BuildingDoorAnimation>()
            .remove::<WindmillMotion>();
        debug!(
            "discarded stale building animation wiring at {:?} (door missing={}, mechanism missing={})",
            root, door_missing, mechanism_missing
        );
    }
}

/// Connect each instantiated building scene's node-animation player to the two
/// authored door clips. Graphs are shared per authored building type; the
/// player and open/close state remain per physical building.
pub(super) fn setup_building_door_animations(
    mut commands: Commands,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut assets: ResMut<BuildingDoorAssets>,
    targets: Query<(Entity, &Name, &AnimatedBy), Without<DoorTargetWired>>,
    mut players: Query<&mut AnimationPlayer>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut node_transforms: Query<&mut Transform>,
    sources: Query<&DoorVisualSource>,
    wired_roots: Query<(), With<BuildingDoorAnimation>>,
) {
    for (target_entity, name, animated_by) in &targets {
        if !name.as_str().ends_with("Door") {
            continue;
        }

        let mut ancestor = target_entity;
        let source_root = loop {
            if let Ok(source) = sources.get(ancestor) {
                break Some((ancestor, source));
            }
            let Ok(parent) = parents.get(ancestor) else {
                break None;
            };
            ancestor = parent.parent();
        };
        let Some((root, source)) = source_root else {
            continue;
        };

        if wired_roots.get(root).is_ok() {
            commands.entity(target_entity).insert(DoorTargetWired);
            continue;
        }

        let player_entity = animated_by.0;
        let Ok(_) = players.get_mut(player_entity) else {
            continue;
        };

        let door_graph = if let Some(graph) = assets.graphs.get(&source.building_type) {
            graph.clone()
        } else {
            let Some(gltf) = gltfs.get(&source.gltf) else {
                continue;
            };
            let Some(open_clip) = gltf.named_animations.get("door_open") else {
                warn!("{} art has no door_open clip", source.kind.label());
                commands.entity(player_entity).insert(DoorPlayerWired);
                continue;
            };
            let Some(close_clip) = gltf.named_animations.get("door_close") else {
                warn!("{} art has no door_close clip", source.kind.label());
                commands.entity(player_entity).insert(DoorPlayerWired);
                continue;
            };
            let mut graph = AnimationGraph::new();
            let open = graph.add_clip(open_clip.clone(), 1.0, graph.root);
            let close = graph.add_clip(close_clip.clone(), 1.0, graph.root);
            let sails = gltf
                .named_animations
                .get("sails_turn")
                .map(|clip| graph.add_clip(clip.clone(), 1.0, graph.root));
            let graph = DoorGraph {
                handle: graphs.add(graph),
                open,
                close,
                sails,
            };
            assets.graphs.insert(source.building_type, graph.clone());
            graph
        };

        let windmill_motion = if let Some(sails) = door_graph.sails {
            let mut stack = vec![root];
            let mut cap = None;
            let mut sails_player = None;
            while let Some(entity) = stack.pop() {
                if let Ok(name) = names.get(entity) {
                    match name.as_str() {
                        "WindMillCap" => cap = Some(entity),
                        "WindMillSails" => {
                            if let Ok((_, _, animated_by)) = targets.get(entity) {
                                sails_player = Some(animated_by.0);
                            }
                        }
                        _ => {}
                    }
                }
                if cap.is_some() && sails_player.is_some() {
                    break;
                }
                if let Ok(entity_children) = children.get(entity) {
                    stack.extend(entity_children.iter());
                }
            }
            let (Some(cap), Some(sails_player)) = (cap, sails_player) else {
                // Door, cap and sails arrive in one glTF scene, but retain the
                // poll in case Bevy has not instantiated every sibling and its
                // AnimatedBy relationship this frame.
                continue;
            };
            let Ok(cap_transform) = node_transforms.get_mut(cap) else {
                continue;
            };
            let cap_rest_rotation = cap_transform.rotation;
            let Ok(mut player) = players.get_mut(sails_player) else {
                continue;
            };
            player.play(sails).repeat();
            if sails_player != player_entity {
                commands.entity(sails_player).insert((
                    AnimationGraphHandle(door_graph.handle.clone()),
                    DoorPlayerWired,
                ));
            }
            debug!(
                "windmill animation players: door {:?}, sails {:?}",
                player_entity, sails_player
            );
            Some(WindmillMotion {
                player: sails_player,
                sails,
                cap,
                cap_rest_rotation,
            })
        } else {
            None
        };
        commands
            .entity(player_entity)
            .insert((AnimationGraphHandle(door_graph.handle), DoorPlayerWired));
        commands.entity(target_entity).insert(DoorTargetWired);
        commands.entity(root).insert(BuildingDoorAnimation {
            player: player_entity,
            open: door_graph.open,
            close: door_graph.close,
            state: DoorState::Shut,
        });
        if let Some(motion) = windmill_motion {
            commands.entity(root).insert(motion);
        }
        debug!(
            "wired {} door animation (root {:?}, player {:?})",
            source.kind.label(),
            root,
            player_entity
        );
    }
}

/// Local cap yaw which points the mill's authored -Z front into the wind. The
/// prevailing vector describes downwind travel, so the visible sails face its
/// opposite while retaining the building's permanent plot rotation.
pub(super) fn windmill_cap_yaw(root_yaw: f32, downwind: Vec2) -> f32 {
    downwind.x.atan2(downwind.y) - root_yaw
}

pub(super) fn drive_windmill_motion(
    world_time: Query<&WorldTime>,
    cloud_seed: Query<&CloudSeed>,
    warp: Query<&TimeWarp>,
    windmills: Query<(
        &WindmillMotion,
        &SettlementBuilding,
        &GoodsInventory,
        Option<&BusinessCondition>,
        Option<&PlayerRotation>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    mut cap_transforms: Query<&mut Transform>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let absolute_seconds = clock.day as f32 * clock.cycle_duration() + clock.seconds_in_cycle;
    let seed_phase = crate::wind::wind_seed_phase(
        cloud_seed
            .iter()
            .next()
            .map_or(0, |cloud_seed| cloud_seed.seed),
    );
    let (_, wind_speed) = crate::wind::wind_state(absolute_seconds, seed_phase);
    let wind_direction = crate::wind::wind_direction(absolute_seconds, seed_phase);
    let time_factor = warp.iter().next().map_or(1.0, |warp| warp.0);
    let natural_speed = 0.5 * (crate::wind::WIND_SPEED_MIN + crate::wind::WIND_SPEED_MAX);
    let playback_speed = wind_speed / natural_speed * time_factor;

    for (motion, building, inventory, condition, root_rotation) in windmills.iter() {
        if let Ok(mut cap_transform) = cap_transforms.get_mut(motion.cap) {
            let root_yaw = root_rotation.map_or(0.0, |rotation| rotation.0);
            cap_transform.rotation =
                Quat::from_rotation_y(windmill_cap_yaw(root_yaw, wind_direction))
                    * motion.cap_rest_rotation;
        }
        let Ok(mut player) = players.get_mut(motion.player) else {
            continue;
        };
        let has_work = clock.is_ordinary_work_time()
            && !building.workers.is_empty()
            && condition.is_none_or(|condition| condition.state != BusinessState::Closed)
            && inventory.amount(Good::Wheat) > 0
            && inventory
                .free_bulk()
                .saturating_add(Good::Wheat.bulk_per_unit())
                >= Good::Flour.bulk_per_unit();
        let playback_speed = if has_work { playback_speed } else { 0.0 };
        if let Some(sails) = player.animation_mut(motion.sails) {
            sails.set_speed(playback_speed);
        } else {
            player.play(motion.sails).repeat().set_speed(playback_speed);
        }
    }
}

/// One door reacts to aggregate demand, not individual arrivals. This is what
/// lets a household file through together without each person snapping the
/// leaf back to the first frame of `door_open`.
pub(super) fn drive_building_doors(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    mut buildings: Query<(
        Entity,
        &PlayerPosition,
        Option<&BuildingDoorDemand>,
        &mut BuildingDoorAnimation,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    mut missing_players: Local<HashSet<Entity>>,
) {
    let factor = warp.iter().next().map(|warp| warp.0).unwrap_or(1.0);
    let dt = time.delta_secs() * factor;
    for (building, position, replicated_demand, mut door) in buildings.iter_mut() {
        let demand = replicated_demand.is_some_and(|demand| demand.open);
        let (next, command) = advance_door_state(door.state, demand, dt);
        let mut player = match players.get_mut(door.player) {
            Ok(player) => {
                missing_players.remove(&building);
                player
            }
            Err(error) => {
                if missing_players.insert(building) {
                    warn!(
                        "door at {:?} lost animation player {:?}: {error}",
                        position.0, door.player
                    );
                }
                continue;
            }
        };

        if let Some(command) = command {
            let (clip, seek_seconds) = match command {
                DoorCommand::Open { seek_seconds } => (door.open, seek_seconds),
                DoorCommand::Close => (door.close, 0.0),
            };
            debug!("door at {:?}: {:?}", position.0, command);
            // Door clips share a player with persistent mechanical animations
            // such as windmill sails. Stop only the opposing door clips.
            player.stop(door.open).stop(door.close);
            player
                .play(clip)
                .set_seek_time(seek_seconds)
                .set_speed(factor);
        } else {
            let active = match next {
                DoorState::Opening { .. } => Some(door.open),
                DoorState::Closing { .. } => Some(door.close),
                DoorState::Shut | DoorState::Open { .. } => None,
            };
            if let Some(active) = active.and_then(|clip| player.animation_mut(clip)) {
                active.set_speed(factor);
            }
        }
        if door.state != next {
            door.state = next;
        }
    }
}

/// Keep the network boundary observable while door timing is being tuned.
/// This deliberately logs the persistent building value rather than the
/// animation state, so an absent line means replication failed before art or
/// clip playback became involved.
pub(super) fn trace_replicated_building_door_demands(
    buildings: Query<(&PlayerPosition, &BuildingDoorDemand), Changed<BuildingDoorDemand>>,
) {
    for (position, demand) in &buildings {
        debug!(
            "replicated door demand at {:.1},{:.1}: {}",
            position.0.x,
            position.0.z,
            if demand.open { "OPEN" } else { "CLOSED" }
        );
    }
}

pub(super) fn advance_door_state(
    state: DoorState,
    demand: bool,
    dt: f32,
) -> (DoorState, Option<DoorCommand>) {
    match state {
        DoorState::Shut if demand => (
            DoorState::Opening { elapsed: 0.0 },
            Some(DoorCommand::Open { seek_seconds: 0.0 }),
        ),
        DoorState::Shut => (DoorState::Shut, None),
        DoorState::Opening { elapsed } => {
            let elapsed = elapsed + dt;
            if elapsed >= DOOR_OPEN_SECONDS {
                (DoorState::Open { clear_for: 0.0 }, None)
            } else {
                (DoorState::Opening { elapsed }, None)
            }
        }
        DoorState::Open { .. } if demand => (DoorState::Open { clear_for: 0.0 }, None),
        DoorState::Open { clear_for } => {
            let clear_for = clear_for + dt;
            if clear_for >= DOOR_CLEAR_HOLD_SECONDS {
                (
                    DoorState::Closing { elapsed: 0.0 },
                    Some(DoorCommand::Close),
                )
            } else {
                (DoorState::Open { clear_for }, None)
            }
        }
        DoorState::Closing { elapsed } if demand => {
            // Continue from the same physical openness instead of restarting
            // `door_open` at its shut pose and visibly snapping the leaf.
            let openness = 1.0 - (elapsed / DOOR_CLOSE_SECONDS).clamp(0.0, 1.0);
            let open_elapsed = openness * DOOR_OPEN_SECONDS;
            (
                DoorState::Opening {
                    elapsed: open_elapsed,
                },
                Some(DoorCommand::Open {
                    seek_seconds: open_elapsed,
                }),
            )
        }
        DoorState::Closing { elapsed } => {
            let elapsed = elapsed + dt;
            if elapsed >= DOOR_CLOSE_SECONDS {
                (DoorState::Shut, None)
            } else {
                (DoorState::Closing { elapsed }, None)
            }
        }
    }
}
