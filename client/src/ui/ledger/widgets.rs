//! Composable book surfaces and decoration; owners supply layout and actions.

use super::artwork::{LedgerIcon, LedgerIllustration, PortraitFrame, Surface};
use super::illustrations::{IllustrationFinish, IllustrationMaterial};
use crate::ui::{
    styles::{INK, PARCHMENT},
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
    paper_surface(Surface::Paper)
}

pub(crate) fn directory_paper() -> impl Bundle {
    paper_surface(Surface::DirectoryPaper)
}

/// A bound-page lip and a five-pixel shadow beside the directory. The absolute
/// decoration sits beyond the scroll area, leaving its geometry and input intact.
pub(crate) fn directory_gutter() -> impl Bundle {
    let strip = |left, width, color| {
        (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(width),
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        )
    };
    (
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(-5.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            width: Val::Px(6.0),
            ..default()
        },
        ZIndex(3),
        Pickable::IGNORE,
        children![
            strip(0.0, 1.0, Color::srgba(1.0, 0.96, 0.83, 0.65)),
            strip(1.0, 1.0, Color::srgba(0.29, 0.18, 0.08, 0.30)),
            strip(2.0, 2.0, Color::srgba(0.29, 0.18, 0.08, 0.13)),
            strip(4.0, 2.0, Color::srgba(0.29, 0.18, 0.08, 0.045)),
        ],
    )
}

fn paper_surface(surface: Surface) -> impl Bundle {
    (
        surface,
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

pub(crate) fn reading_strong(size: f32) -> TextFont {
    typography::reading_strong(size)
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

pub(crate) fn body_strong(text: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        reading_strong(size),
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
        BackgroundColor(Color::srgba(0.43, 0.30, 0.15, 0.42)),
        Pickable::IGNORE,
    )
}

/// A small printer's diamond on major rules. Its absolute ornament keeps the
/// same layout footprint as a plain rule, including compact nested pages.
pub(crate) fn ornament_rule() -> impl Bundle {
    decorated_rule(
        Color::srgba(0.43, 0.30, 0.15, 0.42),
        PARCHMENT,
        crate::ui::styles::BRASS_DARK,
    )
}

pub(crate) fn binding_ornament_rule() -> impl Bundle {
    decorated_rule(
        crate::ui::styles::BRASS.with_alpha(0.60),
        crate::ui::styles::SIGN_WOOD,
        crate::ui::styles::BRASS,
    )
}

fn decorated_rule(line: Color, fill: Color, edge: Color) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(line),
        Pickable::IGNORE,
        children![(
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Px(-4.0),
                width: Val::Px(9.0),
                height: Val::Px(9.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            BackgroundColor(fill),
            BorderColor::all(edge),
            Pickable::IGNORE,
        )],
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
    illustration_with_finish(
        kind,
        size,
        if kind == LedgerIllustration::Company {
            IllustrationFinish::Plain
        } else {
            IllustrationFinish::Vignette
        },
    )
}

fn illustration_with_finish(
    kind: LedgerIllustration,
    size: Vec2,
    finish: IllustrationFinish,
) -> impl Bundle {
    (
        kind,
        finish,
        Node {
            width: Val::Px(size.x),
            height: Val::Px(size.y),
            flex_shrink: 0.0,
            border_radius: BorderRadius::all(if finish == IllustrationFinish::Round {
                Val::Percent(50.0)
            } else {
                Val::Px(3.0)
            }),
            overflow: Overflow::clip(),
            ..default()
        },
        MaterialNode::<IllustrationMaterial>::default(),
        Pickable::IGNORE,
    )
}

pub(crate) fn illustration_medallion(kind: LedgerIllustration, size: f32) -> impl Bundle {
    (
        portrait_frame(size),
        children![(
            illustration_with_finish(
                kind,
                Vec2::splat((size - 12.0).max(8.0)),
                IllustrationFinish::Round
            ),
            BackgroundColor(Color::srgb(0.18, 0.22, 0.18)),
        )],
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
                left: if left { Val::Px(-7.0) } else { Val::Auto },
                right: if left { Val::Auto } else { Val::Px(-7.0) },
                top: if top { Val::Px(-7.0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { Val::Px(-7.0) },
                width: Val::Px(36.0),
                height: Val::Px(36.0),
                ..default()
            },
            Surface::Corner,
            ImageNode {
                // Exclude the generated asset's transparent canvas margin so
                // the metal actually meets the outside binding corner.
                rect: Some(Rect::new(4.0, 4.0, 122.0, 122.0)),
                ..image_node(NodeImageMode::Stretch)
            },
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
