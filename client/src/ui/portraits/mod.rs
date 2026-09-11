//! Shared portraits of observed people, with bounded on-demand generation.

mod cache;
mod raster;
mod service;
mod source;
mod widgets;

pub(crate) use service::PortraitMetrics;
pub use widgets::{outfit, person, PersonPortrait};
pub(crate) use widgets::{OutfitPortrait, PortraitStatus};

use crate::states::GameState;
use bevy::{prelude::*, ui::UiSystems};

pub(crate) fn install(app: &mut App) {
    app.init_resource::<cache::PortraitCache>()
        .init_resource::<service::SourceCache>()
        .init_resource::<PortraitMetrics>()
        .add_systems(OnEnter(GameState::Playing), service::remember_existing)
        .add_systems(
            Update,
            service::remember_changed.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            PostUpdate,
            service::sync_widgets
                .after(UiSystems::PostLayout)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(OnExit(GameState::Playing), service::clear_world);
}
