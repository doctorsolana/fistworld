//! Navigation obstacle grid: keeps `SpatialObstacleGrid` in sync with authored buildings.
//!
//! Salvaged from the deleted NPC AI - it contains no unit types, it just expands each
//! building footprint by its flatten radius into the shared spatial grid that
//! `world::pathfinding` queries. Without it the grid stays permanently empty and
//! "buildings block navigation" is silently lost.

use bevy::prelude::*;
use shared::spatial::{ObstacleEntry, SpatialObstacleGrid};

use crate::collision::building_index::BuildingSpatialIndex;

/// Tracks the last known building index version to detect authored changes.
#[derive(Resource, Default)]
pub struct ObstacleGridState {
    pub last_building_version: u64,
}

/// Sync the `SpatialObstacleGrid` with current buildings.
/// Rebuilds only when the authored building index changes.
pub fn sync_obstacle_grid(
    mut grid: ResMut<SpatialObstacleGrid>,
    mut state: ResMut<ObstacleGridState>,
    building_index: Res<BuildingSpatialIndex>,
) {
    if building_index.version == state.last_building_version && !grid.is_empty() {
        return;
    }

    state.last_building_version = building_index.version;
    grid.clear();

    let buildings = building_index.snapshot();
    for building in buildings {
        let def = building.building_type.definition();
        let half_extents = Vec2::new(
            def.footprint.x / 2.0 + def.flatten_radius,
            def.footprint.y / 2.0 + def.flatten_radius,
        );

        grid.insert(ObstacleEntry {
            center: Vec2::new(building.position.x, building.position.z),
            half_extents,
            rotation: building.rotation,
            obstacle_type: building.building_type as u32,
        });
    }

    if !buildings.is_empty() {
        trace!("Rebuilt spatial grid with {} obstacles", buildings.len());
    }
}
