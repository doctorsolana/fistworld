//! World map overlay (Valheim-style)

pub mod assets;
pub mod layout;
pub mod markers;
pub mod projection;

use assets::{
    ensure_map_texture, ensure_marker_assets, update_map_image_handle, update_marker_image_handle,
};
use layout::{
    close_map_on_escape, despawn_map_ui, ensure_map_bounds, handle_backdrop_click, spawn_map_ui,
    sync_map_open_state, toggle_map,
};
use markers::update_player_marker;
use projection::world_to_map;

use bevy::asset::RenderAssetUsages;
use bevy::math::Rot2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::UiTransform;
use bevy::window::{CursorOptions, PrimaryWindow};

use shared::components::{LocalPlayer, PlayerPosition, PlayerRotation};
use shared::map::MapBounds;

use super::modal::{modal_backdrop_chrome, modal_root_chrome, sync_modal_cursor, ModalRoot};
use super::styles::{EMBER, INK_INVERSE, MENU_BACKGROUND, MODAL_BACKDROP, PLATE_RULE};
use crate::input::InputState;
use crate::states::GameState;

const MAP_TEX_SIZE: u32 = 512;
const MAP_PANEL_SIZE: f32 = 512.0;
const PLAYER_ARROW_SIZE: f32 = 14.0;
const PLAYER_ARROW_TEX: u32 = 24;

pub struct WorldMapPlugin;

impl Plugin for WorldMapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapOpen>();
        app.init_resource::<MapTexture>();
        app.init_resource::<MapUiConfig>();
        app.init_resource::<MapMarkerAssets>();
        app.add_systems(
            Update,
            (
                toggle_map,
                close_map_on_escape,
                handle_backdrop_click,
                sync_map_open_state,
                ensure_map_bounds,
                ensure_map_texture,
                ensure_marker_assets,
                spawn_map_ui,
                despawn_map_ui,
                update_map_image_handle,
                update_marker_image_handle,
                update_player_marker,
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[derive(Resource, Default)]
pub struct MapOpen(pub bool);

#[derive(Resource, Default)]
struct MapTexture {
    handle: Option<Handle<Image>>,
}

#[derive(Resource, Default)]
struct MapUiConfig {
    bounds: Option<MapBounds>,
}

#[derive(Resource, Default)]
struct MapMarkerAssets {
    player_arrow: Option<Handle<Image>>,
}

#[derive(Component)]
struct MapRoot;

#[derive(Component)]
struct MapBackdrop;

#[derive(Component)]
struct MapPanel;

#[derive(Component)]
struct MapImage;

#[derive(Component)]
struct MapMarkerLayer;

#[derive(Component)]
struct MapPlayerMarker;
