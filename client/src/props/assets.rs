use bevy::gltf::Gltf;
use bevy::prelude::*;
use std::collections::HashMap;

use super::is_tree_kind;
use super::{
    ClientDerivedColliderLibrary, DerivedCollider, DerivedHull, HullFace, PropAssets, TreeMeshSet,
};

#[derive(Clone, Copy)]
struct TreeMeshLabels {
    lod0_label: &'static str,
    lod1_label: Option<&'static str>,
    material_label: &'static str,
}

fn tree_mesh_labels(kind: shared::props::PropKind) -> Option<TreeMeshLabels> {
    use shared::props::PropKind::*;
    match kind {
        Tree_01 | Tree_02 | Tree_08 | Tree_09 | Tree_10 | Tree_18 | Tree_29 => {
            Some(TreeMeshLabels {
                lod0_label: "Mesh0/Primitive0",
                lod1_label: Some("Mesh1/Primitive0"),
                material_label: "Material0",
            })
        }
        Dead_tree_1 | Dead_tree_2 | Dead_tree_3 | Pine_Tree_1 | Pine_Tree_2 | Pine_Tree_3
        | Pine_Tree_4 => Some(TreeMeshLabels {
            lod0_label: "Mesh0/Primitive0",
            lod1_label: None,
            material_label: "Material0",
        }),
        _ => None,
    }
}

/// Load all prop GLTF assets at startup.
pub(super) fn load_prop_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let mut scenes = HashMap::new();
    let mut gltfs = HashMap::new();
    let mut tree_meshes = HashMap::new();
    for kind in shared::props::ALL_PROP_KINDS.iter().copied() {
        scenes.insert(kind, asset_server.load(kind.scene_path()));
        let base = kind
            .scene_path()
            .split('#')
            .next()
            .unwrap_or(kind.scene_path());
        gltfs.insert(kind, asset_server.load::<Gltf>(base));
        if is_tree_kind(kind) {
            let Some(labels) = tree_mesh_labels(kind) else {
                warn!("Missing tree mesh labels for kind {:?}", kind);
                continue;
            };
            let mesh0 = asset_server.load(format!("{base}#{}", labels.lod0_label));
            let mesh1 = labels
                .lod1_label
                .map(|label| asset_server.load(format!("{base}#{label}")));
            let material = asset_server.load(format!("{base}#{}", labels.material_label));
            tree_meshes.insert(
                kind,
                TreeMeshSet {
                    lod0: mesh0,
                    lod1: mesh1,
                    material,
                },
            );
        }
    }

    commands.insert_resource(PropAssets {
        scenes,
        gltfs,
        tree_meshes,
    });

    info!("Loaded environmental prop assets");
}

/// Load baked colliders (for debug visualization).
pub(super) fn load_baked_prop_colliders(mut commands: Commands) {
    let path = "client/assets/colliders.bin";
    let db = match shared::colliders::load_baked_collider_db_from_file(path) {
        Ok(db) => db,
        Err(e) => {
            warn!("Could not load baked colliders at {path}: {e} (debug gizmos disabled)");
            return;
        }
    };

    let mut by_kind = HashMap::new();
    for kind in shared::props::ALL_PROP_KINDS.iter().copied() {
        let Some(baked) = db.entries.get(kind.id()) else {
            continue;
        };
        if let Some(d) = derive_collider(baked) {
            by_kind.insert(kind, d);
        }
    }

    info!("Loaded baked colliders for debug: {} kinds", by_kind.len());
    commands.insert_resource(ClientDerivedColliderLibrary { by_kind });
}

fn build_hull_from_points(points: &[[f32; 3]]) -> Option<DerivedHull> {
    if points.len() < 4 {
        return None;
    }

    let mut r2 = 0.0f32;
    let vertices: Vec<Vec3> = points
        .iter()
        .map(|p| {
            r2 = r2.max(p[0] * p[0] + p[2] * p[2]);
            Vec3::new(p[0], p[1], p[2])
        })
        .collect();

    let hull_faces = triangulate_convex_hull(&vertices);
    Some(DerivedHull {
        bounding_radius: r2.sqrt(),
        hull_faces,
    })
}

fn derive_collider(baked: &shared::colliders::BakedCollider) -> Option<DerivedCollider> {
    match baked {
        shared::colliders::BakedCollider::ConvexHull { points } => {
            let hull = build_hull_from_points(points)?;
            Some(DerivedCollider {
                bounding_radius: hull.bounding_radius,
                hulls: vec![hull],
            })
        }
        shared::colliders::BakedCollider::CompoundConvex { hulls } => {
            let mut derived = Vec::new();
            let mut max_r = 0.0f32;
            for hull_points in hulls {
                let Some(hull) = build_hull_from_points(hull_points) else {
                    continue;
                };
                max_r = max_r.max(hull.bounding_radius);
                derived.push(hull);
            }
            if derived.is_empty() {
                return None;
            }
            Some(DerivedCollider {
                bounding_radius: max_r,
                hulls: derived,
            })
        }
    }
}

/// Triangulate a convex hull from its vertices for visualization.
fn triangulate_convex_hull(vertices: &[Vec3]) -> Vec<HullFace> {
    if vertices.len() < 4 {
        return vec![];
    }

    let centroid = vertices.iter().fold(Vec3::ZERO, |a, &b| a + b) / vertices.len() as f32;
    let mut faces = Vec::new();
    let n = vertices.len();

    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let v0 = vertices[i];
                let v1 = vertices[j];
                let v2 = vertices[k];

                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let mut normal = e1.cross(e2);
                let len = normal.length();
                if len < 1e-6 {
                    continue;
                }
                normal /= len;

                let d = normal.dot(v0);

                // Check if all other vertices are behind this plane.
                let mut valid = true;
                for (m, vertex) in vertices.iter().enumerate().take(n) {
                    if m == i || m == j || m == k {
                        continue;
                    }
                    let dist = normal.dot(*vertex) - d;
                    if dist > 1e-4 {
                        valid = false;
                        break;
                    }
                }

                if !valid {
                    // Try flipped normal.
                    let flipped_normal = -normal;
                    let flipped_d = -d;

                    let mut valid_flipped = true;
                    for (m, vertex) in vertices.iter().enumerate().take(n) {
                        if m == i || m == j || m == k {
                            continue;
                        }
                        let dist = flipped_normal.dot(*vertex) - flipped_d;
                        if dist > 1e-4 {
                            valid_flipped = false;
                            break;
                        }
                    }

                    if valid_flipped {
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                        });
                    }
                } else {
                    // Ensure outward-facing.
                    let to_centroid = centroid - v0;
                    if normal.dot(to_centroid) > 0.0 {
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                        });
                    } else {
                        faces.push(HullFace {
                            vertices: [v0, v1, v2],
                        });
                    }
                }
            }
        }
    }

    // Remove duplicates.
    faces.dedup_by(|a, b| {
        let mut a_verts: Vec<_> = a
            .vertices
            .iter()
            .map(|v| {
                (
                    (v.x * 1000.0) as i32,
                    (v.y * 1000.0) as i32,
                    (v.z * 1000.0) as i32,
                )
            })
            .collect();
        let mut b_verts: Vec<_> = b
            .vertices
            .iter()
            .map(|v| {
                (
                    (v.x * 1000.0) as i32,
                    (v.y * 1000.0) as i32,
                    (v.z * 1000.0) as i32,
                )
            })
            .collect();
        a_verts.sort();
        b_verts.sort();
        a_verts == b_verts
    });

    faces
}
