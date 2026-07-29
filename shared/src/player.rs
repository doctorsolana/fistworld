//! Player-related constants and types

/// Player movement speed (units per second)
pub const PLAYER_SPEED: f32 = 8.0;

/// Hero walk speed (m/s). Tuned to the voxel character's 1.04s walk cycle so
/// the feet don't skate; the client scales animation speed from actual
/// velocity, so small changes here stay in sync automatically.
pub const HERO_MOVE_SPEED: f32 = 3.2;
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

/// Sprint speed multiplier when holding Shift on foot.
pub const PLAYER_SPRINT_MULT: f32 = 1.55;

/// Player height (for capsule)
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Player radius (for capsule)
pub const PLAYER_RADIUS: f32 = 0.3;

/// Minimum time to show jump animation (seconds).
pub const JUMP_ANIM_MIN_SECS: f32 = 0.18;

/// Maximum height a player can step up onto (small rocks, curbs, etc.)
pub const STEP_UP_HEIGHT: f32 = 0.4;

/// Mouse sensitivity for look
pub const MOUSE_SENSITIVITY: f32 = 0.003;

/// Spawn position for new players (spawn above terrain to prevent clipping)
pub const SPAWN_POSITION: [f32; 3] = [0.0, 10.0, 0.0];

/// Maximum player health
pub const PLAYER_MAX_HEALTH: f32 = 100.0;

/// Time in seconds before a dead player can respawn
pub const RESPAWN_TIME: f32 = 4.0;
