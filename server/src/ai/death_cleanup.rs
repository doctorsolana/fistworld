//! Dead NPC cleanup systems.

use bevy::prelude::*;
use shared::components::{Health, Npc};
use shared::npc::DEAD_NPC_DESPAWN_TIME;
use shared::protocol::FIXED_TIMESTEP_HZ;

use crate::ai::ragdoll::{despawn_npc_with_bodies, CorpseLifecycle, NpcRagdollBodies};

/// Timer component for tracking how long an NPC has been dead.
/// When the timer reaches 0, the NPC entity will be despawned.
#[derive(Component)]
pub struct DeadNpcDespawnTimer(pub f32);

/// Add despawn timer to newly dead NPCs.
pub fn ensure_dead_npc_despawn_timers(
    mut commands: Commands,
    time: Res<Time>,
    dead_npcs: Query<
        (Entity, &Health, Option<&CorpseLifecycle>),
        (With<Npc>, Without<DeadNpcDespawnTimer>),
    >,
) {
    for (entity, health, lifecycle) in dead_npcs.iter() {
        if health.is_dead() {
            let timeout = lifecycle
                .map(|state| (state.despawn_at - time.elapsed_secs()).max(0.0))
                .unwrap_or(DEAD_NPC_DESPAWN_TIME);
            commands.entity(entity).insert(DeadNpcDespawnTimer(timeout));
            trace!("Added despawn timer to dead NPC {:?}", entity);
        }
    }
}

/// Tick down despawn timers and remove NPCs that have been dead long enough.
pub fn update_dead_npc_despawn_timers(
    mut commands: Commands,
    mut dead_npcs: Query<(
        Entity,
        &Npc,
        &mut DeadNpcDespawnTimer,
        Option<&NpcRagdollBodies>,
    )>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (entity, npc, mut timer, ragdoll_bodies) in dead_npcs.iter_mut() {
        timer.0 -= dt;

        if timer.0 <= 0.0 {
            info!(
                "Despawning dead NPC {} ({:?}) after timeout",
                npc.id, npc.archetype
            );
            despawn_npc_with_bodies(&mut commands, entity, ragdoll_bodies);
        }
    }
}
