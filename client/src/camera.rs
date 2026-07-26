//! Camera constants and peer-id helpers.
//!
//! The first/third-person follow cameras died with the player body; the top-down
//! commander camera lives in [`crate::camera_rts`]. This module keeps the pieces other
//! subsystems still depend on.

use lightyear::prelude::*;

/// Near clip plane for the main 3D camera.
///
/// Read by `render::systems::rendering::setup`. Kept small so geometry very close to the
/// camera does not clip out when zoomed in.
pub(crate) const CAMERA_NEAR_CLIP: f32 = 0.001;

/// Convert a lightyear `PeerId` into a stable `u64` key.
///
/// Covers every variant: `Entity` and `Raw` used to fall into a catch-all that mapped
/// them both to `0`, which silently collided distinct peers.
pub(crate) fn peer_id_to_u64(peer_id: PeerId) -> u64 {
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
