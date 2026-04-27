//! Props plugin wiring.

use bevy::prelude::*;

use crate::states::GameState;

use super::PropLodDebugMode;
use super::{
    assets, debug, foliage, lod, spawn, BuildZoneChunkIndex, FoliageMaterialCache,
    LoadedPropChunks, PropChunkIndex, SimplePropMeshCache,
};

/// Plugin for environmental props.
pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedPropChunks>();
        app.init_resource::<PropChunkIndex>();
        app.init_resource::<BuildZoneChunkIndex>();
        app.init_resource::<FoliageMaterialCache>();
        app.init_resource::<PropLodDebugMode>();
        app.init_resource::<SimplePropMeshCache>();
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
                spawn::spawn_chunk_props,
                spawn::sync_props_enabled_state,
                lod::apply_prop_render_tuning,
                lod::reveal_pending_prop_roots,
                lod::update_tree_lod_visibility,
                foliage::refresh_foliage_materials_on_setting_change,
                foliage::apply_foliage_materials,
                lod::update_prop_visibility_ranges,
                spawn::cleanup_chunk_props,
                debug::debug_draw_prop_colliders,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
