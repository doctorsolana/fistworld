//! NPC spawn systems.

use bevy::prelude::*;
use lightyear::prelude::server::{ClientOf, Started};
use lightyear::prelude::*;
use shared::components::{
    Health, Npc, NpcArchetype, NpcIdentity, NpcPosition, NpcRotation, Player, PlayerPosition,
};
use shared::map::MapBehaviorPreset;
use shared::npc::{npc_max_health, npc_name_for_id};
use shared::physics::ground_clearance_center;
use shared::protocol::SpawnOilmanDebug;
use shared::terrain::{WorldTerrain, WORLD_SEED};

use crate::ai::state::{NpcWander, XorShift64};
use crate::player::index::PlayerEntityIndex;

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
                NpcIdentity {
                    name: npc_name_for_id(WORLD_SEED, npc_id),
                    occupation: match group.preset {
                        MapBehaviorPreset::IdleWanderZone => "Pedestrian".to_string(),
                        MapBehaviorPreset::PatrolRoute => "Patrol".to_string(),
                        MapBehaviorPreset::StandAndFaceFlow => "Vendor".to_string(),
                    },
                    faction: None,
                },
                NpcPosition(pos),
                NpcRotation(0.0),
                Health::new(npc_max_health(group.archetype)),
                wander,
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
            NpcIdentity {
                name: npc_name_for_id(WORLD_SEED, npc_id),
                occupation: "Pedestrian".to_string(),
                faction: None,
            },
            NpcPosition(pos),
            NpcRotation(0.0),
            Health::new(npc_max_health(NpcArchetype::Oilman)),
            NpcWander::new(pos, 12.0, npc_id),
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
                NpcIdentity {
                    name: npc_name_for_id(WORLD_SEED, npc_id),
                    occupation: "Pedestrian".to_string(),
                    faction: None,
                },
                NpcPosition(pos),
                NpcRotation(0.0),
                Health::new(npc_max_health(archetype)),
                NpcWander::new(pos, 20.0, npc_id),
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

                commands.spawn((
                    Npc {
                        id: npc_id,
                        archetype: NpcArchetype::Oilman,
                    },
                    NpcIdentity {
                        name: npc_name_for_id(WORLD_SEED, npc_id),
                        occupation: "Debug Spawn".to_string(),
                        faction: None,
                    },
                    NpcPosition(pos),
                    NpcRotation(0.0),
                    Health::new(npc_max_health(NpcArchetype::Oilman)),
                    NpcWander::new(pos, 14.0, npc_id),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ));
            }
            total_spawned_for_peer = total_spawned_for_peer.saturating_add(count);
        }
        if total_spawned_for_peer > 0 {
            debug!(
                "Debug spawned {} Oilman NPCs for {:?} ({} request(s) this tick)",
                total_spawned_for_peer, peer_id, requests_for_peer
            );
        }
    }
}
