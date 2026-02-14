//! Spatial index for placed buildings.

use bevy::prelude::*;
use std::collections::HashMap;

use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};

const BUILDING_CELL_SIZE: f32 = 32.0;

#[derive(Clone, Copy, Debug)]
pub struct IndexedBuilding {
    pub building_type: BuildingType,
    pub position: Vec3,
    pub rotation: f32,
}

/// Spatial index of placed buildings for collision and streaming broadphase.
#[derive(Resource)]
pub struct BuildingSpatialIndex {
    cell_size: f32,
    cells: HashMap<(i32, i32), Vec<Entity>>,
    snapshot: Vec<IndexedBuilding>,
    pub version: u64,
}

impl Default for BuildingSpatialIndex {
    fn default() -> Self {
        Self {
            cell_size: BUILDING_CELL_SIZE,
            cells: HashMap::new(),
            snapshot: Vec::new(),
            version: 0,
        }
    }
}

impl BuildingSpatialIndex {
    #[inline]
    fn cell_key(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32,
        )
    }

    pub fn collect_nearby_entities(&self, pos: Vec3, radius: f32, out: &mut Vec<Entity>) {
        out.clear();

        let (cx, cz) = self.cell_key(pos);
        let cells = (radius / self.cell_size).ceil() as i32 + 1;

        for dx in -cells..=cells {
            for dz in -cells..=cells {
                if let Some(list) = self.cells.get(&(cx + dx, cz + dz)) {
                    out.extend(list.iter().copied());
                }
            }
        }
    }

    #[inline]
    pub fn snapshot(&self) -> &[IndexedBuilding] {
        &self.snapshot
    }
}

/// Rebuild building spatial index when authored building state changes.
pub fn sync_building_spatial_index(
    mut index: ResMut<BuildingSpatialIndex>,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    changed_buildings: Query<
        (),
        Or<(
            Added<PlacedBuilding>,
            Changed<PlacedBuilding>,
            Changed<BuildingPosition>,
        )>,
    >,
    mut removed_buildings: RemovedComponents<PlacedBuilding>,
) {
    let removed_any = removed_buildings.read().next().is_some();
    let changed_any = !changed_buildings.is_empty();
    let needs_initial_build = index.version == 0 && !buildings.is_empty();
    if !removed_any && !changed_any && !needs_initial_build {
        return;
    }

    index.cells.clear();
    index.snapshot.clear();

    for (entity, building, position) in buildings.iter() {
        let key = index.cell_key(position.0);
        index.cells.entry(key).or_default().push(entity);
        index.snapshot.push(IndexedBuilding {
            building_type: building.building_type,
            position: position.0,
            rotation: building.rotation,
        });
    }

    index.version = index.version.wrapping_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_version_updates_only_when_buildings_change() {
        let mut app = App::new();
        app.init_resource::<BuildingSpatialIndex>();
        app.add_systems(Update, sync_building_spatial_index);

        app.update();
        assert_eq!(app.world().resource::<BuildingSpatialIndex>().version, 0);

        let building_entity = app
            .world_mut()
            .spawn((
                PlacedBuilding {
                    building_type: BuildingType::Windmill,
                    rotation: 0.0,
                },
                BuildingPosition(Vec3::new(0.0, 0.0, 0.0)),
            ))
            .id();

        app.update();
        assert_eq!(app.world().resource::<BuildingSpatialIndex>().version, 1);
        assert_eq!(
            app.world()
                .resource::<BuildingSpatialIndex>()
                .snapshot()
                .len(),
            1
        );

        // No changes on this tick, so version should remain stable.
        app.update();
        assert_eq!(app.world().resource::<BuildingSpatialIndex>().version, 1);

        // Mutating position should trigger a rebuild + version bump.
        {
            let mut pos = app
                .world_mut()
                .get_mut::<BuildingPosition>(building_entity)
                .expect("building position should exist");
            pos.0.x += 2.0;
        }
        app.update();
        assert_eq!(app.world().resource::<BuildingSpatialIndex>().version, 2);

        // Despawning should trigger a rebuild + version bump.
        app.world_mut().entity_mut(building_entity).despawn();
        app.update();
        assert_eq!(app.world().resource::<BuildingSpatialIndex>().version, 3);
        assert!(app
            .world()
            .resource::<BuildingSpatialIndex>()
            .snapshot()
            .is_empty());
    }
}
