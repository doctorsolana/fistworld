//! Shared native text and hairline rules for company pages in the ledger.
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::ledger;
use crate::ui::styles::{INK, INK_MUTED, PLATE_RULE_SOFT};
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
                min_height: Val::Px(31.0),
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(11.0), Val::Px(5.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((UiButtonLabel, ledger::body(label, 12.0)));
}

pub(super) fn detail_stat(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn((
            Node {
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(105.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border: UiRect::right(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|stat| {
            stat.spawn((
                Text::new(label),
                ledger::reading(12.0),
                TextColor(INK_MUTED),
            ));
            stat.spawn(ledger::body(value, 17.0));
        });
}

pub(super) fn spawn_section_title(parent: &mut ChildSpawnerCommands<'_>, title: &str, note: &str) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                flex_shrink: 0.0,
                margin: UiRect::top(Val::Px(5.0)),
                padding: UiRect::bottom(Val::Px(6.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|title_row| {
            title_row.spawn(ledger::heading(title, 19.0));
            if !note.is_empty() {
                title_row.spawn((Text::new(note), ledger::reading(12.0), TextColor(INK_MUTED)));
            }
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
                ledger::reading(13.0),
                TextColor(INK_MUTED),
                Node {
                    width: Val::Px(112.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value),
                ledger::reading(14.0),
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
    parent.spawn((Text::new(text), ledger::reading(13.0), TextColor(INK_MUTED)));
}

pub(super) fn spawn_empty(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text),
        ledger::reading(16.0),
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
        if value < 0 { "−" } else { "+" },
        format_money(value.unsigned_abs())
    )
}
