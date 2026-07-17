//! Distance-based NPC replication relevance.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::*;
use std::collections::HashMap;

use shared::components::{Npc, NpcPosition, Player, PlayerPosition};
use shared::protocol::{NpcRagdollStarted, ReliableChannel};

use crate::ai::ragdoll::{build_started_message, CorpseLifecycle, NpcRagdollBodies};

const DEFAULT_RELEVANCE_RADIUS: f32 = 600.0;
const DEFAULT_RELEVANCE_EXIT_RADIUS: f32 = 680.0;
const DEFAULT_RELEVANCE_UPDATE_HZ: f32 = 10.0;

#[derive(Resource, Clone, Debug)]
pub struct NpcRelevanceSettings {
    radius: f32,
    exit_radius: f32,
    update_interval: f32,
}

#[inline]
fn is_within_horizontal_radius(origin: Vec3, target: Vec3, radius_sq: f32) -> bool {
    let delta = Vec2::new(target.x - origin.x, target.z - origin.z);
    delta.length_squared() <= radius_sq
}

impl Default for NpcRelevanceSettings {
    fn default() -> Self {
        let radius = std::env::var("CITYSIM_NPC_RELEVANCE_RADIUS")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(DEFAULT_RELEVANCE_RADIUS)
            .clamp(64.0, 4000.0);
        let exit_radius = std::env::var("CITYSIM_NPC_RELEVANCE_EXIT_RADIUS")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(DEFAULT_RELEVANCE_EXIT_RADIUS)
            .clamp(radius, 4400.0);
        let update_hz = std::env::var("CITYSIM_NPC_RELEVANCE_HZ")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(DEFAULT_RELEVANCE_UPDATE_HZ)
            .clamp(1.0, 60.0);
        Self {
            radius,
            exit_radius,
            update_interval: 1.0 / update_hz,
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn update_npc_network_visibility(
    time: Res<Time>,
    settings: Res<NpcRelevanceSettings>,
    mut elapsed: Local<f32>,
    players: Query<(&PlayerPosition, &ControlledBy), With<Player>>,
    mut clients: Query<
        (Entity, &mut MessageSender<NpcRagdollStarted>),
        (With<ClientOf>, With<Connected>),
    >,
    mut npcs: Query<
        (
            &Npc,
            &NpcPosition,
            &mut ReplicationState,
            Option<(&CorpseLifecycle, &NpcRagdollBodies)>,
        ),
        (With<Npc>, With<NetworkVisibility>),
    >,
    body_transforms: Query<&Transform>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < settings.update_interval {
        return;
    }
    *elapsed %= settings.update_interval;

    let player_positions: HashMap<Entity, Vec3> = players
        .iter()
        .map(|(position, controlled_by)| (controlled_by.owner, position.0))
        .collect();
    let enter_sq = settings.radius * settings.radius;
    let exit_sq = settings.exit_radius * settings.exit_radius;

    for (npc, position, mut replication, corpse) in npcs.iter_mut() {
        for (client_entity, mut ragdoll_sender) in clients.iter_mut() {
            let was_visible = replication.is_visible(client_entity);
            let max_distance_sq = if was_visible { exit_sq } else { enter_sq };
            let should_be_visible =
                player_positions
                    .get(&client_entity)
                    .is_some_and(|player_pos| {
                        is_within_horizontal_radius(position.0, *player_pos, max_distance_sq)
                    });

            match (was_visible, should_be_visible) {
                (false, true) => {
                    replication.gain_visibility(client_entity);
                    if let Some((lifecycle, bindings)) = corpse {
                        if let Some(message) =
                            build_started_message(npc, lifecycle, bindings, &body_transforms)
                        {
                            ragdoll_sender.send::<ReliableChannel>(message);
                        }
                    }
                }
                (true, false) => replication.lose_visibility(client_entity),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevance_distance_ignores_height_and_honors_boundary() {
        let npc = Vec3::new(10.0, -500.0, 20.0);

        assert!(is_within_horizontal_radius(
            npc,
            Vec3::new(13.0, 900.0, 24.0),
            25.0
        ));
        assert!(!is_within_horizontal_radius(
            npc,
            Vec3::new(13.1, 900.0, 24.0),
            25.0
        ));
    }
}
