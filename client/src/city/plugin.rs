use bevy::prelude::*;

use shared::city::AuthoredCityLayout;

use crate::render::systems;
use crate::states::GameState;

use super::{buildings, spawn};

pub struct CityPlugin;

impl Plugin for CityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AuthoredCityLayout>();
        app.init_resource::<buildings::CityMaterialCache>();
        app.add_systems(
            OnEnter(GameState::Playing),
            (
                spawn::spawn_city_layout_visuals.after(systems::spawn_world),
                buildings::spawn_city_plot_buildings.after(systems::spawn_world),
            ),
        );
        app.add_systems(
            Update,
            buildings::warm_city_building_materials.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            OnExit(GameState::Playing),
            (
                spawn::cleanup_city_layout_visuals,
                buildings::cleanup_city_plot_buildings,
            ),
        );
    }
}
