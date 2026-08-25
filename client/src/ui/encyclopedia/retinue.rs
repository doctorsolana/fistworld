//! The clan roster: your hero and everyone sworn to your banner.
//!
//! "Your people" is everyone whose replicated [`CommandedBy`] carries your
//! account (the same authority the server checks before moving a unit), plus
//! your own hero. Each row keeps a live status line bound in place and a
//! LOCATE button that glides the commander camera to that person and closes
//! the window so you see the flight.

use bevy::prelude::*;

use shared::components::{Hero, PersonId, PlayerPosition};

use super::*;
use crate::camera_rts::{CommanderCamera, LocalPeerId};
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, STATUS_GOOD};

/// Character-scale zoom for LOCATE, matching the immigrant-boat watcher.
const LOCATE_ZOOM: f32 = 82.0;

// --- markers ----------------------------------------------------------------

#[derive(Component)]
pub(super) struct RetinueListContent;

#[derive(Component)]
pub(super) struct RetinueRow(pub shared::components::PersonId);

#[derive(Component)]
pub(super) struct RetinueCountText;

/// The live status line under a member's name, keyed by durable id - never
/// by name, which generated worlds are free to duplicate.
#[derive(Component)]
pub(super) struct RetinueStatusText(pub shared::components::PersonId);

#[derive(Component, Clone)]
pub(super) struct LocateButton {
    pub person: PersonId,
    pub is_self: bool,
}

pub(super) fn retinue_tab_active(tab: Res<EncyclopediaTab>) -> bool {
    *tab == EncyclopediaTab::Retinue
}

// --- data -------------------------------------------------------------------

/// Yourself first, then companions by name.
pub(super) fn clan_rows<'a>(people: &'a KnownPeople, account: &str) -> Vec<&'a PersonRecord> {
    let mut rows: Vec<&PersonRecord> = people
        .records
        .iter()
        .filter(|record| {
            record.alive
                && (record.is_self
                    || (!account.is_empty() && record.commanded_by.as_deref() == Some(account)))
        })
        .collect();
    rows.sort_by(|a, b| {
        b.is_self
            .cmp(&a.is_self)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    rows.dedup_by(|a, b| a.id == b.id);
    rows
}

/// Only the slow-moving facts a row's STRUCTURE renders; the status line is
/// bound in place and deliberately stays out of this hash.
fn retinue_rows_signature(rows: &[&PersonRecord]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for record in rows {
        record.name.hash(&mut hasher);
        record.is_self.hash(&mut hasher);
        record.online.hash(&mut hasher);
        record.id.0.hash(&mut hasher);
        record.occupation.hash(&mut hasher);
    }
    hasher.finish()
}

fn retinue_status_line(record: &PersonRecord) -> String {
    let doing = record
        .objective
        .map(|objective| objective.label().to_string())
        .or_else(|| record.activity.map(|activity| activity.label().to_string()))
        .unwrap_or_else(|| "Beyond your sight".to_string());
    match (record.occupation.as_deref(), record.residence.as_deref()) {
        (Some(occupation), Some(residence)) => format!("{doing}  /  {occupation} of {residence}"),
        (None, Some(residence)) => format!("{doing}  /  of {residence}"),
        (Some(occupation), None) => format!("{doing}  /  {occupation}"),
        (None, None) => doing,
    }
}

// --- layout -----------------------------------------------------------------

pub(super) fn spawn_retinue_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Retinue),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|tab| {
        tab.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new("YOUR CLAN"),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(EMBER),
            ));
            row.spawn((
                RetinueCountText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
        tab.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            scrollbar_width: 8.0,
            ..default()
        })
        .with_children(|viewport| {
            viewport.spawn((
                RetinueListContent,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    flex_shrink: 0.0,
                    padding: UiRect::all(Val::Px(12.0)),
                    row_gap: Val::Px(4.0),
                    ..default()
                },
            ));
        });
    });
}

fn spawn_retinue_row(list: &mut ChildSpawnerCommands<'_>, record: &PersonRecord) {
    list.spawn((
        RetinueRow(record.id),
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(12.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::from(PLATE_RULE_SOFT),
    ))
    .with_children(|row| {
        row.spawn((
            Node {
                width: Val::Px(7.0),
                height: Val::Px(7.0),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if record.online || !record.is_self {
                STATUS_GOOD
            } else {
                INK_MUTED
            }),
        ));
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|copy| {
            copy.spawn((
                Text::new(if record.is_self {
                    format!("{} (you)", record.name)
                } else {
                    record.name.clone()
                }),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(INK),
            ));
            copy.spawn((
                RetinueStatusText(record.id),
                Text::new(retinue_status_line(record)),
                TextFont {
                    font_size: FontSize::Px(12.5),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
        row.spawn((
            LocateButton {
                person: record.id,
                is_self: record.is_self,
            },
            Button,
            Node {
                height: Val::Px(32.0),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(Val::Px(14.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            Text::new("LOCATE"),
            UiButtonLabel,
            TextFont {
                font_size: FontSize::Px(12.5),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
    });
}

// --- systems ----------------------------------------------------------------

pub(super) fn rebuild_retinue_list(
    mut commands: Commands,
    people: Res<KnownPeople>,
    name_input: Res<crate::ui::name_entry::PlayerNameInput>,
    perf: Res<crate::ui::perf::UiPerf>,
    content: Query<Entity, With<RetinueListContent>>,
    existing_rows: Query<Entity, With<RetinueRow>>,
    mut count_text: Query<&mut Text, With<RetinueCountText>>,
    mut signature: Local<u64>,
) {
    let mut scope = perf.scope("rebuild_retinue_list");
    let Ok(content) = content.single() else {
        return;
    };
    let account = name_input.name.trim().to_lowercase();
    let rows = clan_rows(&people, &account);
    let fresh = existing_rows.is_empty();
    let next = retinue_rows_signature(&rows);
    if !fresh && *signature == next {
        return;
    }
    *signature = next;
    scope.rebuilt();
    for row in existing_rows.iter() {
        commands.entity(row).despawn();
    }
    let companions = rows.iter().filter(|record| !record.is_self).count();
    let count_label = match companions {
        0 => "You alone".to_string(),
        1 => "1 companion".to_string(),
        count => format!("{count} companions"),
    };
    for mut text in count_text.iter_mut() {
        if text.0 != count_label {
            text.0 = count_label.clone();
        }
    }
    commands.entity(content).with_children(|list| {
        if rows.is_empty() {
            // The empty state carries the row marker so the next rebuild
            // clears it (same rule as the people list).
            list.spawn((
                RetinueRow(shared::components::PersonId::default()),
                Text::new("Your clan is just you for now. Sworn companions will gather here."),
                TextFont {
                    font_size: FontSize::Px(13.5),
                    ..default()
                },
                TextColor(INK_MUTED),
                Node {
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
        } else {
            for record in &rows {
                spawn_retinue_row(list, record);
            }
        }
    });
}

/// The status line moves with the person's day; bind it in place so rows
/// never rebuild for a villager changing activity.
pub(super) fn bind_retinue_status(
    people: Res<KnownPeople>,
    mut texts: Query<(&RetinueStatusText, &mut Text)>,
) {
    if !people.is_changed() {
        return;
    }
    for (marker, mut text) in texts.iter_mut() {
        let Some(record) = people.find_by_id(marker.0) else {
            continue;
        };
        let next = retinue_status_line(record);
        if text.0 != next {
            text.0 = next;
        }
    }
}

pub(super) fn handle_locate_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &LocateButton)>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(&Hero, &PlayerPosition)>,
    people: Query<(&PersonId, &PlayerPosition), Without<Hero>>,
    mut cameras: Query<&mut CommanderCamera>,
    mut open: ResMut<EncyclopediaOpen>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, locate) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let position = if locate.is_self {
            local
                .as_ref()
                .and_then(|local| {
                    heroes
                        .iter()
                        .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
                })
                .map(|(_, position)| position.0)
        } else {
            people
                .iter()
                .find(|(person, _)| **person == locate.person)
                .map(|(_, position)| position.0)
        };
        let Some(position) = position else {
            continue;
        };
        for mut camera in cameras.iter_mut() {
            camera.focus_target = position;
            camera.zoom_target = LOCATE_ZOOM.clamp(camera.zoom_min, camera.zoom_max);
        }
        // Close the window so the player sees the flight (the camera is
        // frozen while the encyclopedia is open).
        open.0 = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, is_self: bool, commanded_by: Option<&str>) -> PersonRecord {
        PersonRecord {
            id: shared::components::PersonId(name.len() as u64),
            name: name.to_string(),
            kind: PersonKind::Villager,
            affiliation: Affiliation::default(),
            level: 1,
            prestige: 0,
            online: is_self,
            alive: true,
            health: None,
            death_day: None,
            death_cause: None,
            known: true,
            is_self,
            commanded_by: commanded_by.map(str::to_string),
            residence: Some("Brackwater".to_string()),
            home: None,
            occupation: Some("Fisher".to_string()),
            workplace: None,
            wallet: None,
            nutrition: None,
            activity: None,
            objective: None,
            day_plan: None,
            navigation: None,
            attributes: None,
            work_status: None,
            daily_wage: None,
            workforce_requirements: None,
            inventory: None,
            carried: None,
        }
    }

    #[test]
    fn the_clan_is_yourself_plus_your_commanded_people_and_nobody_else() {
        let mut people = KnownPeople::default();
        people.records.push(record("Odo", false, Some("wanderer")));
        people.records.push(record("Aldric", false, None));
        people
            .records
            .push(record("Wanderer", true, Some("wanderer")));
        people
            .records
            .push(record("Brta", false, Some("someone_else")));

        let rows = clan_rows(&people, "wanderer");
        let names: Vec<&str> = rows.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["Wanderer", "Odo"]);

        // An empty account (name not typed yet) never matches strangers.
        let rows = clan_rows(&people, "");
        let names: Vec<&str> = rows.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["Wanderer"]);
    }
}
