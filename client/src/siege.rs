//! Siege presentation and aiming. Orders and damage remain on the server.
use bevy::prelude::*;
use shared::components::*;
mod controls;
mod effects;
mod visuals;
pub use controls::SiegeAim;
pub(crate) use visuals::CatapultSceneReady;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SiegeInputSet;
pub struct SiegePlugin;
impl Plugin for SiegePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SiegeAim>();
        app.add_systems(OnExit(crate::states::GameState::Playing), controls::cleanup);
        app.add_systems(Startup, effects::load_assets);
        app.add_systems(
            OnEnter(crate::states::GameState::Playing),
            controls::spawn_panel,
        );
        app.add_systems(
            Update,
            (
                visuals::attach,
                visuals::tag_rig,
                visuals::animate,
                effects::attach,
                effects::animate,
            )
                .chain()
                .run_if(in_state(crate::states::GameState::Playing)),
        );
        app.add_systems(
            Update,
            (controls::aim_keys, controls::buttons)
                .chain()
                .in_set(SiegeInputSet)
                .before(crate::selection::SelectionGestureSet)
                .run_if(in_state(crate::states::GameState::Playing)),
        );
        app.add_systems(
            Update,
            (controls::panel, controls::preview)
                .after(crate::selection::SelectionGestureSet)
                .run_if(in_state(crate::states::GameState::Playing)),
        );
    }
}
pub(crate) fn seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}
