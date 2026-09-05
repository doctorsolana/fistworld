//! Player-related constants and types

/// Hero walk speed (m/s). Tuned to the voxel character's 1.04s walk cycle so
/// the feet don't skate; the client scales animation speed from actual
/// velocity, so small changes here stay in sync automatically.
pub const HERO_MOVE_SPEED: f32 = 3.52;
/// Distance at which a hero move order counts as arrived.
pub const HERO_ARRIVE_EPSILON: f32 = 0.15;

/// Convert a `PeerId` into a stable `u64` for ownership/index keys.
/// (Client compares `Hero::owner` against its `LocalPeerId(u64)`.)
pub fn peer_id_to_u64(peer_id: lightyear::prelude::PeerId) -> u64 {
    use lightyear::prelude::PeerId;
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        PeerId::Entity(id) => id,
        PeerId::Raw(addr) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            addr.hash(&mut hasher);
            hasher.finish()
        }
        PeerId::Server => 0,
    }
}

/// Player height (for capsule)
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Spawn position for new players (spawn above terrain to prevent clipping)
pub const SPAWN_POSITION: [f32; 3] = [0.0, 10.0, 0.0];
