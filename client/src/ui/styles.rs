//! Shared UI styles - FIST-FORCE desert/rust aesthetic

use bevy::prelude::*;

/// Dark background for menus
pub const MENU_BACKGROUND: Color = Color::srgb(0.06, 0.05, 0.04);

/// Primary button colors - dark with subtle warmth
pub const BUTTON_NORMAL: Color = Color::srgb(0.12, 0.10, 0.08);
pub const BUTTON_HOVERED: Color = Color::srgb(0.22, 0.16, 0.10);
pub const BUTTON_PRESSED: Color = Color::srgb(0.45, 0.28, 0.12);

/// Button border - subtle rust accent
pub const BUTTON_BORDER: Color = Color::srgb(0.35, 0.25, 0.15);

/// Accent color - warm copper/orange (matches logo)
pub const ACCENT_COLOR: Color = Color::srgb(0.77, 0.47, 0.20); // #C47832

/// Secondary accent - rust red for highlights
#[allow(dead_code)]
pub const ACCENT_RED: Color = Color::srgb(0.75, 0.30, 0.15);

/// Text colors
pub const TEXT_COLOR: Color = Color::srgb(0.92, 0.88, 0.82); // Warm off-white
pub const TEXT_MUTED: Color = Color::srgb(0.50, 0.45, 0.40); // Muted warm gray

/// Standard button style
pub(crate) fn button_style() -> Node {
    Node {
        width: Val::Px(280.0),
        height: Val::Px(55.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        margin: UiRect::all(Val::Px(8.0)),
        border: UiRect::all(Val::Px(2.0)),
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
