use bevy::prelude::Vec2;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::*;

mod civic;
mod roofs;
mod rural;
mod windmill;

fn glb_document(path: &Path) -> serde_json::Value {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!(&bytes[0..4], b"glTF", "{} is not a GLB", path.display());
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + json_len])
        .unwrap_or_else(|error| panic!("{}: invalid GLB JSON: {error}", path.display()))
}

fn glb_scene_name(path: &Path) -> String {
    let document = glb_document(path);
    document["scenes"][0]["name"]
        .as_str()
        .unwrap_or_else(|| panic!("{}: default scene has no name", path.display()))
        .to_string()
}

#[test]
fn storage_hall_keeps_its_plot_and_ships_a_lightweight_animated_door() {
    let kind = BuildingType::StorageHall;
    assert_eq!(
        kind as u32, 13,
        "preserve the old blockout's wire discriminant"
    );
    assert_eq!(kind.definition().footprint, Vec2::new(9.0, 7.0));
    assert_animated_building_contract(
        kind,
        crate::components::SettlementBuildingKind::StorageHall.door_offset(),
        "StorageHallDoor",
        1,
        8_000,
    );
}

#[test]
fn lumberjack_hut_preserves_its_plot_door_clearance_and_night_light_anchors() {
    let kind = BuildingType::LumberjackHut;
    assert_eq!(kind.definition().footprint, Vec2::new(5.16, 5.40));
    assert_animated_building_contract(
        kind,
        crate::components::SettlementBuildingKind::LumberjackHut.door_offset(),
        "LumberHutDoor",
        2,
        8_000,
    );
    let document = glb_document(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets/game_assets/buildings/village/LumberjackHut.glb"),
    );
    let nodes = document["nodes"].as_array().unwrap();
    for anchor in [
        "Light_Window.L",
        "Light_Window.R",
        "Light_Interior",
        "Anchor_Work",
    ] {
        assert!(
            nodes.iter().any(|node| node["name"] == anchor),
            "missing {anchor}"
        );
    }
    assert!(document["materials"]
        .as_array()
        .unwrap()
        .iter()
        .any(|material| material["name"] == "HutGlass"));
    assert_eq!(document["meshes"].as_array().unwrap().len(), 3);
}

fn assert_animated_building_contract(
    kind: BuildingType,
    offset: Vec2,
    door_name: &str,
    materials: usize,
    vertex_budget: u64,
) {
    assert!(kind.has_baked_collider());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../client/assets")
        .join(kind.scene_path().unwrap().split('#').next().unwrap());
    let document = glb_document(&path);
    let nodes = document["nodes"].as_array().unwrap();
    let anchor = nodes
        .iter()
        .find(|node| node["name"] == "Anchor_Door")
        .unwrap();
    let anchor_position: Vec<f32> = anchor["translation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_f64().unwrap() as f32)
        .collect();
    assert_eq!(anchor_position, [offset.x, 0.0, offset.y]);
    let colliders = crate::colliders::load_baked_collider_db_from_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/assets/colliders.bin"),
    )
    .unwrap();
    let crate::colliders::BakedCollider::ConvexHull { points } = &colliders.entries[kind.id()]
    else {
        panic!("{kind:?} must use one inexpensive hull");
    };
    let front = points
        .iter()
        .map(|point| point[2])
        .fold(f32::INFINITY, f32::min);
    assert!(
        offset.y + crate::physics::CHARACTER_NAV_RADIUS + 0.05 < front,
        "the baked hull must leave character clearance at the door anchor"
    );
    if kind == BuildingType::LumberjackHut {
        let work = nodes
            .iter()
            .find(|node| node["name"] == "Anchor_Work")
            .unwrap();
        let work_front = work["translation"][2].as_f64().unwrap() as f32;
        assert!(
            work_front + crate::physics::CHARACTER_NAV_RADIUS + 0.05 < front,
            "the chopping approach must also stay outside the yard collider"
        );
    }
    let door = nodes
        .iter()
        .position(|node| node["name"] == door_name)
        .unwrap();
    let animations = document["animations"].as_array().unwrap();
    assert_eq!(
        animations.len(),
        if kind == BuildingType::Windmill { 3 } else { 2 }
    );
    for (name, duration) in [("door_open", 16.0 / 24.0), ("door_close", 22.0 / 24.0)] {
        let clip = animations.iter().find(|clip| clip["name"] == name).unwrap();
        let channels = clip["channels"].as_array().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0]["target"]["node"].as_u64(), Some(door as u64));
        assert_eq!(channels[0]["target"]["path"], "rotation");
        let input = clip["samplers"][0]["input"].as_u64().unwrap() as usize;
        let end = document["accessors"][input]["max"][0].as_f64().unwrap();
        assert!(
            (end - duration).abs() < 0.001,
            "{name} must match runtime door timing"
        );
    }
    assert_eq!(document["materials"].as_array().unwrap().len(), materials);
    assert!(document["skins"].is_null());
    assert!(document["images"].is_null());
    assert!(document["extensionsUsed"].is_null());
    let vertices: u64 = document["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|mesh| mesh["primitives"].as_array().unwrap())
        .map(|primitive| {
            let accessor = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
            document["accessors"][accessor]["count"].as_u64().unwrap()
        })
        .sum();
    assert!(
        vertices <= vertex_budget,
        "{kind:?} exceeded its exported vertex budget: {vertices}"
    );
}

/// Every building the game has must have a definition and a model.
///
/// Written as a sweep over `all()` rather than naming two by hand: the
/// hand-named version broke the moment the bought building sets were
/// deleted, and it would have said nothing about the ones we kept.
#[test]
fn every_building_has_a_definition_and_a_model() {
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for kind in BuildingType::all() {
        let def = kind.definition();
        assert!(!def.display_name.is_empty(), "{kind:?} has no display name");
        let scene = kind
            .scene_path()
            .unwrap_or_else(|| panic!("{kind:?} has no model to draw"));
        assert!(ids.insert(kind.id()), "duplicate building id {}", kind.id());
        assert!(paths.insert(scene), "duplicate building scene {scene}");

        let relative = scene.split('#').next().unwrap();
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets")
            .join(relative);
        assert!(file.is_file(), "missing building asset {}", file.display());
        assert_eq!(
            glb_scene_name(&file),
            file.file_stem().unwrap().to_string_lossy(),
            "{} has stale internal scene metadata",
            file.display()
        );
    }
}

#[test]
fn civic_hall_variants_keep_distinct_serialized_identities() {
    for kind in [
        BuildingType::MootHall,
        BuildingType::VillageHall,
        BuildingType::TownHall,
    ] {
        let encoded = ron::to_string(&kind).unwrap();
        assert_eq!(ron::from_str::<BuildingType>(&encoded).unwrap(), kind);
    }
}

#[test]
fn market_levels_share_the_authored_walkable_square_contract() {
    for kind in [BuildingType::Market, BuildingType::MarketPaved] {
        let definition = kind.definition();
        assert_eq!(definition.footprint, Vec2::splat(12.0));
        assert_eq!(definition.footprint_center, Vec2::ZERO);
        assert_eq!(definition.terrain_flat_margin(), 3.0);
        assert_eq!(definition.terrain_flat_half_extents(), Vec2::splat(9.0));
        assert!((definition.terrain_blend_width() - 1.8).abs() < 1e-5);
        assert!(definition.model_path.is_some());
        assert!(!kind.has_baked_collider());
        assert!(!kind.blocks_ground_navigation());
    }
    assert_eq!(
        ron::from_str::<BuildingType>("PlaceholderMarket").unwrap(),
        BuildingType::Market,
        "pre-art RON saves must continue to deserialize"
    );
}

#[test]
fn flatten_footprint_is_square_and_positive() {
    for kind in BuildingType::all() {
        let flatten = kind.definition().flatten_footprint();
        assert!(
            flatten.x > 0.0 && flatten.y > 0.0,
            "{kind:?} flattens nothing"
        );
    }
}

#[test]
fn all_house_levels_keep_their_plot_entrances_glass_and_animation_contracts() {
    for (kind, approach, budget) in [
        (BuildingType::LogCabin, 4.30, 8424),
        (BuildingType::LongCabin, 3.25, 4656),
        (BuildingType::CabinL2, 4.30, 8808),
        (BuildingType::LongCabinL2, 3.65, 11280),
    ] {
        assert_animated_building_contract(kind, Vec2::new(0.0, -approach), "HouseDoor", 2, budget);
        let document = glb_document(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../client/assets")
                .join(kind.scene_path().unwrap().split('#').next().unwrap()),
        );
        assert_eq!(document["meshes"].as_array().unwrap().len(), 3);
        assert!(document["materials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|material| material["name"] == "CabinGlass"));
        let nodes = document["nodes"].as_array().unwrap();
        for (anchor, side) in [("Light_Window.L", -1.0), ("Light_Window.R", 1.0)] {
            let node = nodes.iter().find(|node| node["name"] == anchor).unwrap();
            assert!(
                node["translation"][0].as_f64().unwrap() * side > 0.0,
                "{kind:?}: {anchor} must be on the named side"
            );
        }
        let definition = kind.definition();
        let half = definition.footprint * 0.5;
        for node in nodes {
            let Some(mesh_index) = node["mesh"].as_u64() else {
                continue;
            };
            // All authored parts use a baked rest pose; only the door has translation.
            assert!(node["rotation"].is_null());
            assert!(node["scale"].is_null());
            for primitive in document["meshes"][mesh_index as usize]["primitives"]
                .as_array()
                .unwrap()
            {
                let accessor = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
                for (axis, center, extent) in [
                    (0, definition.footprint_center.x, half.x),
                    (2, definition.footprint_center.y, half.y),
                ] {
                    let translation = node["translation"][axis].as_f64().unwrap_or(0.0) as f32;
                    let lo = document["accessors"][accessor]["min"][axis]
                        .as_f64()
                        .unwrap() as f32
                        + translation;
                    let hi = document["accessors"][accessor]["max"][axis]
                        .as_f64()
                        .unwrap() as f32
                        + translation;
                    assert!(
                        lo >= center - extent - 0.001 && hi <= center + extent + 0.001,
                        "{kind:?}: geometry left its reserved plot on axis {axis}: {lo}..{hi}"
                    );
                }
            }
        }
    }
}
