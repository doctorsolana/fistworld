//! Keep the yawing cap, independently spinning rotor and entrance contract intact.
use super::*;

#[test]
fn windmill_keeps_its_plot_and_independent_node_animations() {
    let kind = BuildingType::Windmill;
    assert_eq!(kind.definition().footprint, Vec2::new(5.818, 5.818));
    assert_eq!(kind.definition().footprint_center, Vec2::new(0.0, -0.491));
    assert_animated_building_contract(
        kind,
        crate::components::SettlementBuildingKind::Windmill.door_offset(),
        "WindMillDoor",
        2,
        12_500,
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../client/assets/game_assets/buildings/village/WindMill.glb");
    let doc = glb_document(&path);
    let nodes = doc["nodes"].as_array().unwrap();
    let index = |name: &str| nodes.iter().position(|node| node["name"] == name).unwrap();
    let cap = index("WindMillCap");
    let sails = index("WindMillSails");
    assert_eq!(nodes[cap]["children"], serde_json::json!([sails]));
    assert_eq!(
        nodes[cap]["translation"][0], 0.0,
        "cap pivots on tower axis"
    );
    assert!(
        nodes[sails]["rotation"].is_null(),
        "rotation must start at identity"
    );
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 5);
    let clip = doc["animations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|clip| clip["name"] == "sails_turn")
        .unwrap();
    assert_eq!(clip["channels"].as_array().unwrap().len(), 1);
    assert_eq!(clip["channels"][0]["target"]["node"], sails);
    assert_eq!(clip["channels"][0]["target"]["path"], "rotation");
    let sampler = &clip["samplers"][0];
    assert_eq!(sampler["interpolation"], "LINEAR");
    let input = sampler["input"].as_u64().unwrap() as usize;
    assert_eq!(doc["accessors"][input]["max"][0], 2.0);

    // Decode shipped quaternions: a facing/export change must not silently make
    // the rotor turn about X or Y, or introduce a pause/reversal at the loop seam.
    let bytes = fs::read(path).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = &bytes[28 + json_len..];
    let output = sampler["output"].as_u64().unwrap() as usize;
    let accessor = &doc["accessors"][output];
    assert_eq!(accessor["componentType"], 5126);
    assert_eq!(accessor["type"], "VEC4");
    let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"].as_u64().unwrap_or(16) as usize;
    let count = accessor["count"].as_u64().unwrap() as usize;
    assert!(count >= 25);
    for i in 0..count {
        let p = offset + i * stride;
        let q: [f32; 4] = std::array::from_fn(|n| {
            f32::from_le_bytes(bin[p + 4 * n..p + 4 * n + 4].try_into().unwrap())
        });
        let angle = std::f32::consts::PI * i as f32 / (count - 1) as f32;
        assert!(
            q[0].abs() < 1e-5 && q[1].abs() < 1e-5,
            "windshaft is glTF Z"
        );
        // q and -q encode the same rotation; compare by absolute dot product.
        let dot = q[2] * angle.sin() + q[3] * angle.cos();
        assert!(
            dot.abs() > 0.99999,
            "rotor sample {i} breaks the linear revolution"
        );
    }
}
