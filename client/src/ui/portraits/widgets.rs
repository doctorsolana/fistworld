//! Retained image requests; frames and application actions belong to callers.

use bevy::prelude::*;
use shared::components::{HeroOutfit, PersonId};

/// Attach to a retained image; changing its id requests that person's actual
/// observed appearance without respawning any surrounding UI.
#[derive(Component, Clone, Copy, Debug)]
#[require(PortraitStatus)]
pub struct PersonPortrait(pub PersonId);

/// Explicit appearance requests for the selected embodied character, including
/// capture/creator characters without a durable identity. None shows no likeness.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[require(PortraitStatus)]
pub(crate) struct OutfitPortrait(pub Option<HeroOutfit>);

/// A fallback is complete UI but is not a known likeness. Capture fixtures can
/// distinguish unavailable appearance from a queued or completed raster job.
#[derive(Component, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortraitStatus {
    pub known: bool,
    pub ready: bool,
}

pub fn person(id: PersonId, size: f32) -> impl Bundle {
    (PersonPortrait(id), widget(size))
}

pub fn outfit(appearance: HeroOutfit, size: f32) -> impl Bundle {
    (OutfitPortrait(Some(appearance)), widget(size))
}

fn widget(size: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            border_radius: BorderRadius::MAX,
            overflow: Overflow::clip(),
            ..default()
        },
        ImageNode {
            color: Color::NONE,
            ..default()
        },
        Pickable::IGNORE,
    )
}
