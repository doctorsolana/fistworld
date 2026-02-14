//! Dead NPC cleanup systems.

use bevy::prelude::*;
use shared::components::{Health, Npc};
use shared::npc::DEAD_NPC_DESPAWN_TIME;
use shared::protocol::FIXED_TIMESTEP_HZ;

/// Timer component for tracking how long an NPC has been dead.
/// When the timer reaches 0, the NPC entity will be despawned.
#[derive(Component)]
pub struct DeadNpcDespawnTimer(pub f32);

/// Add despawn timer to newly dead NPCs.
pub fn ensure_dead_npc_despawn_timers(
    mut commands: Commands,
    dead_npcs: Query<(Entity, &Health), (With<Npc>, Without<DeadNpcDespawnTimer>)>,
) {
    for (entity, health) in dead_npcs.iter() {
        if health.is_dead() {
            commands
                .entity(entity)
                .insert(DeadNpcDespawnTimer(DEAD_NPC_DESPAWN_TIME));
            trace!("Added despawn timer to dead NPC {:?}", entity);
        }
    }
}

/// Tick down despawn timers and remove NPCs that have been dead long enough.
pub fn update_dead_npc_despawn_timers(
    mut commands: Commands,
    mut dead_npcs: Query<(Entity, &Npc, &mut DeadNpcDespawnTimer)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (entity, npc, mut timer) in dead_npcs.iter_mut() {
        timer.0 -= dt;

        if timer.0 <= 0.0 {
            info!(
                "Despawning dead NPC {} ({:?}) after timeout",
                npc.id, npc.archetype
            );
            commands.entity(entity).despawn();
        }
    }
}
