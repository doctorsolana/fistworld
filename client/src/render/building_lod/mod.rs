//! Full detail nearby, reduced meshes farther out, hidden at extreme distance.
mod assets;
mod binding;
mod selection;
mod state;
#[cfg(test)]
mod tests;

use bevy::prelude::*;
pub(crate) use state::{BuildingLod, BuildingLodOverride};
pub(crate) const LOD_COUNT: usize = 2;
pub(crate) const HIDDEN: usize = 2;

pub(crate) struct BuildingLodPlugin;

impl Plugin for BuildingLodPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<assets::Catalog>()
            .add_observer(binding::scene_ready)
            .add_systems(Update, (binding::register, binding::bind).chain())
            .add_systems(
                PostUpdate,
                selection::select
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
                    .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds),
            );
    }
}
