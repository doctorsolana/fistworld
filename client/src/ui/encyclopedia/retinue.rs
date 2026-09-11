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
use crate::ui::hud::chrome::{self, HudIcon};
use crate::ui::ledger;
use crate::ui::styles::{INK_MUTED, PLATE_RULE_SOFT, STATUS_GOOD};
use bevy::ui::InteractionDisabled;

/// Character-scale zoom for LOCATE, matching the immigrant-boat watcher.
const LOCATE_ZOOM: f32 = 82.0;

// --- markers ----------------------------------------------------------------

#[derive(Component)]
pub(super) struct RetinueListContent;

#[derive(Component)]
pub(super) struct RetinueRow;

#[derive(Component)]
pub(super) struct RetinueCountText;
#[derive(Component)]
pub(super) struct RetinuePresence(pub PersonId);
#[derive(Component)]
pub(super) struct RetinuePresenceDot(pub PersonId);

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

fn retinue_status_line(record: &PersonRecord, present: bool) -> String {
    let doing = state_sync::activity_description(record, present);
    match (record.occupation.as_deref(), record.residence.as_deref()) {
        (Some(occupation), Some(residence)) => format!("{doing} · {occupation} of {residence}"),
        (None, Some(residence)) => format!("{doing} · {residence}"),
        (Some(occupation), None) => format!("{doing} · {occupation}"),
        (None, None) => doing,
    }
}

// --- layout -----------------------------------------------------------------

pub(super) fn spawn_retinue_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Retinue),
        Node {
            display: Display::None,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect::axes(Val::Px(52.0), Val::Px(24.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        ledger::paper(),
    ))
    .with_children(|tab| {
        tab.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            row_gap: Val::Px(7.0),
            padding: UiRect::bottom(Val::Px(24.0)),
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(20.0),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn(chrome::icon(HudIcon::Crest, 76.0));
                    title
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(7.0),
                            ..default()
                        })
                        .with_children(|copy| {
                            copy.spawn(ledger::heading("Your retinue", 38.0));
                            copy.spawn((RetinueCountText, ledger::body("Your hero", 21.0)));
                        });
                });
            header
                .spawn((Node {
                    width: Val::Px(500.0),
                    max_width: Val::Percent(75.0),
                    margin: UiRect::top(Val::Px(12.0)),
                    ..default()
                },))
                .with_child(ledger::ornament_rule());
        });
        tab.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            scrollbar_width: 7.0,
            ..default()
        })
        .with_child((
            RetinueListContent,
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                flex_shrink: 0.0,
                ..default()
            },
        ));
        tab.spawn((
            ledger::body("Locate returns you to their position in the world.", 17.0),
            Node {
                flex_shrink: 0.0,
                margin: UiRect::top(Val::Px(16.0)),
                ..default()
            },
        ));
    });
}

fn spawn_retinue_row(list: &mut ChildSpawnerCommands<'_>, record: &PersonRecord) {
    list.spawn((
        RetinueRow,
        Node {
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(28.0),
            min_height: Val::Px(88.0),
            padding: UiRect::axes(Val::Px(30.0), Val::Px(8.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::from(PLATE_RULE_SOFT),
    ))
    .with_children(|row| {
        row.spawn(ledger::person_portrait(record.id, 72.0));
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            ..default()
        })
        .with_children(|copy| {
            copy.spawn(ledger::body_strong(
                if record.is_self {
                    format!("{} (you)", record.name)
                } else {
                    record.name.clone()
                },
                23.0,
            ));
            copy.spawn((
                RetinueStatusText(record.id),
                ledger::body(retinue_status_line(record, false), 18.0),
            ));
        });
        row.spawn(Node {
            width: Val::Px(170.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|presence| {
            presence.spawn((
                RetinuePresenceDot(record.id),
                Node {
                    width: Val::Px(14.0),
                    height: Val::Px(14.0),
                    border_radius: BorderRadius::all(Val::Px(7.0)),
                    ..default()
                },
                BackgroundColor(INK_MUTED),
            ));
            presence.spawn((RetinuePresence(record.id), ledger::body("Unknown", 18.0)));
        });
        row.spawn((
            LocateButton {
                person: record.id,
                is_self: record.is_self,
            },
            Button,
            InteractionDisabled,
            Node {
                width: Val::Px(132.0),
                height: Val::Px(44.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Primary),
        ))
        .with_child((UiButtonLabel, ledger::heading("LOCATE", 15.0)));
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
    let fresh = existing_rows.is_empty();
    if !fresh && !people.is_changed() && !name_input.is_changed() {
        return;
    }
    let account = name_input.name.trim().to_lowercase();
    let rows = clan_rows(&people, &account);
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
        0 => "Your hero".to_string(),
        1 => "Your hero · 1 companion".to_string(),
        count => format!("Your hero · {count} companions"),
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
                RetinueRow,
                Text::new("Your retinue will gather here once your hero enters the world."),
                crate::ui::typography::text(13.5),
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

/// Bind actual replication presence, so an unavailable Locate never pretends to work.
/// The quarter-second pass reuses its lookup storage while the roster is open.
pub(super) fn bind_retinue_presence(
    mut commands: Commands,
    time: Res<Time>,
    people: Res<KnownPeople>,
    visible: Query<&PersonId, With<PlayerPosition>>,
    buttons: Query<(Entity, Ref<LocateButton>, Has<InteractionDisabled>)>,
    mut labels: Query<(&RetinuePresence, &mut Text), Without<RetinueStatusText>>,
    mut statuses: Query<(&RetinueStatusText, &mut Text), Without<RetinuePresence>>,
    mut dots: Query<(&RetinuePresenceDot, &mut BackgroundColor)>,
    mut last: Local<Option<f32>>,
    mut present: Local<std::collections::HashSet<PersonId>>,
) {
    let now = time.elapsed_secs();
    if last.is_some_and(|last| now - last < 0.25)
        && !buttons.iter().any(|(_, marker, _)| marker.is_added())
        && !people.is_changed()
    {
        return;
    }
    *last = Some(now);
    present.clear();
    present.extend(visible.iter().copied());
    for (entity, locate, disabled) in &buttons {
        let enabled = present.contains(&locate.person);
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for (RetinuePresence(id), mut text) in &mut labels {
        let next = if present.contains(id) {
            "Here"
        } else {
            "Unknown"
        };
        if text.0 != next {
            text.0 = next.to_string();
        }
    }
    for (RetinueStatusText(id), mut text) in &mut statuses {
        let Some(record) = people.find_by_id(*id) else {
            continue;
        };
        let next = retinue_status_line(record, present.contains(id));
        if text.0 != next {
            text.0 = next;
        }
    }
    for (RetinuePresenceDot(id), mut color) in &mut dots {
        let next = if present.contains(id) {
            STATUS_GOOD
        } else {
            INK_MUTED
        };
        if color.0 != next {
            color.0 = next;
        }
    }
}

pub(super) fn handle_locate_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &LocateButton), Without<InteractionDisabled>>,
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
    fn activity_follows_actual_presence_without_requiring_npc_activity_on_heroes() {
        let mut hero_record = record("Aldric", true, None);
        hero_record.kind = PersonKind::Hero;
        hero_record.occupation = None;
        hero_record.residence = None;
        let person = hero_record.id;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(crate::ui::perf::UiPerf::default());
        world.insert_resource(SelectedPerson(Some(person)));
        let mut people = KnownPeople::default();
        people.records.push(hero_record);
        world.insert_resource(people);
        let actor = world.spawn((person, PlayerPosition(Vec3::ZERO))).id();
        let status = world
            .spawn((RetinueStatusText(person), Text::default()))
            .id();
        let presence = world.spawn((RetinuePresence(person), Text::default())).id();
        let detail = world
            .spawn((DetailStat(DetailField::Activity), Text::default()))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems((bind_retinue_presence, state_sync::sync_detail_panel).chain());
        schedule.run(&mut world);
        assert_eq!(world.get::<Text>(presence).unwrap().0, "Here");
        assert_eq!(world.get::<Text>(status).unwrap().0, "Your hero");
        assert_eq!(world.get::<Text>(detail).unwrap().0, "Your hero");

        // Leaving replication range changes presence without dirtying the
        // last-known record or supplying any villager activity component.
        world.despawn(actor);
        world
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(300));
        schedule.run(&mut world);
        assert_eq!(world.get::<Text>(presence).unwrap().0, "Unknown");
        assert_eq!(world.get::<Text>(status).unwrap().0, "Beyond your sight");
        assert_eq!(world.get::<Text>(detail).unwrap().0, "Beyond your sight");

        world.spawn((person, PlayerPosition(Vec3::ZERO)));
        world
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(300));
        schedule.run(&mut world);
        assert_eq!(world.get::<Text>(status).unwrap().0, "Your hero");
        assert_eq!(world.get::<Text>(detail).unwrap().0, "Your hero");
    }

    #[test]
    fn cached_npc_activity_is_not_presented_as_current_when_the_person_is_absent() {
        let mut person = record("Marta", false, Some("wanderer"));
        person.objective = Some(shared::components::CharacterObjective::WalkingAroundTown);
        assert_eq!(
            state_sync::activity_description(&person, true),
            "Walking around town"
        );
        assert_eq!(
            state_sync::activity_description(&person, false),
            "Beyond your sight"
        );
        person.objective = None;
        assert_eq!(
            state_sync::activity_description(&person, true),
            "Activity unrecorded"
        );
    }

    #[test]
    fn locate_tracks_real_presence_and_disables_when_the_person_leaves_interest() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(KnownPeople::default());
        let person = PersonId(9);
        let actor = world.spawn((person, PlayerPosition(Vec3::ZERO))).id();
        let button = world
            .spawn((
                LocateButton {
                    person,
                    is_self: false,
                },
                InteractionDisabled,
            ))
            .id();
        let label = world
            .spawn((RetinuePresence(person), Text::new("Unknown")))
            .id();
        world.run_system_once(bind_retinue_presence).unwrap();
        assert!(!world.entity(button).contains::<InteractionDisabled>());
        assert_eq!(world.get::<Text>(label).unwrap().0, "Here");
        world.despawn(actor);
        world.run_system_once(bind_retinue_presence).unwrap();
        assert!(world.entity(button).contains::<InteractionDisabled>());
        assert_eq!(world.get::<Text>(label).unwrap().0, "Unknown");
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
