//! Encyclopedia state → view sync.
//!
//! Every write is diff-gated: these systems run every frame while the window
//! is open, and a UI that rewrites its own text each frame is a steady stream
//! of pointless change detection.

use bevy::prelude::*;
use lightyear::prelude::MessageReceiver;

use shared::protocol::CharacterRoster;

use super::*;
use crate::input::InputState;
use crate::ui::hud::GodCapability;
use crate::ui::styles::{ACCENT_COLOR, TEXT_COLOR, TEXT_MUTED};

/// The camera must not pan and world clicks must not fire underneath.
pub(super) fn sync_input_state(
    open: Res<EncyclopediaOpen>,
    mut input_state: ResMut<InputState>,
) {
    if input_state.encyclopedia_open != open.0 {
        input_state.encyclopedia_open = open.0;
    }
}

/// Fold the server's character roster into the knowledge registry.
///
/// The roster is the full picture the SERVER has. It is merged rather than
/// assigned, because the registry is knowledge the player accumulates: a person
/// you have met stays in your encyclopedia after they walk out of your interest
/// range, which is the whole point of it being an encyclopedia rather than a
/// list of who is nearby.
pub(super) fn receive_character_roster(
    mut receivers: Query<&mut MessageReceiver<CharacterRoster>, With<crate::GameClient>>,
    mut people: ResMut<KnownPeople>,
) {
    for mut receiver in receivers.iter_mut() {
        for roster in receiver.receive() {
            for entry in roster.entries {
                let kind = match entry.kind {
                    shared::components::CharacterKind::Hero => PersonKind::Hero,
                    shared::components::CharacterKind::Villager => PersonKind::Villager,
                };
                if let Some(existing) = people
                    .records
                    .iter_mut()
                    .find(|record| record.name == entry.name)
                {
                    existing.kind = kind;
                    existing.online = entry.online;
                    existing.is_self = entry.is_self;
                    // Knowing OF someone from the roster does not make them
                    // known -- god mode reveals unknown records still marked
                    // unknown, so the fog stays visible rather than being
                    // silently switched off.
                    existing.known |= entry.is_self;
                } else {
                    people.records.push(PersonRecord {
                        known: entry.is_self,
                        is_self: entry.is_self,
                        name: entry.name,
                        kind,
                        affiliation: Affiliation::Neutral,
                        level: 0,
                        prestige: 0,
                        online: entry.online,
                    });
                }
            }
        }
    }
}

/// Anyone you can actually SEE becomes known, permanently.
///
/// Replication only delivers entities inside your interest range, so this is
/// literally "people you have laid eyes on". Once known they stay in the
/// registry after they leave range -- that is what makes it knowledge rather
/// than a proximity list.
pub(super) fn learn_visible_characters(
    seen: Query<
        (&shared::components::CharacterName, &shared::components::CharacterKind),
        Added<shared::components::CharacterName>,
    >,
    mut people: ResMut<KnownPeople>,
) {
    for (name, kind) in seen.iter() {
        let kind = match kind {
            shared::components::CharacterKind::Hero => PersonKind::Hero,
            shared::components::CharacterKind::Villager => PersonKind::Villager,
        };
        if let Some(existing) = people.records.iter_mut().find(|r| r.name == name.0) {
            existing.known = true;
            existing.kind = kind;
        } else {
            people.records.push(PersonRecord {
                name: name.0.clone(),
                kind,
                affiliation: Affiliation::Neutral,
                level: 0,
                prestige: 0,
                online: false,
                known: true,
                is_self: false,
            });
        }
    }
}

/// Rebuild rows when the registry, filter or capability changes.
///
/// Rebuild-on-change rather than per-frame: the list is small, and diffing
/// rows against records would cost more code than it saves.
pub(super) fn rebuild_people_list(
    mut commands: Commands,
    people: Res<KnownPeople>,
    filter: Res<PeopleFilter>,
    god: Res<GodCapability>,
    mut selected: ResMut<SelectedPerson>,
    content: Query<Entity, With<PeopleListContent>>,
    existing_rows: Query<Entity, With<PersonRow>>,
    mut count_text: Query<&mut Text, With<PeopleCountText>>,
    mut last: Local<Option<(usize, PeopleFilter, bool)>>,
) {
    let signature = (people.records.len(), *filter, god.0);
    let dirty = people.is_changed() || filter.is_changed() || god.is_changed();
    if !dirty && *last == Some(signature) {
        return;
    }
    *last = Some(signature);

    let Ok(content_entity) = content.single() else {
        return;
    };
    for row in existing_rows.iter() {
        commands.entity(row).despawn();
    }

    let visible = people.visible(*filter, god.0);

    // Drop a selection the filter just hid, so the detail pane never describes
    // someone who is no longer listed.
    if let Some(name) = selected.0.clone() {
        if !visible.iter().any(|record| record.name == name) {
            selected.0 = None;
        }
    }

    for mut text in count_text.iter_mut() {
        let label = match visible.len() {
            1 => "1 person".to_string(),
            n => format!("{n} people"),
        };
        if text.0 != label {
            text.0 = label;
        }
    }

    commands.entity(content_entity).with_children(|list| {
        if visible.is_empty() {
            list.spawn((
                // MUST carry the row marker: the rebuild despawns
                // `Query<Entity, With<PersonRow>>`, so an unmarked empty-state
                // node is never cleaned up and sits above a populated list.
                PersonRow(String::new()),
                Text::new("No one here yet"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    margin: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
            return;
        }
        for record in &visible {
            spawn_person_row(list, record);
        }
    });
}

fn spawn_person_row(list: &mut ChildSpawnerCommands<'_>, record: &PersonRecord) {
    let name_color = if record.known { TEXT_COLOR } else { TEXT_MUTED };
    list.spawn((
        Button,
        PersonRow(record.name.clone()),
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(9.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(ROW_NORMAL),
    ))
    .with_children(|row| {
        // Status pip: filled + green online, hollow-dim otherwise.
        row.spawn((
            Node {
                width: Val::Px(7.0),
                height: Val::Px(7.0),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if record.online {
                STATUS_ONLINE
            } else if record.known {
                TEXT_MUTED
            } else {
                DIVIDER
            }),
        ));
        row.spawn((
            Text::new(if record.is_self {
                format!("{} (you)", record.name)
            } else {
                record.name.clone()
            }),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(name_color),
            Node {
                flex_grow: 1.0,
                ..default()
            },
        ));
        row.spawn((
            Text::new(if record.known {
                record.affiliation.badge().to_string()
            } else {
                "UNKNOWN".to_string()
            }),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(if record.known { TEXT_MUTED } else { DIVIDER }),
        ));
    });
}

pub(super) fn sync_tab_visuals(
    tab: Res<EncyclopediaTab>,
    mut buttons: Query<(
        &TabButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut bodies: Query<(&TabBody, &mut Node)>,
    mut labels: Query<&mut TextColor>,
) {
    for (TabButton(button_tab), interaction, mut bg, mut border, children) in buttons.iter_mut() {
        let active = *button_tab == *tab;
        let background = if active {
            ROW_SELECTED
        } else if *interaction == Interaction::Hovered {
            ROW_HOVERED
        } else {
            Color::NONE
        };
        if bg.0 != background {
            bg.0 = background;
        }
        let border_color = BorderColor::from(if active { ACCENT_COLOR } else { Color::NONE });
        if *border != border_color {
            *border = border_color;
        }
        let text_color = if active { TEXT_COLOR } else { TEXT_MUTED };
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                if color.0 != text_color {
                    color.0 = text_color;
                }
            }
        }
    }

    for (body_tab, mut node) in bodies.iter_mut() {
        let display = if body_tab.0 == *tab {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

pub(super) fn sync_filter_visuals(
    filter: Res<PeopleFilter>,
    god: Res<GodCapability>,
    mut buttons: Query<(&FilterButton, &Interaction, &mut BackgroundColor, &mut Node)>,
) {
    for (FilterButton(button_filter), interaction, mut bg, mut node) in buttons.iter_mut() {
        // UNKNOWN is the god-mode view of the fog; hide it without capability.
        let display = if *button_filter == PeopleFilter::Unknown && !god.0 {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }

        let active = *button_filter == *filter;
        let background = if active {
            ROW_SELECTED
        } else if *interaction == Interaction::Hovered {
            ROW_HOVERED
        } else {
            Color::NONE
        };
        if bg.0 != background {
            bg.0 = background;
        }
    }
}

pub(super) fn style_person_rows(
    selected: Res<SelectedPerson>,
    mut rows: Query<(&PersonRow, &Interaction, &mut BackgroundColor)>,
) {
    for (PersonRow(name), interaction, mut bg) in rows.iter_mut() {
        let is_selected = selected.0.as_deref() == Some(name.as_str());
        let background = if is_selected {
            ROW_SELECTED
        } else {
            match *interaction {
                Interaction::Hovered | Interaction::Pressed => ROW_HOVERED,
                Interaction::None => ROW_NORMAL,
            }
        };
        if bg.0 != background {
            bg.0 = background;
        }
    }
}

pub(super) fn sync_detail_panel(
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    mut card: Query<&mut Node, (With<DetailCard>, Without<DetailEmptyState>)>,
    mut empty: Query<&mut Node, (With<DetailEmptyState>, Without<DetailCard>)>,
    mut name_text: Query<&mut Text, (With<DetailName>, Without<DetailSubtitle>)>,
    mut subtitle: Query<&mut Text, (With<DetailSubtitle>, Without<DetailName>)>,
    mut stats: Query<(&DetailStat, &mut Text), (Without<DetailName>, Without<DetailSubtitle>)>,
) {
    let record = selected.0.as_deref().and_then(|name| people.find(name));

    let show_card = record.is_some();
    for mut node in card.iter_mut() {
        let display = if show_card { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in empty.iter_mut() {
        let display = if show_card { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }

    let Some(record) = record else {
        return;
    };

    for mut text in name_text.iter_mut() {
        if text.0 != record.name {
            text.0 = record.name.clone();
        }
    }
    for mut text in subtitle.iter_mut() {
        let value = if record.is_self {
            format!("{} (you)", record.kind.label())
        } else {
            record.kind.label().to_string()
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (DetailStat(field), mut text) in stats.iter_mut() {
        let value = match field {
            DetailField::Affiliation => {
                if record.known {
                    record.affiliation.badge().to_string()
                } else {
                    "Unrecorded".to_string()
                }
            }
            DetailField::Standing => {
                if record.known {
                    format!("Level {}, {} prestige", record.level, record.prestige)
                } else {
                    "Unrecorded".to_string()
                }
            }
            DetailField::Status => if record.online {
                "In the world now"
            } else {
                "Away"
            }
            .to_string(),
            DetailField::Knowledge => if record.is_self {
                "Yourself"
            } else if record.known {
                "Known to you"
            } else {
                "Unknown to you (god mode)"
            }
            .to_string(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}
