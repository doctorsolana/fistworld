//! Death and respawn lifecycle.

use bevy::prelude::*;
use shared::components::{Health, Player, PlayerPosition, PlayerVelocity};
use shared::physics::ground_clearance_center;
use shared::player::{RESPAWN_TIME, SPAWN_POSITION};
use shared::protocol::FIXED_TIMESTEP_HZ;
use shared::terrain::WorldTerrain;


/// Component added to dead players while waiting to respawn.
#[derive(Component)]
pub struct RespawnTimer {
    pub time_remaining: f32,
}

fn resolve_map_spawn_position(terrain: &WorldTerrain) -> Vec3 {
    if let Some(spawn) = terrain.generator.loaded_map().definition.player_spawn {
        let ground_y = terrain.get_height(spawn[0], spawn[2]);
        return Vec3::new(spawn[0], ground_y + ground_clearance_center(), spawn[2]);
    }

    let spawn_x = SPAWN_POSITION[0];
    let spawn_z = SPAWN_POSITION[2];
    let ground_y = terrain.get_height(spawn_x, spawn_z);
    Vec3::new(spawn_x, ground_y + ground_clearance_center(), spawn_z)
}

/// Check for dead players and add respawn timer.
pub fn handle_player_deaths(
    mut commands: Commands,
    players: Query<(Entity, &Player, &Health), (Without<RespawnTimer>,)>,
) {
    for (entity, player, health) in players.iter() {
        if health.is_dead() {
            info!("Player {:?} died! Starting respawn timer", player.client_id);

            commands.entity(entity).insert(RespawnTimer {
                time_remaining: RESPAWN_TIME,
            });
        }
    }
}

/// Tick respawn timers and respawn players when ready.
pub fn update_respawn_timers(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut players: Query<(
        Entity,
        &Player,
        &mut Health,
        &mut PlayerPosition,
        &mut PlayerVelocity,
        &mut RespawnTimer,
    )>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (entity, player, mut health, mut position, mut velocity, mut timer) in players.iter_mut() {
        timer.time_remaining -= dt;

        if timer.time_remaining <= 0.0 {
            info!("Respawning player {:?}", player.client_id);

            health.current = health.max;

            position.0 = resolve_map_spawn_position(&terrain);

            velocity.0 = Vec3::ZERO;

            commands.entity(entity).remove::<RespawnTimer>();
        }
    }
}

/// Skip input processing for dead players.
pub(crate) fn is_player_alive(health: &Health, respawn_timer: Option<&RespawnTimer>) -> bool {
    !health.is_dead() && respawn_timer.is_none()
}
