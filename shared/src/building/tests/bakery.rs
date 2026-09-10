//! Bakery scene contracts consumed by doors, inventory, smoke and window lighting.
use super::*;

#[test]
fn bakery_preserves_its_plot_and_interactive_nodes() {
    let kind = BuildingType::Bakery;
    let def = kind.definition();
    assert_eq!(def.footprint, Vec2::new(7.042, 8.24));
    assert_eq!(def.footprint_center, Vec2::new(0.099, 0.720));
    assert_animated_building_contract(
        kind,
        crate::components::SettlementBuildingKind::Bakery.door_offset(),
        "BakeryDoor",
        2,
        12_500,
    );
    let doc = glb_document(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets/game_assets/buildings/village/Bakery.glb"),
    );
    let nodes = doc["nodes"].as_array().unwrap();
    for name in [
        "Anchor_Counter",
        "Light_Interior",
        "Light_Lantern",
        "Light_Oven",
        "Light_Window.L",
        "Light_Window.R",
        "FX_ChimneySmoke",
        "BakeryGlass",
    ] {
        assert!(nodes.iter().any(|n| n["name"] == name), "missing {name}");
    }
    for i in 1..=6 {
        assert!(
            nodes
                .iter()
                .any(|n| n["name"] == format!("Stock_Bread_{i}") && n["mesh"].is_u64())
        );
    }
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 9);
    for node in nodes.iter().filter(|n| n["mesh"].is_u64()) {
        assert!(node["rotation"].is_null() && node["scale"].is_null());
        for primitive in doc["meshes"][node["mesh"].as_u64().unwrap() as usize]["primitives"]
            .as_array()
            .unwrap()
        {
            let bounds =
                &doc["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
            for (axis, centre, half) in [
                (0, def.footprint_center.x, def.footprint.x / 2.),
                (2, def.footprint_center.y, def.footprint.y / 2.),
            ] {
                let t = node["translation"][axis].as_f64().unwrap_or(0.) as f32;
                let low = bounds["min"][axis].as_f64().unwrap() as f32 + t;
                let high = bounds["max"][axis].as_f64().unwrap() as f32 + t;
                assert!(
                    low >= centre - half - 0.001 && high <= centre + half + 0.001,
                    "{} outside plot",
                    node["name"]
                );
            }
            let t = node["translation"][1].as_f64().unwrap_or(0.);
            let low = bounds["min"][1].as_f64().unwrap() + t;
            let high = bounds["max"][1].as_f64().unwrap() + t;
            assert!(high <= def.height as f64 && low >= -0.201);
            if node["name"] == "BakeryDoor" {
                assert!((0.02..=0.04).contains(&low) && high - low >= 2.17);
            }
        }
    }
}
