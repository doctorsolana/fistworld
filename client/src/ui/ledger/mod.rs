//! Shared book artwork, widgets and button styling. Pages own their data/input.

mod artwork;
mod illustrations;
mod scrollbars;
mod skin;
mod widgets;

pub(crate) use artwork::{LedgerArtwork, LedgerIcon, LedgerIllustration};
pub(crate) use illustrations::IllustrationMaterial;
pub(crate) use skin::LedgerButtonFace;
pub(crate) use widgets::{
    binding_ornament_rule, body, body_strong, corners, directory_gutter, directory_paper, heading,
    icon, illustration, illustration_medallion, ornament_rule, paper, pennant, person_portrait,
    portrait_frame, reading, reading_strong, rule, wood,
};

use bevy::{prelude::*, ui::UiSystems};

/// Opts a retained panel into the shared worn button faces without coupling
/// its controls or navigation to the encyclopedia.
#[derive(Component)]
pub(crate) struct LedgerButtonScope;

pub(crate) fn install(app: &mut App) {
    app.add_plugins(UiMaterialPlugin::<IllustrationMaterial>::default())
        .init_resource::<illustrations::IllustrationMaterials>()
        .add_systems(Startup, artwork::load_artwork)
        .add_systems(
            PostUpdate,
            scrollbars::sync_scrollbars.after(UiSystems::PostLayout),
        )
        .add_systems(
            PostUpdate,
            (
                artwork::bind_icons,
                artwork::bind_surfaces,
                artwork::bind_portrait_frames,
                illustrations::bind_illustrations,
                scrollbars::bind_scrollbars,
                skin::bind_buttons,
            )
                .before(UiSystems::Prepare),
        )
        .add_systems(
            PostUpdate,
            skin::paint_buttons
                .after(skin::bind_buttons)
                .after(crate::ui::button_motion::animate_buttons)
                .before(UiSystems::Layout),
        );
}
