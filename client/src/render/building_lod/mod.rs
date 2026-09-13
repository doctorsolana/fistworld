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
            .add_systems(Update, (binding::register, binding::bind).chain());
        configure_selection(app);
    }
}

fn configure_selection(app: &mut App) {
    app.add_systems(
        PostUpdate,
        selection::select
            .after(bevy::transform::TransformSystems::Propagate)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
            .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
            // Bevy 0.19 retains mesh bins across frames. The change detector must
            // see the swap before extraction uses the new mesh; otherwise one
            // frame can draw it through the previous forward/prepass/shadow bins.
            .before(bevy::pbr::check_entities_needing_specialization::<StandardMaterial>),
    );
}
