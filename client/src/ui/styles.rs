//! Shared game chrome: wood frames, brass binding, parchment pages and dark ink.
//!
//! Text lives on opaque plates with a clear edge and shadow against bright terrain.
//! Dark wood titles and inverse controls connect civilian ledgers to the war UI.
//! Ember marks selection, crimson marks combat, red marks destructive actions;
//! debug tools keep their neutral slate. Ornament never carries information.
//! Use typography, frame and motion alongside this palette rather than inventing
//! per-screen fonts, hover handlers or animation math. Corners stay at 2–3 px.

use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

/// Front-of-house stays dark: main menu, name entry, pause. Retuned into the
/// same warm family as everything else.
pub const MENU_BACKGROUND: Color = Color::srgb(0.086, 0.078, 0.070);
/// Opaque dark panel used inside front-of-house screens.
pub const FRONT_PANEL: Color = Color::srgba(0.14, 0.09, 0.06, 0.98);

/// Every in-game plate. Aged parchment — deliberately DARKER than paper-white so
/// it separates from the hazed horizon behind it.
pub const LIMEWASH: Color = Color::srgba(0.890, 0.832, 0.706, 0.99);

/// The lit face of a plate: the selected-unit plate, which should feel like a
/// fresher surface laid on top.
pub const LIMEWASH_LIT: Color = Color::srgba(0.965, 0.923, 0.824, 0.99);

/// A recessed well inside a plate — the strip a row of buttons sits in.
pub const LIMEWASH_WELL: Color = Color::srgba(0.800, 0.720, 0.571, 0.98);
/// Denser header strip and lighter detail leaf used by ledger windows.
pub const LIMEWASH_HEADER: Color = Color::srgba(0.839, 0.763, 0.609, 1.0);
pub const LIMEWASH_DETAIL: Color = Color::srgba(0.965, 0.923, 0.824, 0.98);

/// Carved wood, brass binding and parchment shared with the war UI.
pub const SIGN_WOOD: Color = Color::srgba(0.14, 0.09, 0.06, 0.98);
pub const WOOD_LIT: Color = Color::srgb(0.25, 0.16, 0.095);
pub const PARCHMENT: Color = Color::srgb(0.97, 0.94, 0.86);
pub const BRASS: Color = Color::srgb(0.70, 0.53, 0.29);
pub const BRASS_DARK: Color = Color::srgb(0.37, 0.26, 0.13);
pub const CRIMSON: Color = Color::srgb(0.62, 0.16, 0.12);

/// Button rest fill. A shade lighter than the plate, as if raised from it.
pub const BUTTON_NORMAL: Color = Color::srgba(0.930, 0.864, 0.725, 1.0);
/// Hover LIGHTENS: a raised surface catching more light, not ink soaking in.
pub const BUTTON_HOVERED: Color = Color::srgba(0.994, 0.948, 0.833, 1.0);
/// Pressed pushes into shade.
pub const BUTTON_PRESSED: Color = Color::srgba(0.777, 0.677, 0.505, 1.0);
/// Disabled controls remain visible but clearly unavailable.
pub const BUTTON_DISABLED: Color = Color::srgba(0.800, 0.720, 0.571, 1.0);

/// Flat list-row hover and selection fills. Rows are controls, but should not
/// visually inflate into raised buttons inside dense directories.
pub const ROW_HOVERED: Color = Color::srgba(0.777, 0.655, 0.453, 0.35);
pub const ROW_SELECTED: Color = Color::srgba(0.560, 0.325, 0.129, 0.20);

/// The carved outer edge on every plate. Dark and near-opaque on purpose: this
/// is half of how a plate keeps a silhouette against bright terrain.
pub const PLATE_RULE: Color = Color::srgba(0.37, 0.26, 0.13, 0.85);
/// Hairline dividers INSIDE a plate, replacing nested bordered boxes.
pub const PLATE_RULE_SOFT: Color = Color::srgba(0.37, 0.26, 0.13, 0.27);
// ---------------------------------------------------------------------------
// Ink
// ---------------------------------------------------------------------------

/// Iron-gall ink. All primary text.
pub const INK: Color = Color::srgb(0.18, 0.125, 0.075);
/// Small-caps labels, hints, keybind lines. Use at 12 px or larger.
pub const INK_MUTED: Color = Color::srgb(0.38, 0.29, 0.19);
/// Text ON a saturated or slate fill, where ink would disappear.
pub const INK_INVERSE: Color = Color::srgb(0.965, 0.957, 0.933);
/// Secondary copy on dark front-of-house surfaces.
pub const INK_INVERSE_MUTED: Color = Color::srgb(0.72, 0.69, 0.64);
/// Warm heading ink used only on dark front-of-house surfaces.
pub const INK_INVERSE_HEADING: Color = Color::srgb(0.93, 0.72, 0.48);

// ---------------------------------------------------------------------------
// The one accent, and the dev inversion
// ---------------------------------------------------------------------------

/// Burnt sienna. **Selection only.** If this appears anywhere else in play-mode
/// chrome the player can no longer tell at a glance what they are commanding.
pub const EMBER: Color = Color::srgb(0.560, 0.325, 0.129);
/// A defining rule around an ember fill.
pub const EMBER_RULE: Color = Color::srgb(0.400, 0.208, 0.078);
/// Neutral dark fill for GOD and debug affordances. Deliberately not the accent:
/// dev chrome must never look like game state.
pub const SLATE: Color = Color::srgba(0.255, 0.243, 0.227, 0.94);
pub const SLATE_HOVERED: Color = Color::srgba(0.335, 318.0 / 1000.0, 0.294, 0.98);
pub const SLATE_PRESSED: Color = Color::srgba(0.196, 0.184, 0.169, 0.98);

/// Danger. Madder red, reserved for destructive actions.
pub const ACCENT_RED: Color = Color::srgb(0.545, 0.196, 0.169);
/// Moss-green status mark. Dark enough to retain contrast on limewash.
pub const STATUS_GOOD: Color = Color::srgb(0.243, 0.435, 0.196);

/// Warm scrim behind a modal. Stays DARK behind now-light panels; that
/// inversion is what makes a modal read as modal.
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
