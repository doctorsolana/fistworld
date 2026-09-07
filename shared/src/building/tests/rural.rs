//! Rural render assets preserve the authoritative plots and worker entry points.
use super::*;
use crate::components::SettlementBuildingKind;

#[test]
fn rural_assets_keep_entrances_scale_and_export_budgets() {
    assert_eq!(BuildingType::Church as u32, 7);
    assert_eq!(BuildingType::StoneQuarry as u32, 14);
    for (kind, name, budget) in [
        (SettlementBuildingKind::Farmstead, "Farmstead", 8_040),
        (
            SettlementBuildingKind::LivestockFarm,
            "LivestockFarm",
            11_280,
        ),
        (SettlementBuildingKind::StoneQuarry, "StoneQuarry", 12_000),
        (SettlementBuildingKind::Church, "Church", 24_000),
    ] {
        let art = kind.art();
        assert_animated_building_contract(
            art,
            kind.door_offset(),
            &format!("{name}Door"),
            2,
            budget,
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        let doc = glb_document(&path);
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 3);
        assert!(doc["materials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == format!("{name}Glass")));
        let def = art.definition();
        for node in doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["mesh"].is_u64())
        {
            let mesh = node["mesh"].as_u64().unwrap() as usize;
            assert!(node["rotation"].is_null() && node["scale"].is_null());
            for primitive in doc["meshes"][mesh]["primitives"].as_array().unwrap() {
                let bounds = &doc["accessors"]
                    [primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
                for (axis, centre, half) in [
                    (0, def.footprint_center.x, def.footprint.x / 2.0),
                    (2, def.footprint_center.y, def.footprint.y / 2.0),
                ] {
                    let translation = node["translation"][axis].as_f64().unwrap_or(0.0) as f32;
                    let low = bounds["min"][axis].as_f64().unwrap() as f32 + translation;
                    let high = bounds["max"][axis].as_f64().unwrap() as f32 + translation;
                    assert!(
                        low >= centre - half - 0.001 && high <= centre + half + 0.001,
                        "{name}: geometry outside the reserved plot on axis {axis}: {low}..{high}"
                    );
                }
                if node["name"] == format!("{name}Door") {
                    let low = bounds["min"][1].as_f64().unwrap()
                        + node["translation"][1].as_f64().unwrap();
                    let high = bounds["max"][1].as_f64().unwrap()
                        + node["translation"][1].as_f64().unwrap();
                    assert!((0.02..0.04).contains(&low) && high - low >= 2.17);
                }
            }
        }
    }
}

#[test]
fn wheat_stays_walkable_and_inside_both_reserved_fields() {
    let doc = glb_document(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets/game_assets/environment/crops/WheatField.glb"),
    );
    assert!(doc["animations"].is_null() && doc["images"].is_null());
    assert_eq!(doc["materials"].as_array().unwrap().len(), 1);
    assert_eq!(doc["materials"][0]["doubleSided"], true);
    assert_ne!(doc["materials"][0]["alphaMode"], "BLEND");
    let primitive = &doc["meshes"][0]["primitives"][0];
    let accessor =
        &doc["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
    assert!(accessor["count"].as_u64().unwrap() <= 15_728);
    let half = SettlementBuildingKind::Farmstead
        .field_half_extents()
        .unwrap();
    for (axis, extent) in [(0, half.x), (2, half.y)] {
        assert!(accessor["min"][axis].as_f64().unwrap() >= -(extent as f64) - 0.001);
        assert!(accessor["max"][axis].as_f64().unwrap() <= extent as f64 + 0.001);
    }
    let colliders = crate::colliders::load_baked_collider_db_from_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/assets/colliders.bin"),
    )
    .unwrap();
    assert!(!colliders
        .entries
        .keys()
        .any(|key| key.to_lowercase().contains("wheat")));
}
