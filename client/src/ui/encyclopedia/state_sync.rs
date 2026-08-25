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
use crate::ui::foundation::{button_chrome, UiButtonStyle, UiButtonVariant};
use crate::ui::hud::GodCapability;
use crate::ui::styles::{INK, INK_MUTED};

type VisiblePersonFacts<'a> = (
    &'a shared::components::CharacterName,
    Option<&'a shared::components::Residence>,
    Option<&'a shared::components::Occupation>,
    Option<&'a shared::components::WorkStatus>,
    Option<&'a shared::economy::Wallet>,
    (
        Option<&'a shared::components::Nutrition>,
        Option<&'a shared::components::Health>,
    ),
    (
        Option<&'a shared::components::CharacterActivity>,
        Option<&'a shared::components::CharacterDayPlan>,
    ),
    (
        Option<&'a shared::components::CharacterObjective>,
        Option<&'a shared::components::CharacterNavigationStatus>,
    ),
    Option<&'a shared::components::CharacterAttributes>,
    Option<&'a shared::economy::GoodsInventory>,
    Option<&'a shared::economy::CarriedLoad>,
    Option<&'a shared::components::EmployedAt>,
    Option<&'a shared::components::CivicEmployment>,
    Option<&'a shared::components::LivesAt>,
    Option<&'a shared::components::PersonId>,
);

/// How often the visible-people fact pass re-reads the world.
const PERSON_FACTS_INTERVAL_SECS: f32 = 0.25;

/// The camera must not pan and world clicks must not fire underneath.
pub(super) fn sync_input_state(open: Res<EncyclopediaOpen>, mut input_state: ResMut<InputState>) {
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
                // An entry whose id has not been minted yet is not mergeable
                // by anything durable; the next roster refresh carries it.
                if !entry.id.is_assigned() {
                    continue;
                }
                if let Some(existing) = people
                    .records
                    .iter_mut()
                    .find(|record| record.id == entry.id)
                {
                    existing.kind = kind;
                    existing.affiliation = entry.affiliation;
                    existing.online = entry.online;
                    existing.alive = entry.alive;
                    existing.health = Some(entry.health);
                    existing.death_day = entry.death_day;
                    existing.death_cause = entry.death_cause;
                    existing.is_self = entry.is_self;
                    existing.attributes = Some(entry.attributes);
                    // Knowing OF someone from the roster does not make them
                    // known -- god mode reveals unknown records still marked
                    // unknown, so the fog stays visible rather than being
                    // silently switched off.
                    existing.known |= entry.is_self;
                } else {
                    people.records.push(PersonRecord {
                        id: entry.id,
                        known: entry.is_self,
                        is_self: entry.is_self,
                        name: entry.name,
                        kind,
                        affiliation: entry.affiliation,
                        level: 0,
                        prestige: 0,
                        online: entry.online,
                        alive: entry.alive,
                        health: Some(entry.health),
                        death_day: entry.death_day,
                        death_cause: entry.death_cause,
                        commanded_by: None,
                        residence: None,
                        home: None,
                        occupation: None,
                        workplace: None,
                        wallet: None,
                        nutrition: None,
                        activity: None,
                        objective: None,
                        day_plan: None,
                        navigation: None,
                        attributes: Some(entry.attributes),
                        work_status: None,
                        daily_wage: None,
                        workforce_requirements: None,
                        inventory: None,
                        carried: None,
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
        (
            &shared::components::CharacterName,
            &shared::components::CharacterKind,
            // OPTIONAL, and that is load-bearing: replication can deliver a
            // character's components in separate batches, so requiring the
            // banner here would silently skip anyone whose name arrived first
            // -- and `Added` fires once, so they would never be learned at all.
            // `track_affiliation_changes` fills it in when it lands.
            Option<&shared::components::CharacterAffiliation>,
            Option<&shared::components::CommandedBy>,
            // REQUIRED: a record is never created without its durable id.
            // Names collide (generated names drew two "Jarl Haldenson"s in
            // one skirmish), so an id-less record would poison every merge
            // that has to fall back to name matching. The Or<> below fires
            // whichever of the pair lands second, so batching cannot skip
            // anyone permanently.
            &shared::components::PersonId,
        ),
        Or<(
            Added<shared::components::CharacterName>,
            Added<shared::components::PersonId>,
        )>,
    >,
    mut people: ResMut<KnownPeople>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
) {
    let mut _ui_scope = ui_perf.scope("learn_visible_characters");
    for (name, kind, affiliation, commanded, person_id) in seen.iter() {
        if !person_id.is_assigned() {
            continue;
        }
        let affiliation = affiliation.copied().unwrap_or_default();
        let kind = match kind {
            shared::components::CharacterKind::Hero => PersonKind::Hero,
            shared::components::CharacterKind::Villager => PersonKind::Villager,
        };
        if let Some(existing) = people
            .records
            .iter_mut()
            .find(|record| record.id == *person_id)
        {
            existing.known = true;
            existing.kind = kind;
            existing.affiliation = affiliation;
            existing.commanded_by = commanded.map(|c| c.0.clone());
        } else {
            people.records.push(PersonRecord {
                id: *person_id,
                name: name.0.clone(),
                kind,
                affiliation,
                level: 0,
                prestige: 0,
                online: false,
                alive: true,
                health: None,
                death_day: None,
                death_cause: None,
                known: true,
                is_self: false,
                commanded_by: commanded.map(|c| c.0.clone()),
                residence: None,
                home: None,
                occupation: None,
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
            });
        }
    }
}

/// Refresh the last-known life facts for characters currently replicated.
///
/// Workplace and cabin are derived from the building rosters themselves, so
/// the encyclopedia cannot claim a job or bed that the corresponding building
/// does not also show.
pub(super) fn refresh_visible_person_facts(
    seen: Query<VisiblePersonFacts<'_>>,
    buildings: Query<(
        &shared::components::SettlementBuilding,
        Option<&shared::components::BuildingId>,
        Option<&shared::economy::BusinessWagePolicy>,
        Option<&shared::economy::WorkforceRequirements>,
    )>,
    households: Query<(
        &shared::components::SettlementBuilding,
        Option<&shared::components::BuildingId>,
        &shared::components::Household,
    )>,
    administrations: Query<(
        &shared::components::Settlement,
        Option<&shared::components::SettlementId>,
        &shared::components::MootAdministration,
    )>,
    mut people: ResMut<KnownPeople>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
    time: Res<Time>,
    mut last_run: Local<Option<f32>>,
) {
    let mut _ui_scope = ui_perf.scope("refresh_visible_person_facts");
    // Facts feed text readouts: four refreshes a second read identically to
    // sixty, and every refresh that finds a change marks the registry
    // changed, which is what the list and detail panes key their work on.
    let now = time.elapsed_secs();
    if last_run.is_some_and(|last| now - last < PERSON_FACTS_INTERVAL_SECS) {
        return;
    }
    *last_run = Some(now);

    // Index once per pass. The per-person linear scans this replaces were
    // O(people x buildings x workers) for employment and O(people^2) for the
    // registry lookup -- a thousand residents made them a per-frame tax.
    let person_by_id: std::collections::HashMap<shared::components::PersonId, usize> = people
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.id.is_assigned())
        .map(|(index, record)| (record.id, index))
        .collect();

    let building_entries: Vec<_> = buildings.iter().collect();
    let mut building_by_id = std::collections::HashMap::new();
    let mut building_by_worker: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (index, (building, building_id, _, _)) in building_entries.iter().enumerate() {
        if let Some(id) = building_id {
            building_by_id.entry(**id).or_insert(index);
        }
        for worker in &building.workers {
            building_by_worker.entry(worker.as_str()).or_insert(index);
        }
    }
    let household_entries: Vec<_> = households.iter().collect();
    let mut household_by_id = std::collections::HashMap::new();
    let mut household_by_resident: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (index, (_, building_id, household)) in household_entries.iter().enumerate() {
        if let Some(id) = building_id {
            household_by_id.entry(**id).or_insert(index);
        }
        for resident in &household.residents {
            household_by_resident
                .entry(resident.as_str())
                .or_insert(index);
        }
    }

    for (
        name,
        residence,
        occupation,
        work_status,
        wallet,
        (nutrition, health),
        (activity, day_plan),
        (objective, navigation),
        attributes,
        inventory,
        carried,
        employed_at,
        civic_job,
        lives_at,
        person_id,
    ) in seen.iter()
    {
        let employment = match employed_at {
            Some(job) => building_by_id.get(&job.0),
            None => building_by_worker.get(name.0.as_str()),
        }
        .map(|&index| building_entries[index]);
        let workplace = employment.map(|(building, _, _, _)| {
            format!("{} in {}", building.kind.label(), building.settlement)
        });
        let next_daily_wage = employment
            .and_then(|(_, _, wage, _)| wage)
            .map(|wage| wage.daily_wage);
        let next_requirements = employment
            .and_then(|(_, _, _, requirements)| requirements)
            .copied();
        let civic_employment = administrations.iter().find_map(|(settlement, id, office)| {
            let role = if let Some(job) = civic_job {
                if !id.is_some_and(|id| *id == job.settlement) {
                    return None;
                }
                match job.role {
                    shared::components::CivicRole::Reeve => {
                        ("Reeve", Some(shared::economy::FOUNDING_DAILY_WAGE))
                    }
                    shared::components::CivicRole::CityWorker => {
                        ("City Worker", Some(shared::economy::FOUNDING_DAILY_WAGE))
                    }
                    shared::components::CivicRole::Guard => {
                        ("Guard", Some(shared::economy::FOUNDING_DAILY_WAGE))
                    }
                    shared::components::CivicRole::MootSteward => {
                        ("Moot Steward", Some(office.steward_daily_salary))
                    }
                }
            } else if office.reeve.as_deref() == Some(name.0.as_str()) {
                ("Reeve", Some(shared::economy::FOUNDING_DAILY_WAGE))
            } else if office.lead_steward.as_deref() == Some(name.0.as_str()) {
                ("Moot Steward", Some(office.steward_daily_salary))
            } else if office.guards.iter().any(|guard| guard == &name.0) {
                ("Guard", Some(shared::economy::FOUNDING_DAILY_WAGE))
            } else if office.city_workers.iter().any(|worker| worker == &name.0) {
                ("City Worker", Some(shared::economy::FOUNDING_DAILY_WAGE))
            } else {
                return None;
            };
            Some((
                format!("{} at the Moot Hall in {}", role.0, settlement.name),
                role.1,
            ))
        });
        let workplace =
            workplace.or_else(|| civic_employment.as_ref().map(|(place, _)| place.clone()));
        let next_daily_wage =
            next_daily_wage.or_else(|| civic_employment.and_then(|(_, wage)| wage));
        let home = match lives_at {
            Some(home) => household_by_id.get(&home.0),
            None => household_by_resident.get(name.0.as_str()),
        }
        .map(|&index| format!("Cabin in {}", household_entries[index].0.settlement));
        let next_residence = residence.map(|residence| residence.0.clone());
        let next_occupation = occupation
            .and_then(|occupation| occupation.0.clone())
            .or_else(|| work_status.map(|status| status.label().to_string()));
        let next_wallet = wallet.map(|wallet| wallet.balance());
        let next_nutrition = nutrition.copied();
        let next_health = health.cloned();
        let next_alive = !health.is_some_and(|health| health.is_dead());
        let next_activity = activity.copied();
        let next_objective = objective.copied();
        let next_day_plan = day_plan.copied();
        let next_navigation = navigation.copied();
        let next_attributes = attributes.copied();
        let next_work_status = work_status.copied();
        let next_inventory = inventory.cloned();
        let next_carried = carried.copied();

        let Some(index) = person_id.and_then(|id| person_by_id.get(id).copied()) else {
            continue;
        };
        let current = &people.records[index];
        let changed = current.residence != next_residence
            || current.home != home
            || current.occupation != next_occupation
            || current.workplace != workplace
            || current.wallet != next_wallet
            || current.nutrition != next_nutrition
            || current.health != next_health
            || current.alive != next_alive
            || current.activity != next_activity
            || current.objective != next_objective
            || current.day_plan != next_day_plan
            || current.navigation != next_navigation
            || current.attributes != next_attributes
            || current.work_status != next_work_status
            || current.daily_wage != next_daily_wage
            || current.workforce_requirements != next_requirements
            || current.inventory != next_inventory
            || current.carried != next_carried;
        if !changed {
            continue;
        }
        {
            let record = &mut people.records[index];
            if let Some(id) = person_id {
                record.id = *id;
            }
            record.residence = next_residence;
            record.home = home;
            record.occupation = next_occupation;
            record.workplace = workplace;
            record.wallet = next_wallet;
            record.nutrition = next_nutrition;
            record.health = next_health;
            record.alive = next_alive;
            record.activity = next_activity;
            record.objective = next_objective;
            record.day_plan = next_day_plan;
            record.navigation = next_navigation;
            record.attributes = next_attributes;
            record.work_status = next_work_status;
            record.daily_wage = next_daily_wage;
            record.workforce_requirements = next_requirements;
            record.inventory = next_inventory;
            record.carried = next_carried;
        }
    }
}

/// Track banner changes on characters you can see, so a god-mode edit shows up
/// immediately rather than waiting for the next roster request.
pub(super) fn track_affiliation_changes(
    changed: Query<
        (
            &shared::components::CharacterName,
            &shared::components::CharacterAffiliation,
            Option<&shared::components::PersonId>,
        ),
        Changed<shared::components::CharacterAffiliation>,
    >,
    mut people: ResMut<KnownPeople>,
) {
    for (_, affiliation, person_id) in changed.iter() {
        let Some(record) = people
            .records
            .iter_mut()
            .find(|record| person_id.is_some_and(|id| record.id == *id))
        else {
            continue;
        };
        if record.affiliation != *affiliation {
            record.affiliation = *affiliation;
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
    mut last: Local<Option<u64>>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
) {
    let mut _ui_scope = ui_perf.scope("rebuild_people_list");
    // A row shows only slow-moving facts: name, kind, banner, occupation,
    // online, known. The registry itself changes several times a second while
    // a thousand villagers walk, eat and earn, and keying this on
    // `is_changed()` tore down and respawned every row at that rate: measured
    // at 1,000 residents, 127 rebuilds in 127 frames and a 6 ms frame became
    // 75 ms. Hash the ROW projection and rebuild only when it moves; a fresh
    // (just respawned) list container always rebuilds.
    let fresh = existing_rows.is_empty();
    let dirty = people.is_changed() || filter.is_changed() || god.is_changed();
    if !dirty && !fresh && last.is_some() {
        return;
    }
    let visible = people.visible(*filter, god.0);
    let signature = people_rows_signature(&visible);
    if !fresh && *last == Some(signature) {
        return;
    }

    let Ok(content_entity) = content.single() else {
        return;
    };
    *last = Some(signature);
    _ui_scope.rebuilt();
    for row in existing_rows.iter() {
        commands.entity(row).despawn();
    }

    // Drop a selection the filter just hid, so the detail pane never describes
    // someone who is no longer listed.
    if let Some(id) = selected.0 {
        if !visible.iter().any(|record| record.id == id) {
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
                PersonRow {
                    id: shared::components::PersonId::default(),
                    name: String::new(),
                },
                Text::new("No one here yet"),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(INK_MUTED),
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

/// Hash of exactly what [`spawn_person_row`] renders, in display order.
fn people_rows_signature(visible: &[&PersonRecord]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    visible.len().hash(&mut hasher);
    for record in visible {
        record.name.hash(&mut hasher);
        record.is_self.hash(&mut hasher);
        record.known.hash(&mut hasher);
        record.online.hash(&mut hasher);
        (record.kind == PersonKind::Villager).hash(&mut hasher);
        record.occupation.hash(&mut hasher);
        record.affiliation.label().hash(&mut hasher);
    }
    hasher.finish()
}

fn spawn_person_row(list: &mut ChildSpawnerCommands<'_>, record: &PersonRecord) {
    let name_color = if record.known { INK } else { INK_MUTED };
    list.spawn((
        Button,
        PersonRow {
            id: record.id,
            name: record.name.clone(),
        },
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(9.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        button_chrome(UiButtonVariant::Row),
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
                STATUS_GOOD
            } else if record.known {
                INK_MUTED
            } else {
                PLATE_RULE_SOFT
            }),
        ));
        row.spawn((
            Text::new(if record.is_self {
                format!("{} (you)", record.name)
            } else {
                record.name.clone()
            }),
            TextFont {
                font_size: FontSize::Px(16.0),
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
                if record.kind == PersonKind::Villager {
                    record
                        .occupation
                        .as_deref()
                        .unwrap_or("Unemployed")
                        .to_uppercase()
                } else {
                    record.affiliation.label().to_string()
                }
            } else {
                "UNKNOWN".to_string()
            }),
            TextFont {
                font_size: FontSize::Px(12.5),
                ..default()
            },
            TextColor(if record.known {
                INK_MUTED
            } else {
                PLATE_RULE_SOFT
            }),
        ));
    });
}

pub(super) fn sync_tab_visuals(
    tab: Res<EncyclopediaTab>,
    mut buttons: Query<(&TabButton, &mut UiButtonStyle)>,
    mut bodies: Query<(&TabBody, &mut Node)>,
) {
    for (TabButton(button_tab), mut style) in buttons.iter_mut() {
        let active = *button_tab == *tab;
        style.selected = active;
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
    mut buttons: Query<(&FilterButton, &mut UiButtonStyle, &mut Node)>,
) {
    for (FilterButton(button_filter), mut style, mut node) in buttons.iter_mut() {
        // UNKNOWN is the god-mode view of the fog; hide it without capability.
        let display = if *button_filter == PeopleFilter::Unknown && !god.0 {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }

        style.selected = *button_filter == *filter;
    }
}

pub(super) fn style_person_rows(
    selected: Res<SelectedPerson>,
    mut rows: Query<(&PersonRow, &mut UiButtonStyle)>,
) {
    for (row, mut style) in rows.iter_mut() {
        style.selected = row.id.is_assigned() && selected.0 == Some(row.id);
    }
}

fn format_plan_minute(minute: u16) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

fn format_day_plan(plan: shared::components::CharacterDayPlan) -> String {
    let work = plan.work_minutes.map_or_else(
        || plan.planned_work_status.label().to_string(),
        |(start, end)| {
            format!(
                "work {}–{}",
                format_plan_minute(start),
                format_plan_minute(end)
            )
        },
    );
    format!(
        "Day {} · wake {} · {} · meal {} · {} {}–{} ({}) · sleep {}",
        plan.day,
        format_plan_minute(plan.wake_minute),
        work,
        format_plan_minute(plan.meal_minute),
        plan.leisure.label(),
        format_plan_minute(plan.leisure_minutes.0),
        format_plan_minute(plan.leisure_minutes.1),
        plan.leisure_status.label(),
        format_plan_minute(plan.sleep_minute),
    )
}

pub(super) fn sync_detail_panel(
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    mut card: Query<&mut Node, (With<DetailCard>, Without<DetailEmptyState>)>,
    mut empty: Query<&mut Node, (With<DetailEmptyState>, Without<DetailCard>)>,
    mut name_text: Query<&mut Text, (With<DetailName>, Without<DetailSubtitle>)>,
    mut subtitle: Query<&mut Text, (With<DetailSubtitle>, Without<DetailName>)>,
    mut stats: Query<(&DetailStat, &mut Text), (Without<DetailName>, Without<DetailSubtitle>)>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
) {
    let mut _ui_scope = ui_perf.scope("sync_detail_panel");
    let record = selected.0.and_then(|id| people.find_by_id(id));

    let show_card = record.is_some();
    for mut node in card.iter_mut() {
        let display = if show_card {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in empty.iter_mut() {
        let display = if show_card {
            Display::None
        } else {
            Display::Flex
        };
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
            DetailField::Attributes => record.attributes.map_or_else(
                || "No recent reading".to_string(),
                |attributes| {
                    format!(
                        "Physique {} / Intelligence {} / Charm {} (max {})",
                        attributes.physique(),
                        attributes.intelligence(),
                        attributes.charm(),
                        shared::components::CharacterAttributes::MAX,
                    )
                },
            ),
            DetailField::Health => record.health.as_ref().map_or_else(
                || "No recent reading".to_string(),
                |health| {
                    if record.alive {
                        format!("{:.0} / {:.0}", health.current, health.max)
                    } else {
                        format!(
                            "DEAD / {} / day {}",
                            record.death_cause.map_or("Unknown", |cause| cause.label()),
                            record.death_day.unwrap_or(0),
                        )
                    }
                },
            ),
            DetailField::Home => {
                if !record.known {
                    "Unrecorded".to_string()
                } else if let Some(home) = &record.home {
                    home.clone()
                } else if let Some(place) = &record.residence {
                    format!("Unhoused in {place}")
                } else {
                    "No settled home".to_string()
                }
            }
            DetailField::Work => {
                if !record.known {
                    "Unrecorded".to_string()
                } else if let Some(workplace) = &record.workplace {
                    match &record.occupation {
                        Some(job) => format!("{job} / {workplace}"),
                        None => workplace.clone(),
                    }
                } else {
                    record
                        .occupation
                        .clone()
                        .unwrap_or_else(|| "Unemployed".to_string())
                }
            }
            DetailField::Employment => {
                let status = record
                    .work_status
                    .map(|status| status.label())
                    .unwrap_or("Not assessed");
                let wage = record.daily_wage.map_or_else(
                    || "no recorded wage".to_string(),
                    |wage| format!("{} coin/day", shared::economy::format_money(wage)),
                );
                let requirements = record.workforce_requirements.map_or_else(
                    || "open to all skill levels".to_string(),
                    |requirements| {
                        format!(
                            "requires P{} I{} C{}",
                            requirements.minimum_physique.min(100),
                            requirements.minimum_intelligence.min(100),
                            requirements.minimum_charm.min(100),
                        )
                    },
                );
                format!("{status} / {wage} / {requirements}")
            }
            DetailField::Hunger => match record.nutrition {
                Some(nutrition) if nutrition.is_hungry() => format!(
                    "{} / missed {} consecutive meal{} / Health ceiling {}%",
                    nutrition.condition().label(),
                    nutrition.consecutive_missed_meals,
                    if nutrition.consecutive_missed_meals == 1 {
                        ""
                    } else {
                        "s"
                    },
                    nutrition.health_ceiling_percent(),
                ),
                Some(nutrition) if nutrition.last_meal_day.is_some() => {
                    format!("Fed / last ate day {}", nutrition.last_meal_day.unwrap())
                }
                Some(_) => "Not yet assessed".to_string(),
                None => "No recent reading".to_string(),
            },
            DetailField::Wealth => record
                .wallet
                .map(|money| format!("{} coin", shared::economy::format_money(money)))
                .unwrap_or_else(|| "No recent reading".to_string()),
            DetailField::Inventory => record.inventory.as_ref().map_or_else(
                || "No recent reading".to_string(),
                |inventory| {
                    let mut goods: Vec<_> = shared::economy::Good::ALL
                        .iter()
                        .filter_map(|good| {
                            let amount = inventory.amount(*good);
                            (amount > 0).then(|| format!("{} {amount}", good.label()))
                        })
                        .collect();
                    if let Some(load) = record.carried.filter(|load| !load.is_empty()) {
                        if let Some(good) = load.good {
                            goods.push(format!("carrying {} {}", good.label(), load.amount));
                        }
                    }
                    let contents = if goods.is_empty() {
                        "empty".to_string()
                    } else {
                        goods.join(", ")
                    };
                    format!(
                        "{} / {} bulk / {contents}",
                        inventory.used_bulk(),
                        inventory.bulk_capacity(),
                    )
                },
            ),
            DetailField::Activity => record.objective.map_or_else(
                || {
                    record
                        .activity
                        .map(|activity| activity.label().to_string())
                        .unwrap_or_else(|| "Not nearby".to_string())
                },
                |objective| {
                    record
                        .navigation
                        .and_then(|navigation| navigation.label())
                        .map_or_else(
                            || objective.label().to_string(),
                            |navigation| format!("{} · {navigation}", objective.label()),
                        )
                },
            ),
            DetailField::Schedule => record
                .day_plan
                .map_or_else(|| "No current calendar".to_string(), format_day_plan),
            DetailField::Affiliation => {
                if record.known {
                    record.affiliation.label().to_string()
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
            // Only ever rendered for a hero -- see DetailField::applies_to. A
            // villager is not "away" when nobody is driving them; they live
            // here, which is a different thing entirely.
            DetailField::Status => if record.online {
                "Playing now"
            } else {
                "Logged off"
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

/// Show the banner control only with god capability, and hide detail rows that
/// say nothing true about the selected person's kind.
pub(super) fn sync_banner_controls(
    god: Res<crate::ui::hud::GodCapability>,
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    mut buttons: Query<&mut Node, With<BannerButton>>,
    mut rows: Query<(&DetailRow, &mut Node), Without<BannerButton>>,
) {
    let kind = selected
        .0
        .and_then(|id| people.find_by_id(id))
        .map(|record| record.kind);

    for mut node in buttons.iter_mut() {
        // Editable only in god mode, and only when the row it lives on is shown.
        let visible = god.0 && kind.is_some_and(|k| DetailField::Affiliation.applies_to(k));
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }

    for (DetailRow(field), mut node) in rows.iter_mut() {
        let display = match kind {
            Some(kind) if field.applies_to(kind) => Display::Flex,
            Some(_) => Display::None,
            // Nothing selected: the whole card is hidden anyway, so leave the
            // rows alone rather than flickering them.
            None => continue,
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Keep the registry's retinue column current for anyone you can see, so
/// conscripting shows immediately rather than waiting for a roster request.
pub(super) fn track_retinue_changes(
    changed: Query<
        (
            &shared::components::CharacterName,
            Option<&shared::components::CommandedBy>,
            Option<&shared::components::PersonId>,
        ),
        Changed<shared::components::CommandedBy>,
    >,
    removed: RemovedComponents<shared::components::CommandedBy>,
    mut people: ResMut<KnownPeople>,
) {
    let _ = removed;
    for (_, commanded, person_id) in changed.iter() {
        let Some(record) = people
            .records
            .iter_mut()
            .find(|record| person_id.is_some_and(|id| record.id == *id))
        else {
            continue;
        };
        let next = commanded.map(|c| c.0.clone());
        if record.commanded_by != next {
            record.commanded_by = next;
        }
    }
}

/// God-only: show CONSCRIPT or DISMISS for the selected villager.
///
/// Heroes are never offered: a hero is somebody's persisted body, and taking one
/// into a retinue would hand a player's character to another player.
pub(super) fn sync_retinue_button(
    god: Res<crate::ui::hud::GodCapability>,
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    mut buttons: Query<&mut Node, With<RetinueButton>>,
    mut labels: Query<&mut Text, With<RetinueLabel>>,
) {
    let record = selected.0.and_then(|id| people.find_by_id(id));
    let offerable = god.0 && record.is_some_and(|r| r.kind == PersonKind::Villager);
    for mut node in buttons.iter_mut() {
        let display = if offerable {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let my_account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();
    let mine = record.is_some_and(|r| r.commanded_by.as_deref() == Some(my_account.as_str()));
    let label = if mine { "DISMISS" } else { "CONSCRIPT" };
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
}
