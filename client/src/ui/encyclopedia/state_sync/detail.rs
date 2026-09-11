//! Scalar and decorative bindings for the People spread.
use super::*;
use crate::ui::encyclopedia::layout::people::{
    DetailAttribute, DetailHealthFill, DetailInventoryGoods, DetailPortrait, DetailPrivateNote,
    DetailSection,
};

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

#[derive(Default)]
pub(in crate::ui::encyclopedia) struct DetailPresence {
    person: Option<shared::components::PersonId>,
    checked_at: Option<f32>,
    present: bool,
}

pub(in crate::ui::encyclopedia) fn sync_detail_panel(
    fresh: Query<(), Added<DetailCard>>,
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    positioned: Query<&shared::components::PersonId, With<shared::components::PlayerPosition>>,
    time: Res<Time>,
    mut presence: Local<DetailPresence>,
    mut card: Query<&mut Node, (With<DetailCard>, Without<DetailEmptyState>)>,
    mut empty: Query<&mut Node, (With<DetailEmptyState>, Without<DetailCard>)>,
    mut name_text: Query<&mut Text, (With<DetailName>, Without<DetailSubtitle>)>,
    mut subtitle: Query<&mut Text, (With<DetailSubtitle>, Without<DetailName>)>,
    mut stats: Query<(&DetailStat, &mut Text), (Without<DetailName>, Without<DetailSubtitle>)>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
) {
    let mut _ui_scope = ui_perf.scope("sync_detail_panel");
    let previous_presence = (presence.person, presence.present);
    let now = time.elapsed_secs();
    if presence.person != selected.0
        || presence
            .checked_at
            .is_none_or(|last| now - last >= PERSON_FACTS_INTERVAL_SECS)
    {
        presence.person = selected.0;
        presence.checked_at = Some(now);
        presence.present = selected
            .0
            .is_some_and(|id| positioned.iter().any(|person| *person == id));
    }
    if fresh.is_empty()
        && !people.is_changed()
        && !selected.is_changed()
        && !account.as_ref().is_some_and(|input| input.is_changed())
        && previous_presence == (presence.person, presence.present)
    {
        return;
    }
    let record = selected.0.and_then(|id| people.find_by_id(id));
    let account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();

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
        let role = record.occupation.as_deref().unwrap_or(record.kind.label());
        let value = if record.is_self {
            format!(
                "{} · Your hero",
                record.residence.as_deref().unwrap_or("Adventurer")
            )
        } else if let Some(place) = record.residence.as_deref() {
            format!("{role} · {place}")
        } else {
            role.to_string()
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (DetailStat(field), mut text) in stats.iter_mut() {
        let value = if matches!(field, DetailField::Wealth | DetailField::Inventory)
            && !can_read_possessions(record, &account)
        {
            "Private".to_string()
        } else {
            match field {
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
                        format!(
                            "{} / {} bulk",
                            inventory.used_bulk(),
                            inventory.bulk_capacity()
                        )
                    },
                ),
                DetailField::Activity => activity_description(record, presence.present),
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
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

/// Show the banner control only with god capability, and hide detail rows that
/// say nothing true about the selected person's kind.
pub(in crate::ui::encyclopedia) fn sync_banner_controls(
    god: Res<crate::ui::hud::GodCapability>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    mut buttons: Query<&mut Node, With<BannerButton>>,
    mut rows: Query<(&DetailRow, &mut Node), Without<BannerButton>>,
) {
    let record = selected.0.and_then(|id| people.find_by_id(id));
    let kind = record.map(|record| record.kind);
    let account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();
    let private = record.is_some_and(|record| can_read_possessions(record, &account));

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
            Some(kind)
                if field.applies_to(kind)
                    && (!matches!(field, DetailField::Wealth | DetailField::Inventory)
                        || private) =>
            {
                Display::Flex
            }
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

/// Art updates never rebuild records or reset either scroll position.
pub(in crate::ui::encyclopedia) fn sync_detail_art(
    fresh: Query<(), Added<DetailCard>>,
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    mut portraits: Query<&mut crate::ui::portraits::PersonPortrait, With<DetailPortrait>>,
    mut fills: Query<
        &mut Node,
        (
            With<DetailHealthFill>,
            Without<DetailSection>,
            Without<DetailPrivateNote>,
        ),
    >,
    mut attributes: Query<(&DetailAttribute, &mut Text)>,
    mut sections: Query<
        (&DetailSection, &mut Node),
        (Without<DetailHealthFill>, Without<DetailPrivateNote>),
    >,
    mut private_notes: Query<
        &mut Node,
        (
            With<DetailPrivateNote>,
            Without<DetailHealthFill>,
            Without<DetailSection>,
        ),
    >,
) {
    if fresh.is_empty()
        && !people.is_changed()
        && !selected.is_changed()
        && !account.as_ref().is_some_and(|input| input.is_changed())
    {
        return;
    }
    let Some(record) = selected.0.and_then(|id| people.find_by_id(id)) else {
        return;
    };
    for mut portrait in &mut portraits {
        if portrait.0 != record.id {
            portrait.0 = record.id;
        }
    }
    let percent = record.health.as_ref().map_or(0.0, |health| {
        if health.max > 0.0 {
            (100.0 * health.current / health.max).clamp(0.0, 100.0)
        } else {
            0.0
        }
    });
    for mut node in &mut fills {
        let width = Val::Percent(percent);
        if node.width != width {
            node.width = width;
        }
    }
    for (DetailAttribute(index), mut text) in &mut attributes {
        let value = record
            .attributes
            .map(|attributes| {
                [
                    attributes.physique(),
                    attributes.intelligence(),
                    attributes.charm(),
                ][*index]
                    .to_string()
            })
            .unwrap_or_else(|| "—".to_string());
        if text.0 != value {
            text.0 = value;
        }
    }
    for (DetailSection(field), mut node) in &mut sections {
        let display = if field.applies_to(record.kind) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();
    let display = if can_read_possessions(record, &account) {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut private_notes {
        if node.display != display {
            node.display = display;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::encyclopedia) struct InventoryProjection {
    person: shared::components::PersonId,
    availability: InventoryAvailability,
    goods: Vec<(shared::economy::Good, u32)>,
    carrying: Option<shared::economy::Good>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InventoryAvailability {
    Private,
    Unrecorded,
    Known,
}

impl InventoryProjection {
    fn new(
        person: shared::components::PersonId,
        inventory: Option<&shared::economy::GoodsInventory>,
        carrying: Option<shared::economy::Good>,
        permitted: bool,
    ) -> Self {
        let availability = match (permitted, inventory.is_some()) {
            (false, _) => InventoryAvailability::Private,
            (true, false) => InventoryAvailability::Unrecorded,
            (true, true) => InventoryAvailability::Known,
        };
        Self {
            person,
            availability,
            goods: inventory
                .filter(|_| permitted)
                .map(|inventory| {
                    shared::economy::Good::ALL
                        .iter()
                        .filter_map(|good| {
                            let amount = inventory.amount(*good);
                            (amount > 0).then_some((*good, amount))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            carrying: carrying.filter(|_| permitted),
        }
    }

    fn empty_label(&self) -> &'static str {
        match self.availability {
            InventoryAvailability::Private => "Private",
            InventoryAvailability::Unrecorded => "No recent reading",
            InventoryAvailability::Known => "Empty",
        }
    }
}

/// Small inventory pictures reuse existing goods assets. Only a goods/identity
/// change replaces these few children; wallets and ongoing activities do not.
pub(in crate::ui::encyclopedia) fn sync_detail_inventory(
    mut commands: Commands,
    assets: Res<AssetServer>,
    people: Res<KnownPeople>,
    selected: Res<SelectedPerson>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    hosts: Query<(Entity, Option<&Children>), With<DetailInventoryGoods>>,
    mut last: Local<Option<InventoryProjection>>,
) {
    let Ok((host, children)) = hosts.single() else {
        return;
    };
    let Some(record) = selected.0.and_then(|id| people.find_by_id(id)) else {
        return;
    };
    if children.is_some()
        && !people.is_changed()
        && !selected.is_changed()
        && !account.as_ref().is_some_and(|input| input.is_changed())
    {
        return;
    }
    let account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();
    let permitted = can_read_possessions(record, &account);
    let next = InventoryProjection::new(
        record.id,
        record.inventory.as_ref(),
        record.carried.and_then(|load| load.good),
        permitted,
    );
    if children.is_some() && last.as_ref() == Some(&next) {
        return;
    }
    commands.entity(host).despawn_children();
    commands.entity(host).with_children(|row| {
        if next.goods.is_empty() {
            row.spawn(crate::ui::ledger::body(next.empty_label(), 16.0));
        }
        for (good, amount) in &next.goods {
            row.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|item| {
                item.spawn((
                    ImageNode::new(assets.load(crate::ui::good_icon_path(*good))),
                    Node {
                        width: Val::Px(28.0),
                        height: Val::Px(28.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                item.spawn(crate::ui::ledger::body(
                    format!("{} ×{amount}", good.label()),
                    16.0,
                ));
            });
        }
        if let Some(good) = next.carrying {
            row.spawn(crate::ui::ledger::body(
                format!("Carrying {}", good.label()),
                16.0,
            ));
        }
    });
    *last = Some(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{
        components::PersonId,
        economy::{Good, GoodsInventory},
    };

    #[test]
    fn inventory_access_and_readiness_changes_invalidate_empty_presentation() {
        let inventory = GoodsInventory::new(20);
        let private = InventoryProjection::new(PersonId(2), None, None, false);
        let awaiting = InventoryProjection::new(PersonId(2), None, None, true);
        let empty = InventoryProjection::new(PersonId(2), Some(&inventory), None, true);
        assert_ne!(
            private, awaiting,
            "gaining command must replace the private label"
        );
        assert_ne!(
            awaiting, empty,
            "replication arriving must replace missing information"
        );
        assert_eq!(private.empty_label(), "Private");
        assert_eq!(awaiting.empty_label(), "No recent reading");
        assert_eq!(empty.empty_label(), "Empty");
    }

    #[test]
    fn revoked_inventory_access_drops_goods_even_before_component_removal() {
        let mut inventory = GoodsInventory::new(20);
        assert_eq!(inventory.add(Good::Wheat, 3), 3);
        let owned =
            InventoryProjection::new(PersonId(2), Some(&inventory), Some(Good::Wheat), true);
        let private =
            InventoryProjection::new(PersonId(2), Some(&inventory), Some(Good::Wheat), false);
        assert_ne!(owned, private);
        assert_eq!(owned.goods, vec![(Good::Wheat, 3)]);
        assert!(private.goods.is_empty());
        assert!(private.carrying.is_none());
        assert_eq!(private.empty_label(), "Private");
    }
}
