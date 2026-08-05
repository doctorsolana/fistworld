use std::sync::{Arc, OnceLock, RwLock};

use crate::map::{load_map, LoadedMap, MapBounds, DEFAULT_MAP_ID};

use super::WORLD_RADIUS_METERS;

// The active bounds may be refreshed during map setup and isolated tests; every
// in_world_bounds check must observe the same process-wide value.
static ACTIVE_MAP_BOUNDS: RwLock<Option<MapBounds>> = RwLock::new(None);
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

    set_active_map_bounds(loaded.definition.bounds);
    loaded
}

/// Update the process-wide active bounds during map setup.
pub fn set_active_map_bounds(bounds: MapBounds) {
    if let Ok(mut slot) = ACTIVE_MAP_BOUNDS.write() {
        *slot = Some(bounds);
    }
}

/// Whether a world position is inside active map bounds.
pub fn world_pos_in_bounds(x: f32, z: f32) -> bool {
    if let Some(bounds) = ACTIVE_MAP_BOUNDS.read().ok().and_then(|b| *b) {
        return bounds.contains_xz(x, z);
    }

    let radius_sq = WORLD_RADIUS_METERS * WORLD_RADIUS_METERS;
    (x * x) + (z * z) <= radius_sq
}
