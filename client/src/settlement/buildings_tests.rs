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

#[test]
fn house_extension_keeps_one_ground_claim_and_never_raises_a_second_house() {
    use crate::settlement::construction::{
        attach_construction_supply_visuals, raise_construction_visuals,
        sync_construction_supply_visuals, ConstructionSupplyBundle,
    };
    use crate::settlement::house_upgrades::attach_house_upgrade_scaffolds;
    use shared::components::{BuildingId, HouseUpgradeWorksite, PersonId};
    use shared::economy::{Good, GoodsInventory};
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::asset::AssetPlugin::default(),
    ))
    .init_resource::<Assets<Mesh>>()
    .init_resource::<Assets<StandardMaterial>>()
    .init_resource::<Time>()
    .insert_resource(flat_terrain())
    .add_systems(
        Update,
        (
            claim_building_ground,
            attach_construction_supply_visuals,
            attach_house_upgrade_scaffolds,
            raise_construction_visuals,
            sync_construction_supply_visuals,
        ),
    );
    let home = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Extension".into(),
                owner: None,
                quality: 0.8,
                workers: Vec::new(),
            },
            HouseAppearance::default(),
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
        ))
        .id();
    let mut inventory = GoodsInventory::new(100);
    inventory.add(Good::Wood, 8);
    let site = app
        .world_mut()
        .spawn((
            HouseUpgradeWorksite {
                house: BuildingId(1),
                owner: PersonId(2),
                target: HouseAppearance {
                    line: HouseLine::Cabin,
                    level: HouseLevel::UpperStorey,
                },
                wood_required: 8,
            },
            ConstructionSite {
                kind: SettlementBuildingKind::House,
                settlement: "Extension".into(),
                raising: false,
                stand: Vec3::Z * 5.0,
                rotation: 0.0,
            },
            PlayerPosition(Vec3::ZERO),
            inventory,
        ))
        .id();
    app.update();
    assert!(app.world().get::<PlacedBuilding>(home).is_some());
    let child_count = app.world().get::<Children>(site).unwrap().len();
    assert!(
        child_count > 8,
        "real material and scaffold geometry attaches"
    );
    assert_eq!(
        app.world().get::<Transform>(site).unwrap().translation,
        Vec3::ZERO
    );
    assert!(app.world().get::<PlacedBuilding>(site).is_none());
    assert!(app.world().get::<BuildingVisual>(site).is_none());
    app.world_mut()
        .get_mut::<ConstructionSite>(site)
        .unwrap()
        .raising = true;
    app.update();
    assert_eq!(
        app.world().get::<Children>(site).unwrap().len(),
        child_count,
        "repeated review does not duplicate scaffold parts"
    );
    assert!(app.world().get::<PlacedBuilding>(site).is_none());
    assert!(app.world().get::<BuildingVisual>(site).is_none());
    assert!(app.world().get::<WorldAssetRoot>(site).is_none());
    let visible_bundles = app
        .world_mut()
        .query::<(&ConstructionSupplyBundle, &Visibility)>()
        .iter(app.world())
        .filter(|(bundle, visibility)| bundle.site == site && **visibility == Visibility::Inherited)
        .count();
    assert_eq!(
        visible_bundles, 8,
        "recoverable Wood remains visible throughout work"
    );
    assert_eq!(
        app.world().get::<HouseAppearance>(home).unwrap().level,
        HouseLevel::Ground
    );
}
