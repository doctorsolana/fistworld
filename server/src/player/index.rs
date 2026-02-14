//! Fast peer<->entity lookup index for player entities.

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use std::collections::HashMap;

use shared::components::Player;

/// Runtime index mapping player peers to ECS entities.
#[derive(Resource, Default)]
pub struct PlayerEntityIndex {
    by_peer: HashMap<PeerId, Entity>,
    pub version: u64,
}

impl PlayerEntityIndex {
    #[inline]
    pub fn entity_for_peer(&self, peer_id: PeerId) -> Option<Entity> {
        self.by_peer.get(&peer_id).copied()
    }
}

/// Rebuild the player entity lookup index once per fixed tick.
pub fn sync_player_entity_index(
    mut index: ResMut<PlayerEntityIndex>,
    players: Query<(Entity, &Player)>,
) {
    index.by_peer.clear();

    for (entity, player) in players.iter() {
        index.by_peer.insert(player.client_id, entity);
    }

    index.version = index.version.wrapping_add(1);
}
