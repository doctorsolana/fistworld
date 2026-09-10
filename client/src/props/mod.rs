//! Environmental props - rocks, trees, grass, etc.
//!
//! Spawns authored map objects by chunk.

mod assets;
mod debug;
pub(crate) mod foliage;
mod ground_cover_chunked;
mod ground_cover_instancing;
mod kinds;
mod lod;
mod plugin;
mod simple_mesh;
mod spawn;
mod types;
mod wind;
pub use wind::WindFoliageMaterial;

pub use debug::PropLodDebugMode;
pub(crate) use ground_cover_chunked::ChunkedGroundCover;
pub use plugin::PropsPlugin;
pub use types::*;

pub(crate) fn reset_world_streaming(world: &mut bevy::prelude::World) {
    use bevy::{ecs::system::RunSystemOnce, prelude::*};
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<EnvironmentProp>>()
        .iter(world)
        .collect();
    for entity in entities {
        world.despawn(entity);
    }
    world.insert_resource(LoadedPropChunks::default());
    world.insert_resource(PendingPropSpawns::default());
    world.insert_resource(PropChunkIndex::default());
    world.insert_resource(BuildZoneChunkIndex::default());
    let _ = world.run_system_once(ground_cover_chunked::clear_chunked_ground_cover);
}

pub(crate) use kinds::{is_tree_kind, uses_swap_mesh_lod};
pub(crate) use simple_mesh::{try_spawn_simple_prop_mesh, SimplePropMeshCache};

/// Full map view is represented by the far-terrain mesh. Individual trees,
/// rocks and ground cover are sub-pixel at this height, so keeping a streamed
/// square around the camera focus only creates a conspicuous central clump and
/// wastes CPU/GPU work.
pub(crate) const PROP_STREAM_OFF_ZOOM: f32 = crate::terrain::map_view::MAP_VIEW_BLEND_END;

pub(crate) fn props_suppressed_at_zoom(zoom: f32) -> bool {
    zoom >= PROP_STREAM_OFF_ZOOM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_props_stop_at_full_map_view() {
        assert!(!props_suppressed_at_zoom(PROP_STREAM_OFF_ZOOM - 0.1));
        assert!(props_suppressed_at_zoom(PROP_STREAM_OFF_ZOOM));
        assert!(props_suppressed_at_zoom(12_000.0));
    }
}
