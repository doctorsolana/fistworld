//! Spatial index for placed buildings.

use bevy::prelude::*;

use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};

#[derive(Clone, Copy, Debug)]
pub struct IndexedBuilding {
    pub building_type: BuildingType,
    pub position: Vec3,
    pub rotation: f32,
}

/// Spatial index of placed buildings for collision and streaming broadphase.
#[derive(Resource, Default)]
pub struct BuildingSpatialIndex {
    snapshot: Vec<IndexedBuilding>,
    pub version: u64,
}

impl BuildingSpatialIndex {
    #[inline]
    pub fn snapshot(&self) -> &[IndexedBuilding] {
        &self.snapshot
    }
}

/// Rebuild building spatial index when authored building state changes.
pub fn sync_building_spatial_index(
    mut index: ResMut<BuildingSpatialIndex>,
    buildings: Query<(&PlacedBuilding, &BuildingPosition)>,
    changed_buildings: Query<
        (),
        Or<(
            Added<PlacedBuilding>,
            Changed<PlacedBuilding>,
            Changed<BuildingPosition>,
        )>,
    >,
    mut removed_buildings: RemovedComponents<PlacedBuilding>,
    mut removed_positions: RemovedComponents<BuildingPosition>,
) {
    let removed_any = !removed_buildings.is_empty() || !removed_positions.is_empty();
    // One snapshot incorporates the whole removal batch. Leaving unread
    // messages behind needlessly invalidates navigation again next tick.
    removed_buildings.clear();
    removed_positions.clear();
    let changed_any = !changed_buildings.is_empty();
    let needs_initial_build = index.version == 0 && !buildings.is_empty();
    if !removed_any && !changed_any && !needs_initial_build {
        return;
    }

    index.snapshot.clear();

    for (building, position) in buildings.iter() {
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
    fn removing_a_building_position_removes_its_spatial_entry() {
        let mut app = App::new();
        app.init_resource::<BuildingSpatialIndex>();
        app.add_systems(Update, sync_building_spatial_index);
        let building = app
            .world_mut()
            .spawn((
                PlacedBuilding {
                    building_type: BuildingType::MootHall,
                    rotation: 0.0,
                },
                BuildingPosition(Vec3::ZERO),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .resource::<BuildingSpatialIndex>()
                .snapshot()
                .len(),
            1
        );

        app.world_mut()
            .entity_mut(building)
            .remove::<BuildingPosition>();
        app.update();
        assert!(app
            .world()
            .resource::<BuildingSpatialIndex>()
            .snapshot()
            .is_empty());

        app.world_mut()
            .entity_mut(building)
            .insert(BuildingPosition(Vec3::X));
        app.update();
        assert_eq!(
            app.world().resource::<BuildingSpatialIndex>().snapshot()[0].position,
            Vec3::X
        );
        let version = app.world().resource::<BuildingSpatialIndex>().version;
        app.update();
        assert_eq!(
            app.world().resource::<BuildingSpatialIndex>().version,
            version
        );
    }

    #[test]
    fn a_batch_of_building_removals_invalidates_the_index_once() {
        let mut app = App::new();
        app.init_resource::<BuildingSpatialIndex>();
        app.add_systems(Update, sync_building_spatial_index);
        let buildings: Vec<_> = (0..4)
            .map(|index| {
                app.world_mut()
                    .spawn((
                        PlacedBuilding {
                            building_type: BuildingType::LogCabin,
                            rotation: 0.0,
                        },
                        BuildingPosition(Vec3::X * index as f32 * 20.0),
                    ))
                    .id()
            })
            .collect();
        app.update();
        for building in buildings {
            app.world_mut().despawn(building);
        }
        app.update();
        assert!(app
            .world()
            .resource::<BuildingSpatialIndex>()
            .snapshot()
            .is_empty());
        let version = app.world().resource::<BuildingSpatialIndex>().version;
        app.update();
        assert_eq!(
            app.world().resource::<BuildingSpatialIndex>().version,
            version
        );
    }

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
                    building_type: BuildingType::MootHall,
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
