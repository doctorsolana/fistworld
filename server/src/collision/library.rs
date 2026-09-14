//! Static collider resource definitions and startup loading.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::props::PropKind;
use shared::terrain::ChunkCoord;

/// Derived collision info from the baked hull points.
#[derive(Resource)]
pub struct DerivedColliderLibrary {
    pub by_kind: HashMap<PropKind, DerivedCollider>,
}

#[derive(Clone, Debug)]
pub struct DerivedCollider {
    /// Conservative X/Z radius used by navigation and ground movement. Unlike
    /// a full 3D radius, a tall tree does not become a six-metre-wide blocker.
    pub horizontal_radius: f32,
}

/// A single static collider instance in the world (one prop spawn).
#[derive(Clone, Debug)]
pub struct StaticColliderInstance {
    pub kind: PropKind,
    pub position: Vec3,
    pub scale: f32,
    pub rotation: Quat,
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
    /// Trees felled by embodied road work before the corresponding ribbon
    /// segment is complete. The completed road becomes the durable spatial
    /// authority; this small runtime set closes the unload/reload gap between
    /// the axe swing and the next laid point.
    pub cleared_road_trees: HashSet<(i32, i32)>,
}

pub(crate) const COLLIDER_CELL_SIZE: f32 = 16.0;

impl StaticColliders {
    /// None means not streamed yet. A loaded chunk is authoritative for trees
    /// cleared by actual buildings, crop claims and road work.
    pub(crate) fn tree_is_present(&self, point: Vec3) -> Option<bool> {
        if self.road_tree_was_cleared(point.xz()) {
            return Some(false);
        }
        if !self
            .loaded_chunks
            .contains(&ChunkCoord::from_world_pos(point))
        {
            return None;
        }
        let cell = (
            (point.x / COLLIDER_CELL_SIZE).floor() as i32,
            (point.z / COLLIDER_CELL_SIZE).floor() as i32,
        );
        Some(self.cells.get(&cell).is_some_and(|ids| {
            ids.iter().any(|id| {
                self.instances.get(id).is_some_and(|instance| {
                    instance.kind.is_tree()
                        && instance.position.xz().distance_squared(point.xz()) < 0.01
                })
            })
        }))
    }

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
    let mut derived = HashMap::new();
    for kind in shared::props::ALL_PROP_KINDS.iter().copied() {
        if let Some(c) = db.entries.get(kind.id()).cloned() {
            if let Some(d) = derive_collider(&c) {
                derived.insert(kind, d);
            } else {
                warn!(
                    "Baked collider for {} has no usable points; skipping",
                    kind.id()
                );
            }
        }
    }

    info!(
        "Loaded navigation collider radii for {} props (db version {})",
        derived.len(),
        db.version
    );

    commands.insert_resource(DerivedColliderLibrary { by_kind: derived });
    commands.init_resource::<StaticColliders>();
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
            if points.len() < 4 {
                return None;
            }
            Some(DerivedCollider {
                horizontal_radius: horizontal_radius(points),
            })
        }
        shared::colliders::BakedCollider::CompoundConvex { hulls } => {
            let mut max_horizontal = 0.0f32;
            let mut valid_hulls = 0usize;
            for hull_points in hulls {
                if hull_points.len() < 4 {
                    continue;
                }
                max_horizontal = max_horizontal.max(horizontal_radius(hull_points));
                valid_hulls += 1;
            }
            if valid_hulls == 0 {
                return None;
            }
            Some(DerivedCollider {
                horizontal_radius: max_horizontal,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_database_contains_every_collidable_authored_building() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/assets/colliders.bin");
        let db = shared::colliders::load_baked_collider_db_from_file(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for building in shared::building::ALL_BUILDING_TYPES {
            if !building.has_baked_collider() {
                continue;
            }
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

#[cfg(test)]
mod tree_presence_tests {
    use super::*;

    #[test]
    fn timber_search_distinguishes_unloaded_present_and_cleared_trees() {
        let point = Vec3::new(-3.0, 7.0, 18.0);
        let mut colliders = StaticColliders::default();
        assert_eq!(colliders.tree_is_present(point), None);
        colliders
            .loaded_chunks
            .insert(ChunkCoord::from_world_pos(point));
        assert_eq!(colliders.tree_is_present(point), Some(false));
        let cell = (-1, 1);
        colliders.cells.insert(cell, vec![1]);
        colliders.instances.insert(
            1,
            StaticColliderInstance {
                kind: PropKind::OakA,
                position: point,
                scale: 1.0,
                rotation: Quat::IDENTITY,
                cell,
            },
        );
        assert_eq!(colliders.tree_is_present(point), Some(true));
        colliders.mark_road_tree_cleared(point.xz());
        assert_eq!(colliders.tree_is_present(point), Some(false));
        colliders.loaded_chunks.clear();
        assert_eq!(
            colliders.tree_is_present(point),
            Some(false),
            "road clearing survives streaming"
        );
    }
}
