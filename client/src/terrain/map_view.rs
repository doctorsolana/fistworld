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
use bevy::pbr::{DistanceFog, FogFalloff};
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

/// How far into map view the camera is, `0.0..=1.0`. Drives the fog fade.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct MapViewBlend(pub f32);

pub fn update_map_view_state(cameras: Query<&CommanderCamera>, mut blend: ResMut<MapViewBlend>) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    blend.0 = ((camera.zoom - DETAIL_FADE_START) / (DETAIL_FADE_END - DETAIL_FADE_START))
        .clamp(0.0, 1.0);
}

/// Fade distance fog out as the camera pulls back.
///
/// Aerial haze sells depth at ground level, but at map scale it is integrated over
/// kilometres and turns the whole map into a grey-brown wash. A map is meant to be
/// legible, so the haze is dialled out exactly as the detail chunks fade.
pub fn fade_fog_for_map_view(blend: Res<MapViewBlend>, mut fog: Query<&mut DistanceFog>) {
    if !blend.is_changed() {
        return;
    }

    let clear = 1.0 - blend.0;
    for mut fog in fog.iter_mut() {
        fog.color.set_alpha(FOG_BASE_ALPHA * clear);
        // Rebuilt from the authored values every time rather than scaled in place —
        // multiplying the live value would compound each frame and drive fog to zero.
        fog.falloff = FogFalloff::from_visibility_colors(
            FOG_VISIBILITY_METERS / clear.max(0.02),
            Color::srgb(0.70, 0.80, 0.90),
            Color::srgb(0.88, 0.92, 0.96),
        );
    }
}

/// Fog values authored for ground level; map view scales down from these.
const FOG_BASE_ALPHA: f32 = 0.05;
const FOG_VISIBILITY_METERS: f32 = 2_000.0;
