//! Map view — what the world looks like when you pull back to realm scale.
//!
//! Close up, terrain is streamed chunks using the splat shader with a separate water
//! surface. Far out, the whole map is one low-resolution mesh with baked vertex colours.
//! Both existing at once is the problem this module solves: the streamed square sat
//! inside the far mesh looking obviously different — sharper, differently lit, with real
//! water — so a zoomed-out view showed a patch of "real" world floating in a map.
//!
//! Past a zoom threshold the detailed layer is hidden entirely and only the far mesh
//! draws, so the whole world reads at one consistent level of detail. It is also much
//! cheaper: at realm scale the streamed chunks are sub-pixel detail nobody can see.

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::camera_rts::CommanderCamera;
use crate::terrain::TerrainChunk;
use crate::water::WaterChunk;

/// Camera distance at which the detailed layer stops being worth drawing.
///
/// Below this the streamed chunks dominate the frame; above it they are a small
/// high-detail island inside the far mesh, which reads as an inconsistency rather than
/// as detail.
const MAP_VIEW_ZOOM: f32 = 2_200.0;

/// Zoom over which the transition happens, so fog fades rather than snapping.
const MAP_VIEW_BLEND: f32 = 900.0;

/// True when the camera is far enough out that the world should render as a map.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapViewActive(pub bool);

/// How far into map view we are, `0.0..=1.0`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct MapViewBlend(pub f32);

pub fn update_map_view_state(
    cameras: Query<&CommanderCamera>,
    mut active: ResMut<MapViewActive>,
    mut blend: ResMut<MapViewBlend>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };

    let t = ((camera.zoom - MAP_VIEW_ZOOM) / MAP_VIEW_BLEND).clamp(0.0, 1.0);
    blend.0 = t;

    // Switch the detailed layer off only once the blend has actually committed, so a
    // camera hovering at the threshold does not flicker chunks on and off.
    let next = t >= 1.0;
    if active.0 != next {
        active.0 = next;
    }
}

/// Hide the detailed terrain and water layers in map view.
pub fn apply_map_view_visibility(
    active: Res<MapViewActive>,
    mut chunks: Query<&mut Visibility, (With<TerrainChunk>, Without<WaterChunk>)>,
    mut water: Query<&mut Visibility, (With<WaterChunk>, Without<TerrainChunk>)>,
) {
    if !active.is_changed() {
        return;
    }

    let desired = if active.0 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };

    for mut visibility in chunks.iter_mut() {
        if *visibility != desired {
            *visibility = desired;
        }
    }
    for mut visibility in water.iter_mut() {
        if *visibility != desired {
            *visibility = desired;
        }
    }
}

/// Fade distance fog out as the camera pulls back.
///
/// Aerial haze sells depth at ground level, but at realm scale it is applied over
/// kilometres and turns the entire map into a flat grey-brown wash. A map is meant to be
/// legible, so the haze is dialled out exactly as the map view comes in.
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
