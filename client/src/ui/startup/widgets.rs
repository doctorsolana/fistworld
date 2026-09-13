//! Native, responsive controls over reusable text-free brass and parchment.

use super::{
    motion::{LoadingClock, LoadingRing},
    LoadingDiamond, StartupArtwork,
};
use crate::ui::{
    foundation::{button_chrome, surface_block, UiButtonLabel, UiButtonVariant},
    ledger, typography,
};
use bevy::{prelude::*, ui::RelativeCursorPosition};

pub(crate) const INK: Color = Color::srgb(0.19, 0.11, 0.045);
pub(crate) const GOLD: Color = Color::srgb(0.82, 0.63, 0.32);
pub(crate) const IVORY: Color = Color::srgb(1.0, 0.94, 0.79);

pub(crate) fn screen() -> impl Bundle {
    (
        Name::new("startup-root"),
        super::input::StartupScreen,
        bevy::input_focus::tab_navigation::TabGroup::modal(),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(90),
        ledger::LedgerButtonScope,
        Pickable::IGNORE,
    )
}

pub(crate) fn wordmark(art: &StartupArtwork, width: Val) -> impl Bundle {
    (
        Name::new("startup-wordmark"),
        Node {
            width,
            aspect_ratio: Some(2.4),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode::new(art.wordmark.clone()),
        Pickable::IGNORE,
    )
}

#[derive(Component)]
pub(crate) struct CompactWordmark;

pub(crate) fn small_wordmark(parent: &mut ChildSpawnerCommands<'_>, art: &StartupArtwork) {
    parent.spawn((
        CompactWordmark,
        Name::new("startup-wordmark"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Vh(7.5),
            width: Val::Px(270.0),
            height: Val::Px(81.0),
            ..default()
        },
        ImageNode::new(art.compact_wordmark.clone()),
        Pickable::IGNORE,
    ));
}

pub(crate) fn frame(parent: &mut ChildSpawnerCommands<'_>, art: &StartupArtwork) {
    parent.spawn((
        Name::new("startup-brass-frame"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(-12.0),
            right: Val::Px(-12.0),
            top: Val::Px(-12.0),
            bottom: Val::Px(-12.0),
            ..default()
        },
        art.frame(),
        ZIndex(8),
        Pickable::IGNORE,
    ));
}

pub(crate) fn panel() -> impl Bundle {
    (
        Name::new("startup-panel"),
        crate::ui::motion::UiReveal::panel(),
        ledger::LedgerButtonScope,
        surface_block(),
        crate::ui::styles::plate_shadow(),
    )
}

pub(crate) fn rule(parent: &mut ChildSpawnerCommands<'_>, width: f32, dark: bool) {
    let color = if dark {
        Color::srgb(0.53, 0.36, 0.17)
    } else {
        GOLD
    };
    parent
        .spawn((
            Node {
                width: Val::Px(width),
                height: Val::Px(12.0),
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|line| {
            line.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(color),
                Pickable::IGNORE,
            ));
            line.spawn((
                Node {
                    width: Val::Px(6.0),
                    height: Val::Px(6.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(color),
                UiTransform::from_rotation(Rot2::degrees(45.0)),
                Pickable::IGNORE,
            ));
            line.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(color),
                Pickable::IGNORE,
            ));
        });
}

pub(crate) fn action_button(
    parent: &mut ChildSpawnerCommands<'_>,
    art: &StartupArtwork,
    label: &str,
    action: impl Bundle,
    width: Val,
    height: f32,
    primary: bool,
) -> Entity {
    let variant = if primary {
        UiButtonVariant::Primary
    } else {
        UiButtonVariant::Inverse
    };
    parent
        .spawn((
            Button,
            action,
            Name::new(if primary {
                "startup-primary"
            } else {
                "startup-secondary"
            }),
            RelativeCursorPosition::default(),
            Node {
                width,
                height: Val::Px(height),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(24.0)),
                ..default()
            },
            button_chrome(variant),
            ledger::LedgerButtonFace(if primary {
                art.gold()
            } else {
                art.dark_button()
            }),
        ))
        .with_children(|button| {
            if primary {
                for left in [true, false] {
                    button.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: if left { Val::Px(23.0) } else { Val::Auto },
                            right: if left { Val::Auto } else { Val::Px(23.0) },
                            width: Val::Px(7.0),
                            height: Val::Px(7.0),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(IVORY),
                        UiTransform::from_rotation(Rot2::degrees(45.0)),
                        Pickable::IGNORE,
                    ));
                }
            }
            button.spawn((
                UiButtonLabel,
                Text::new(label),
                typography::heading(if primary {
                    height * 0.43
                } else {
                    height * 0.38
                }),
                TextColor(IVORY),
                TextLayout::no_wrap(),
                TextShadow {
                    offset: Vec2::new(0.0, 1.5),
                    color: INK,
                },
                Pickable::IGNORE,
            ));
        })
        .id()
}

pub(crate) fn back_button(parent: &mut ChildSpawnerCommands<'_>, label: &str, action: impl Bundle) {
    parent
        .spawn((
            Name::new("startup-secondary"),
            Button,
            action,
            Node {
                height: Val::Px(36.0),
                min_width: Val::Px(118.0),
                column_gap: Val::Px(12.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            button_chrome(UiButtonVariant::Ghost),
        ))
        .with_children(|button| {
            // The back arrow is native geometry, not a platform-dependent font glyph.
            button
                .spawn((
                    Node {
                        width: Val::Px(10.0),
                        height: Val::Px(16.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|arrow| {
                    for (y, angle) in [(4.0, -45.0), (11.0, 45.0)] {
                        arrow.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                top: Val::Px(y),
                                width: Val::Px(10.0),
                                height: Val::Px(2.0),
                                ..default()
                            },
                            BackgroundColor(INK),
                            UiTransform::from_rotation(Rot2::degrees(angle)),
                            Pickable::IGNORE,
                        ));
                    }
                });
            button.spawn((
                UiButtonLabel,
                Text::new(label),
                typography::reading(20.0),
                TextColor(INK),
                Pickable::IGNORE,
            ));
        });
}

#[derive(Component)]
pub(super) struct StartupField;

pub(crate) fn field(art: &StartupArtwork, height: f32) -> impl Bundle {
    let paper = art.input();
    (
        Button,
        StartupField,
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(height),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(Val::Px(12.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        button_chrome(UiButtonVariant::Secondary),
        ledger::LedgerButtonFace(paper),
    )
}

/// One reusable neutral busy panel. The caller owns its visibility and status text.
#[derive(Component)]
pub(crate) struct LoadingStatus;

pub(crate) fn loading_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    art: &StartupArtwork,
    title: &str,
    extra: impl Bundle,
) -> Entity {
    parent
        .spawn((
            panel(),
            LoadingClock::default(),
            extra,
            Node {
                width: Val::Px(460.0),
                max_width: Val::Vw(80.0),
                height: Val::Px(340.0),
                padding: UiRect::all(Val::Px(30.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(24.0),
                ..default()
            },
            art.paper(),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Name::new("startup-loading-compass"),
                    Node {
                        width: Val::Px(160.0),
                        height: Val::Px(160.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|compass| {
                    compass.spawn((
                        LoadingRing,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        ImageNode::new(art.compass_ring.clone()),
                        Pickable::IGNORE,
                    ));
                    compass.spawn((
                        Node {
                            width: Val::Percent(68.0),
                            height: Val::Percent(68.0),
                            ..default()
                        },
                        ImageNode::new(art.compass_star.clone()),
                        Pickable::IGNORE,
                    ));
                });
            panel.spawn((
                LoadingStatus,
                Name::new("startup-status"),
                Text::new(title),
                typography::reading_strong(32.0),
                TextColor(INK),
                TextLayout::justify(Justify::Center),
                Pickable::IGNORE,
            ));
            panel
                .spawn((
                    Name::new("startup-diamonds"),
                    Node {
                        height: Val::Px(20.0),
                        column_gap: Val::Px(15.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|line| {
                    line.spawn((
                        Node {
                            width: Val::Px(65.0),
                            height: Val::Px(1.0),
                            ..default()
                        },
                        BackgroundColor(GOLD),
                        Pickable::IGNORE,
                    ));
                    for index in 0..3 {
                        line.spawn((
                            Name::new(format!("startup-diamond-{index}")),
                            LoadingDiamond(index),
                            Node {
                                width: Val::Px(12.0),
                                height: Val::Px(12.0),
                                border: UiRect::all(Val::Px(1.5)),
                                ..default()
                            },
                            BackgroundColor(GOLD),
                            BorderColor::all(INK),
                            UiTransform::from_rotation(Rot2::degrees(45.0)),
                            Pickable::IGNORE,
                        ));
                    }
                    line.spawn((
                        Node {
                            width: Val::Px(65.0),
                            height: Val::Px(1.0),
                            ..default()
                        },
                        BackgroundColor(GOLD),
                        Pickable::IGNORE,
                    ));
                });
            frame(panel, art);
        })
        .id()
}
