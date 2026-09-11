//! Composable book surfaces and decoration; owners supply layout and actions.

use super::artwork::{LedgerIcon, LedgerIllustration, PortraitFrame, Surface};
use crate::ui::{
    styles::{INK, PARCHMENT, PLATE_RULE_SOFT},
    typography,
};
use bevy::{
    prelude::*,
    sprite::{BorderRect, TextureSlicer},
    ui::VisualBox,
};
use shared::components::PersonId;

fn image_node(mode: NodeImageMode) -> ImageNode {
    ImageNode {
        image_mode: mode,
        visual_box: VisualBox::BorderBox,
        ..default()
    }
}

pub(crate) fn paper() -> impl Bundle {
    (
        Surface::Paper,
        ImageNode {
            visual_box: VisualBox::PaddingBox,
            ..image_node(NodeImageMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            })
        },
    )
}

pub(crate) fn wood() -> impl Bundle {
    (
        Surface::Wood,
        ImageNode {
            visual_box: VisualBox::PaddingBox,
            ..image_node(NodeImageMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            })
        },
    )
}

pub(crate) fn reading(size: f32) -> TextFont {
    typography::reading(size)
}

pub(crate) fn heading(text: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        typography::heading(size),
        TextColor(INK),
        Pickable::IGNORE,
    )
}

pub(crate) fn body(text: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        reading(size),
        TextColor(INK),
        Pickable::IGNORE,
    )
}

pub(crate) fn rule() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(PLATE_RULE_SOFT),
        Pickable::IGNORE,
    )
}

pub(crate) fn portrait_frame(size: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            padding: UiRect::all(Val::Px(6.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        PortraitFrame,
        Pickable::IGNORE,
    )
}

pub(crate) fn person_portrait(id: PersonId, size: f32) -> impl Bundle {
    (
        portrait_frame(size),
        children![crate::ui::portraits::person(id, (size - 12.0).max(8.0))],
    )
}

pub(crate) fn illustration(kind: LedgerIllustration, size: Vec2) -> impl Bundle {
    (
        kind,
        Node {
            width: Val::Px(size.x),
            height: Val::Px(size.y),
            flex_shrink: 0.0,
            border_radius: BorderRadius::all(Val::Px(3.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        image_node(NodeImageMode::Stretch),
        Pickable::IGNORE,
    )
}

pub(crate) fn pennant(roman: &str, size: Vec2) -> impl Bundle {
    (
        Node {
            width: Val::Px(size.x),
            height: Val::Px(size.y),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::bottom(Val::Px(size.y * 0.10)),
            ..default()
        },
        Surface::Pennant,
        image_node(NodeImageMode::Stretch),
        Pickable::IGNORE,
        children![(
            Text::new(roman),
            typography::heading((size.x * 0.34).min(28.0)),
            TextColor(PARCHMENT),
            Pickable::IGNORE
        )],
    )
}

pub(crate) fn corners(parent: &mut ChildSpawnerCommands<'_>) {
    for (left, top) in [(true, true), (false, true), (true, false), (false, false)] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: if left { Val::Px(-1.0) } else { Val::Auto },
                right: if left { Val::Auto } else { Val::Px(-1.0) },
                top: if top { Val::Px(-1.0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { Val::Px(-1.0) },
                width: Val::Px(53.0),
                height: Val::Px(53.0),
                ..default()
            },
            Surface::Corner,
            image_node(NodeImageMode::Stretch),
            UiTransform::from_scale(Vec2::new(
                if left { 1.0 } else { -1.0 },
                if top { 1.0 } else { -1.0 },
            )),
            ZIndex(20),
            Pickable::IGNORE,
        ));
    }
}

pub(super) fn sliced_button() -> ImageNode {
    image_node(NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect::all(8.0),
        ..default()
    }))
}

pub(crate) fn icon(kind: LedgerIcon, size: f32) -> impl Bundle {
    (
        kind,
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        },
        image_node(NodeImageMode::Stretch),
        Pickable::IGNORE,
    )
}
