pub mod detection;
pub mod ranges;
pub mod visibility;

pub(super) use detection::apply_prop_render_tuning;
pub(super) use ranges::update_prop_visibility_ranges;
pub(super) use visibility::{reveal_pending_prop_roots, update_tree_lod_visibility};

use detection::{adjust_lod_level, detect_prop_lod_level};
use ranges::{build_prop_visibility_range, prop_fade_distance, update_prop_ranges_for_root};
use visibility::{apply_prop_visibility_for_mesh, apply_tree_mesh_defaults};

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::props::PropRenderTuning;
use shared::terrain::CHUNK_SIZE;

use crate::render::lod::{apply_lod_visibility, LodDebugAction};
use crate::render::systems::GraphicsSettings;
use crate::terrain::TerrainChunk;
use crate::water::WaterChunk;

use super::{
    is_tree_kind, EnvironmentProp, PendingPropVisibility, PropKindTag, PropLodDebugMode,
    PropLodPresence, PropVisibilityReady, TreeActiveLod, TreeLodEntities, TreeLodRoot,
    TreeLodRuntimeState,
};
use shared::components::{LocalPlayer, PlayerPosition};
const PROP_LOD1_SPLIT_RATIO: f32 = 0.7;
const PROP_LOD1_START_FALLBACK: f32 = 200.0;
const PROP_LOD1_END_FALLBACK: f32 = 2000.0;
const PROP_LOD_FADE_DISTANCE: f32 = 20.0;
const TREE_LOD_HYSTERESIS: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PropLodLevel {
    Lod0,
    Lod1,
}
