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
    triangles_except(path, "")
}

fn triangles_except(path: &Path, excluded_node: &str) -> Vec<[Vec3; 3]> {
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
        if node["name"].as_str() == Some(excluded_node) {
            continue;
        }
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
    [(a, b), (b, c), (c, a)].iter().all(|(start, end)| {
        (*end - *start)
            .cross(point - *start)
            .dot(normal.normalize())
            >= -1e-6
    })
}

#[test]
fn all_authored_roofs_have_outward_facing_undersides() {
    // Main roofs, hips, porch roofs and both lean-to shelters. Bounds exclude
    // nearby framing, so two-sided materials cannot conceal an open mesh.
    let cases: &[(&str, &[(f32, f32, f32, f32)])] = &[
        (
            "Bakery",
            &[(3.10, 0.3, 3.02, 3.20), (1.5, -3.0, 2.65, 2.80)],
        ),
        (
            "MootHall",
            &[(3.3, 0.7, 4.55, 4.80), (1.0, -4.4, 5.85, 6.05)],
        ),
        (
            "VillageHall",
            &[(3.85, 0.7, 5.45, 5.65), (1.0, -4.4, 7.0, 7.2)],
        ),
        (
            "TownHall",
            &[(4.95, 0.7, 9.20, 9.40), (1.0, -4.4, 14.0, 14.3)],
        ),
        (
            "LogCabin",
            &[(1.4, -3.23, 3.05, 3.25), (0.4, -3.70, 2.55, 2.72)],
        ),
        (
            "CabinL2",
            &[(1.4, -3.41, 5.5, 5.7), (0.4, -3.70, 2.55, 2.72)],
        ),
        (
            "LongCabin",
            &[
                (0.65, -2.31, 2.4, 2.6),
                (0.4, -2.72, 2.55, 2.72),
                (3.78, 0.50, 2.6, 2.9),
            ],
        ),
        (
            "LongCabinL2",
            &[(2.0, -2.52, 4.5, 4.8), (4.0, 0.5, 4.7, 5.0)],
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
        ("LogCabin", -3.75, 2.19, 2.23),
        ("LongCabin", -2.82, 2.19, 2.23),
        ("CabinL2", -3.75, 2.19, 2.23),
        ("LongCabinL2", -2.82, 2.22, 2.30),
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

#[test]
fn resized_houses_keep_human_scale_and_a_level_open_entrance() {
    for (name, width, depth) in [
        ("LogCabin", 5.0, 6.0),
        ("CabinL2", 5.0, 6.0),
        ("LongCabin", 7.2, 4.2),
        ("LongCabinL2", 7.2, 4.2),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        let doc = glb_document(&path);
        let door = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == "HouseDoor")
            .unwrap();
        let primitive = &doc["meshes"][door["mesh"].as_u64().unwrap() as usize]["primitives"][0];
        let bounds =
            &doc["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
        let low = bounds["min"][1].as_f64().unwrap() + door["translation"][1].as_f64().unwrap();
        let high = bounds["max"][1].as_f64().unwrap() + door["translation"][1].as_f64().unwrap();
        assert!(
            (0.02..0.04).contains(&low) && high - low >= 2.09,
            "{name}: doorway must fit a 1.70 m person at terrain grade"
        );
        let closed = triangles(&path);
        for x in [-0.36, -0.12, 0.12, 0.36] {
            for height in [0.4, 1.1, 2.0] {
                assert!(
                    closed.iter().any(|triangle| {
                        let turned = triangle.map(|v| Vec3::new(v.x, v.z, v.y));
                        let front = -depth / 2.0;
                        underside_at(turned, x, height, front - 0.11, front + 0.03)
                            || underside_at(
                                [turned[0], turned[2], turned[1]],
                                x,
                                height,
                                front - 0.11,
                                front + 0.03,
                            )
                    }),
                    "{name}: see-through seam in the closed door"
                );
            }
        }
        let mesh = triangles_except(&path, "HouseDoor");
        assert!(
            mesh.iter()
                .any(|&triangle| underside_at(triangle, 0.0, 0.0, 2.25, 2.42)),
            "{name}: an open door must reveal a ceiling, not sky through the attic"
        );
        // Probe the facade itself at torso height, excluding the roof silhouette.
        let wall_vertices: Vec<_> = mesh
            .iter()
            .flatten()
            .filter(|v| (0.4..2.0).contains(&v.y))
            .collect();
        for (axis, minimum) in [(0, width), (2, depth)] {
            let lo = wall_vertices
                .iter()
                .map(|v| v[axis])
                .fold(f32::INFINITY, f32::min);
            let hi = wall_vertices
                .iter()
                .map(|v| v[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                hi - lo >= minimum,
                "{name}: the wall plan shrank on axis {axis}"
            );
        }
        // With the leaf omitted, the entire entrance corridor must be clear from
        // 3 cm above grade to 1.95 m. This catches a raised solid foundation or
        // a rail/brace across the opening even when the door animation still works.
        for x in [-0.28, 0.0, 0.28] {
            for offset in [-0.20, 0.0, 0.20, 0.50, 0.75] {
                let z = -depth / 2.0 - offset;
                for &triangle in &mesh {
                    assert!(
                        !underside_at(triangle, x, z, 0.03, 1.95)
                            && !underside_at(
                                [triangle[0], triangle[2], triangle[1]],
                                x,
                                z,
                                0.03,
                                1.95
                            ),
                        "{name}: raised step or obstruction at ({x}, {z}): {triangle:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn house_attic_walls_meet_the_sloping_roof_above_the_wall_plate() {
    // Probe horizontally through the former daylight gaps. An intact roof
    // underside alone cannot close these gaps between the roof and its walls.
    for (name, side, facade, height, along) in [
        ("LogCabin", true, 2.50, 2.50, 0.7),
        ("LogCabin", false, -3.00, 3.13, 1.4),
        ("CabinL2", true, 2.72, 4.75, 0.7),
        ("CabinL2", false, -3.20, 5.58, 1.4),
        ("LongCabin", false, -2.10, 2.58, 0.7),
        ("LongCabin", true, 3.60, 2.95, 0.7),
        ("LongCabinL2", false, -2.30, 4.73, 0.7),
        ("LongCabinL2", true, 3.82, 4.98, 0.7),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        assert!(
            triangles(&path).iter().any(|triangle| {
                let turned = triangle.map(|v| {
                    if side {
                        Vec3::new(v.z, v.x, v.y)
                    } else {
                        Vec3::new(v.x, v.z, v.y)
                    }
                });
                underside_at(turned, along, height, facade - 0.01, facade + 0.01)
                    || underside_at(
                        [turned[0], turned[2], turned[1]],
                        along,
                        height,
                        facade - 0.01,
                        facade + 0.01,
                    )
            }),
            "{name}: daylight gap at facade {facade}, height {height}"
        );
    }
}

#[test]
fn civic_halls_keep_full_size_walls_and_clear_level_entrances() {
    for (name, width, depth) in [
        ("MootHall", 6.0, 7.6),
        ("VillageHall", 6.5, 8.8),
        ("TownHall", 9.2, 13.35),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        let mesh = triangles_except(&path, &format!("{name}Door"));
        let walls: Vec<_> = mesh
            .iter()
            .flatten()
            .filter(|v| (0.4..2.0).contains(&v.y))
            .collect();
        for (axis, minimum) in [(0, width), (2, depth)] {
            let lo = walls.iter().map(|v| v[axis]).fold(f32::INFINITY, f32::min);
            let hi = walls
                .iter()
                .map(|v| v[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                hi - lo >= minimum - 0.001,
                "{name}: wall plan shrank on axis {axis}"
            );
        }
        for x in [-0.28, 0.0, 0.28] {
            for z in [-3.82, -4.02, -4.22, -4.52] {
                assert!(
                    mesh.iter()
                        .all(|&triangle| !underside_at(triangle, x, z, 0.031, 2.05)
                            && !underside_at(
                                [triangle[0], triangle[2], triangle[1]],
                                x,
                                z,
                                0.031,
                                2.05
                            )),
                    "{name}: raised sill or obstruction in entrance"
                );
            }
        }
    }
}

#[test]
fn civic_front_window_panes_are_in_front_of_the_wall_finish() {
    for (name, x) in [
        ("MootHall", 2.48),
        ("VillageHall", 2.67),
        ("TownHall", 3.15),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        let closed = triangles(&path);
        let opaque = triangles_except(&path, &format!("{name}Glass"));
        let hit = |triangle: [Vec3; 3], low, high| {
            let t = triangle.map(|v| Vec3::new(v.x, v.z, v.y));
            underside_at(t, x, 1.30, low, high)
                || underside_at([t[0], t[2], t[1]], x, 1.30, low, high)
        };
        assert!(
            closed.iter().any(|&t| hit(t, -4.085, -4.079)),
            "{name}: missing exterior pane"
        );
        assert!(
            opaque.iter().all(|&t| !hit(t, -5.0, -4.083)),
            "{name}: opaque wall/detail covers the window"
        );
    }
}

#[test]
fn rural_roofs_close_their_eaves_and_entries_stay_at_grade() {
    for (name, side, facade, roof_low, roof_high) in [
        ("Farmstead", 2.45, -2.70, 2.68, 2.90),
        ("LivestockFarm", 4.02, -2.80, 3.13, 3.39),
        ("StoneQuarry", -2.56, -3.20, 2.76, 3.03),
        ("Church", 3.14, -5.40, 5.25, 5.52),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../client/assets/game_assets/buildings/village/{name}.glb"
        ));
        let mesh = triangles_except(&path, &format!("{name}Door"));
        assert!(
            mesh.iter()
                .any(|&t| underside_at(t, side, 0.0, roof_low, roof_high)),
            "{name}: exposed roof must have an outward-facing underside"
        );
        for x in [-0.28, 0.0, 0.28] {
            for offset in [-0.08, 0.0, 0.25, 0.60] {
                assert!(
                    mesh.iter()
                        .all(|&t| !underside_at(t, x, facade - offset, 0.031, 2.05)
                            && !underside_at([t[0], t[2], t[1]], x, facade - offset, 0.031, 2.05)),
                    "{name}: entrance blocked above terrain grade"
                );
            }
        }
    }
}

#[test]
fn livestock_side_posts_leave_every_window_open() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../client/assets/game_assets/buildings/village/LivestockFarm.glb");
    let opaque = triangles_except(&path, "LivestockFarmGlass");
    for side in [-1.0, 1.0] {
        for z in [-1.65, 0.40, 2.65] {
            for offset in [-0.22, 0.22] {
                assert!(
                    opaque.iter().all(|triangle| {
                        let t = triangle.map(|v| Vec3::new(v.z, v.x * side, v.y));
                        !underside_at(t, z + offset, 1.90, 3.865, 4.10)
                            && !underside_at([t[0], t[2], t[1]], z + offset, 1.90, 3.865, 4.10)
                    }),
                    "barn: a post or wall finish covers a side window"
                );
            }
        }
    }
}

#[test]
fn church_apse_glass_faces_outward() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../client/assets/game_assets/buildings/village/Church.glb");
    // Exclude the opaque shell. The door is at negative Z, leaving only glass
    // on the polygonal apse behind the nave in this rear-facing sample.
    let panes: Vec<_> = triangles_except(&path, "Church")
        .into_iter()
        .filter(|triangle| triangle.iter().all(|vertex| vertex.z > 3.5))
        .collect();
    assert!(panes.len() >= 8);
    for [a, b, c] in panes {
        let normal = (b - a).cross(c - a);
        let outward = (a + b + c) / 3.0 - Vec3::new(0.0, 0.0, 3.3);
        assert!(
            normal.x * outward.x + normal.z * outward.z > 1e-6,
            "backface culling must not hide the rear stained glass"
        );
    }
}
