//! Streaming anchor: the world position terrain/prop/LOD systems center on.
//!
//! Priority:
//! 1. RTS rail camera focus (the point the player is looking at) when the RTS
//!    controller is active — panning the camera streams the world under it.
//! 2. The local player position (FPS build).
//! 3. The 3D camera translation as a last resort, so streaming still works in
//!    builds/moments where no local player entity exists.

use bevy::prelude::*;
use shared::components::{LocalPlayer, PlayerPosition};

use crate::rail::RtsRailCamera;

pub type AnchorPlayer<'w, 's> = Query<'w, 's, &'static PlayerPosition, With<LocalPlayer>>;
pub type AnchorCamera<'w, 's> = Query<
    'w,
    's,
    (&'static GlobalTransform, Option<&'static RtsRailCamera>),
    With<Camera3d>,
>;

pub fn streaming_anchor(player: &AnchorPlayer, camera: &AnchorCamera) -> Option<Vec3> {
    let camera_hit = camera.iter().next();
    if let Some((_, Some(controller))) = camera_hit {
        return Some(controller.focus);
    }
    if let Ok(player_pos) = player.single() {
        return Some(player_pos.0);
    }
    camera_hit.map(|(transform, _)| transform.translation())
}
