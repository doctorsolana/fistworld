//! Streaming anchor: the world position terrain/prop/LOD systems center on.
//!
//! Priority:
//! 1. Commander camera focus (the point the view is centered on) when the
//!    top-down controller is active — panning streams the world under it.
//! 2. The local player position.
//! 3. The 3D camera translation as a last resort, so streaming still works in
//!    builds/moments where no local player entity exists.

use bevy::prelude::*;
use shared::components::{LocalPlayer, PlayerPosition};

use crate::camera_rts::CommanderCamera;

pub type AnchorPlayer<'w, 's> = Query<'w, 's, &'static PlayerPosition, With<LocalPlayer>>;
pub type AnchorCamera<'w, 's> =
    Query<'w, 's, (&'static GlobalTransform, Option<&'static CommanderCamera>), With<Camera3d>>;

/// Returns the world position to stream terrain/props/LOD around.
///
/// NOTE: this API **fails open** — every caller does `let Some(anchor) = … else { return; }`,
/// so returning `None` silently produces an empty world rather than an error. The `warn_once!`
/// below is the only signal that the anchor has been broken by a refactor.
pub fn streaming_anchor(player: &AnchorPlayer, camera: &AnchorCamera) -> Option<Vec3> {
    let camera_hit = camera.iter().next();
    if let Some((_, Some(controller))) = camera_hit {
        return Some(controller.focus);
    }
    if let Ok(player_pos) = player.single() {
        return Some(player_pos.0);
    }
    let fallback = camera_hit.map(|(transform, _)| transform.translation());
    if fallback.is_none() {
        warn_once!(
            "streaming_anchor(): no commander camera, no local player, no Camera3d — \
             terrain/prop streaming is disabled and the world will appear empty"
        );
    }
    fallback
}

/// How far the camera is from the ground it is looking at.
///
/// Streaming radii scale off this: with a top-down camera the visible ground footprint
/// grows with zoom, so anything using a fixed radius empties the screen when zoomed out.
pub fn camera_view_distance(camera: &AnchorCamera) -> f32 {
    camera
        .iter()
        .next()
        .and_then(|(_, controller)| controller.map(|c| c.zoom))
        .unwrap_or(DEFAULT_VIEW_DISTANCE)
}

/// Fallback when no commander camera exists yet (first frames, or a non-gameplay app).
const DEFAULT_VIEW_DISTANCE: f32 = 220.0;
