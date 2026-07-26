//! Commander view synchronisation.
//!
//! The commander has no body. Its `PlayerPosition` is the world point its camera is
//! centered on and `PlayerRotation` is the camera yaw — the client owns both, the server
//! stores and replicates them.
//!
//! This is load-bearing beyond cosmetics: `physics::terrain_colliders::gather_centers`
//! streams collider chunks around `PlayerPosition`, and it is the only anchor left. If
//! this system stops running, colliders silently collapse to chunk (0,0) — no crash,
//! no log, just raycasts passing through distant hills.

use bevy::prelude::*;

use shared::components::{Player, PlayerPosition, PlayerRotation};

use crate::net::input::ClientInputs;

/// Write each commander's replicated view state from its latest input message.
pub fn sync_commander_views(
    inputs: Res<ClientInputs>,
    mut commanders: Query<(&Player, &mut PlayerPosition, &mut PlayerRotation)>,
) {
    for (player, mut position, mut rotation) in commanders.iter_mut() {
        let Some(input) = inputs.latest.get(&player.client_id) else {
            continue;
        };

        if !input.focus.is_finite() || !input.yaw.is_finite() {
            continue;
        }

        // Change detection drives replication, so only touch these when they move.
        if position.0 != input.focus {
            position.0 = input.focus;
        }
        if rotation.0 != input.yaw {
            rotation.0 = input.yaw;
        }
    }
}
