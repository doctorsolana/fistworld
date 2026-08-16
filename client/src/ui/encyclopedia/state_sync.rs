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
    Option<&'a shared::components::CharacterActivity>,
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
                if let Some(existing) = people.records.iter_mut().find(|record| {
                    record.id == entry.id || (!record.id.is_assigned() && record.name == entry.name)
                }) {
                    existing.id = entry.id;
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
            Option<&shared::components::PersonId>,
        ),
        Added<shared::components::CharacterName>,
    >,
    mut people: ResMut<KnownPeople>,
) {
    for (name, kind, affiliation, commanded, person_id) in seen.iter() {
        let affiliation = affiliation.copied().unwrap_or_default();
        let kind = match kind {
            shared::components::CharacterKind::Hero => PersonKind::Hero,
            shared::components::CharacterKind::Villager => PersonKind::Villager,
        };
        if let Some(existing) = people.records.iter_mut().find(|record| {
            person_id.is_some_and(|id| record.id == *id)
                || (!record.id.is_assigned() && record.name == name.0)
        }) {
            if let Some(id) = person_id {
                existing.id = *id;
            }
            existing.known = true;
            existing.kind = kind;
            existing.affiliation = affiliation;
            existing.commanded_by = commanded.map(|c| c.0.clone());
        } else {
            people.records.push(PersonRecord {
                id: person_id.copied().unwrap_or_default(),
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
) {
    for (
        name,
        residence,
        occupation,
        work_status,
        wallet,
        (nutrition, health),
        activity,
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
        let employment = buildings.iter().find(|(building, building_id, _, _)| {
            employed_at.map_or_else(
                || building.workers.iter().any(|worker| worker == &name.0),
                |job| building_id.is_some_and(|id| *id == job.0),
            )
        });
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
                    shared::components::CivicRole::MarketPorter => (
                        "Market Porter (legacy)",
                        Some(shared::economy::FOUNDING_DAILY_WAGE),
                    ),
                    shared::components::CivicRole::RoadSteward => (
                        "Road Steward (legacy)",
                        Some(office.road_steward_daily_salary),
                    ),
                    shared::components::CivicRole::CityWorker => {
                        ("City Worker", Some(shared::economy::FOUNDING_DAILY_WAGE))
                    }
                    shared::components::CivicRole::Guard => {
                        ("Guard", Some(shared::economy::FOUNDING_DAILY_WAGE))
                    }
                    shared::components::CivicRole::MootSteward => {
                        ("Moot Steward", Some(office.road_steward_daily_salary))
                    }
                }
            } else if office.reeve.as_deref() == Some(name.0.as_str()) {
                ("Reeve", Some(shared::economy::FOUNDING_DAILY_WAGE))
            } else if office.road_steward.as_deref() == Some(name.0.as_str()) {
                ("Moot Steward", Some(office.road_steward_daily_salary))
            } else if office.market_porter.as_deref() == Some(name.0.as_str()) {
                (
                    "Market Porter (legacy)",
                    Some(shared::economy::FOUNDING_DAILY_WAGE),
                )
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
        let home = households
            .iter()
            .find(|(_, building_id, household)| {
                lives_at.map_or_else(
                    || {
                        household
                            .residents
                            .iter()
                            .any(|resident| resident == &name.0)
                    },
                    |home| building_id.is_some_and(|id| *id == home.0),
                )
            })
            .map(|(building, _, _)| format!("Cabin in {}", building.settlement));
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
        let next_navigation = navigation.copied();
        let next_attributes = attributes.copied();
        let next_work_status = work_status.copied();
        let next_inventory = inventory.cloned();
        let next_carried = carried.copied();

        let Some(current) = people.records.iter().find(|record| {
            person_id.is_some_and(|id| record.id == *id)
                || (!record.id.is_assigned() && record.name == name.0)
        }) else {
            continue;
        };
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
        if let Some(record) = people.records.iter_mut().find(|record| {
            person_id.is_some_and(|id| record.id == *id)
                || (!record.id.is_assigned() && record.name == name.0)
        }) {
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
    for (name, affiliation, person_id) in changed.iter() {
        if let Some(record) = people.records.iter_mut().find(|record| {
            person_id.is_some_and(|id| record.id == *id)
                || (!record.id.is_assigned() && record.name == name.0)
        }) {
            if record.affiliation != *affiliation {
                record.affiliation = *affiliation;
            }
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

fn spawn_person_row(list: &mut ChildSpawnerCommands<'_>, record: &PersonRecord) {
    let name_color = if record.known { INK } else { INK_MUTED };
    list.spawn((
        Button,
        PersonRow(record.name.clone()),
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
                font_size: FontSize::Px(9.0),
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
    for (PersonRow(name), mut style) in rows.iter_mut() {
        style.selected = selected.0.as_deref() == Some(name.as_str());
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
        .as_deref()
        .and_then(|name| people.find(name))
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
    for (name, commanded, person_id) in changed.iter() {
        if let Some(record) = people.records.iter_mut().find(|record| {
            person_id.is_some_and(|id| record.id == *id)
                || (!record.id.is_assigned() && record.name == name.0)
        }) {
            let next = commanded.map(|c| c.0.clone());
            if record.commanded_by != next {
                record.commanded_by = next;
            }
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
    let record = selected.0.as_deref().and_then(|name| people.find(name));
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
