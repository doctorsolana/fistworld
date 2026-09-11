//! Fishing anchors stay aligned with authoritative shore work and the walkable pier.
use super::*;
use bevy::prelude::Vec3;

#[test]
fn fisherman_keeps_shore_anchors_human_scale_and_clear_work_approaches() {
    let kind = BuildingType::FishermansHut;
    let site = crate::components::SettlementBuildingKind::FishermansHut;
    let def = kind.definition();
    assert_eq!(def.footprint, Vec2::new(6.44, 6.51));
    assert_eq!(def.footprint_center, Vec2::new(-0.4, -0.5125));
    assert_animated_building_contract(kind, site.door_offset(), "FishermansHutDoor", 2, 8500);
    let doc = glb_document(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets/game_assets/buildings/village/FishermansHut.glb"),
    );
    let nodes = doc["nodes"].as_array().unwrap();
    for (name, expected) in [
        ("Anchor_Nets", site.nets_position(Vec3::ZERO, 0.0).unwrap()),
        ("Anchor_Pier", site.pier_position(Vec3::ZERO, 0.0).unwrap()),
    ] {
        let node = nodes.iter().find(|n| n["name"] == name).unwrap();
        let actual = Vec3::from_array(std::array::from_fn(|i| {
            node["translation"][i].as_f64().unwrap_or(0.0) as f32
        }));
        assert!(
            actual.distance(expected) < 0.001,
            "{name} disagrees with shore routing"
        );
    }
    for name in [
        "Light_Window.L",
        "Light_Window.R",
        "Light_Lantern",
        "Light_Interior",
        "FishermansHutGlass",
    ] {
        assert!(nodes.iter().any(|n| n["name"] == name), "missing {name}");
    }
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 3);
    for node in nodes.iter().filter(|n| n["mesh"].is_u64()) {
        let prim = &doc["meshes"][node["mesh"].as_u64().unwrap() as usize]["primitives"][0];
        let bounds = &doc["accessors"][prim["attributes"]["POSITION"].as_u64().unwrap() as usize];
        for (axis, center, half) in [
            (0, def.footprint_center.x, def.footprint.x / 2.),
            (2, def.footprint_center.y, def.footprint.y / 2.),
        ] {
            let t = node["translation"][axis].as_f64().unwrap_or(0.) as f32;
            assert!(bounds["min"][axis].as_f64().unwrap() as f32 + t >= center - half - 0.001);
            assert!(bounds["max"][axis].as_f64().unwrap() as f32 + t <= center + half + 0.001);
        }
        if node["name"] == "FishermansHutDoor" {
            let t = node["translation"][1].as_f64().unwrap();
            let low = bounds["min"][1].as_f64().unwrap() + t;
            let high = bounds["max"][1].as_f64().unwrap() + t;
            assert!((0.02..=0.04).contains(&low) && high - low >= 2.09);
        }
    }
    let db = crate::colliders::load_baked_collider_db_from_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/assets/colliders.bin"),
    )
    .unwrap();
    let crate::colliders::BakedCollider::ConvexHull { points } = &db.entries[kind.id()] else {
        panic!("shore hull");
    };
    let left = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let nets = site.nets_position(Vec3::ZERO, 0.0).unwrap();
    assert!(nets.x + crate::physics::CHARACTER_NAV_RADIUS + 0.05 < left);
    assert!(
        points.iter().all(|p| p[1] < 2.01),
        "roof must not inflate navigation hull"
    );
}
