//! Preserve civic scale, queue positions and the exported animation/material contract.
use super::*;

#[test]
fn civic_halls_keep_scale_and_share_their_entrance_contract() {
    for (kind, name, original_budget, minimum_height) in [
        (BuildingType::MootHall, "MootHall", 12_264, 8.02),
        (BuildingType::VillageHall, "VillageHall", 25_992, 9.26),
        (BuildingType::TownHall, "TownHall", 63_632, 21.40),
    ] {
        assert_animated_building_contract(
            kind,
            Vec2::new(0.0, -5.2),
            &format!("{name}Door"),
            2,
            original_budget,
        );
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
            .any(|m| m["name"] == "CivicHallGlass"));
        let nodes = document["nodes"].as_array().unwrap();
        for anchor in ["Light_Window.L", "Light_Window.R"] {
            assert!(nodes.iter().any(|n| n["name"] == anchor));
        }
        let definition = kind.definition();
        assert!(
            (definition.footprint_center.y - definition.footprint.y * 0.5 + 4.60).abs() < 0.0001,
            "{name}: preserve the occupied forecourt through an upgrade"
        );
        let mut max_height = f64::NEG_INFINITY;
        for node in nodes.iter().filter(|node| node["mesh"].is_u64()) {
            assert!(node["rotation"].is_null() && node["scale"].is_null());
            let mesh = node["mesh"].as_u64().unwrap() as usize;
            for primitive in document["meshes"][mesh]["primitives"].as_array().unwrap() {
                let bounds = &document["accessors"]
                    [primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
                max_height = max_height.max(
                    bounds["max"][1].as_f64().unwrap()
                        + node["translation"][1].as_f64().unwrap_or(0.0),
                );
                for (axis, centre, extent) in [
                    (
                        0,
                        definition.footprint_center.x,
                        definition.footprint.x / 2.0,
                    ),
                    (
                        2,
                        definition.footprint_center.y,
                        definition.footprint.y / 2.0,
                    ),
                ] {
                    let translation = node["translation"][axis].as_f64().unwrap_or(0.0) as f32;
                    let lo = bounds["min"][axis].as_f64().unwrap() as f32 + translation;
                    let hi = bounds["max"][axis].as_f64().unwrap() as f32 + translation;
                    assert!(
                        lo >= centre - extent - 0.001 && hi <= centre + extent + 0.001,
                        "{name}: outside reserved plot on axis {axis}"
                    );
                }
                if node["name"] == format!("{name}Door") {
                    let low = bounds["min"][1].as_f64().unwrap()
                        + node["translation"][1].as_f64().unwrap();
                    let high = bounds["max"][1].as_f64().unwrap()
                        + node["translation"][1].as_f64().unwrap();
                    assert!(
                        (0.02..0.04).contains(&low) && high - low >= 2.29,
                        "{name}: keep the entrance at walking grade"
                    );
                }
            }
        }
        assert!(
            max_height >= minimum_height - 0.001,
            "{name}: do not shrink the civic silhouette"
        );
    }
}
