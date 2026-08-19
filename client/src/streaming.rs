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
use shared::terrain::{ChunkCoord, CHUNK_SIZE};

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

/// Camera plane used only to prioritize streaming work.
///
/// We deliberately keep a safety ring behind the camera so a quick orbit does
/// not expose an empty world. Bevy's renderer frustum-culls those meshes; this
/// hint merely makes chunks in front finish loading before chunks behind.
#[derive(Debug, Clone, Copy)]
pub struct StreamingViewPriority {
    position: Vec3,
    forward: Vec3,
}

pub fn streaming_view_priority(camera: &AnchorCamera) -> Option<StreamingViewPriority> {
    camera
        .iter()
        .next()
        .map(|(transform, _)| StreamingViewPriority {
            position: transform.translation(),
            forward: Vec3::from(transform.forward()),
        })
}

/// Deterministic front-first priority for a chunk around the streaming anchor.
pub fn chunk_stream_priority(
    coord: ChunkCoord,
    anchor: Vec3,
    view: Option<StreamingViewPriority>,
) -> (u8, i32, i32, i32, i32) {
    let anchor_chunk = ChunkCoord::from_world_pos(anchor);
    let dx = (coord.x - anchor_chunk.x).abs();
    let dz = (coord.z - anchor_chunk.z).abs();
    let ring = dx.max(dz);

    let (behind, forward_rank) = view.map_or((0, 0), |view| {
        let origin = coord.world_pos();
        let center = Vec3::new(
            origin.x + CHUNK_SIZE * 0.5,
            anchor.y,
            origin.z + CHUNK_SIZE * 0.5,
        );
        let forward_distance = (center - view.position).dot(view.forward);
        // Let a chunk intersect the camera plane before treating it as behind.
        // The nearest ring always remains first as an anti-pop safety core.
        let behind = u8::from(ring > 1 && forward_distance < -CHUNK_SIZE * 0.75);
        let forward_rank = (-forward_distance / CHUNK_SIZE).round() as i32;
        (behind, forward_rank)
    });

    (behind, ring, forward_rank, coord.x, coord.z)
}

/// Fallback when no commander camera exists yet (first frames, or a non-gameplay app).
const DEFAULT_VIEW_DISTANCE: f32 = 220.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_priority_keeps_near_core_then_prefers_camera_front() {
        let anchor = Vec3::ZERO;
        let view = Some(StreamingViewPriority {
            position: Vec3::new(0.0, 100.0, 100.0),
            forward: Vec3::new(0.0, -1.0, -1.0).normalize(),
        });
        let near = chunk_stream_priority(ChunkCoord::new(0, 0), anchor, view);
        let ahead = chunk_stream_priority(ChunkCoord::new(0, -4), anchor, view);
        let behind = chunk_stream_priority(ChunkCoord::new(0, 4), anchor, view);

        assert_eq!(near.0, 0);
        assert_eq!(ahead.0, 0);
        assert_eq!(behind.0, 1);
        assert!(near < behind);
        assert!(ahead < behind);
    }
}
