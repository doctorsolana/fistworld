//! Client wildlife presentation. The server owns every horse and its behavior;
//! this module only smooths snapshots and renders a bounded nearby set of rigs.
mod rendering;
use crate::states::GameState;
use bevy::prelude::*;
pub(crate) use rendering::{HorseRestSeat, HorseRig};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AnimalsPresentation;

pub struct AnimalsPlugin;
impl Plugin for AnimalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<rendering::HorseAssets>();
        app.add_systems(
            Update,
            (
                rendering::attach,
                rendering::setup_proxy_assets,
                rendering::select_rigs,
                rendering::setup_animation,
                rendering::animate,
            )
                .chain()
                .in_set(AnimalsPresentation)
                .run_if(in_state(GameState::Playing)),
        );
    }
}
