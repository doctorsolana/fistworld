//! Company-page text, stat and button primitives shared by its views.

use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::economy::format_money;

pub(super) fn detail_button<M: Component>(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: M,
    label: &str,
) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                height: Val::Px(28.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            Text::new(label),
            UiButtonLabel,
            crate::ui::typography::text(11.5),
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

pub(super) fn detail_stat(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn((
            Node {
                width: Val::Percent(31.8),
                min_width: Val::Px(150.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::all(Val::Px(9.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.84, 0.81, 0.76, 0.36)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(label),
                crate::ui::typography::text(11.5),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                Text::new(value),
                crate::ui::typography::text(15.0),
                TextColor(INK),
            ));
        });
}

pub(super) fn spawn_section_title(parent: &mut ChildSpawnerCommands<'_>, title: &str, note: &str) {
    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::End,
            margin: UiRect::top(Val::Px(5.0)),
            padding: UiRect::bottom(Val::Px(5.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                crate::ui::typography::text(13.5),
                TextColor(EMBER),
            ));
            row.spawn((
                Text::new(note),
                crate::ui::typography::text(11.0),
                TextColor(INK_MUTED),
            ));
        });
}

pub(super) fn key_value(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(14.0),
                padding: UiRect::vertical(Val::Px(6.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                crate::ui::typography::text(11.5),
                TextColor(INK_MUTED),
                Node {
                    width: Val::Px(112.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value),
                crate::ui::typography::text(13.0),
                TextColor(INK),
                TextLayout::justify(Justify::Right),
                Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

pub(super) fn spawn_note(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text),
        crate::ui::typography::text(13.0),
        TextColor(INK_MUTED),
    ));
}

pub(super) fn spawn_empty(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text),
        crate::ui::typography::text(14.0),
        TextColor(INK_MUTED),
        TextLayout::justify(Justify::Center),
        Node {
            align_self: AlignSelf::Center,
            max_width: Val::Px(360.0),
            margin: UiRect::all(Val::Px(28.0)),
            ..default()
        },
    ));
}

pub(super) fn signed_money(value: i64) -> String {
    format!(
        "{}{} coin",
        if value < 0 { "-" } else { "+" },
        format_money(value.unsigned_abs()),
    )
}
