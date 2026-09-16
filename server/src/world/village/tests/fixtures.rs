//! Village fixtures regression fixtures and invariants.

use super::*;

pub(super) fn village_test_app() -> App {
    let mut app = App::new();
    app.init_resource::<crate::world::identity::WorldIdAllocator>()
        .init_resource::<crate::world::identity::WorldIdentityIndex>()
        .init_resource::<BusinessEventQueue>()
        .init_resource::<MootQueueClock>()
        .add_systems(
            PreUpdate,
            (
                crate::world::identity::assign_stable_world_ids,
                crate::world::identity::rebuild_world_identity_index,
                crate::world::identity::reconcile_stable_world_relationships,
                crate::world::identity::reconcile_stable_adjunct_relationships,
                crate::world::identity::reconcile_stable_road_relationships,
                crate::world::identity::reconcile_stable_civic_employment,
            )
                .chain(),
        );
    app
}

pub(super) fn spawn_test_company(app: &mut App, id: u64, cash: u64) -> Entity {
    app.world_mut()
        .spawn((
            shared::components::CompanyId(id),
            shared::economy::CompanyAccount { cash, ..default() },
        ))
        .id()
}

/// A finite authored dry plane for physical movement/transaction fixtures.
/// No terrain generation, observer or random prop placement affects this floor.
pub(crate) fn dry_test_terrain() -> WorldTerrain {
    use shared::map::{HeightmapData, LoadedMap, MapBounds, MapDefinition, MapTerrain};
    let bounds = MapBounds {
        min: [-160.0, -160.0],
        max: [160.0, 160.0],
    };
    WorldTerrain::from_loaded_map(LoadedMap {
        definition: MapDefinition {
            map_id: "physical-village-unit".into(),
            bounds,
            terrain: MapTerrain {
                heightmap: "authored-test-floor".into(),
                minimap: None,
                water_level: Some(-10.0),
                height_min: 0.0,
                height_max: 0.0,
            },
            generated: None,
            player_spawn: None,
            objects: Vec::new(),
            blockers: Vec::new(),
        },
        heightmap: HeightmapData::new(bounds, 2, 2, vec![0.0; 4], Some(-10.0)),
        edits: Default::default(),
        terrain_deltas_by_chunk: Default::default(),
        objects_by_chunk: Default::default(),
        biome_field: None,
        rivers: std::sync::Arc::new(Vec::new()),
        river_segments_by_chunk: Default::default(),
        content_hash: 1,
        map_dir: Default::default(),
    })
}
