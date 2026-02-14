use std::sync::{Arc, OnceLock};

use crate::map::{load_default_map, LoadedMap, MapBounds};

use super::WORLD_RADIUS_METERS;

static ACTIVE_MAP_BOUNDS: OnceLock<MapBounds> = OnceLock::new();
static ACTIVE_LOADED_MAP: OnceLock<Arc<LoadedMap>> = OnceLock::new();

pub(super) fn load_active_map() -> Arc<LoadedMap> {
    let loaded = ACTIVE_LOADED_MAP
        .get_or_init(|| {
            Arc::new(load_default_map().unwrap_or_else(|err| {
                panic!(
                    "Failed to load authored map '{}': {err}",
                    crate::map::DEFAULT_MAP_ID
                )
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
