//! NPC spawn systems.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    AdditionalMassProperties, Ccd, Collider, Damping, ExternalImpulse, RigidBody, Velocity,
};
use lightyear::prelude::server::{ClientOf, Started};
use lightyear::prelude::*;
use shared::components::{
    DebugPhysicsBox, DebugPhysicsBoxPosition, DebugPhysicsBoxRotation, Health, Npc, NpcActivity,
    NpcActivityKind, NpcArchetype, NpcPosition, NpcRotation, NpcVelocity, Player, PlayerPosition,
};
use shared::map::MapBehaviorPreset;
use shared::npc::npc_max_health;
use shared::physics::ground_clearance_center;
use shared::protocol::{SpawnOilmanDebug, SpawnPhysicsBoxDebug};
use shared::terrain::{WorldTerrain, WORLD_SEED};

use crate::ai::identity::{
    npc_identity_for_archetype, npc_identity_for_debug, npc_identity_for_group,
};
use crate::ai::state::{NpcWander, XorShift64};
use crate::physics::layers;
use crate::player::index::PlayerEntityIndex;

const NPC_REPLICATION_PRIORITY: f32 = 0.35;
const DEBUG_BOX_REPLICATION_PRIORITY: f32 = 0.30;

fn configured_npc_cap() -> u32 {
    match std::env::var("CITYSIM_MAX_NPCS") {
        Ok(raw) => match raw.parse::<u32>() {
            Ok(0) => 0,
            Ok(value) => value,
            Err(_) => {
                warn!(
                    "Invalid CITYSIM_MAX_NPCS='{}'; defaulting to 0 startup NPCs",
                    raw
                );
                0
            }
        },
        Err(_) => 0,
    }
}

fn choose_top_up_archetype(
    groups: &[shared::map::MapNpcGroup],
    rng: &mut XorShift64,
) -> NpcArchetype {
    if groups.is_empty() {
        return NpcArchetype::Oilman;
    }
    let idx = (rng.next_u64() as usize) % groups.len();
    groups[idx].archetype
}

/// One-shot resource to ensure NPCs are only spawned once.
#[derive(Resource)]
pub struct NpcsSpawned;

/// Spawn NPCs near the player spawn once the server is started.
pub fn spawn_npcs_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<NpcsSpawned>>,
    // The `Started` component is present on the server entity once networking is up.
    server_started: Query<(), With<Started>>,
) {
    if spawned.is_some() || server_started.is_empty() {
        return;
    }
    commands.insert_resource(NpcsSpawned);

    let mut total_spawned = 0usize;
    let mut next_npc_id: u64 = 10_000;
    let loaded_map = terrain.generator.loaded_map();
    let npc_groups = &loaded_map.definition.npc_groups;
    let world_bounds = loaded_map.definition.bounds;

    // Anatomical combat dummies near the player spawn. They are built from the
    // same body table used by hit detection and ragdoll physics.
    let dummy_count = std::env::var("CITYSIM_DUMMY_NPCS")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(1);
    let spawn_point = loaded_map
        .definition
        .player_spawn
        .unwrap_or(shared::player::SPAWN_POSITION);
    for i in 0..dummy_count {
        let x = spawn_point[0] - 6.0 - 2.0 * i as f32;
        let z = spawn_point[2] - 8.0;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let pos = Vec3::new(x, y, z);
        let npc_id = next_npc_id;
        next_npc_id = next_npc_id.saturating_add(1);

        // Stands still (huge idle timer) so it's a stable shooting reference.
        let mut wander = NpcWander::new(pos, 1.0, npc_id);
        wander.idle_timer = 9999.0;

        commands.spawn((
            Npc {
                id: npc_id,
                archetype: NpcArchetype::CombatDummy,
            },
            npc_identity_for_archetype(WORLD_SEED, npc_id, NpcArchetype::CombatDummy),
            NpcPosition(pos),
            NpcRotation(0.0),
            NpcVelocity(Vec3::ZERO),
            NpcActivity(NpcActivityKind::Idle),
            Health::new(npc_max_health(NpcArchetype::CombatDummy)),
            wander,
            ReplicationGroup::new_from_entity().set_priority(NPC_REPLICATION_PRIORITY),
            NetworkVisibility,
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        total_spawned += 1;
    }
    if dummy_count > 0 {
        info!("Spawned {dummy_count} anatomical combat dummy target(s) near spawn");
    }

    let configured_cap = configured_npc_cap();
    let mut remaining = configured_cap;
    if remaining == 0 {
        warn!("CITYSIM_MAX_NPCS is set to 0; skipping NPC spawn");
        return;
    }

    for (group_index, group) in npc_groups.iter().enumerate() {
        if remaining == 0 {
            break;
        }

        let requested_count = group.count;
        let spawn_count = requested_count.min(remaining);
        if spawn_count < requested_count {
            warn!(
                "Capping NPC group {} from {} to {} (CITYSIM_MAX_NPCS={})",
                group_index, requested_count, spawn_count, remaining,
            );
        }

        let mut rng = XorShift64::new(
            (WORLD_SEED as u64)
                ^ ((group_index as u64 + 1) * 0x9E37_79B9u64)
                ^ (spawn_count as u64 * 0xA5A5_5A5Au64),
        );

        for i in 0..spawn_count {
            let (x, z) = match group.preset {
                MapBehaviorPreset::IdleWanderZone | MapBehaviorPreset::StandAndFaceFlow => {
                    let rx = (rng.next_f32() * 2.0 - 1.0) * group.zone_half_extents[0];
                    let rz = (rng.next_f32() * 2.0 - 1.0) * group.zone_half_extents[1];
                    (group.zone_center[0] + rx, group.zone_center[1] + rz)
                }
                MapBehaviorPreset::PatrolRoute => {
                    if group.route.is_empty() {
                        (group.zone_center[0], group.zone_center[1])
                    } else {
                        let wp = group.route[(i as usize) % group.route.len()];
                        (wp[0], wp[1])
                    }
                }
            };

            let y = terrain.get_height(x, z) + ground_clearance_center();
            let pos = Vec3::new(x, y, z);
            let npc_id = next_npc_id;
            next_npc_id = next_npc_id.saturating_add(1);

            let mut wander = NpcWander::new(pos, 20.0, npc_id);
            if matches!(group.preset, MapBehaviorPreset::StandAndFaceFlow) {
                wander.idle_timer = 9999.0;
            }

            commands.spawn((
                Npc {
                    id: npc_id,
                    archetype: group.archetype,
                },
                npc_identity_for_group(WORLD_SEED, npc_id, group),
                NpcPosition(pos),
                NpcRotation(0.0),
                NpcVelocity(Vec3::ZERO),
                NpcActivity(NpcActivityKind::Idle),
                Health::new(npc_max_health(group.archetype)),
                wander,
                ReplicationGroup::new_from_entity().set_priority(NPC_REPLICATION_PRIORITY),
                NetworkVisibility,
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            ));

            total_spawned += 1;
            remaining = remaining.saturating_sub(1);
        }
    }

    if total_spawned == 0 {
        warn!(
            "No npc_groups authored in map {}; spawning one fallback NPC",
            loaded_map.definition.map_id
        );

        if remaining == 0 {
            return;
        }

        let x = 0.0;
        let z = 0.0;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let npc_id = next_npc_id;
        let pos = Vec3::new(x, y, z);
        commands.spawn((
            Npc {
                id: npc_id,
                archetype: NpcArchetype::Oilman,
            },
            npc_identity_for_archetype(WORLD_SEED, npc_id, NpcArchetype::Oilman),
            NpcPosition(pos),
            NpcRotation(0.0),
            NpcVelocity(Vec3::ZERO),
            NpcActivity(NpcActivityKind::Idle),
            Health::new(npc_max_health(NpcArchetype::Oilman)),
            NpcWander::new(pos, 12.0, npc_id),
            ReplicationGroup::new_from_entity().set_priority(NPC_REPLICATION_PRIORITY),
            NetworkVisibility,
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        total_spawned = 1;
        next_npc_id = next_npc_id.saturating_add(1);
        remaining = remaining.saturating_sub(1);
    }

    // For stress-testing with CITYSIM_MAX_NPCS above authored counts, top-up NPCs
    // with deterministic random positions/archetypes within map bounds.
    if remaining > 0 && configured_cap != u32::MAX {
        let mut rng =
            XorShift64::new((WORLD_SEED as u64) ^ 0xD6E8_FD9Au64 ^ (total_spawned as u64));
        while remaining > 0 {
            let x =
                world_bounds.min[0] + rng.next_f32() * (world_bounds.max[0] - world_bounds.min[0]);
            let z =
                world_bounds.min[1] + rng.next_f32() * (world_bounds.max[1] - world_bounds.min[1]);
            let y = terrain.get_height(x, z) + ground_clearance_center();
            let npc_id = next_npc_id;
            next_npc_id = next_npc_id.saturating_add(1);

            let archetype = choose_top_up_archetype(npc_groups, &mut rng);
            let pos = Vec3::new(x, y, z);

            commands.spawn((
                Npc {
                    id: npc_id,
                    archetype,
                },
                npc_identity_for_archetype(WORLD_SEED, npc_id, archetype),
                NpcPosition(pos),
                NpcRotation(0.0),
                NpcVelocity(Vec3::ZERO),
                NpcActivity(NpcActivityKind::Idle),
                Health::new(npc_max_health(archetype)),
                NpcWander::new(pos, 20.0, npc_id),
                ReplicationGroup::new_from_entity().set_priority(NPC_REPLICATION_PRIORITY),
                NetworkVisibility,
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            ));

            total_spawned += 1;
            remaining = remaining.saturating_sub(1);
        }
    }

    info!(
        "Spawned {} NPCs from authored map {}",
        total_spawned, loaded_map.definition.map_id
    );
}

/// Handle debug requests to spawn Oilman NPCs near the requesting player.
pub fn handle_spawn_oilman_debug(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    player_index: Res<PlayerEntityIndex>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<SpawnOilmanDebug>), With<ClientOf>>,
    player_positions: Query<&PlayerPosition, With<Player>>,
    npcs: Query<&Npc>,
) {
    let mut next_npc_id: Option<u64> = None;

    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        let mut total_spawned_for_peer = 0usize;
        let mut requests_for_peer = 0usize;
        for msg in receiver.receive() {
            requests_for_peer = requests_for_peer.saturating_add(1);
            let count = msg.count.clamp(1, 200) as usize;
            if count != msg.count as usize {
                warn!(
                    "Debug NPC spawn count clamped for {:?}: requested {}, using {}",
                    peer_id, msg.count, count
                );
            }

            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                continue;
            };
            let Ok(player_pos) = player_positions.get(player_entity) else {
                continue;
            };

            let cursor = next_npc_id.get_or_insert_with(|| {
                npcs.iter()
                    .map(|npc| npc.id)
                    .max()
                    .unwrap_or(9_999)
                    .saturating_add(1)
            });

            for i in 0..count {
                let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
                let ring = 3.0 + (i / 12) as f32 * 2.0;
                let x = player_pos.0.x + angle.cos() * ring;
                let z = player_pos.0.z + angle.sin() * ring;
                let y = terrain.get_height(x, z) + ground_clearance_center();
                let pos = Vec3::new(x, y, z);

                let npc_id = *cursor;
                *cursor = cursor.saturating_add(1);

                let archetype = msg.archetype;
                // Reference dummies stand still so they stay a stable target.
                let mut wander = NpcWander::new(
                    pos,
                    if matches!(archetype, NpcArchetype::Dummy | NpcArchetype::CombatDummy) {
                        1.0
                    } else {
                        14.0
                    },
                    npc_id,
                );
                if matches!(archetype, NpcArchetype::Dummy | NpcArchetype::CombatDummy) {
                    wander.idle_timer = 9999.0;
                }
                commands.spawn((
                    Npc {
                        id: npc_id,
                        archetype,
                    },
                    npc_identity_for_debug(WORLD_SEED, npc_id, archetype),
                    NpcPosition(pos),
                    NpcRotation(0.0),
                    NpcVelocity(Vec3::ZERO),
                    NpcActivity(NpcActivityKind::Idle),
                    Health::new(npc_max_health(archetype)),
                    wander,
                    ReplicationGroup::new_from_entity().set_priority(NPC_REPLICATION_PRIORITY),
                    NetworkVisibility,
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ));
            }
            total_spawned_for_peer = total_spawned_for_peer.saturating_add(count);
        }
        if total_spawned_for_peer > 0 {
            debug!(
                "Debug spawned {} NPCs for {:?} ({} request(s) this tick)",
                total_spawned_for_peer, peer_id, requests_for_peer
            );
        }
    }
}

/// Handle debug requests to spawn dynamic physics test boxes near the requesting player.
pub fn handle_spawn_physics_box_debug(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    player_index: Res<PlayerEntityIndex>,
    mut client_links: Query<
        (&RemoteId, &mut MessageReceiver<SpawnPhysicsBoxDebug>),
        With<ClientOf>,
    >,
    player_positions: Query<&PlayerPosition, With<Player>>,
    existing_boxes: Query<&DebugPhysicsBox>,
) {
    let mut next_box_id: Option<u64> = None;

    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        for msg in receiver.receive() {
            let count = msg.count.clamp(1, 32) as usize;
            let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
                warn!(
                    "SpawnPhysicsBoxDebug ignored for {:?}: no player entity indexed yet",
                    peer_id
                );
                continue;
            };
            let Ok(player_pos) = player_positions.get(player_entity) else {
                warn!(
                    "SpawnPhysicsBoxDebug ignored for {:?}: missing player state",
                    peer_id
                );
                continue;
            };
            let mut anchor_pos = msg.anchor_position.unwrap_or(player_pos.0);
            if !anchor_pos.is_finite() {
                anchor_pos = player_pos.0;
            }
            let half_extents = Vec3::splat(0.60);

            let cursor = next_box_id.get_or_insert_with(|| {
                existing_boxes
                    .iter()
                    .map(|box_data| box_data.id)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1)
            });

            let mut first_spawn: Option<Vec3> = None;
            for i in 0..count {
                let (x, z) = if i == 0 {
                    (anchor_pos.x, anchor_pos.z)
                } else {
                    let ring_i = i - 1;
                    let angle = (ring_i as f32 / 8.0) * std::f32::consts::TAU;
                    let ring = 1.8 + (ring_i / 8) as f32 * 1.4;
                    (
                        anchor_pos.x + angle.cos() * ring,
                        anchor_pos.z + angle.sin() * ring,
                    )
                };
                let y = terrain.get_height(x, z) + half_extents.y + 0.25;
                let pos = Vec3::new(x, y, z);
                let rot = Quat::IDENTITY;
                let spawn_tf = Transform::from_translation(pos).with_rotation(rot);
                first_spawn.get_or_insert(pos);

                let box_id = *cursor;
                *cursor = cursor.saturating_add(1);

                commands
                    .spawn((
                        DebugPhysicsBox {
                            id: box_id,
                            half_extents,
                        },
                        DebugPhysicsBoxPosition(pos),
                        DebugPhysicsBoxRotation(rot),
                        spawn_tf,
                        GlobalTransform::from(spawn_tf),
                        Visibility::Inherited,
                        InheritedVisibility::default(),
                        RigidBody::Dynamic,
                        Collider::cuboid(half_extents.x, half_extents.y, half_extents.z),
                        layers::debug_box_groups(),
                        AdditionalMassProperties::Mass(24.0),
                        Damping {
                            linear_damping: 2.4,
                            angular_damping: 3.6,
                        },
                        Ccd::enabled(),
                        Velocity::default(),
                        ExternalImpulse::default(),
                    ))
                    .insert((
                        ReplicationGroup::new_from_entity()
                            .set_priority(DEBUG_BOX_REPLICATION_PRIORITY),
                        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                    ));
            }
            info!(
                "Debug spawned {} physics box(es) for {:?} near player={:?} server_player_pos={:?} request_anchor={:?} first_spawn={:?}",
                count, peer_id, player_entity, player_pos.0, anchor_pos, first_spawn
            );
        }
    }
}

/// Sync replicated debug box position/rotation from physics transform.
pub fn sync_debug_physics_boxes(
    _time: Res<Time>,
    _terrain: Res<WorldTerrain>,
    mut boxes: Query<(
        &DebugPhysicsBox,
        &mut DebugPhysicsBoxPosition,
        &mut DebugPhysicsBoxRotation,
        &mut Transform,
        &mut GlobalTransform,
        Option<&mut Velocity>,
    )>,
) {
    for (_box_data, mut box_pos, mut box_rot, transform, mut global_transform, _box_vel) in
        boxes.iter_mut()
    {
        box_pos.0 = transform.translation;
        box_rot.0 = transform.rotation;
        *global_transform = GlobalTransform::from(transform.compute_affine());
    }
}
