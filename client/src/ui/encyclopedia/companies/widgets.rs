//! Shared native text and hairline rules for company pages in the ledger.
use super::binding::{bound_text, CompanyBound, CompanyView};
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::ledger;
use crate::ui::styles::{INK, INK_MUTED, PLATE_RULE_SOFT};
use bevy::prelude::*;
use shared::economy::format_money;

/// `marker` is the button's payload component, optionally paired with the
/// [`CompanyBound`] key that rebinds that payload each snapshot.
pub(super) fn detail_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Bundle,
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
        .with_child((UiButtonLabel, ledger::body_strong(label, 12.0)));
}

/// A stat card whose value binds in place under `key`.
pub(super) fn detail_stat(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    label: &str,
    key: CompanyBound,
) {
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
                ledger::reading_strong(12.0),
                TextColor(INK_MUTED),
            ));
            bound_text(stat, view, key, ledger::reading_strong(17.0), INK).insert(Pickable::IGNORE);
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
            title_row
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|heading| {
                    use crate::ui::hud::chrome::{icon, HudIcon};
                    let symbol = match title {
                        "Your Position" => HudIcon::Person,
                        "Today's Ledger" => HudIcon::Book,
                        "Operating Sites" => HudIcon::Pin,
                        "Trade Routes" => HudIcon::Compass,
                        _ => HudIcon::Scales,
                    };
                    heading.spawn(icon(symbol, 24.0)).insert(ImageNode {
                        color: Color::srgb(0.42, 0.25, 0.10),
                        ..default()
                    });
                    heading.spawn(ledger::heading(title, 20.0));
                });
            if !note.is_empty() {
                title_row.spawn((Text::new(note), ledger::reading(12.0), TextColor(INK_MUTED)));
            }
        });
}

/// The label column of a [`key_value`] row: fixed copy or a bound key.
#[derive(Clone, Copy)]
pub(super) enum Label<'a> {
    Fixed(&'a str),
    Bound(CompanyBound),
}

/// A label / value row. `row_key` may bind the row's own `Display` so an
/// optional row (INTERNAL FLOW MEMO) is always spawned and merely hidden.
pub(super) fn key_value(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    row_key: Option<CompanyBound>,
    label: Label<'_>,
    value: CompanyBound,
) {
    let display = row_key
        .and_then(|key| view.value(key))
        .and_then(|value| value.display)
        .unwrap_or_default();
    let mut row = parent.spawn((
        Node {
            display,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(14.0),
            padding: UiRect::vertical(Val::Px(6.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::from(PLATE_RULE_SOFT),
    ));
    if let Some(key) = row_key {
        row.insert(key);
    }
    row.with_children(|row| {
        let label_node = Node {
            width: Val::Px(112.0),
            flex_shrink: 0.0,
            ..default()
        };
        match label {
            Label::Fixed(text) => {
                row.spawn((
                    Text::new(text),
                    ledger::reading(14.0),
                    TextColor(INK),
                    label_node,
                ));
            }
            Label::Bound(key) => {
                bound_text(row, view, key, ledger::reading(14.0), INK).insert(label_node);
            }
        }
        bound_text(row, view, value, ledger::reading_strong(14.0), INK).insert((
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

/// A note whose text and visibility bind under `key` (empty text hides it).
pub(super) fn bound_note(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    key: CompanyBound,
) {
    bound_text(parent, view, key, ledger::reading(13.0), INK_MUTED);
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
