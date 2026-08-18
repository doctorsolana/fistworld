//! Map view — how the world degrades naturally into a map as the camera pulls back.
//!
//! Close up, terrain is streamed chunks using the splat shader plus a separate water
//! surface; far out, the whole map is one low-resolution vertex-coloured mesh. The wrong
//! way to switch between them is a global visibility toggle: it is stateful (chunks that
//! stream in *after* the flip get missed, which showed up as a square of high-detail
//! water floating in the map), and it snaps the entire view at once.
//!
//! Terrain chunks use Bevy's per-entity dithered [`VisibilityRange`]. Water is a broad,
//! translucent surface where that 4x4 discard pattern is conspicuous, so its shader uses
//! a continuous alpha fade and its range only performs an abrupt, already-transparent
//! CPU cull.
//!
//! The far mesh sits ~5cm below the detail chunks. Detailed land retains a cut-out hole
//! to prevent coarse geometry poking through, while far ocean remains beneath the water
//! fade at every zoom.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use crate::camera_rts::CommanderCamera;

/// Camera distance at which detail chunks begin dithering into the far mesh.
pub const DETAIL_FADE_START: f32 = 1_050.0;

/// Camera distance at which detail chunks are fully gone and only the map remains.
pub const DETAIL_FADE_END: f32 = 1_550.0;

/// Water starts handing off sooner than terrain because its animated foam and
/// glints make a whole-chunk streaming edge much easier to see. At the usual
/// RTS camera height this leaves the centre fully detailed while the outer
/// water ring dissolves continuously into the matching far surface.
pub const WATER_FADE_START: f32 = 650.0;
pub const WATER_FADE_END: f32 = 1_000.0;

/// Abrupt CPU cull after the shader has already reached zero alpha. The extra
/// margin covers a 64m chunk's diagonal, preventing a chunk-center cull from
/// cutting off a still-visible edge.
pub const WATER_CULL_DISTANCE: f32 = 1_100.0;

/// Zoom past which the far mesh stops cutting a hole under the streamed chunks.
///
/// Keep this shortly below [`DETAIL_FADE_START`]: the far mesh must be solid
/// before chunks begin dithering, but enabling it hundreds of metres earlier
/// leaves two differently tessellated land surfaces competing at ordinary RTS
/// zooms. The short lead-in gives the renderer several wheel steps to prepare
/// the fallback without exposing that overlap during close play.
pub const HOLE_FILL_ZOOM: f32 = DETAIL_FADE_START - 100.0;

const _: () = assert!(WATER_CULL_DISTANCE > WATER_FADE_END);
const _: () = assert!(HOLE_FILL_ZOOM < DETAIL_FADE_START);
const _: () = assert!(DETAIL_FADE_START - HOLE_FILL_ZOOM <= 100.0);

/// Distance fade for a streamed terrain chunk.
///
/// `use_aabb` matters here: chunks are 64m slabs, and fading on the centre point would
/// make a chunk under the screen edge pop earlier than one under the cursor.
pub fn detail_visibility_range() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: DETAIL_FADE_START..DETAIL_FADE_END,
        use_aabb: true,
    }
}

/// Distance fade for the water surface.
///
/// Water performs its visible transition with ordinary alpha in toon_water.wgsl.
/// This range is deliberately abrupt and begins only after that fade is fully
/// transparent: enabling Bevy's crossfade here adds a conspicuous 4x4 checker
/// pattern to broad water surfaces.
pub fn water_visibility_range() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: WATER_CULL_DISTANCE..WATER_CULL_DISTANCE,
        use_aabb: true,
    }
}

/// How far into map view the camera is, `0.0..=1.0`.
///
/// The single fog writer (`update_day_night_cycle`) reads this to dial the
/// aerial haze out at map scale; a second system writing `DistanceFog` here
/// would race it (unordered double-writes made the winner scheduler-dependent).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct MapViewBlend(pub f32);

pub fn update_map_view_state(cameras: Query<&CommanderCamera>, mut blend: ResMut<MapViewBlend>) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let next =
        ((camera.zoom - DETAIL_FADE_START) / (DETAIL_FADE_END - DETAIL_FADE_START)).clamp(0.0, 1.0);
    if blend.0 != next {
        blend.0 = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_range_culls_only_after_shader_fade_without_dithering() {
        let range = water_visibility_range();
        assert!(range.is_abrupt());
        assert_eq!(range.end_margin, WATER_CULL_DISTANCE..WATER_CULL_DISTANCE);
    }
}
