//! Static collider resource definitions and startup loading.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::props::PropKind;
use shared::terrain::ChunkCoord;

/// A baked collider library keyed by [`PropKind`].
#[derive(Resource)]
pub struct BakedColliderLibrary {
    pub by_kind: HashMap<PropKind, shared::colliders::BakedCollider>,
}

/// Derived collision info from the baked hull points.
#[derive(Resource)]
pub struct DerivedColliderLibrary {
    pub by_kind: HashMap<PropKind, DerivedCollider>,
}

/// Derived building collider library keyed by [`BuildingType`].
#[derive(Resource)]
pub struct DerivedBuildingColliderLibrary {
    pub by_type: HashMap<shared::building::BuildingType, DerivedCollider>,
}

/// A face of the convex hull (triangle).
#[derive(Clone, Debug)]
pub struct HullFace {
    pub vertices: [Vec3; 3],
    pub normal: Vec3,
    pub d: f32, // plane equation: normal.dot(p) = d
}

#[derive(Clone, Debug)]
pub struct DerivedHull {
    /// Bounding radius for this hull (local space)
    pub bounding_radius: f32,
    /// Triangulated faces of the convex hull with outward normals
    pub hull_faces: Vec<HullFace>,
}

#[derive(Clone, Debug)]
pub struct DerivedCollider {
    /// Bounding radius for broad-phase rejection
    pub bounding_radius: f32,
    /// One or more convex hulls for this collider
    pub hulls: Vec<DerivedHull>,
}

/// A single static collider instance in the world (one prop spawn).
#[derive(Clone, Debug)]
pub struct StaticColliderInstance {
    pub kind: PropKind,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub cell: (i32, i32),
}

/// Streaming state for static colliders.
#[derive(Resource, Default)]
pub struct StaticColliders {
    pub loaded_chunks: HashSet<ChunkCoord>,
    /// Chunk -> list of instance ids
    pub chunk_instances: HashMap<ChunkCoord, Vec<u32>>,
    /// Instance id -> instance
    pub instances: HashMap<u32, StaticColliderInstance>,
    /// Spatial hash cell -> instance ids
    pub cells: HashMap<(i32, i32), Vec<u32>>,
    pub next_id: u32,
    pub version: u64,
}

/// Load baked colliders at startup.
///
/// For now we load from the workspace path `client/assets/colliders.bin`.
pub fn setup_baked_colliders(mut commands: Commands) {
    let path = "client/assets/colliders.bin";
    let db = shared::colliders::load_baked_collider_db_from_file(path)
        .unwrap_or_else(|e| panic!("Failed to load baked colliders from {path}: {e}"));

    // Load prop colliders.
    let mut by_kind = HashMap::new();
    let mut derived = HashMap::new();
    for kind in shared::props::ALL_PROP_KINDS.iter().copied() {
        if let Some(c) = db.entries.get(kind.id()).cloned() {
            if let Some(d) = derive_collider(&c) {
                derived.insert(kind, d);
                by_kind.insert(kind, c);
            } else {
                warn!(
                    "Baked collider for {} has no usable points; skipping",
                    kind.id()
                );
            }
        }
    }

    // Load building colliders.
    let mut building_derived = HashMap::new();
    for building_type in shared::building::ALL_BUILDING_TYPES.iter().copied() {
        if let Some(c) = db.entries.get(building_type.id()).cloned() {
            if let Some(d) = derive_collider(&c) {
                building_derived.insert(building_type, d);
            } else {
                warn!(
                    "Baked collider for building {} has no usable points; skipping",
                    building_type.id()
                );
            }
        }
    }

    info!(
        "Loaded baked colliders: {} props, {} buildings (db version {})",
        by_kind.len(),
        building_derived.len(),
        db.version
    );

    commands.insert_resource(BakedColliderLibrary { by_kind });
    commands.insert_resource(DerivedColliderLibrary { by_kind: derived });
    commands.insert_resource(DerivedBuildingColliderLibrary {
        by_type: building_derived,
    });
    commands.init_resource::<StaticColliders>();
}

fn build_hull_from_points(points: &[[f32; 3]]) -> Option<DerivedHull> {
    if points.len() < 4 {
        return None;
    }

    let mut r2 = 0.0f32;
    let hull_vertices: Vec<Vec3> = points
        .iter()
        .map(|p| {
            r2 = r2.max(p[0] * p[0] + p[1] * p[1] + p[2] * p[2]);
            Vec3::new(p[0], p[1], p[2])
        })
        .collect();

    let hull_faces = triangulate_convex_hull(&hull_vertices);
    if hull_faces.is_empty() {
        warn!(
            "Failed to triangulate convex hull with {} vertices",
            hull_vertices.len()
        );
        return None;
    }

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

/// Triangulate a convex hull from its vertices.
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

                let mut all_behind = true;
                let mut any_in_front = false;

                for (m, other_vertex) in vertices.iter().enumerate().take(n) {
                    if m == i || m == j || m == k {
                        continue;
                    }
                    let dist = normal.dot(*other_vertex) - d;
                    if dist > 1e-4 {
                        any_in_front = true;
                        all_behind = false;
                        break;
                    }
                }

                if !all_behind || any_in_front {
                    let flipped_normal = -normal;
                    let flipped_d = -d;

                    let mut all_behind_flipped = true;
                    for (m, other_vertex) in vertices.iter().enumerate().take(n) {
                        if m == i || m == j || m == k {
                            continue;
                        }
                        let dist = flipped_normal.dot(*other_vertex) - flipped_d;
                        if dist > 1e-4 {
                            all_behind_flipped = false;
                            break;
                        }
                    }

                    if all_behind_flipped {
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                            normal: flipped_normal,
                            d: flipped_d,
                        });
                    }
                } else {
                    let to_centroid = centroid - v0;
                    if normal.dot(to_centroid) > 0.0 {
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                            normal: -normal,
                            d: -d,
                        });
                    } else {
                        faces.push(HullFace {
                            vertices: [v0, v1, v2],
                            normal,
                            d,
                        });
                    }
                }
            }
        }
    }

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
