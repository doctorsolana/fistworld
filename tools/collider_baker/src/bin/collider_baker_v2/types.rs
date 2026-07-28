use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ColliderManifest {
    pub(crate) version: u32,
    pub(crate) entries: Vec<ColliderManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ColliderManifestEntry {
    pub(crate) kind: String,
    pub(crate) gltf_path: String,
    pub(crate) mode: ColliderMode,
    pub(crate) vertex_filter: VertexFilter,
    pub(crate) collidable: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum ColliderMode {
    ConvexHull,
    ConvexDecomposition,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum VertexFilter {
    All,
    LowerYPercent { percent: f32 },
    TrunkCore { y_percent: f32, xz_percentile: f32 },
    XZRadiusPercentile { percentile: f32 },
}

#[derive(Resource)]
pub(crate) struct BakeConfig {
    pub(crate) manifest_path: PathBuf,
    pub(crate) output_path: PathBuf,
}

#[derive(Resource)]
pub(crate) struct BakeState {
    pub(crate) manifest: ColliderManifest,
    pub(crate) handles: HashMap<String, Handle<WorldAsset>>,
    pub(crate) started: bool,
}
