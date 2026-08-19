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

pub use debug::PropLodDebugMode;
pub use plugin::PropsPlugin;
pub use types::*;

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
