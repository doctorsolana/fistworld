//! Static collider resource definitions and startup loading.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};

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
    /// Conservative X/Z radius used by navigation and ground movement. Unlike
    /// `bounding_radius`, a tall tree does not become a six-metre-wide blocker.
    pub horizontal_radius: f32,
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
    /// Spatial collision revisions used by navigation cache invalidation.
    /// Rebuilding one streamed/build-zone chunk must not discard certified
    /// routes on the opposite side of the world.
    pub chunk_versions: HashMap<ChunkCoord, u64>,
    pub next_chunk_version: u64,
    /// Instance ids awaiting Rapier collider creation.
    pub pending_added: VecDeque<u32>,
    /// Instance ids whose Rapier collider should be removed.
    pub pending_removed: Vec<u32>,
    /// Trees felled by embodied road work before the corresponding ribbon
    /// segment is complete. The completed road becomes the durable spatial
    /// authority; this small runtime set closes the unload/reload gap between
    /// the axe swing and the next laid point.
    pub cleared_road_trees: HashSet<(i32, i32)>,
}

impl StaticColliders {
    const CLEARED_TREE_KEY_SCALE: f32 = 10.0;

    fn cleared_tree_key(point: Vec2) -> (i32, i32) {
        (
            (point.x * Self::CLEARED_TREE_KEY_SCALE).round() as i32,
            (point.y * Self::CLEARED_TREE_KEY_SCALE).round() as i32,
        )
    }

    pub fn mark_road_tree_cleared(&mut self, point: Vec2) {
        self.cleared_road_trees
            .insert(Self::cleared_tree_key(point));
    }

    pub fn road_tree_was_cleared(&self, point: Vec2) -> bool {
        self.cleared_road_trees
            .contains(&Self::cleared_tree_key(point))
    }

    /// Remove one streamed prop from every collision index immediately.
    ///
    /// Road-clearance jobs use this at the moment a tree is felled. Updating
    /// the spatial revision wakes only routes near that chunk, while the
    /// streaming system's road filter prevents the tree from returning after
    /// an unload/reload cycle.
    pub fn remove_instance(&mut self, id: u32) -> Option<StaticColliderInstance> {
        let instance = self.instances.remove(&id)?;
        self.pending_added.retain(|pending| *pending != id);
        self.pending_removed.push(id);
        if let Some(ids) = self.cells.get_mut(&instance.cell) {
            ids.retain(|candidate| *candidate != id);
            if ids.is_empty() {
                self.cells.remove(&instance.cell);
            }
        }
        let chunk = ChunkCoord::from_world_pos(instance.position);
        if let Some(ids) = self.chunk_instances.get_mut(&chunk) {
            ids.retain(|candidate| *candidate != id);
        }
        self.version = self.version.wrapping_add(1);
        self.next_chunk_version = self.next_chunk_version.wrapping_add(1).max(1);
        self.chunk_versions.insert(chunk, self.next_chunk_version);
        Some(instance)
    }
}

/// Load baked colliders at startup.
///
/// For now we load from the workspace path `client/assets/colliders.bin`.
pub fn setup_baked_colliders(mut commands: Commands) {
    // Anchor to this crate rather than the process working directory. `cargo
    // run` from the workspace happened to make the old relative path work,
    // while tests and diagnostic binaries launched from `server/` could not
    // load the exact same collision world.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/assets/colliders.bin");
    let db = shared::colliders::load_baked_collider_db_from_file(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to load baked colliders from {}: {e}",
            path.display()
        )
    });

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
    let horizontal_radius = |points: &[[f32; 3]]| {
        points
            .iter()
            .map(|point| point[0] * point[0] + point[2] * point[2])
            .fold(0.0f32, f32::max)
            .sqrt()
    };
    match baked {
        shared::colliders::BakedCollider::ConvexHull { points } => {
            let hull = build_hull_from_points(points)?;
            Some(DerivedCollider {
                bounding_radius: hull.bounding_radius,
                horizontal_radius: horizontal_radius(points),
                hulls: vec![hull],
            })
        }
        shared::colliders::BakedCollider::CompoundConvex { hulls } => {
            let mut derived = Vec::new();
            let mut max_r = 0.0f32;
            let mut max_horizontal = 0.0f32;
            for hull_points in hulls {
                let Some(hull) = build_hull_from_points(hull_points) else {
                    continue;
                };
                max_r = max_r.max(hull.bounding_radius);
                max_horizontal = max_horizontal.max(horizontal_radius(hull_points));
                derived.push(hull);
            }
            if derived.is_empty() {
                return None;
            }
            Some(DerivedCollider {
                bounding_radius: max_r,
                horizontal_radius: max_horizontal,
                hulls: derived,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_database_contains_every_authored_building() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/assets/colliders.bin");
        let db = shared::colliders::load_baked_collider_db_from_file(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for building in shared::building::ALL_BUILDING_TYPES {
            let baked = db.entries.get(building.id()).unwrap_or_else(|| {
                panic!(
                    "{} is registered as authored art but missing from colliders.bin",
                    building.id()
                )
            });
            assert!(
                derive_collider(baked).is_some(),
                "{} baked to an empty collider",
                building.id()
            );
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
