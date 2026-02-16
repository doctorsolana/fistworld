use std::sync::{Arc, OnceLock};

use crate::map::{load_map, LoadedMap, MapBounds, DEFAULT_MAP_ID};

use super::WORLD_RADIUS_METERS;

static ACTIVE_MAP_BOUNDS: OnceLock<MapBounds> = OnceLock::new();
static ACTIVE_LOADED_MAP: OnceLock<Arc<LoadedMap>> = OnceLock::new();

pub(super) fn load_active_map() -> Arc<LoadedMap> {
    let loaded =
        ACTIVE_LOADED_MAP
            .get_or_init(|| {
                let map_id = std::env::var("CITYSIM_MAP_ID")
                    .ok()
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_MAP_ID.to_string());

                Arc::new(load_map(&map_id).unwrap_or_else(|err| {
                    panic!("Failed to load authored map '{}': {err}", map_id)
                }))
            })
            .clone();

    let _ = ACTIVE_MAP_BOUNDS.set(loaded.definition.bounds);
    loaded
}

/// Whether a world position is inside active map bounds.
pub fn world_pos_in_bounds(x: f32, z: f32) -> bool {
    if let Some(bounds) = ACTIVE_MAP_BOUNDS.get() {
        return bounds.contains_xz(x, z);
    }

    let radius_sq = WORLD_RADIUS_METERS * WORLD_RADIUS_METERS;
    (x * x) + (z * z) <= radius_sq
}
