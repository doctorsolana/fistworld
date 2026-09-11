//! Shared book artwork, widgets and button styling. Pages own their data/input.

mod artwork;
mod skin;
mod widgets;

pub(crate) use artwork::{LedgerArtwork, LedgerIcon, LedgerIllustration};
pub(crate) use widgets::{
    body, corners, heading, icon, illustration, paper, pennant, person_portrait, portrait_frame,
    reading, rule, wood,
};

use bevy::{prelude::*, ui::UiSystems};

pub(crate) fn install(app: &mut App) {
    app.add_systems(Startup, artwork::load_artwork)
        .add_systems(
            PostUpdate,
            artwork::fit_illustrations.after(UiSystems::PostLayout),
        )
        .add_systems(
            PostUpdate,
            (
                artwork::bind_icons,
                artwork::bind_surfaces,
                artwork::bind_portrait_frames,
                artwork::bind_illustrations,
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
