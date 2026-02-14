//! Shared prop domain components, resources, and collider metadata types.

use bevy::asset::AssetId;
use bevy::prelude::*;
use shared::building::BuildZoneEntry;
use shared::props::PropKind;
use shared::terrain::ChunkCoord;
use std::collections::{HashMap, HashSet};

/// Marker for environment prop entities.
#[derive(Component)]
pub struct EnvironmentProp {
    pub chunk: ChunkCoord,
}

/// Stores which prop kind was spawned (used for collider debug / future collision).
#[derive(Component, Clone, Copy, Debug)]
pub struct PropKindTag(pub PropKind);

/// Marker for props that need double-sided alpha-masked foliage materials.
#[derive(Component)]
pub struct NeedsFoliageMaterials;

/// Marker for prop roots that should stay hidden until render ranges are applied.
#[derive(Component)]
pub struct PendingPropVisibility;

/// Marker for prop mesh entities that have had visibility ranges applied.
#[derive(Component)]
pub struct PropVisibilityReady;

/// Marker for prop roots that use manual LOD switching (trees).
#[derive(Component)]
pub struct TreeLodRoot;

/// Cached LOD mesh entities for tree props.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TreeLodEntities {
    pub lod0: Option<Entity>,
    pub lod1: Option<Entity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TreeActiveLod {
    #[default]
    Hidden,
    Lod0,
    Lod1,
}

/// Runtime state for manual tree LOD switching so we only issue ECS commands on state changes.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TreeLodRuntimeState {
    pub active_lod: TreeActiveLod,
    pub casts_shadows: bool,
}

/// Tracks which LODs exist under a prop root.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PropLodPresence {
    pub has_lod0: bool,
    pub has_lod1: bool,
}

/// Tracks which chunks have had props spawned.
#[derive(Resource, Default)]
pub struct LoadedPropChunks {
    pub chunks: HashSet<ChunkCoord>,
}

/// Index of prop entities by chunk for fast cleanup.
#[derive(Resource, Default)]
pub struct PropChunkIndex {
    pub by_chunk: HashMap<ChunkCoord, Vec<Entity>>,
}

/// Cached build-zone lookup by chunk for prop exclusion checks.
#[derive(Resource)]
pub struct BuildZoneChunkIndex {
    pub by_chunk: HashMap<ChunkCoord, Vec<BuildZoneEntry>>,
    pub dirty: bool,
}

impl Default for BuildZoneChunkIndex {
    fn default() -> Self {
        Self {
            by_chunk: HashMap::new(),
            dirty: true,
        }
    }
}

/// Handles to loaded prop assets.
#[derive(Resource)]
pub struct PropAssets {
    pub scenes: HashMap<PropKind, Handle<Scene>>,
    pub tree_meshes: HashMap<PropKind, TreeMeshSet>,
}

#[derive(Clone)]
pub struct TreeMeshSet {
    pub lod0: Handle<Mesh>,
    pub lod1: Option<Handle<Mesh>>,
    pub material: Handle<StandardMaterial>,
}

/// Client-side derived collider info (for debug gizmos).
#[derive(Resource)]
pub struct ClientDerivedColliderLibrary {
    pub by_kind: HashMap<PropKind, DerivedCollider>,
}

/// Cache of foliage materials already adjusted.
#[derive(Resource, Default)]
pub struct FoliageMaterialCache {
    pub processed: HashSet<AssetId<StandardMaterial>>,
    pub last_cutout_enabled: Option<bool>,
}

/// A face of the convex hull (triangle).
#[derive(Clone, Debug)]
pub struct HullFace {
    pub vertices: [Vec3; 3],
}

#[derive(Clone, Debug)]
pub struct DerivedHull {
    pub bounding_radius: f32,
    /// Triangulated faces of the 3D convex hull.
    pub hull_faces: Vec<HullFace>,
}

#[derive(Clone, Debug)]
pub struct DerivedCollider {
    pub bounding_radius: f32,
    pub hulls: Vec<DerivedHull>,
}
