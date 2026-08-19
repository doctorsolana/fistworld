//! chunks systems.

use super::mesh::build_water_mesh;
use super::*;
use bevy::light::NotShadowCaster;

use crate::camera_rts::CommanderCamera;
use crate::terrain::map_view::{WATER_FADE_END, WATER_FADE_START};

#[derive(Component)]
pub struct WaterChunk;

#[derive(Resource, Default)]
pub struct LoadedWaterChunks {
    pub entries: HashMap<ChunkCoord, Option<Entity>>,
}

/// The detailed-water square currently exposed by both water materials.
///
/// This is deliberately separate from the requested streaming center. During
/// a fast pan, the previous completed square remains authoritative until the
/// complete replacement square exists, preventing its fade edge from jumping
/// across half-built 64 m chunks.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WaterDetailCoverage {
    pub center: Option<ChunkCoord>,
    pub radius: i32,
}

#[derive(Resource)]
pub struct WaterRenderAssets {
    pub material: Handle<ToonWaterMaterial>,
}

/// Water needs to cover the camera footprint, not only the gameplay-terrain
/// streaming square. At middle zoom that footprint is substantially wider
/// than the default eight chunks; clipping water to the terrain radius made
/// islands appear to vanish before the complete map renderer took over.
pub(super) fn desired_water_render_distance(terrain_distance: i32, zoom: f32) -> i32 {
    let terrain_distance = terrain_distance.max(0);
    // The commander camera's visible ground footprint is roughly its zoom in
    // each horizontal direction at the normal RTS tilt. Three guard chunks
    // cover aspect ratio, tilt and the quantized streaming centre. Stop
    // growing at the start of the global map-view fade: the detailed surface
    // is progressively replaced after that point.
    let covered_zoom = zoom.min(WATER_FADE_START);
    let footprint_chunks = (covered_zoom / CHUNK_SIZE).ceil() as i32 + 3;
    let full_distance = terrain_distance.max(footprint_chunks);

    // Keep full viewport coverage through most of the map-view crossfade.
    // Only contract once detailed water is already faint, and reach the base
    // terrain radius exactly when its shader alpha reaches zero.
    let shrink_start = WATER_FADE_START + (WATER_FADE_END - WATER_FADE_START) * 0.75;
    if zoom <= shrink_start {
        return full_distance;
    }
    let shrink = ((zoom - shrink_start) / (WATER_FADE_END - shrink_start)).clamp(0.0, 1.0);
    ((full_distance as f32).lerp(terrain_distance as f32, shrink)).ceil() as i32
}

/// Largest fully-built square around `center`, including dry chunks recorded
/// as `None`. The shader handoff may only move this far; using the desired
/// radius immediately would cut away fallback water while a newly exposed
/// leading strip was still being generated.
pub(super) fn complete_water_render_distance(
    loaded: &LoadedWaterChunks,
    center: ChunkCoord,
    desired: i32,
) -> i32 {
    let mut complete = -1;
    for radius in 0..=desired.max(0) {
        let ring_complete = center
            .chunks_in_radius(radius)
            .into_iter()
            .filter(|coord| {
                (coord.x - center.x).abs().max((coord.z - center.z).abs()) == radius
                    && coord.in_world_bounds()
            })
            .all(|coord| loaded.entries.contains_key(&coord));
        if !ring_complete {
            break;
        }
        complete = radius;
    }
    complete
}

pub(super) fn next_water_detail_coverage(
    current: WaterDetailCoverage,
    loaded: &LoadedWaterChunks,
    desired_center: ChunkCoord,
    desired_radius: i32,
    min_recenter_radius: i32,
) -> WaterDetailCoverage {
    let desired_radius = desired_radius.max(0);
    let complete = complete_water_render_distance(loaded, desired_center, desired_radius);
    if complete < 0 {
        return current;
    }

    // Recenter as one atomic handoff — progressively moving the square while
    // its leading rings are incomplete is the visible dark-box bug — but do
    // not wait for the ENTIRE middle-zoom footprint (a 41x41 square taking
    // seconds to fill after a long pan). Once the terrain square plus the fade
    // band is complete, the handoff is already invisible; the same-center arm
    // then grows the radius outward every frame as further rings land.
    let recenter_radius = desired_radius.min(min_recenter_radius.max(0));
    match current.center {
        None => WaterDetailCoverage {
            center: Some(desired_center),
            radius: complete,
        },
        Some(center) if center == desired_center => WaterDetailCoverage {
            center: Some(desired_center),
            radius: complete,
        },
        Some(_) if complete >= recenter_radius => WaterDetailCoverage {
            center: Some(desired_center),
            radius: complete,
        },
        Some(_) => current,
    }
}

fn coord_in_coverage(coord: ChunkCoord, coverage: WaterDetailCoverage) -> bool {
    let Some(center) = coverage.center else {
        return false;
    };
    let dx = (coord.x - center.x).abs();
    let dz = (coord.z - center.z).abs();
    dx.max(dz) <= coverage.radius
}

pub(super) fn cleanup_water_chunks(
    mut commands: Commands,
    mut loaded_water: ResMut<LoadedWaterChunks>,
    streaming: Res<crate::terrain::TerrainStreamingState>,
    cameras: Query<&CommanderCamera>,
    coverage: Res<WaterDetailCoverage>,
) {
    let Some(center) = streaming.center else {
        return;
    };
    let zoom = cameras.single().map_or(0.0, |camera| camera.zoom);
    let radius = desired_water_render_distance(streaming.render_distance, zoom);
    let mut to_remove = Vec::new();
    for (coord, entity) in loaded_water.entries.iter() {
        let dx = (coord.x - center.x).abs();
        let dz = (coord.z - center.z).abs();
        let wanted_at_new_center = dx <= radius && dz <= radius;
        let protects_committed_handoff = coord_in_coverage(*coord, *coverage);
        if (!wanted_at_new_center && !protects_committed_handoff) || !coord.in_world_bounds() {
            to_remove.push((*coord, *entity));
        }
    }

    // A quick wheel-zoom can invalidate hundreds of water-only entries at
    // once. Retire a bounded batch so cleanup itself never becomes the frame
    // hitch that this LOD path is intended to prevent.
    const MAX_WATER_REMOVALS_PER_FRAME: usize = 64;
    for (coord, entity) in to_remove.into_iter().take(MAX_WATER_REMOVALS_PER_FRAME) {
        if let Some(entity) = entity {
            commands.entity(entity).despawn();
        }
        loaded_water.entries.remove(&coord);
    }
}

pub(super) fn spawn_water_chunks(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    render_assets: Option<Res<WaterRenderAssets>>,
    streaming: Res<crate::terrain::TerrainStreamingState>,
    cameras: Query<&CommanderCamera>,
    mut loaded_water: ResMut<LoadedWaterChunks>,
    mut meshes: ResMut<Assets<Mesh>>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    let Some(center) = streaming.center else {
        return;
    };
    let zoom = cameras.single().map_or(0.0, |camera| camera.zoom);
    let radius = desired_water_render_distance(streaming.render_distance, zoom);

    // Water grows from the focus out as one contiguous surface. It is
    // intentionally independent of LoadedChunks: the generator can build a
    // water-only ring cheaply without paying for terrain materials, colliders
    // or prop streaming across the entire middle-zoom viewport.
    let mut candidates = center.chunks_in_radius(radius);
    candidates.retain(|coord| coord.in_world_bounds());
    candidates
        .sort_unstable_by_key(|coord| (coord.x - center.x).abs().max((coord.z - center.z).abs()));

    const MAX_WATER_MESHES_PER_FRAME: usize = 12;
    const MAX_CHUNKS_EXAMINED_PER_FRAME: usize = 48;
    let mut spawned = 0usize;
    let mut examined = 0usize;

    for coord in candidates.drain(..) {
        if spawned >= MAX_WATER_MESHES_PER_FRAME || examined >= MAX_CHUNKS_EXAMINED_PER_FRAME {
            break;
        }
        if loaded_water.entries.contains_key(&coord) {
            continue;
        }
        examined += 1;

        let Some(mesh) = build_water_mesh(&terrain, coord) else {
            loaded_water.entries.insert(coord, None);
            continue;
        };

        let chunk_pos = coord.world_pos();
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(render_assets.material.clone()),
                Transform::from_translation(chunk_pos),
                // Blended materials still enter Bevy's shadow pass unless
                // explicitly excluded. The water mesh overlaps the bank, so
                // casting from it creates a solid black moving shoreline.
                NotShadowCaster,
                WaterChunk,
                // The shader fades this into far-map water before the abrupt
                // CPU cull, so no chunk edge is visible at map scale.
                crate::terrain::map_view::water_visibility_range(),
            ))
            .id();
        commands.entity(world_root).add_child(entity);
        loaded_water.entries.insert(coord, Some(entity));
        spawned += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_zoom_water_covers_more_than_the_terrain_square() {
        assert_eq!(desired_water_render_distance(8, 220.0), 8);
        assert!(desired_water_render_distance(8, 1_092.0) >= 20);
        assert!(desired_water_render_distance(8, 1_500.0) >= 20);
        assert_eq!(desired_water_render_distance(8, WATER_FADE_END), 8);
    }

    #[test]
    fn completed_radius_waits_for_every_chunk_in_the_next_ring() {
        let center = ChunkCoord::new(0, 0);
        let mut loaded = LoadedWaterChunks::default();
        assert_eq!(complete_water_render_distance(&loaded, center, 2), -1);
        loaded.entries.insert(center, None);
        assert_eq!(complete_water_render_distance(&loaded, center, 2), 0);

        for coord in center.chunks_in_radius(1) {
            loaded.entries.insert(coord, None);
        }
        assert_eq!(complete_water_render_distance(&loaded, center, 2), 1);
    }

    #[test]
    fn coverage_recenter_waits_for_the_whole_requested_square() {
        let old_center = ChunkCoord::new(0, 0);
        let new_center = ChunkCoord::new(1, 0);
        let current = WaterDetailCoverage {
            center: Some(old_center),
            radius: 1,
        };
        let mut loaded = LoadedWaterChunks::default();
        loaded.entries.insert(new_center, None);

        assert_eq!(
            next_water_detail_coverage(current, &loaded, new_center, 1, 1),
            current
        );

        for coord in new_center.chunks_in_radius(1) {
            loaded.entries.insert(coord, None);
        }
        assert_eq!(
            next_water_detail_coverage(current, &loaded, new_center, 1, 1),
            WaterDetailCoverage {
                center: Some(new_center),
                radius: 1,
            }
        );
    }

    #[test]
    fn coverage_recenter_does_not_wait_for_the_full_middle_zoom_footprint() {
        let old_center = ChunkCoord::new(0, 0);
        let new_center = ChunkCoord::new(6, 0);
        let current = WaterDetailCoverage {
            center: Some(old_center),
            radius: 2,
        };
        let mut loaded = LoadedWaterChunks::default();
        for coord in new_center.chunks_in_radius(1) {
            loaded.entries.insert(coord, None);
        }

        // Desired footprint is much larger, but the terrain square plus fade
        // band (min_recenter_radius = 1 here) is complete: recenter now with
        // the complete radius and let the same-center arm grow it afterward.
        assert_eq!(
            next_water_detail_coverage(current, &loaded, new_center, 20, 1),
            WaterDetailCoverage {
                center: Some(new_center),
                radius: 1,
            }
        );
    }
}
