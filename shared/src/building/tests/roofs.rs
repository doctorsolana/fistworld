//! Probe exposed overhangs in the shipped geometry, where no wall or rafter
//! can mask a missing roof underside. glTF coordinates are metres, +Y up.

use super::*;
use bevy::prelude::Vec3;
use serde_json::Value;

fn accessor_rows<'a>(doc: &Value, bin: &'a [u8], index: usize, size: usize) -> Vec<&'a [u8]> {
    let accessor = &doc["accessors"][index];
    let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"].as_u64().unwrap_or(size as u64) as usize;
    (0..accessor["count"].as_u64().unwrap() as usize)
        .map(|i| &bin[start + i * stride..start + i * stride + size])
        .collect()
}

fn triangles(path: &Path) -> Vec<[Vec3; 3]> {
    let bytes = fs::read(path).unwrap();
    let doc = glb_document(path);
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(&bytes[24 + json_len..28 + json_len], b"BIN\0");
    let bin = &bytes[28 + json_len..];
    let mut triangles = Vec::new();
    let nodes = doc["nodes"].as_array().unwrap();
    let mut parents = vec![None; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        if let Some(children) = node["children"].as_array() {
            for child in children {
                parents[child.as_u64().unwrap() as usize] = Some(parent);
            }
        }
    }
    for (node_index, node) in nodes.iter().enumerate() {
        let Some(mesh) = node["mesh"].as_u64() else {
            continue;
        };
        // These building exports bake their static transforms; the animated
        // door is translated to its hinge in the authored closed pose.
        assert!(node["matrix"].is_null() && node["rotation"].is_null() && node["scale"].is_null());
        let mut translation = Vec3::ZERO;
        let mut ancestor = Some(node_index);
        while let Some(index) = ancestor {
            translation += Vec3::from_array(std::array::from_fn(|i| {
                nodes[index]["translation"][i].as_f64().unwrap_or(0.0) as f32
            }));
            ancestor = parents[index];
        }
        for primitive in doc["meshes"][mesh as usize]["primitives"]
            .as_array()
            .unwrap()
        {
            assert_eq!(primitive["mode"].as_u64().unwrap_or(4), 4);
            let positions = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
            assert_eq!(doc["accessors"][positions]["componentType"], 5126);
            assert_eq!(doc["accessors"][positions]["type"], "VEC3");
            let vertices: Vec<_> = accessor_rows(&doc, bin, positions, 12)
                .into_iter()
                .map(|row| {
                    Vec3::from_array(std::array::from_fn(|i| {
                        f32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap())
                    })) + translation
                })
                .collect();
            let indices = primitive["indices"].as_u64().unwrap() as usize;
            let size = match doc["accessors"][indices]["componentType"].as_u64().unwrap() {
                5123 => 2,
                5125 => 4,
                other => panic!("unsupported index type {other}"),
            };
            let indices: Vec<_> = accessor_rows(&doc, bin, indices, size)
                .into_iter()
                .map(|row| {
                    if size == 2 {
                        u16::from_le_bytes(row.try_into().unwrap()) as usize
                    } else {
                        u32::from_le_bytes(row.try_into().unwrap()) as usize
                    }
                })
                .collect();
            for triangle in indices.chunks_exact(3) {
                triangles.push(std::array::from_fn(|i| vertices[triangle[i]]));
            }
        }
    }
    triangles
}

fn underside_at(triangle: [Vec3; 3], x: f32, z: f32, bottom: f32, top: f32) -> bool {
    let [a, b, c] = triangle;
    let normal = (b - a).cross(c - a);
    if normal.y >= -1e-6 {
        return false;
    }
    let height = a.y - (normal.x * (x - a.x) + normal.z * (z - a.z)) / normal.y;
    if !(bottom..top).contains(&height) {
        return false;
    }
    let point = Vec3::new(x, height, z);
    [(a, b), (b, c), (c, a)]
        .iter()
        .all(|(start, end)| (*end - *start).cross(point - *start).dot(normal) >= -1e-5)
}

#[test]
fn all_authored_roofs_have_outward_facing_undersides() {
    // Main roofs, hips, porch roofs and both lean-to shelters. Bounds exclude
    // nearby framing, so two-sided materials cannot conceal an open mesh.
    let cases: &[(&str, &[(f32, f32, f32, f32)])] = &[
        (
            "LogCabin",
            &[(1.4, -2.58, 2.95, 3.20), (0.4, -3.05, 2.35, 2.60)],
        ),
        (
            "CabinL2",
            &[(1.4, -2.76, 5.4, 5.7), (0.4, -3.05, 2.35, 2.60)],
        ),
        (
            "LongCabin",
            &[
                (0.65, -1.89, 2.4, 2.6),
                (0.4, -2.30, 2.35, 2.6),
                (3.54, 0.50, 2.55, 2.9),
            ],
        ),
        (
            "LongCabinL2",
            &[(2.0, -2.1, 4.5, 4.8), (3.76, 0.5, 4.7, 5.0)],
        ),
        (
            "LumberjackHut",
            &[(0.7, -1.96, 2.5, 2.8), (2.25, 0.7, 1.78, 2.0)],
        ),
        (
            "WindMill",
            &[
                (2.4, -0.491, 2.30, 2.55),
                (1.55, -0.491, 6.77, 7.10),
                (0.4, -3.15, 2.4, 2.6),
            ],
        ),
        (
            "StorageHall",
            &[(3.68, 0.4, 3.4, 3.65), (4.0, 0.4, 2.65, 2.85)],
        ),
    ];
    for (name, probes) in cases {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        let mesh = triangles(&path);
        for &(x, z, bottom, top) in *probes {
            assert!(
                mesh.iter()
                    .any(|&triangle| underside_at(triangle, x, z, bottom, top)),
                "{name}: open roof underside above ({x}, {z}) at {bottom}..{top} m"
            );
        }
    }
}

#[test]
fn every_house_porch_has_a_header_below_its_roof_or_balcony() {
    // Probe between the knee braces: an unsupported brace or a gap below
    // the L2 balcony cannot pass merely because a roof exists higher up.
    for (name, front, bottom, top) in [
        ("LogCabin", -3.10, 2.00, 2.10),
        ("LongCabin", -2.40, 2.00, 2.10),
        ("CabinL2", -3.10, 2.00, 2.10),
        ("LongCabinL2", -2.40, 2.22, 2.30),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        assert!(
            triangles(&path)
                .iter()
                .any(|&triangle| { underside_at(triangle, 0.0, front, bottom, top) }),
            "{name}: porch braces need a supporting header"
        );
    }
}
