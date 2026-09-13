//! Replacement loading must not discard a still-working building scene.

use super::*;
use crate::settlement::animation::DoorState;
use bevy::asset::io::{memory::MemoryAssetReader, AssetSourceBuilder, AssetSourceId};
use shared::components::{HouseLevel, HouseLine, SettlementTier};
use shared::map::{HeightmapData, LoadedMap, MapBounds, MapDefinition, MapTerrain};

fn flat_terrain() -> WorldTerrain {
    let radius = shared::terrain::WORLD_RADIUS_METERS;
    let bounds = MapBounds {
        min: [-radius; 2],
        max: [radius; 2],
    };
    WorldTerrain::from_loaded_map(LoadedMap {
        definition: MapDefinition {
            map_id: "building-upgrade-test".into(),
            bounds,
            terrain: MapTerrain {
                heightmap: String::new(),
                minimap: None,
                water_level: None,
                height_min: 0.0,
                height_max: 0.0,
            },
            generated: None,
            player_spawn: None,
            objects: Vec::new(),
            blockers: Vec::new(),
        },
        heightmap: HeightmapData::new(bounds, 2, 2, vec![0.0; 4], None),
        edits: default(),
        terrain_deltas_by_chunk: default(),
        objects_by_chunk: default(),
        biome_field: None,
        rivers: default(),
        river_segments_by_chunk: default(),
        content_hash: 0,
        map_dir: default(),
    })
}

#[test]
fn pending_upgrades_retain_house_and_hall_roots_and_wiring() {
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(|| Box::new(MemoryAssetReader::default())),
    )
    .add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::asset::AssetPlugin {
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..default()
        },
    ))
    .init_asset::<WorldAsset>()
    .init_asset::<bevy::gltf::Gltf>()
    .init_resource::<Assets<Mesh>>()
    .init_resource::<Assets<StandardMaterial>>()
    .insert_resource(flat_terrain())
    .add_systems(Update, (attach_building_visuals, attach_settlement_visuals));

    let house_data = SettlementBuilding {
        kind: SettlementBuildingKind::House,
        settlement: "Retention".into(),
        owner: Some("Resident".into()),
        quality: 1.0,
        workers: Vec::new(),
    };
    let appearance = HouseAppearance {
        line: HouseLine::Cabin,
        level: HouseLevel::UpperStorey,
    };
    let hall_data = Settlement {
        name: "Retention".into(),
        tier: SettlementTier::Village,
        residents: 10,
        treasury: 42,
    };
    let house = app
        .world_mut()
        .spawn((
            house_data.clone(),
            appearance,
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            BuildingVisual {
                building_type: BuildingType::LogCabin,
            },
        ))
        .id();
    let hall = app
        .world_mut()
        .spawn((
            hall_data.clone(),
            CivicHallLevel::Village,
            PlayerPosition(Vec3::X * 20.0),
            SettlementVisual {
                building_type: BuildingType::MootHall,
            },
        ))
        .id();
    let old_scene = Handle::<WorldAsset>::default();
    let node = AnimationGraph::new().root;
    let mut old_children = Vec::new();
    for root in [house, hall] {
        let child = app.world_mut().spawn(AnimationPlayer::default()).id();
        app.world_mut()
            .entity_mut(root)
            .insert((
                WorldAssetRoot(old_scene.clone()),
                BuildingDoorAnimation {
                    player: child,
                    open: node,
                    close: node,
                    state: DoorState::Open { clear_for: 0.0 },
                },
                WindowLighting {
                    glass: default(),
                    lamps: vec![child],
                    strength: 1.0,
                    lamp_strength: 0.5,
                },
            ))
            .add_child(child);
        old_children.push(child);
    }
    let new_house = app
        .world_mut()
        .spawn((
            house_data,
            appearance,
            PlayerPosition(Vec3::X * 40.0),
            PlayerRotation(0.0),
        ))
        .id();
    let new_hall = app
        .world_mut()
        .spawn((
            hall_data,
            CivicHallLevel::Village,
            PlayerPosition(Vec3::X * 60.0),
        ))
        .id();

    // No loader or asset data is installed: replacement dependencies cannot
    // become ready. Run twice to cover retention across a loading frame.
    for _ in 0..2 {
        app.update();
        for (root, child) in [house, hall].into_iter().zip(&old_children) {
            assert_eq!(
                app.world().get::<WorldAssetRoot>(root).unwrap().0,
                old_scene
            );
            assert_eq!(app.world().get::<ChildOf>(*child).unwrap().parent(), root);
            assert_eq!(
                app.world()
                    .get::<BuildingDoorAnimation>(root)
                    .unwrap()
                    .player,
                *child
            );
            assert_eq!(
                app.world().get::<WindowLighting>(root).unwrap().lamps,
                [*child]
            );
            assert!(app.world().get::<PendingBuildingScene>(root).is_some());
        }
    }
    assert_eq!(
        app.world()
            .get::<BuildingVisual>(house)
            .unwrap()
            .building_type,
        BuildingType::LogCabin
    );
    assert_eq!(
        app.world()
            .get::<SettlementVisual>(hall)
            .unwrap()
            .building_type,
        BuildingType::MootHall
    );
    for root in [new_house, new_hall] {
        assert_ne!(
            app.world().get::<WorldAssetRoot>(root).unwrap().0,
            old_scene,
            "initial appearances must still stream without waiting for a prior model"
        );
        assert!(app.world().get::<PendingBuildingScene>(root).is_none());
    }

    app.world_mut()
        .entity_mut(house)
        .insert(HouseAppearance::default());
    app.world_mut()
        .entity_mut(hall)
        .insert(CivicHallLevel::Moot);
    app.update();
    for root in [house, hall] {
        assert!(
            app.world().get::<PendingBuildingScene>(root).is_none(),
            "a cancelled upgrade must release its pending strong handle"
        );
        assert_eq!(
            app.world().get::<WorldAssetRoot>(root).unwrap().0,
            old_scene
        );
    }
}
