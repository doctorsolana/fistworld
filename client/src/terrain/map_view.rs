//! Map view — how the world degrades naturally into a map as the camera pulls back.
//!
//! Close up, terrain is streamed chunks using the splat shader plus a separate water
//! surface; far out, the whole map is one low-resolution vertex-coloured mesh.
//!
//! Detailed land uses one zoom-driven switch synchronized with the moving hole in the
//! far terrain. A per-chunk distance switch cannot match that square cutout: it either
//! leaves open strips or overlaps two differently tessellated height fields. Water uses
//! a continuous shader fade and an abrupt, already-transparent CPU cull.
//!
//! The far mesh sits ~5cm below the detail chunks. Detailed land retains a cut-out hole
//! to prevent coarse geometry poking through, while far ocean remains beneath the water
//! fade at every zoom.

use crate::camera_rts::CommanderCamera;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use super::chunks::{FarTerrain, FarTerrainState, TerrainChunk};
use crate::render::systems::GraphicsSettings;

/// Commander zoom at which all detailed land switches to the far mesh.
pub const DETAIL_SWITCH_ZOOM: f32 = 1_800.0;

/// Commander-zoom band used for atmosphere/cloud map-view blending.
pub const MAP_VIEW_BLEND_START: f32 = 1_250.0;
pub const MAP_VIEW_BLEND_END: f32 = 1_800.0;

/// Keep the true high-resolution shoreline throughout ordinary middle zoom.
/// The water shader separately fades across the outer streamed-chunk ring, so
/// retaining central detail here cannot expose a hard square boundary.
pub const WATER_FADE_START: f32 = MAP_VIEW_BLEND_START;
pub const WATER_FADE_END: f32 = MAP_VIEW_BLEND_END;

/// Abrupt CPU cull after the shader has already reached zero alpha. The extra
/// margin covers a 64m chunk's diagonal, preventing a chunk-center cull from
/// cutting off a still-visible edge.
pub const WATER_CULL_DISTANCE: f32 = DETAIL_SWITCH_ZOOM + 100.0;

/// The opaque map-water continuation sits 5 mm beneath the detailed water's
/// undisplaced 2 cm surface. Keeping it at one real height avoids deforming
/// rivers and ocean into visible boxes during the LOD handoff.
pub const FAR_WATER_SURFACE_OFFSET: f32 = shared::water::WATER_SURFACE_OFFSET - 0.005;

/// Static separation between detailed land and its coarse underlay.
///
/// Five centimetres prevents depth fighting without changing perceptible
/// terrain shape. Unlike the old moving 64 m skirt, this never varies with
/// zoom or distance and therefore cannot create a warped transition band.
pub const FAR_LAND_Y_OFFSET: f32 = -0.05;

/// Zoom past which the far mesh stops cutting a hole under the streamed chunks.
///
/// Close the coarse land cutout at the same map-view threshold where detailed
/// land switches off. Closing it earlier overlays two differently tessellated
/// height fields and creates a conspicuous striped/warped square.
pub const HOLE_FILL_ZOOM: f32 = DETAIL_SWITCH_ZOOM;

const _: () = assert!(WATER_CULL_DISTANCE > WATER_FADE_END);
const _: () = assert!(WATER_FADE_END == MAP_VIEW_BLEND_END);
const _: () = assert!(MAP_VIEW_BLEND_START < MAP_VIEW_BLEND_END);
const _: () = assert!(MAP_VIEW_BLEND_END <= DETAIL_SWITCH_ZOOM);
const _: () = assert!(HOLE_FILL_ZOOM == DETAIL_SWITCH_ZOOM);

/// Switch every streamed land chunk in lockstep with the far-terrain cutout.
///
/// This runs after chunk finalization, so chunks created while already in map
/// view are hidden in the same frame rather than briefly flashing into view.
pub(crate) fn sync_detail_visibility(
    cameras: Query<&CommanderCamera>,
    settings: Res<GraphicsSettings>,
    far_terrain: Query<&FarTerrainState, With<FarTerrain>>,
    mut chunks: Query<(&TerrainChunk, &mut Visibility)>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let committed_hole = far_terrain.single().ok();
    for (chunk, mut visibility) in chunks.iter_mut() {
        let target = detail_visibility_for_chunk(
            camera.zoom,
            settings.far_terrain_enabled,
            chunk.coord,
            committed_hole,
        );
        if *visibility != target {
            *visibility = target;
        }
    }
}

fn detail_visibility_for_chunk(
    zoom: f32,
    far_terrain_enabled: bool,
    coord: shared::terrain::ChunkCoord,
    committed_hole: Option<&FarTerrainState>,
) -> Visibility {
    if far_terrain_enabled && zoom > DETAIL_SWITCH_ZOOM {
        return Visibility::Hidden;
    }
    if !far_terrain_enabled {
        return Visibility::Inherited;
    }

    // The far mesh moves its land cutout only after the complete replacement
    // detail square is loaded. Match that committed square exactly: showing a
    // newly completed leading-edge chunk before the cutout moves overlays two
    // differently tessellated surfaces, which reads as a dark 64 m box while
    // panning at middle zoom.
    let Some(state) = committed_hole else {
        return Visibility::Inherited;
    };
    if state.hole_filled || state.view_distance < 0 {
        return Visibility::Hidden;
    }
    let dx = (coord.x - state.center_cell.x).abs();
    let dz = (coord.z - state.center_cell.y).abs();
    if dx.max(dz) <= state.view_distance {
        Visibility::Inherited
    } else {
        Visibility::Hidden
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
    let next = ((camera.zoom - MAP_VIEW_BLEND_START) / (MAP_VIEW_BLEND_END - MAP_VIEW_BLEND_START))
        .clamp(0.0, 1.0);
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
        assert!(WATER_FADE_START < WATER_FADE_END);
        assert!(WATER_FADE_END < WATER_CULL_DISTANCE);
        assert!(WATER_FADE_END <= HOLE_FILL_ZOOM);
    }

    #[test]
    fn terrain_switch_matches_far_hole_threshold() {
        let committed = FarTerrainState {
            center_cell: IVec2::ZERO,
            view_distance: 8,
            hole_filled: false,
        };
        assert_eq!(
            detail_visibility_for_chunk(
                DETAIL_SWITCH_ZOOM,
                true,
                shared::terrain::ChunkCoord::new(0, 0),
                Some(&committed),
            ),
            Visibility::Inherited
        );
        assert_eq!(
            detail_visibility_for_chunk(
                DETAIL_SWITCH_ZOOM + 0.01,
                true,
                shared::terrain::ChunkCoord::new(0, 0),
                Some(&committed),
            ),
            Visibility::Hidden
        );
        assert_eq!(
            detail_visibility_for_chunk(
                DETAIL_SWITCH_ZOOM + 1_000.0,
                false,
                shared::terrain::ChunkCoord::new(0, 0),
                Some(&committed),
            ),
            Visibility::Inherited
        );
        assert_eq!(DETAIL_SWITCH_ZOOM, HOLE_FILL_ZOOM);
    }

    #[test]
    fn leading_chunks_wait_for_the_committed_far_hole() {
        let committed = FarTerrainState {
            center_cell: IVec2::new(10, -4),
            view_distance: 2,
            hole_filled: false,
        };
        assert_eq!(
            detail_visibility_for_chunk(
                1_000.0,
                true,
                shared::terrain::ChunkCoord::new(12, -4),
                Some(&committed),
            ),
            Visibility::Inherited
        );
        assert_eq!(
            detail_visibility_for_chunk(
                1_000.0,
                true,
                shared::terrain::ChunkCoord::new(13, -4),
                Some(&committed),
            ),
            Visibility::Hidden
        );

        let filled = FarTerrainState {
            hole_filled: true,
            ..committed
        };
        assert_eq!(
            detail_visibility_for_chunk(
                1_000.0,
                true,
                shared::terrain::ChunkCoord::new(10, -4),
                Some(&filled),
            ),
            Visibility::Hidden
        );
    }
}
