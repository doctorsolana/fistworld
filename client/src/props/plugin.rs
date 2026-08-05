//! Props plugin wiring.

use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

use crate::states::GameState;

use super::ground_cover::{GroundCoverIndex, LoadedGroundCoverChunks, PendingGroundCover};
use super::PropLodDebugMode;
use super::{
    assets, debug, foliage, ground_cover, lod, spawn, BuildZoneChunkIndex, FoliageMaterialCache,
    LoadedPropChunks, PendingPropSpawns, PropChunkIndex, SimplePropMeshCache,
};

/// Plugin for environmental props.
pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<super::wind::WindFoliageMaterial>::default());
        app.init_resource::<LoadedPropChunks>();
        app.init_resource::<PendingPropSpawns>();
        app.init_resource::<PropChunkIndex>();
        app.init_resource::<BuildZoneChunkIndex>();
        app.init_resource::<FoliageMaterialCache>();
        app.init_resource::<PropLodDebugMode>();
        app.init_resource::<SimplePropMeshCache>();
        app.init_resource::<LoadedGroundCoverChunks>();
        app.init_resource::<PendingGroundCover>();
        app.init_resource::<GroundCoverIndex>();
        app.add_systems(OnExit(GameState::Playing), ground_cover::clear_ground_cover);
        app.add_systems(
            Startup,
            (assets::load_prop_assets, assets::load_baked_prop_colliders),
        );
        app.add_systems(
            Update,
            (
                debug::toggle_prop_lod_debug,
                debug::log_prop_density_snapshot,
                spawn::invalidate_props_for_new_buildings,
                spawn::sync_build_zone_chunk_index,
                spawn::clear_props_for_built_village_roads,
                spawn::spawn_chunk_props,
                ground_cover::clear_ground_cover_for_new_buildings,
                ground_cover::clear_ground_cover_for_built_village_roads,
                ground_cover::stream_ground_cover,
                spawn::sync_props_enabled_state,
                lod::apply_prop_render_tuning,
                lod::reveal_pending_prop_roots,
                lod::update_tree_lod_visibility,
                foliage::refresh_foliage_materials_on_setting_change,
                foliage::apply_foliage_materials,
                lod::update_prop_visibility_ranges,
                spawn::cleanup_chunk_props,
                ground_cover::cleanup_ground_cover,
                debug::debug_draw_prop_colliders,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
