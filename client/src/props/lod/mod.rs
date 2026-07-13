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
use crate::streaming::{streaming_anchor, AnchorCamera, AnchorPlayer};
use crate::terrain::TerrainChunk;
use crate::water::WaterChunk;

use super::{
    is_tree_kind, EnvironmentProp, PendingPropVisibility, PropKindTag, PropLodDebugMode,
    PropLodPresence, PropVisibilityReady, TreeActiveLod, TreeLodMeshHandles, TreeLodRoot,
    TreeLodRuntimeState,
};
const PROP_LOD1_SPLIT_RATIO: f32 = 0.7;
const PROP_LOD1_START_FALLBACK: f32 = 200.0;
const PROP_LOD1_END_FALLBACK: f32 = 2000.0;
const PROP_LOD_FADE_DISTANCE: f32 = 20.0;
const TREE_LOD_HYSTERESIS: f32 = 8.0;
/// Absolute LOD0 range for trees, scaled by `prop_render_multiplier`.
///
/// The ratio-based split (`visible_end_distance * 0.7` = ~315m) sits far
/// beyond the prop streaming radius (~128m at defaults), so LOD1 never
/// engaged: every live tree rendered its full mesh. Clamping the split to an
/// absolute distance inside the streamed range makes the low-poly meshes and
/// the shadow cutoff actually do their job.
const TREE_LOD0_MAX_DISTANCE: f32 = 72.0;
/// Trees stop casting shadows beyond this distance (scaled by
/// `prop_render_multiplier`). Matches the LOD0 range so full-detail trees cast
/// and far trees do not — including tree kinds that have no LOD1 mesh (pines),
/// which previously cast alpha-tested, double-sided shadows at any distance.
const TREE_SHADOW_MAX_DISTANCE: f32 = 72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PropLodLevel {
    Lod0,
    Lod1,
}
