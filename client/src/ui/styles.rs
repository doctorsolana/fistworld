//! Shared UI styles — limewash and iron-gall ink.
//!
//! The look is a medieval colony ledger: pale limewashed surfaces, warm dark
//! ink, and exactly one saturated colour. Three rules hold the whole thing
//! together, and breaking any of them is what makes a HUD read as a programmer's
//! debug overlay:
//!
//! 1. **No HUD text ever sits directly on the world.** Text lives on a plate.
//!    The world behind it is grass, snow, sand, water and bright sky, and no
//!    single text colour is legible on all of them. Put it on a plate and the
//!    contrast becomes a constant you control.
//! 2. **The plate's SILHOUETTE is the hard problem, not its text.** Ink on
//!    limewash is ~11:1 and cannot break. What breaks is seeing that a plate is
//!    there at all: at RTS tilt the top third of the frame is the hazed horizon,
//!    which `DistanceFog` paints at roughly srgb(0.88, 0.92, 0.96) — BRIGHTER
//!    than the plates. That is why every plate carries [`plate_shadow`] (a hard
//!    contact layer plus a wide ambient one) and a darker-than-comfortable
//!    hairline. Those are load-bearing, not decoration.
//! 3. **One saturated colour, reserved for selection.** [`EMBER`] means "this is
//!    the thing you are commanding" and appears nowhere else in play-mode
//!    chrome. God and debug affordances use the neutral [`SLATE`] inversion, so
//!    dev tools can never be mistaken for game state.
//!
//! Radii stay at 2-3px. Paper and stone do not have 9px corners.

use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

/// Front-of-house stays dark: main menu, name entry, pause. Retuned into the
/// same warm family as everything else.
pub const MENU_BACKGROUND: Color = Color::srgb(0.086, 0.078, 0.070);

/// Every in-game plate. Aged limewash — deliberately DARKER than paper-white so
/// it separates from the hazed horizon behind it.
pub const LIMEWASH: Color = Color::srgba(0.836, 0.812, 0.769, 0.96);

/// The lit face of a plate: the selected-unit plate, which should feel like a
/// fresher surface laid on top.
pub const LIMEWASH_LIT: Color = Color::srgba(0.886, 0.867, 0.827, 0.97);

/// A recessed well inside a plate — the strip a row of buttons sits in.
#[allow(dead_code)]
pub const LIMEWASH_WELL: Color = Color::srgba(0.741, 0.718, 0.678, 0.96);

/// Button rest fill. A shade lighter than the plate, as if raised from it.
pub const BUTTON_NORMAL: Color = Color::srgba(0.871, 0.851, 0.812, 0.96);
/// Hover LIGHTENS: a raised surface catching more light, not ink soaking in.
pub const BUTTON_HOVERED: Color = Color::srgba(0.941, 0.929, 0.902, 0.98);
/// Pressed pushes into shade.
pub const BUTTON_PRESSED: Color = Color::srgba(0.729, 0.706, 0.671, 0.98);

/// The carved outer edge on every plate. Dark and near-opaque on purpose: this
/// is half of how a plate keeps a silhouette against bright terrain.
pub const PLATE_RULE: Color = Color::srgba(0.361, 0.345, 0.318, 0.85);
/// Hairline dividers INSIDE a plate, replacing nested bordered boxes.
pub const PLATE_RULE_SOFT: Color = Color::srgba(0.361, 0.345, 0.318, 0.32);
/// Legacy name, kept so existing menus compile unchanged.
pub const BUTTON_BORDER: Color = PLATE_RULE;

// ---------------------------------------------------------------------------
// Ink
// ---------------------------------------------------------------------------

/// Iron-gall ink. All primary text.
pub const INK: Color = Color::srgb(0.129, 0.118, 0.102);
/// Secondary values.
#[allow(dead_code)]
pub const INK_SOFT: Color = Color::srgb(0.243, 0.227, 0.203);
/// Small-caps labels, hints, keybind lines. Tuned to stay readable at 10px.
pub const INK_MUTED: Color = Color::srgb(0.361, 0.345, 0.318);
/// Text ON a saturated or slate fill, where ink would disappear.
pub const INK_INVERSE: Color = Color::srgb(0.965, 0.957, 0.933);

/// Legacy names, kept so the rest of the UI compiles unchanged.
pub const TEXT_COLOR: Color = INK;
pub const TEXT_MUTED: Color = INK_MUTED;

// ---------------------------------------------------------------------------
// The one accent, and the dev inversion
// ---------------------------------------------------------------------------

/// Burnt sienna. **Selection only.** If this appears anywhere else in play-mode
/// chrome the player can no longer tell at a glance what they are commanding.
pub const EMBER: Color = Color::srgb(0.560, 0.325, 0.129);
/// A defining rule around an ember fill.
pub const EMBER_RULE: Color = Color::srgb(0.400, 0.208, 0.078);
/// Legacy name.
pub const ACCENT_COLOR: Color = EMBER;

/// Neutral dark fill for GOD and debug affordances. Deliberately not the accent:
/// dev chrome must never look like game state.
pub const SLATE: Color = Color::srgba(0.255, 0.243, 0.227, 0.94);

/// Danger. Madder red, reserved for destructive actions.
#[allow(dead_code)]
pub const ACCENT_RED: Color = Color::srgb(0.545, 0.196, 0.169);

/// Warm scrim behind a modal. Stays DARK behind now-light panels; that
/// inversion is what makes a modal read as modal.
#[allow(dead_code)]
pub const MODAL_BACKDROP: Color = Color::srgba(0.055, 0.047, 0.039, 0.46);

// ---------------------------------------------------------------------------
// Shared style helpers
// ---------------------------------------------------------------------------

/// The two-layer drop shadow every plate carries.
///
/// The HARD CONTACT layer is the one that matters: a tight, fairly opaque
/// 3px-blur shadow redraws the plate's edge even when plate and background are
/// nearly the same luminance (pale plate on snow, or on the hazed horizon). The
/// wide ambient layer does the "physical object" read and contributes almost
/// nothing to contrast.
///
/// A single soft shadow — the obvious choice — fails exactly here: too diffuse
/// to redraw an edge, too transparent to be seen doing it.
pub fn plate_shadow() -> BoxShadow {
    BoxShadow(vec![
        ShadowStyle {
            color: Color::srgba(0.078, 0.070, 0.062, 0.70),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(1.0),
            spread_radius: Val::Px(0.0),
            blur_radius: Val::Px(3.0),
        },
        ShadowStyle {
            color: Color::srgba(0.078, 0.070, 0.062, 0.28),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(4.0),
            spread_radius: Val::Px(0.0),
            blur_radius: Val::Px(18.0),
        },
    ])
}

/// Corner radius for plates and buttons. Paper and stone are not iOS cards.
pub const RADIUS: f32 = 3.0;

/// Standard button style
pub(crate) fn button_style() -> Node {
    Node {
        width: Val::Px(280.0),
        height: Val::Px(55.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        margin: UiRect::all(Val::Px(8.0)),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(RADIUS)),
        ..default()
    }
}

/// Standard button text style
pub(crate) fn button_text_style() -> TextFont {
    TextFont {
        font_size: FontSize::Px(22.0),
        ..default()
    }
}

/// Title text style
pub(crate) fn title_text_style() -> TextFont {
    TextFont {
        font_size: FontSize::Px(72.0),
        ..default()
    }
}

/// Subtitle text style
#[allow(dead_code)]
pub(crate) fn subtitle_text_style() -> TextFont {
    TextFont {
        font_size: FontSize::Px(18.0),
        ..default()
    }
}
