//! Map view — how the world degrades naturally into a map as the camera pulls back.
//!
//! Close up, terrain is streamed chunks using the splat shader plus a separate water
//! surface; far out, the whole map is one low-resolution vertex-coloured mesh. The wrong
//! way to switch between them is a global visibility toggle: it is stateful (chunks that
//! stream in *after* the flip get missed, which showed up as a square of high-detail
//! water floating in the map), and it snaps the entire view at once.
//!
//! Instead every detail chunk carries a [`VisibilityRange`], Bevy's per-entity distance
//! fade with built-in dithered crossfade. Each chunk fades out individually as *its own*
//! distance to the camera crosses the band, so the detail boundary is a soft radial
//! gradient that tracks the camera — the Google Maps feel — and freshly streamed chunks
//! are handled automatically because the range rides on the entity itself.
//!
//! The far mesh sits ~5cm below the detail chunks. Its cut-out hole must be filled
//! *before* the chunks start fading (see `HOLE_FILL_ZOOM`), otherwise the dither
//! reveals void instead of map.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use crate::camera_rts::CommanderCamera;

/// Camera distance at which detail chunks begin dithering into the far mesh.
pub const DETAIL_FADE_START: f32 = 1_050.0;

/// Camera distance at which detail chunks are fully gone and only the map remains.
pub const DETAIL_FADE_END: f32 = 1_550.0;

/// Zoom past which the far mesh stops cutting a hole under the streamed chunks.
///
/// Deliberately below [`DETAIL_FADE_START`]: the far mesh must already be solid
/// underneath before any chunk starts to dither out.
pub const HOLE_FILL_ZOOM: f32 = 950.0;

/// Distance fade for a streamed detail chunk (terrain or water).
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
/// The SAME band as the terrain chunks, deliberately: the detail terrain includes
/// the sea floor, so if the water melts away earlier there is a zoom band where
/// bare sand floor dithers against the far mesh's baked ocean — every coastal
/// chunk becomes a square patch flickering between "land" and "water" as the
/// camera moves. Fading water and floor together keeps the coast reading as
/// water on both sides of the crossfade. (Water rides the terrain chunk set —
/// `LoadedChunks` — so their footprints already match; an earlier version had
/// its own smaller radius, which is why this band used to be earlier.)
pub fn water_visibility_range() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: DETAIL_FADE_START..DETAIL_FADE_END,
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
    let next = ((camera.zoom - DETAIL_FADE_START) / (DETAIL_FADE_END - DETAIL_FADE_START))
        .clamp(0.0, 1.0);
    if blend.0 != next {
        blend.0 = next;
    }
}
