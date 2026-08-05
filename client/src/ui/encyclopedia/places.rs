//! The PLACES tab: settlements you know of, and what they actually are.
//!
//! Deliberately shows only what the world can currently answer. A settlement
//! today knows its name, its rung, where it stands, who lives there, what has
//! been built, where physical goods are stored and which public food policy is
//! active — so those are the rows. Detailed live quotes remain in the settlement
//! panel; this long-term record retains the compact market and policy snapshot.
//!
//! The rows that ARE here are chosen to make the missing parts legible instead
//! of invisible: a hall with an empty roster reads as "a foundation, nobody
//! lives here yet", which is exactly what WORLD-DESIGN §1 says it is.

use bevy::prelude::*;

use shared::components::{
    ConstructionSite, FarmField, FishingPier, Household, MootAdministration, PlayerPosition,
    Settlement, SettlementBuilding, SettlementBuildingKind, SettlementDevelopment,
    SettlementPolicies, SettlementTier,
};
use shared::economy::{
    format_money, next_settlement_building, Good, GoodsInventory, MootMarket, SettlementEconomy,
};

use super::*;
use crate::ui::hud::GodCapability;
use crate::ui::styles::{TEXT_COLOR, TEXT_MUTED};

/// One settlement the player knows about.
#[derive(Clone, Debug)]
pub struct PlaceRecord {
    pub name: String,
    pub tier: SettlementTier,
    pub position: Vec3,
    /// How many people live there. Zero means a founded site with no life in it
    /// yet, which is a real and distinct state rather than a missing number.
    pub residents: u32,
    /// Local coin. Zero until permits cost something.
    pub treasury: u64,
    pub market: Option<MootMarket>,
    pub economy: Option<SettlementEconomy>,
    pub administration: Option<MootAdministration>,
    pub development: Option<SettlementDevelopment>,
    pub poor_relief: bool,
    pub inventory: Vec<(Good, u32)>,
    pub inventory_used: u32,
    pub inventory_capacity: u32,
    pub buildings: Vec<PlaceBuildingRecord>,
    pub wheat_fields: u32,
    pub fishing_piers: u32,
    pub permits: Vec<PlacePermitRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlaceBuildingRecord {
    pub kind: SettlementBuildingKind,
    pub position: Vec3,
    pub owner: Option<String>,
    pub quality: f32,
    pub workers: Vec<String>,
    pub residents: Vec<String>,
    pub inventory: Vec<(Good, u32)>,
    pub inventory_used: u32,
    pub inventory_capacity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacePermitRecord {
    pub kind: SettlementBuildingKind,
    pub raising: bool,
    pub delivered_wood: u32,
    pub required_wood: u32,
}

/// Every settlement the client is aware of.
///
/// Accumulated rather than derived from what is currently replicated: knowing a
/// place is permanent. Settlements replicate globally today (they are the map
/// screen, WORLD-DESIGN §7), so in practice this fills immediately — but the
/// registry is written the same way as [`KnownPeople`] so that the day
/// settlements become interest-managed, walking away does not erase them.
#[derive(Resource, Default)]
pub struct KnownPlaces {
    pub records: Vec<PlaceRecord>,
}

impl KnownPlaces {
    /// Ordered for display: biggest first, then alphabetical. A player scanning
    /// this list is looking for somewhere that matters.
    pub fn ordered(&self) -> Vec<&PlaceRecord> {
        let mut out: Vec<&PlaceRecord> = self.records.iter().collect();
        out.sort_by(|a, b| {
            (b.tier as u8)
                .cmp(&(a.tier as u8))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn find(&self, name: &str) -> Option<&PlaceRecord> {
        self.records.iter().find(|record| record.name == name)
    }
}

/// Selected row, held by NAME so it survives list rebuilds.
#[derive(Resource, Default)]
pub struct SelectedPlace(pub Option<String>);

/// Which node inside the selected settlement the explorer is showing.
///
/// The hall is derived from the settlement itself, while completed buildings
/// are stable indices in the position-sorted building snapshot.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectedPlaceEntry {
    #[default]
    Overview,
    Hall,
    Building(usize),
}

// --- markers ---------------------------------------------------------------

#[derive(Component)]
pub struct PlacesListContent;

#[derive(Component)]
pub struct PlacesListViewport;

#[derive(Component, Clone)]
pub struct PlaceRow(pub String);

#[derive(Component, Clone)]
pub struct PlaceBuildingRow {
    pub place: String,
    pub entry: SelectedPlaceEntry,
}

#[derive(Component)]
pub struct PlaceCountText;

#[derive(Component)]
pub struct PlaceDetailCard;

#[derive(Component)]
pub struct PlaceDetailEmptyState;

#[derive(Component)]
pub struct PlaceDetailName;

#[derive(Component)]
pub struct PlaceDetailSubtitle;

#[derive(Component, Clone, Copy)]
pub struct PlaceDetailLine(pub usize);

#[derive(Component, Clone, Copy)]
pub struct PlaceDetailLabel(pub usize);

#[derive(Component, Clone, Copy)]
pub struct PlaceDetailValue(pub usize);

// --- systems ---------------------------------------------------------------

/// Fold replicated settlements into the registry.
///
/// Polls every settlement rather than reacting to `Added`, because replication
/// delivers a settlement's name and position in separate batches and `Added`
/// fires once — the same trap that has now bitten characters, selection and
/// affiliation in this codebase.
pub(super) fn learn_settlements(
    seen: Query<(
        &Settlement,
        &PlayerPosition,
        Option<&GoodsInventory>,
        Option<&MootMarket>,
        Option<&SettlementEconomy>,
        Option<&MootAdministration>,
        Option<&SettlementDevelopment>,
        Option<&SettlementPolicies>,
    )>,
    buildings: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        Option<&GoodsInventory>,
        Option<&Household>,
    )>,
    fields: Query<&FarmField>,
    piers: Query<&FishingPier>,
    sites: Query<(&ConstructionSite, Option<&GoodsInventory>)>,
    mut places: ResMut<KnownPlaces>,
) {
    // Decide FIRST whether anything changed, using read-only access, and only
    // then take the mutable borrow. Touching `ResMut` marks the resource changed
    // even when every write is diff-gated, because merely calling
    // `.records.iter_mut()` is a `DerefMut`. That flagged `KnownPlaces` as
    // changed every frame, which made `rebuild_place_list` despawn and respawn
    // every row every frame, which meant a row never survived long enough for
    // its `Interaction` to reach `Pressed`. The list rendered perfectly and was
    // completely unclickable. Measured before the fix: 275 rebuilds in one short
    // capture, where the correct answer is 1.
    let snapshot = |settlement: &Settlement| {
        let mut records: Vec<PlaceBuildingRecord> = buildings
            .iter()
            .filter(|(building, _, _, _)| building.settlement == settlement.name)
            .map(
                |(building, position, inventory, household)| PlaceBuildingRecord {
                    kind: building.kind,
                    position: position.0,
                    owner: building.owner.clone(),
                    quality: building.quality,
                    workers: building.workers.clone(),
                    residents: household
                        .map(|household| household.residents.clone())
                        .unwrap_or_default(),
                    inventory: inventory_contents(inventory),
                    inventory_used: inventory_bulk(inventory).0,
                    inventory_capacity: inventory_bulk(inventory).1,
                },
            )
            .collect();
        records.sort_by(|a, b| {
            a.kind
                .label()
                .cmp(b.kind.label())
                .then_with(|| a.position.x.total_cmp(&b.position.x))
                .then_with(|| a.position.z.total_cmp(&b.position.z))
        });
        records
    };
    let permit_snapshot = |settlement: &Settlement| {
        let mut records: Vec<PlacePermitRecord> = sites
            .iter()
            .filter(|(site, _)| site.settlement == settlement.name)
            .map(|(site, inventory)| PlacePermitRecord {
                kind: site.kind,
                raising: site.raising,
                delivered_wood: inventory.map_or(0, |store| store.amount(Good::Wood)),
                required_wood: site.kind.construction_wood_required(),
            })
            .collect();
        records.sort_by_key(|permit| permit.kind.label());
        records
    };
    let needs_update = seen.iter().any(
        |(
            settlement,
            position,
            inventory,
            market,
            economy,
            administration,
            development,
            policies,
        )| {
            let building_records = snapshot(settlement);
            let permits = permit_snapshot(settlement);
            let wheat_fields = fields
                .iter()
                .filter(|field| field.settlement == settlement.name)
                .count() as u32;
            let fishing_piers = piers
                .iter()
                .filter(|pier| pier.settlement == settlement.name)
                .count() as u32;
            match places.find(&settlement.name) {
                Some(record) => {
                    record.tier != settlement.tier
                        || record.position != position.0
                        || record.residents != settlement.residents
                        || record.treasury != settlement.treasury
                        || record.inventory != inventory_contents(inventory)
                        || record.market.as_ref() != market
                        || record.economy.as_ref() != economy
                        || record.administration.as_ref() != administration
                        || record.development.as_ref() != development
                        || record.poor_relief != policies.is_some_and(|policy| policy.poor_relief)
                        || (record.inventory_used, record.inventory_capacity)
                            != inventory_bulk(inventory)
                        || record.buildings != building_records
                        || record.wheat_fields != wheat_fields
                        || record.fishing_piers != fishing_piers
                        || record.permits != permits
                }
                None => true,
            }
        },
    );
    if !needs_update {
        return;
    }

    for (settlement, position, inventory, market, economy, administration, development, policies) in
        seen.iter()
    {
        let building_records = snapshot(settlement);
        let permits = permit_snapshot(settlement);
        let wheat_fields = fields
            .iter()
            .filter(|field| field.settlement == settlement.name)
            .count() as u32;
        let fishing_piers = piers
            .iter()
            .filter(|pier| pier.settlement == settlement.name)
            .count() as u32;
        let (inventory_used, inventory_capacity) = inventory_bulk(inventory);
        let inventory = inventory_contents(inventory);
        match places
            .records
            .iter_mut()
            .find(|record| record.name == settlement.name)
        {
            Some(record) => {
                record.tier = settlement.tier;
                record.position = position.0;
                record.residents = settlement.residents;
                record.treasury = settlement.treasury;
                record.market = market.cloned();
                record.economy = economy.cloned();
                record.administration = administration.cloned();
                record.development = development.cloned();
                record.poor_relief = policies.is_some_and(|policy| policy.poor_relief);
                record.inventory = inventory;
                record.inventory_used = inventory_used;
                record.inventory_capacity = inventory_capacity;
                record.buildings = building_records;
                record.wheat_fields = wheat_fields;
                record.fishing_piers = fishing_piers;
                record.permits = permits;
            }
            None => places.records.push(PlaceRecord {
                name: settlement.name.clone(),
                tier: settlement.tier,
                position: position.0,
                residents: settlement.residents,
                treasury: settlement.treasury,
                market: market.cloned(),
                economy: economy.cloned(),
                administration: administration.cloned(),
                development: development.cloned(),
                poor_relief: policies.is_some_and(|policy| policy.poor_relief),
                inventory,
                inventory_used,
                inventory_capacity,
                buildings: building_records,
                wheat_fields,
                fishing_piers,
                permits,
            }),
        }
    }
}

fn inventory_contents(inventory: Option<&GoodsInventory>) -> Vec<(Good, u32)> {
    Good::ALL
        .into_iter()
        .filter_map(|good| {
            let amount = inventory
                .map(|inventory| inventory.amount(good))
                .unwrap_or(0);
            (amount > 0).then_some((good, amount))
        })
        .collect()
}

fn inventory_bulk(inventory: Option<&GoodsInventory>) -> (u32, u32) {
    inventory
        .map(|inventory| (inventory.used_bulk(), inventory.bulk_capacity()))
        .unwrap_or_default()
}

fn next_permit_summary(place: &PlaceRecord) -> String {
    let count = |kind: SettlementBuildingKind| {
        place
            .buildings
            .iter()
            .filter(|building| building.kind == kind)
            .count()
            + place
                .permits
                .iter()
                .filter(|permit| permit.kind == kind)
                .count()
    };
    let next = next_settlement_building(
        count(SettlementBuildingKind::Farmstead),
        count(SettlementBuildingKind::FishermansHut),
        count(SettlementBuildingKind::LumberjackHut),
        count(SettlementBuildingKind::House),
        place.residents,
        place.economy.as_ref(),
    );
    let Some(kind) = next else {
        return "No measured shortage".to_string();
    };
    let reason = match kind {
        SettlementBuildingKind::Farmstead
            if count(SettlementBuildingKind::Farmstead)
                + count(SettlementBuildingKind::FishermansHut)
                == 0 =>
        {
            "first food supply"
        }
        SettlementBuildingKind::Farmstead => "food production or reserve shortage",
        SettlementBuildingKind::LumberjackHut => "no local timber workplace",
        SettlementBuildingKind::House => "not enough assigned beds",
        SettlementBuildingKind::FishermansHut => "food supply",
        SettlementBuildingKind::Hall => "civic foundation",
        SettlementBuildingKind::Market => "village trade infrastructure",
        SettlementBuildingKind::Tavern => "town amenity",
        SettlementBuildingKind::Church => "regional civic amenity",
    };
    format!("{} / {reason}", kind.label())
}

fn permit_queue_summary(place: &PlaceRecord) -> String {
    if place.permits.is_empty() {
        return "No approved worksites".to_string();
    }
    place
        .permits
        .iter()
        .map(|permit| {
            format!(
                "{}: {} ({}/{})",
                permit.kind.label(),
                if permit.raising {
                    "raising"
                } else {
                    "supplying"
                },
                permit.delivered_wood,
                permit.required_wood
            )
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Rebuild rows when the registry changes.
pub(super) fn rebuild_place_list(
    mut commands: Commands,
    places: Res<KnownPlaces>,
    mut selected: ResMut<SelectedPlace>,
    mut selected_entry: ResMut<SelectedPlaceEntry>,
    content: Query<Entity, With<PlacesListContent>>,
    existing: Query<Entity, Or<(With<PlaceRow>, With<PlaceBuildingRow>)>>,
    mut count_text: Query<&mut Text, With<PlaceCountText>>,
    mut last: Local<Option<usize>>,
) {
    let signature = places.records.len();
    if !places.is_changed() && !selected.is_changed() && *last == Some(signature) {
        return;
    }
    *last = Some(signature);

    let Ok(content_entity) = content.single() else {
        return;
    };
    for row in existing.iter() {
        commands.entity(row).despawn();
    }

    let ordered = places.ordered();

    // Drop a selection that no longer exists.
    if let Some(name) = selected.0.clone() {
        if !ordered.iter().any(|record| record.name == name) {
            selected.0 = None;
            *selected_entry = SelectedPlaceEntry::Overview;
        } else if let SelectedPlaceEntry::Building(index) = *selected_entry {
            if places
                .find(&name)
                .is_none_or(|record| index >= record.buildings.len())
            {
                *selected_entry = SelectedPlaceEntry::Overview;
            }
        }
    }

    for mut text in count_text.iter_mut() {
        let label = match ordered.len() {
            1 => "1 place".to_string(),
            n => format!("{n} places"),
        };
        if text.0 != label {
            text.0 = label;
        }
    }

    commands.entity(content_entity).with_children(|list| {
        if ordered.is_empty() {
            list.spawn((
                // Carries the row marker so the rebuild's despawn pass cleans it
                // up; an unmarked empty state survives under a populated list.
                PlaceRow(String::new()),
                Text::new("No places known yet"),
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
        for record in &ordered {
            let expanded = selected.0.as_deref() == Some(record.name.as_str());
            spawn_place_row(list, record, expanded);
            if expanded {
                spawn_place_building_row(
                    list,
                    &record.name,
                    SelectedPlaceEntry::Hall,
                    SettlementBuildingKind::Hall.label(),
                    "COMMON STORE",
                );
                for (index, building) in record.buildings.iter().enumerate() {
                    let label = building_label(record, index);
                    let summary = building_tree_summary(building);
                    spawn_place_building_row(
                        list,
                        &record.name,
                        SelectedPlaceEntry::Building(index),
                        &label,
                        &summary,
                    );
                }
            }
        }
    });
}

fn spawn_place_row(list: &mut ChildSpawnerCommands<'_>, record: &PlaceRecord, expanded: bool) {
    list.spawn((
        Button,
        PlaceRow(record.name.clone()),
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(9.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(ROW_NORMAL),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(if expanded { "-" } else { "+" }),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                width: Val::Px(12.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        row.spawn((
            Text::new(record.name.clone()),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(TEXT_COLOR),
            Node {
                flex_grow: 1.0,
                ..default()
            },
        ));
        row.spawn((
            Text::new(format!(
                "{} / {}",
                record.tier.label(),
                record.buildings.len() + 1
            )),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
        ));
    });
}

fn spawn_place_building_row(
    list: &mut ChildSpawnerCommands<'_>,
    place: &str,
    entry: SelectedPlaceEntry,
    label: &str,
    summary: &str,
) {
    list.spawn((
        Button,
        PlaceBuildingRow {
            place: place.to_string(),
            entry,
        },
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(8.0),
            margin: UiRect::left(Val::Px(18.0)),
            padding: UiRect::new(Val::Px(14.0), Val::Px(8.0), Val::Px(6.0), Val::Px(6.0)),
            border: UiRect::left(Val::Px(1.0)),
            border_radius: BorderRadius::right(Val::Px(5.0)),
            ..default()
        },
        BorderColor::from(DIVIDER),
        BackgroundColor(ROW_NORMAL),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(TEXT_COLOR),
            Node {
                flex_grow: 1.0,
                ..default()
            },
        ));
        row.spawn((
            Text::new(summary.to_string()),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
        ));
    });
}

fn building_label(place: &PlaceRecord, index: usize) -> String {
    let building = &place.buildings[index];
    let same_kind: Vec<usize> = place
        .buildings
        .iter()
        .enumerate()
        .filter_map(|(candidate, other)| (other.kind == building.kind).then_some(candidate))
        .collect();
    if same_kind.len() <= 1 {
        return building.kind.label().to_string();
    }
    let ordinal = same_kind
        .iter()
        .position(|candidate| *candidate == index)
        .unwrap_or(0)
        + 1;
    format!("{} {ordinal}", building.kind.label())
}

fn building_tree_summary(building: &PlaceBuildingRecord) -> String {
    if building.kind.housing_capacity() > 0 {
        format!(
            "{}/{} BEDS",
            building.residents.len(),
            building.kind.housing_capacity()
        )
    } else if building.kind.positions() > 0 {
        format!(
            "{}/{} STAFF",
            building.workers.len(),
            building.kind.positions()
        )
    } else {
        "BUILDING".to_string()
    }
}

pub(super) fn handle_place_rows(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut selected: ResMut<SelectedPlace>,
    mut selected_entry: ResMut<SelectedPlaceEntry>,
    rows: Query<(&Interaction, &PlaceRow), Changed<Interaction>>,
    buildings: Query<(&Interaction, &PlaceBuildingRow), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, PlaceRow(name)) in rows.iter() {
        // The empty-state row names nowhere; clicking it must not select it.
        if *interaction == Interaction::Pressed && !name.is_empty() {
            selected.0 = Some(name.clone());
            *selected_entry = SelectedPlaceEntry::Overview;
        }
    }
    for (interaction, row) in buildings.iter() {
        if *interaction == Interaction::Pressed {
            selected.0 = Some(row.place.clone());
            *selected_entry = row.entry;
        }
    }
}

pub(super) fn style_place_rows(
    selected: Res<SelectedPlace>,
    selected_entry: Res<SelectedPlaceEntry>,
    mut rows: Query<(&PlaceRow, &Interaction, &mut BackgroundColor), Without<PlaceBuildingRow>>,
    mut buildings: Query<
        (&PlaceBuildingRow, &Interaction, &mut BackgroundColor),
        Without<PlaceRow>,
    >,
) {
    for (PlaceRow(name), interaction, mut bg) in rows.iter_mut() {
        let is_selected = selected.0.as_deref() == Some(name.as_str())
            && *selected_entry == SelectedPlaceEntry::Overview;
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
    for (row, interaction, mut bg) in buildings.iter_mut() {
        let is_selected =
            selected.0.as_deref() == Some(row.place.as_str()) && *selected_entry == row.entry;
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

#[allow(clippy::type_complexity)]
pub(super) fn sync_place_detail(
    places: Res<KnownPlaces>,
    selected: Res<SelectedPlace>,
    selected_entry: Res<SelectedPlaceEntry>,
    god: Res<GodCapability>,
    mut card: Query<
        &mut Node,
        (
            With<PlaceDetailCard>,
            Without<PlaceDetailEmptyState>,
            Without<PlaceDetailLine>,
        ),
    >,
    mut empty: Query<
        &mut Node,
        (
            With<PlaceDetailEmptyState>,
            Without<PlaceDetailCard>,
            Without<PlaceDetailLine>,
        ),
    >,
    mut name_text: Query<&mut Text, (With<PlaceDetailName>, Without<PlaceDetailSubtitle>)>,
    mut subtitle: Query<&mut Text, (With<PlaceDetailSubtitle>, Without<PlaceDetailName>)>,
    mut lines: Query<
        (&PlaceDetailLine, &mut Node),
        (Without<PlaceDetailCard>, Without<PlaceDetailEmptyState>),
    >,
    mut line_text: Query<
        (
            &mut Text,
            Option<&PlaceDetailLabel>,
            Option<&PlaceDetailValue>,
        ),
        (
            Or<(With<PlaceDetailLabel>, With<PlaceDetailValue>)>,
            Without<PlaceDetailName>,
            Without<PlaceDetailSubtitle>,
        ),
    >,
) {
    let record = selected.0.as_deref().and_then(|name| places.find(name));

    let show = record.is_some();
    for mut node in card.iter_mut() {
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in empty.iter_mut() {
        let display = if show { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }

    let Some(record) = record else {
        return;
    };

    let model = place_detail_model(record, *selected_entry, god.0);
    for mut text in name_text.iter_mut() {
        if text.0 != model.title {
            text.0 = model.title.clone();
        }
    }
    for mut text in subtitle.iter_mut() {
        if text.0 != model.subtitle {
            text.0 = model.subtitle.clone();
        }
    }
    for (PlaceDetailLine(index), mut node) in lines.iter_mut() {
        let display = if *index < model.rows.len() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (mut text, label, value) in line_text.iter_mut() {
        let index = label
            .map(|part| part.0)
            .or_else(|| value.map(|part| part.0));
        let Some((row_label, row_value)) = index.and_then(|index| model.rows.get(index)) else {
            continue;
        };
        let next = if label.is_some() {
            row_label
        } else {
            row_value
        };
        if text.0 != *next {
            text.0 = next.clone();
        }
    }
}

struct PlaceDetailModel {
    title: String,
    subtitle: String,
    rows: Vec<(String, String)>,
}

fn place_detail_model(
    place: &PlaceRecord,
    entry: SelectedPlaceEntry,
    exact_location: bool,
) -> PlaceDetailModel {
    let location = |position: Vec3| {
        if exact_location {
            format!("{:.0}, {:.0}", position.x, position.z)
        } else {
            compass_bearing(position)
        }
    };
    match entry {
        SelectedPlaceEntry::Overview => {
            let status = if place.residents == 0 {
                "FOUNDATION"
            } else {
                place.tier.label()
            };
            let residents = match place.residents {
                0 => "Nobody yet -- a hall, not a village".to_string(),
                1 => "1 person".to_string(),
                count => format!("{count} people"),
            };
            let structures = format!(
                "{} completed / {} wheat field{} / {} fishing pier{}",
                place.buildings.len() + 1,
                place.wheat_fields,
                if place.wheat_fields == 1 { "" } else { "s" },
                place.fishing_piers,
                if place.fishing_piers == 1 { "" } else { "s" },
            );
            let advance = match place.development.as_ref() {
                Some(development) if development.required_days > 0 => format!(
                    "{} / {} of {} sustained days",
                    development.next_gate.label(),
                    development.progress_days,
                    development.required_days,
                ),
                Some(development) => development.next_gate.label().to_string(),
                None => match place.tier.next_requirement() {
                    Some(requirement) if place.residents == 0 => {
                        format!("Settlers first, then {requirement}")
                    }
                    Some(requirement) => requirement.to_string(),
                    None => "This is as large as places get".to_string(),
                },
            };
            PlaceDetailModel {
                title: place.name.clone(),
                subtitle: format!("{status} / SETTLEMENT OVERVIEW"),
                rows: vec![
                    ("RESIDENTS".into(), residents),
                    ("STRUCTURES".into(), structures),
                    (
                        "COMMON STORE".into(),
                        inventory_summary(
                            place.inventory_used,
                            place.inventory_capacity,
                            &place.inventory,
                        ),
                    ),
                    (
                        "TREASURY".into(),
                        format!("{} coin", format_money(place.treasury)),
                    ),
                    ("TO ADVANCE".into(), advance),
                    ("LOCATION".into(), location(place.position)),
                ],
            }
        }
        SelectedPlaceEntry::Hall => {
            let steward = place
                .administration
                .as_ref()
                .and_then(|office| office.road_steward.clone())
                .unwrap_or_else(|| "Vacant".to_string());
            let reeve = place
                .administration
                .as_ref()
                .and_then(|office| office.reeve.clone())
                .unwrap_or_else(|| "Vacant".to_string());
            let porter = place
                .administration
                .as_ref()
                .and_then(|office| office.market_porter.clone())
                .unwrap_or_else(|| "Vacant".to_string());
            let roads = place.administration.as_ref().map_or_else(
                || "No audit recorded".to_string(),
                |office| {
                    format!(
                        "{} roadless / {} disconnected / {} pending / day {}",
                        office.roadless_buildings,
                        office.disconnected_buildings,
                        office.pending_road_buildings,
                        office.last_road_audit_day
                    )
                },
            );
            let public_jobs = place.administration.as_ref().map_or_else(
                || "Administration starting".to_string(),
                |office| {
                    format!(
                        "Reeve {} / porter {} / steward {} / guards {}/{}",
                        office.reeve.as_deref().unwrap_or("vacant"),
                        office.market_porter.as_deref().unwrap_or("vacant"),
                        office.road_steward.as_deref().unwrap_or("vacant"),
                        office.guards.len(),
                        place.tier.public_guard_positions(),
                    )
                },
            );
            let plan = place.development.as_ref().map_or_else(
                || "Awaiting charter".to_string(),
                |development| {
                    format!(
                        "{} / {} / seed {}",
                        development.layout.label(),
                        development.center.label(),
                        development.plan_seed,
                    )
                },
            );
            let walls = place.development.as_ref().map_or_else(
                || "Not reserved".to_string(),
                |development| {
                    format!(
                        "inner {} / outer {}",
                        development.inner_wall.label(),
                        development.outer_wall.label(),
                    )
                },
            );
            let road_materials = place.development.as_ref().map_or_else(
                || "Awaiting survey".to_string(),
                |development| {
                    if development.stone_needed > 0 {
                        format!(
                            "{} dirt / {} stone / needs {} Stone",
                            development.dirt_roads,
                            development.stone_roads,
                            development.stone_needed,
                        )
                    } else {
                        format!(
                            "{} dirt / {} stone / {} Stone committed",
                            development.dirt_roads,
                            development.stone_roads,
                            development.stone_committed,
                        )
                    }
                },
            );
            let food = place.economy.as_ref().map_or_else(
                || "Awaiting first daily reading".to_string(),
                |economy| {
                    format!(
                        "{:.1} reserve days / {} unfed last meal",
                        economy.reserve_days, economy.unmet_food
                    )
                },
            );
            let (liquidity, volume) = place.market.as_ref().map_or_else(
                || ("Not operating".to_string(), "No trades".to_string()),
                |market| {
                    (
                        format!("{} coin", format_money(market.total_liquidity())),
                        format!("{} coin", format_money(market.total_volume())),
                    )
                },
            );
            PlaceDetailModel {
                title: SettlementBuildingKind::Hall.label().to_string(),
                subtitle: format!("{} / CIVIC & MARKET RECORD", place.name.to_uppercase()),
                rows: vec![
                    ("OWNER".into(), "The settlement common".into()),
                    (
                        "SERVICES".into(),
                        "Permits / market / common storage".into(),
                    ),
                    ("ROAD STEWARD".into(), steward),
                    ("REEVE".into(), reeve),
                    ("MARKET PORTER".into(), porter),
                    ("PUBLIC POSITIONS".into(), public_jobs),
                    ("ROAD AUDIT".into(), roads),
                    ("ROAD MATERIALS".into(), road_materials),
                    ("LAYOUT CHARTER".into(), plan),
                    ("DEFENCE RESERVES".into(), walls),
                    ("NEXT PERMIT".into(), next_permit_summary(place)),
                    ("APPROVED WORKS".into(), permit_queue_summary(place)),
                    (
                        "PERMIT POLICY".into(),
                        "Needed housing free / businesses pay need-priced fees".into(),
                    ),
                    (
                        "COMMON STORE".into(),
                        inventory_summary(
                            place.inventory_used,
                            place.inventory_capacity,
                            &place.inventory,
                        ),
                    ),
                    (
                        "TREASURY".into(),
                        format!("{} coin", format_money(place.treasury)),
                    ),
                    ("MARKET LIQUIDITY".into(), liquidity),
                    ("LIFETIME TRADE".into(), volume),
                    ("FOOD SECURITY".into(), food),
                    (
                        "POOR RELIEF".into(),
                        if place.poor_relief {
                            "On / sustainable surplus only"
                        } else {
                            "Off / households buy their own meals"
                        }
                        .into(),
                    ),
                    ("LOCATION".into(), location(place.position)),
                ],
            }
        }
        SelectedPlaceEntry::Building(index) => {
            let Some(building) = place.buildings.get(index) else {
                return place_detail_model(place, SelectedPlaceEntry::Overview, exact_location);
            };
            let mut rows = vec![
                (
                    "OWNER".into(),
                    building.owner.as_deref().unwrap_or("The settlement").into(),
                ),
                (
                    "PURPOSE".into(),
                    match building.kind {
                        SettlementBuildingKind::Farmstead => {
                            "Produces Wheat / 2 work positions".into()
                        }
                        SettlementBuildingKind::LumberjackHut => {
                            "Produces Wood / 1 work position".into()
                        }
                        SettlementBuildingKind::FishermansHut => {
                            "Produces Food at its fishing pier / 2 work positions".into()
                        }
                        SettlementBuildingKind::House => {
                            format!("Housing / {} beds", building.kind.housing_capacity())
                        }
                        SettlementBuildingKind::Hall => "Civic building".into(),
                        SettlementBuildingKind::Market => {
                            "Public exchange / 2 work positions".into()
                        }
                        SettlementBuildingKind::Tavern => {
                            "Food and lodging amenity / 2 work positions".into()
                        }
                        SettlementBuildingKind::Church => "Civic amenity / 1 work position".into(),
                    },
                ),
            ];
            if building.kind.housing_capacity() > 0 {
                rows.push((
                    "BEDS".into(),
                    format!(
                        "{} / {} occupied",
                        building.residents.len(),
                        building.kind.housing_capacity()
                    ),
                ));
                rows.push((
                    "HOUSEHOLD".into(),
                    if building.residents.is_empty() {
                        "Empty".into()
                    } else {
                        building.residents.join(", ")
                    },
                ));
            } else {
                rows.push((
                    "STAFFING".into(),
                    format!(
                        "{} / {} workers",
                        building.workers.len(),
                        building.kind.positions()
                    ),
                ));
                rows.push((
                    "WORKERS".into(),
                    if building.workers.is_empty() {
                        "Vacant".into()
                    } else {
                        building.workers.join(", ")
                    },
                ));
                rows.push((
                    "PLOT QUALITY".into(),
                    format!("{:.0}%", building.quality * 100.0),
                ));
            }
            rows.push((
                "STORE".into(),
                inventory_summary(
                    building.inventory_used,
                    building.inventory_capacity,
                    &building.inventory,
                ),
            ));
            rows.push(("LOCATION".into(), location(building.position)));
            PlaceDetailModel {
                title: building_label(place, index),
                subtitle: format!("{} / BUILDING RECORD", place.name.to_uppercase()),
                rows,
            }
        }
    }
}

fn inventory_summary(used: u32, capacity: u32, goods: &[(Good, u32)]) -> String {
    let contents = if goods.is_empty() {
        "empty".to_string()
    } else {
        goods
            .iter()
            .map(|(good, amount)| format!("{} {amount}", good.label()))
            .collect::<Vec<_>>()
            .join(" / ")
    };
    format!("{used} / {capacity} bulk / {contents}")
}

/// Rough compass description of a world position.
///
/// The map is centred on the origin with -Z north and +Z south (the climate
/// bands in WORLD-DESIGN pillar 2 are built on that convention), so this reads
/// the same way the world looks.
fn compass_bearing(position: Vec3) -> String {
    let ns = if position.z < -800.0 {
        "northern"
    } else if position.z > 800.0 {
        "southern"
    } else {
        "central"
    };
    let ew = if position.x < -800.0 {
        " west"
    } else if position.x > 800.0 {
        " east"
    } else {
        ""
    };
    format!("The {ns}{ew} reaches")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_ordered_biggest_first() {
        let mut places = KnownPlaces::default();
        for (name, tier) in [
            ("Ashfell", SettlementTier::Hamlet),
            ("Brackwater", SettlementTier::City),
            ("Coldmoor", SettlementTier::Hamlet),
            ("Dunreach", SettlementTier::Town),
        ] {
            places.records.push(PlaceRecord {
                name: name.to_string(),
                tier,
                position: Vec3::ZERO,
                residents: 0,
                treasury: 0,
                market: None,
                economy: None,
                administration: None,
                development: None,
                poor_relief: false,
                inventory: Vec::new(),
                inventory_used: 0,
                inventory_capacity: 0,
                buildings: Vec::new(),
                wheat_fields: 0,
                fishing_piers: 0,
                permits: Vec::new(),
            });
        }
        let names: Vec<&str> = places.ordered().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Brackwater", "Dunreach", "Ashfell", "Coldmoor"],
            "expected biggest first, then alphabetical within a rung"
        );
    }

    /// The bearing must describe the world the same way the climate does, or a
    /// place called "northern" would be in the desert.
    #[test]
    fn bearings_match_the_climate_convention() {
        assert!(compass_bearing(Vec3::new(0.0, 0.0, -2000.0)).contains("northern"));
        assert!(compass_bearing(Vec3::new(0.0, 0.0, 2000.0)).contains("southern"));
        assert!(compass_bearing(Vec3::ZERO).contains("central"));
        assert!(compass_bearing(Vec3::new(-2000.0, 0.0, 0.0)).contains("west"));
        assert!(compass_bearing(Vec3::new(2000.0, 0.0, 0.0)).contains("east"));
    }

    fn explorer_place() -> PlaceRecord {
        PlaceRecord {
            name: "Brackwater".into(),
            tier: SettlementTier::Village,
            position: Vec3::new(120.0, 0.0, -80.0),
            residents: 7,
            treasury: 0,
            market: None,
            economy: None,
            administration: None,
            development: None,
            poor_relief: false,
            inventory: vec![(Good::Wood, 3)],
            inventory_used: 12,
            inventory_capacity: 200,
            buildings: vec![PlaceBuildingRecord {
                kind: SettlementBuildingKind::Farmstead,
                position: Vec3::new(145.0, 0.0, -100.0),
                owner: Some("Ada".into()),
                quality: 0.76,
                workers: vec!["Ada".into()],
                residents: Vec::new(),
                inventory: vec![(Good::Wheat, 4)],
                inventory_used: 8,
                inventory_capacity: 80,
            }],
            wheat_fields: 1,
            fishing_piers: 0,
            permits: Vec::new(),
        }
    }

    #[test]
    fn place_overview_summarizes_structures_instead_of_clumping_building_records() {
        let model = place_detail_model(&explorer_place(), SelectedPlaceEntry::Overview, true);
        assert_eq!(model.title, "Brackwater");
        assert_eq!(model.subtitle, "VILLAGE / SETTLEMENT OVERVIEW");
        let structures = model
            .rows
            .iter()
            .find(|(label, _)| label == "STRUCTURES")
            .map(|(_, value)| value.as_str());
        assert_eq!(
            structures,
            Some("2 completed / 1 wheat field / 0 fishing piers")
        );
        assert!(model.rows.iter().all(|(_, value)| !value.contains("Ada")));
    }

    #[test]
    fn selecting_a_building_produces_its_own_detail_sheet() {
        let model = place_detail_model(&explorer_place(), SelectedPlaceEntry::Building(0), true);
        assert_eq!(model.title, "FARMSTEAD");
        assert_eq!(model.subtitle, "BRACKWATER / BUILDING RECORD");
        assert!(model
            .rows
            .iter()
            .any(|(label, value)| label == "OWNER" && value == "Ada"));
        assert!(model
            .rows
            .iter()
            .any(|(label, value)| label == "PLOT QUALITY" && value == "76%"));
        assert!(model
            .rows
            .iter()
            .any(|(label, value)| label == "STORE" && value.contains("Wheat 4")));
    }
}
