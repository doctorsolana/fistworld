//! NPC spawn systems.

use bevy::prelude::*;
use lightyear::prelude::server::Started;
use lightyear::prelude::*;
use shared::components::{Health, Npc, NpcArchetype, NpcIdentity, NpcPosition, NpcRotation};
use shared::map::MapBehaviorPreset;
use shared::npc::{npc_max_health, npc_name_for_id};
use shared::physics::ground_clearance_center;
use shared::terrain::{WorldTerrain, WORLD_SEED};

use crate::ai::state::{NpcWander, XorShift64};

fn configured_npc_cap() -> u32 {
    match std::env::var("CITYSIM_MAX_NPCS") {
        Ok(raw) => match raw.parse::<u32>() {
            Ok(0) => 0,
            Ok(value) => value,
            Err(_) => {
                warn!(
                    "Invalid CITYSIM_MAX_NPCS='{}'; using map-authored counts",
                    raw
                );
                u32::MAX
            }
        },
        Err(_) => u32::MAX,
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
