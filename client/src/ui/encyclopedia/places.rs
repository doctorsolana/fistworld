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
    CivicHallLevel, CompanyId, ConstructionSite, FarmField, FishingPier, Household,
    MootAdministration, OperatedBy, PlayerPosition, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementDevelopment, SettlementOpportunityBoard, SettlementPolicies,
    SettlementTier,
};
use shared::economy::{
    business_working_capital, format_money, BusinessAccount, BusinessCondition, BusinessForSale,
    BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy,
    BusinessStaffingPolicy, BusinessWagePolicy, Good, GoodsInventory, MootMarket,
    SettlementEconomy,
};

use super::*;
use crate::ui::foundation::{button_chrome, UiButtonStyle, UiButtonVariant};
use crate::ui::hud::GodCapability;
use crate::ui::styles::{INK, INK_MUTED};

/// One settlement the player knows about.
#[derive(Clone, Debug)]
pub struct PlaceRecord {
    pub id: shared::components::SettlementId,
    pub name: String,
    pub tier: SettlementTier,
    /// Physical civic building, separate from the unlocked settlement tier so
    /// future paid construction may lag behind promotion.
    pub hall_level: CivicHallLevel,
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
    pub policies: Option<SettlementPolicies>,
    pub opportunities: Option<SettlementOpportunityBoard>,
    pub inventory: Vec<(Good, u32)>,
    pub inventory_used: u32,
    pub inventory_capacity: u32,
    pub buildings: Vec<PlaceBuildingRecord>,
    /// Counts from the global lightweight directory. Detailed records replace
    /// these when the settlement is inside the client's interest area.
    pub summary_buildings: [u16; 6],
    pub wheat_fields: u32,
    pub fishing_piers: u32,
    pub permits: Vec<PlacePermitRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlaceBuildingRecord {
    pub id: Option<shared::components::BuildingId>,
    pub kind: SettlementBuildingKind,
    pub position: Vec3,
    pub owner: Option<String>,
    pub for_sale: Option<BusinessForSale>,
    pub quality: f32,
    pub workers: Vec<String>,
    pub residents: Vec<String>,
    pub inventory: Vec<(Good, u32)>,
    pub inventory_used: u32,
    pub inventory_capacity: u32,
    pub business: Option<PlaceBusinessRecord>,
}

/// Last-known live operating record for one private workplace. Historical
/// archives stay pull-based; this is only the current state already replicated
/// for nearby building inspection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceBusinessRecord {
    pub account: BusinessAccount,
    pub company: Option<shared::economy::CompanyAccount>,
    pub company_id: Option<CompanyId>,
    pub sale: Option<BusinessSalePolicy>,
    pub wage: Option<BusinessWagePolicy>,
    pub staffing: Option<BusinessStaffingPolicy>,
    pub management: Option<BusinessManagementPolicy>,
    pub procurement: Option<BusinessProcurementPolicy>,
    pub condition: Option<BusinessCondition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacePermitRecord {
    pub kind: SettlementBuildingKind,
    pub raising: bool,
    pub delivered_wood: u32,
    pub required_wood: u32,
    pub for_sale: Option<BusinessForSale>,
}

/// Every settlement the client is aware of.
///
/// Accumulated rather than derived from what is currently replicated: knowing a
/// place is permanent. Lightweight summaries arrive globally; full settlement
/// detail is interest-managed. The registry merges both, so walking away drops
/// live detail without erasing what the player learned.
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

pub(super) fn learn_settlement_summaries(
    summaries: Query<(&shared::components::SettlementSummary, &PlayerPosition)>,
    mut places: ResMut<KnownPlaces>,
) {
    for (summary, position) in summaries.iter() {
        let existing = places.records.iter().position(|record| {
            record.id == summary.id || (!record.id.is_assigned() && record.name == summary.name)
        });
        let counts = [
            summary.houses,
            summary.farmsteads,
            summary.fishing_huts,
            summary.lumber_huts,
            summary.windmills,
            summary.bakeries,
        ];
        let unchanged = existing.is_some_and(|index| {
            let record = &places.records[index];
            record.tier == summary.tier
                && record.hall_level == CivicHallLevel::for_tier(summary.tier)
                && record.position == position.0
                && record.residents == summary.residents
                && record.treasury == summary.treasury
                && record.summary_buildings == counts
        });
        if unchanged {
            continue;
        }
        if let Some(index) = existing {
            let record = &mut places.records[index];
            record.tier = summary.tier;
            record.hall_level = CivicHallLevel::for_tier(summary.tier);
            record.position = position.0;
            record.residents = summary.residents;
            record.treasury = summary.treasury;
            record.summary_buildings = counts;
        } else {
            places.records.push(PlaceRecord {
                id: summary.id,
                name: summary.name.clone(),
                tier: summary.tier,
                hall_level: CivicHallLevel::for_tier(summary.tier),
                position: position.0,
                residents: summary.residents,
                treasury: summary.treasury,
                market: None,
                economy: None,
                administration: None,
                development: None,
                policies: None,
                opportunities: None,
                inventory: Vec::new(),
                inventory_used: 0,
                inventory_capacity: 0,
                buildings: Vec::new(),
                summary_buildings: counts,
                wheat_fields: 0,
                fishing_piers: 0,
                permits: Vec::new(),
            });
        }
    }
}

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

/// Static action slot in the Places detail card. It becomes a real
/// `BusinessHistoryButton` only while a stable private business is selected.
#[derive(Component)]
pub struct PlaceBusinessHistoryAction;
#[derive(Component)]
pub struct PlaceBackToCompanyAction;

// --- systems ---------------------------------------------------------------

/// Fold replicated settlements into the registry.
///
/// Polls every settlement rather than reacting to `Added`, because replication
/// delivers a settlement's name and position in separate batches and `Added`
/// fires once — the same trap that has now bitten characters, selection and
/// affiliation in this codebase.
#[allow(clippy::type_complexity)]
pub(super) fn learn_settlements(
    seen: Query<(
        &Settlement,
        Option<&CivicHallLevel>,
        Option<&shared::components::SettlementId>,
        &PlayerPosition,
        Option<&GoodsInventory>,
        Option<&MootMarket>,
        Option<&SettlementEconomy>,
        Option<&MootAdministration>,
        Option<&SettlementDevelopment>,
        Option<&SettlementPolicies>,
        Option<&SettlementOpportunityBoard>,
    )>,
    buildings: Query<(
        &SettlementBuilding,
        Option<&shared::components::BuildingOf>,
        Option<&shared::components::BuildingId>,
        &PlayerPosition,
        Option<&GoodsInventory>,
        Option<&Household>,
        Option<&BusinessAccount>,
        Option<&BusinessSalePolicy>,
        Option<&BusinessWagePolicy>,
        Option<&BusinessStaffingPolicy>,
        Option<&BusinessManagementPolicy>,
        Option<&BusinessProcurementPolicy>,
        Option<&BusinessCondition>,
        Option<&BusinessForSale>,
        Option<&OperatedBy>,
    )>,
    companies: Query<(&CompanyId, &shared::economy::CompanyAccount)>,
    fields: Query<&FarmField>,
    piers: Query<&FishingPier>,
    sites: Query<(
        &ConstructionSite,
        Option<&shared::components::BuildingOf>,
        Option<&GoodsInventory>,
        Option<&BusinessForSale>,
    )>,
    mut places: ResMut<KnownPlaces>,
) {
    let company_accounts: std::collections::HashMap<CompanyId, shared::economy::CompanyAccount> =
        companies
            .iter()
            .map(|(id, account)| (*id, *account))
            .collect();
    // Decide FIRST whether anything changed, using read-only access, and only
    // then take the mutable borrow. Touching `ResMut` marks the resource changed
    // even when every write is diff-gated, because merely calling
    // `.records.iter_mut()` is a `DerefMut`. That flagged `KnownPlaces` as
    // changed every frame, which made `rebuild_place_list` despawn and respawn
    // every row every frame, which meant a row never survived long enough for
    // its `Interaction` to reach `Pressed`. The list rendered perfectly and was
    // completely unclickable. Measured before the fix: 275 rebuilds in one short
    // capture, where the correct answer is 1.
    let snapshot = |settlement: &Settlement,
                    settlement_id: Option<&shared::components::SettlementId>,
                    public_inventory: Option<&GoodsInventory>| {
        let mut records: Vec<PlaceBuildingRecord> = buildings
            .iter()
            .filter(|(building, owner, ..)| {
                settlement_id.map_or_else(
                    || building.settlement == settlement.name,
                    |id| owner.is_some_and(|owner| owner.0 == *id),
                )
            })
            .map(
                |(
                    building,
                    _,
                    building_id,
                    position,
                    inventory,
                    household,
                    account,
                    sale,
                    wage,
                    staffing,
                    management,
                    procurement,
                    condition,
                    for_sale,
                    operated_by,
                )| {
                    let inventory = if building.kind == SettlementBuildingKind::Market {
                        public_inventory
                    } else {
                        inventory
                    };
                    PlaceBuildingRecord {
                        id: building_id.copied(),
                        kind: building.kind,
                        position: position.0,
                        owner: building.owner.clone(),
                        for_sale: for_sale.copied(),
                        quality: building.quality,
                        workers: building.workers.clone(),
                        residents: household
                            .map(|household| household.residents.clone())
                            .unwrap_or_default(),
                        inventory: inventory_contents(inventory),
                        inventory_used: inventory_bulk(inventory).0,
                        inventory_capacity: inventory_bulk(inventory).1,
                        business: account.map(|account| PlaceBusinessRecord {
                            account: *account,
                            company: operated_by
                                .and_then(|company| company_accounts.get(&company.0))
                                .copied(),
                            company_id: operated_by.map(|company| company.0),
                            sale: sale.copied(),
                            wage: wage.copied(),
                            staffing: staffing.copied(),
                            management: management.copied(),
                            procurement: procurement.copied(),
                            condition: condition.copied(),
                        }),
                    }
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
    let permit_snapshot =
        |settlement: &Settlement, settlement_id: Option<&shared::components::SettlementId>| {
            let mut records: Vec<PlacePermitRecord> = sites
                .iter()
                .filter(|(site, owner, _, _)| {
                    settlement_id.map_or_else(
                        || site.settlement == settlement.name,
                        |id| owner.is_some_and(|owner| owner.0 == *id),
                    )
                })
                .map(|(site, _, inventory, for_sale)| PlacePermitRecord {
                    kind: site.kind,
                    raising: site.raising,
                    delivered_wood: inventory.map_or(0, |store| store.amount(Good::Wood)),
                    required_wood: site.kind.construction_wood_required(),
                    for_sale: for_sale.copied(),
                })
                .collect();
            records.sort_by_key(|permit| permit.kind.label());
            records
        };
    let needs_update = seen.iter().any(
        |(
            settlement,
            hall_level,
            settlement_id,
            position,
            inventory,
            market,
            economy,
            administration,
            development,
            policies,
            opportunities,
        )| {
            let building_records = snapshot(settlement, settlement_id, inventory);
            let permits = permit_snapshot(settlement, settlement_id);
            let wheat_fields = fields
                .iter()
                .filter(|field| field.settlement == settlement.name)
                .count() as u32;
            let fishing_piers = piers
                .iter()
                .filter(|pier| pier.settlement == settlement.name)
                .count() as u32;
            let record = places.records.iter().find(|record| {
                settlement_id.map_or_else(
                    || record.name == settlement.name,
                    |id| {
                        record.id == *id
                            || (!record.id.is_assigned() && record.name == settlement.name)
                    },
                )
            });
            match record {
                Some(record) => {
                    record.tier != settlement.tier
                        || record.hall_level
                            != hall_level
                                .copied()
                                .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier))
                        || record.position != position.0
                        || record.residents != settlement.residents
                        || record.treasury != settlement.treasury
                        || record.inventory != inventory_contents(inventory)
                        || record.market.as_ref() != market
                        || record.economy.as_ref() != economy
                        || record.administration.as_ref() != administration
                        || record.development.as_ref() != development
                        || record.policies.as_ref() != policies
                        || record.opportunities.as_ref() != opportunities
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

    for (
        settlement,
        hall_level,
        settlement_id,
        position,
        inventory,
        market,
        economy,
        administration,
        development,
        policies,
        opportunities,
    ) in seen.iter()
    {
        let hall_level = hall_level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let building_records = snapshot(settlement, settlement_id, inventory);
        let summary_buildings = summarize_buildings(&building_records);
        let permits = permit_snapshot(settlement, settlement_id);
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
        match places.records.iter_mut().find(|record| {
            settlement_id.map_or_else(
                || record.name == settlement.name,
                |id| {
                    record.id == *id || (!record.id.is_assigned() && record.name == settlement.name)
                },
            )
        }) {
            Some(record) => {
                if let Some(id) = settlement_id {
                    record.id = *id;
                }
                record.tier = settlement.tier;
                record.hall_level = hall_level;
                record.position = position.0;
                record.residents = settlement.residents;
                record.treasury = settlement.treasury;
                record.market = market.cloned();
                record.economy = economy.cloned();
                record.administration = administration.cloned();
                record.development = development.cloned();
                record.policies = policies.copied();
                record.opportunities = opportunities.cloned();
                record.inventory = inventory;
                record.inventory_used = inventory_used;
                record.inventory_capacity = inventory_capacity;
                record.buildings = building_records;
                record.summary_buildings = summary_buildings;
                record.wheat_fields = wheat_fields;
                record.fishing_piers = fishing_piers;
                record.permits = permits;
            }
            None => places.records.push(PlaceRecord {
                id: settlement_id.copied().unwrap_or_default(),
                name: settlement.name.clone(),
                tier: settlement.tier,
                hall_level,
                position: position.0,
                residents: settlement.residents,
                treasury: settlement.treasury,
                market: market.cloned(),
                economy: economy.cloned(),
                administration: administration.cloned(),
                development: development.cloned(),
                policies: policies.copied(),
                opportunities: opportunities.cloned(),
                inventory,
                inventory_used,
                inventory_capacity,
                buildings: building_records,
                summary_buildings,
                wheat_fields,
                fishing_piers,
                permits,
            }),
        }
    }
}

fn summarize_buildings(records: &[PlaceBuildingRecord]) -> [u16; 6] {
    let count = |kind| {
        records
            .iter()
            .filter(|building| building.kind == kind)
            .count()
            .min(u16::MAX as usize) as u16
    };
    [
        count(SettlementBuildingKind::House),
        count(SettlementBuildingKind::Farmstead),
        count(SettlementBuildingKind::FishermansHut),
        count(SettlementBuildingKind::LumberjackHut),
        count(SettlementBuildingKind::Windmill),
        count(SettlementBuildingKind::Bakery),
    ]
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
    let Some(board) = place
        .opportunities
        .as_ref()
        .filter(|board| !board.opportunities.is_empty())
    else {
        return "No active opportunity signals".to_string();
    };
    board
        .opportunities
        .iter()
        .take(4)
        .map(|opportunity| {
            format!(
                "{}: {} signal / {}",
                opportunity.kind.label(),
                opportunity.score,
                if opportunity.subsidized {
                    "discounted permit"
                } else {
                    "full-price permit"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

fn permit_queue_summary(place: &PlaceRecord) -> String {
    if place.permits.is_empty() {
        return "No approved worksites".to_string();
    }
    place
        .permits
        .iter()
        .map(|permit| {
            let state = format!(
                "{}: {} ({}/{})",
                permit.kind.label(),
                if permit.raising {
                    "raising"
                } else {
                    "supplying"
                },
                permit.delivered_wood,
                permit.required_wood
            );
            permit.for_sale.map_or(state.clone(), |listing| {
                format!(
                    "{state}, FOR SALE {} coin / {}",
                    format_money(listing.asking_price),
                    listing.reason.label(),
                )
            })
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
                TextColor(INK_MUTED),
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
                    record.hall_level.label(),
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
        button_chrome(UiButtonVariant::Row),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(if expanded { "-" } else { "+" }),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(INK_MUTED),
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
            TextColor(INK),
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
            TextColor(INK_MUTED),
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
        button_chrome(UiButtonVariant::Row),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(INK),
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
            TextColor(INK_MUTED),
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
    mut return_to: ResMut<companies::CompanyDrilldownReturn>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, PlaceRow(name)) in rows.iter() {
        // The empty-state row names nowhere; clicking it must not select it.
        if *interaction == Interaction::Pressed && !name.is_empty() {
            return_to.0 = None;
            selected.0 = Some(name.clone());
            *selected_entry = SelectedPlaceEntry::Overview;
        }
    }
    for (interaction, row) in buildings.iter() {
        if *interaction == Interaction::Pressed {
            return_to.0 = None;
            selected.0 = Some(row.place.clone());
            *selected_entry = row.entry;
        }
    }
}

pub(super) fn handle_back_to_company(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<&Interaction, (With<PlaceBackToCompanyAction>, Changed<Interaction>)>,
    mut return_to: ResMut<companies::CompanyDrilldownReturn>,
    mut selected: ResMut<companies::SelectedCompany>,
    mut tab: ResMut<EncyclopediaTab>,
) {
    for interaction in buttons.iter() {
        if !guard.0
            || !mouse.just_pressed(MouseButton::Left)
            || *interaction != Interaction::Pressed
        {
            continue;
        }
        let Some(company) = return_to.0.take() else {
            continue;
        };
        selected.0 = Some(company);
        *tab = EncyclopediaTab::Companies;
    }
}

pub(super) fn sync_back_to_company(
    return_to: Res<companies::CompanyDrilldownReturn>,
    mut buttons: Query<&mut Node, With<PlaceBackToCompanyAction>>,
) {
    for mut node in buttons.iter_mut() {
        node.display = if return_to.0.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(super) fn style_place_rows(
    selected: Res<SelectedPlace>,
    selected_entry: Res<SelectedPlaceEntry>,
    mut rows: Query<(&PlaceRow, &mut UiButtonStyle), Without<PlaceBuildingRow>>,
    mut buildings: Query<(&PlaceBuildingRow, &mut UiButtonStyle), Without<PlaceRow>>,
) {
    for (PlaceRow(name), mut style) in rows.iter_mut() {
        style.selected = selected.0.as_deref() == Some(name.as_str())
            && *selected_entry == SelectedPlaceEntry::Overview;
    }
    for (row, mut style) in buildings.iter_mut() {
        style.selected =
            selected.0.as_deref() == Some(row.place.as_str()) && *selected_entry == row.entry;
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

/// Bind the expanded building record's action to the same pull-based business
/// history view used by the compact world card.
pub(super) fn sync_place_business_history_action(
    mut commands: Commands,
    places: Res<KnownPlaces>,
    selected: Res<SelectedPlace>,
    selected_entry: Res<SelectedPlaceEntry>,
    settlements: Query<(
        Entity,
        &Settlement,
        Option<&shared::components::SettlementId>,
    )>,
    mut buttons: Query<
        (
            Entity,
            &mut Node,
            Option<&crate::ui::history::BusinessHistoryButton>,
        ),
        With<PlaceBusinessHistoryAction>,
    >,
) {
    let target = selected
        .0
        .as_deref()
        .and_then(|name| places.find(name))
        .and_then(|place| {
            let SelectedPlaceEntry::Building(index) = *selected_entry else {
                return None;
            };
            let building = place.buildings.get(index)?;
            building.business.as_ref()?;
            let building_id = building.id.filter(|id| id.is_assigned())?;
            let settlement = settlements
                .iter()
                .find(|(_, settlement, id)| {
                    if place.id.is_assigned() {
                        id.is_some_and(|id| *id == place.id)
                    } else {
                        settlement.name == place.name
                    }
                })
                .map(|(entity, ..)| entity)?;
            Some(crate::ui::history::BusinessHistoryButton {
                settlement,
                place: place.name.clone(),
                business: building_id,
            })
        });

    for (entity, mut node, current) in buttons.iter_mut() {
        node.display = if target.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        match (&target, current) {
            (Some(target), Some(current)) if current == target => {}
            (Some(target), _) => {
                commands.entity(entity).insert(target.clone());
            }
            (None, Some(_)) => {
                commands
                    .entity(entity)
                    .remove::<crate::ui::history::BusinessHistoryButton>();
            }
            (None, None) => {}
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
            let known_building_count = place.buildings.len().max(
                place
                    .summary_buildings
                    .iter()
                    .map(|count| *count as usize)
                    .sum(),
            );
            let structures = format!(
                "{} completed / {} wheat field{} / {} fishing pier{}",
                known_building_count + 1,
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
            let moot_stewards = place.administration.as_ref().map_or_else(
                || "Vacant".to_string(),
                |office| {
                    if office.city_workers.is_empty() {
                        office
                            .road_steward
                            .clone()
                            .unwrap_or_else(|| "Vacant".to_string())
                    } else {
                        office.city_workers.join(", ")
                    }
                },
            );
            let reeve = place
                .administration
                .as_ref()
                .and_then(|office| office.reeve.clone())
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
                    let (worker_target, guard_target) = place.policies.as_ref().map_or(
                        (
                            place.tier.public_worker_positions(),
                            place.tier.public_guard_positions(),
                        ),
                        |policy| policy.staffing_posture.targets(place.tier),
                    );
                    let stewards = if office.city_workers.is_empty() {
                        office
                            .road_steward
                            .as_deref()
                            .unwrap_or("vacant")
                            .to_string()
                    } else {
                        office.city_workers.join(", ")
                    };
                    format!(
                        "Reeve {} / Moot Stewards {} ({}/{}) / guards {}/{}",
                        office.reeve.as_deref().unwrap_or("vacant"),
                        stewards,
                        office.city_workers.len(),
                        worker_target,
                        office.guards.len(),
                        guard_target,
                    )
                },
            );
            let policy = place.policies.as_ref().map_or_else(
                || "Awaiting charter".to_string(),
                |policy| {
                    format!(
                        "{} / {} / {:.1}% market fee / {:.1}% profit levy",
                        policy.strategy.label(),
                        if policy.autopilot {
                            "autopilot"
                        } else {
                            "manual"
                        },
                        policy.market_fee_bps as f32 / 100.0,
                        policy.business_profit_tax_bps as f32 / 100.0,
                    )
                },
            );
            let social_policy = place.policies.as_ref().map_or_else(
                || "Awaiting charter".to_string(),
                |policy| {
                    format!(
                        "Relief {} / food reserve {} days / payroll reserve {} days / staffing {} / permit subsidy {:.1}%",
                        policy.poor_relief.label(),
                        policy.food_reserve_target_days,
                        policy.civic_payroll_reserve_days,
                        policy.staffing_posture.label(),
                        policy.business_permit_subsidy_bps as f32 / 100.0,
                    )
                },
            );
            let civic_arrears = place
                .administration
                .as_ref()
                .map_or(0, |office| office.wage_arrears);
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
            let labour = place.economy.as_ref().map_or_else(
                || "Awaiting first reading".to_string(),
                |economy| {
                    format!(
                        "Private {}/{} ({} vacant), civic {}/{} ({} vacant), {} seeking; best opening {} coin/day",
                        economy.private_filled_jobs,
                        economy.private_job_positions,
                        economy.private_vacant_jobs,
                        economy.civic_filled_jobs,
                        economy.civic_job_positions,
                        economy.civic_vacant_jobs,
                        economy.job_seekers,
                        format_money(economy.best_open_private_wage),
                    )
                },
            );
            let purchasable_food = place
                .market
                .as_ref()
                .map_or(0, MootMarket::listed_edible_units);
            let mut counted_companies = std::collections::HashSet::new();
            let business_cash = place
                .buildings
                .iter()
                .filter_map(|building| building.business)
                .filter_map(|business| {
                    let company = business.company_id?;
                    counted_companies
                        .insert(company)
                        .then_some(business.company.map_or(0, |account| account.cash))
                })
                .fold(0u64, u64::saturating_add);
            let business_arrears = place
                .buildings
                .iter()
                .filter_map(|building| building.business.map(|business| business.account))
                .fold((0u64, 0u64), |(wages, taxes), account| {
                    (
                        wages.saturating_add(account.wage_arrears),
                        taxes.saturating_add(account.tax_arrears),
                    )
                });
            let unlisted_business_food = place
                .buildings
                .iter()
                .flat_map(|building| building.inventory.iter())
                .filter(|(good, _)| good.is_edible())
                .map(|(_, units)| *units)
                .fold(0u32, u32::saturating_add);
            let (market_model, volume) = place.market.as_ref().map_or_else(
                || ("Not operating".to_string(), "No trades".to_string()),
                |market| {
                    (
                        format!(
                            "Private consignment / {} listed units / {}% fee",
                            Good::ALL
                                .into_iter()
                                .map(|good| market.listed_units(good))
                                .sum::<u32>(),
                            market.market_fee_bps() as f32 / 100.0,
                        ),
                        format!("{} coin", format_money(market.total_volume())),
                    )
                },
            );
            PlaceDetailModel {
                title: place.hall_level.label().to_string(),
                subtitle: format!("{} / CIVIC & MARKET RECORD", place.name.to_uppercase()),
                rows: vec![
                    ("OWNER".into(), "The settlement common".into()),
                    (
                        "SERVICES".into(),
                        "Permits / market / common storage".into(),
                    ),
                    ("MOOT STEWARDS".into(), moot_stewards),
                    ("REEVE".into(), reeve),
                    ("PUBLIC POSITIONS".into(), public_jobs),
                    ("CIVIC POLICY".into(), policy),
                    ("SOCIAL / GROWTH POLICY".into(), social_policy),
                    (
                        "CIVIC WAGE ARREARS".into(),
                        format!("{} coin", format_money(civic_arrears)),
                    ),
                    ("ROAD AUDIT".into(), roads),
                    ("ROAD MATERIALS".into(), road_materials),
                    ("LAYOUT CHARTER".into(), plan),
                    ("DEFENCE RESERVES".into(), walls),
                    ("PERMIT MARKET".into(), next_permit_summary(place)),
                    ("APPROVED WORKS".into(), permit_queue_summary(place)),
                    (
                        "PERMIT POLICY".into(),
                        place.policies.as_ref().map_or_else(
                            || "Needed housing free / businesses pay need-priced fees".into(),
                            |policy| {
                                format!(
                                    "Needed housing free / requested businesses receive {:.1}% permit discount",
                                    policy.business_permit_subsidy_bps as f32 / 100.0,
                                )
                            },
                        ),
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
                    ("MARKET MODEL".into(), market_model),
                    ("LIFETIME TRADE".into(), volume),
                    ("FOOD SECURITY".into(), food),
                    ("LABOUR MARKET".into(), labour),
                    (
                        "PURCHASABLE / AT BUSINESSES".into(),
                        format!("{purchasable_food} / {unlisted_business_food} food units"),
                    ),
                    (
                        "LOCAL COMPANY TREASURIES".into(),
                        format!("{} coin", format_money(business_cash)),
                    ),
                    (
                        "BUSINESS ARREARS".into(),
                        format!(
                            "{} wage / {} tax coin",
                            format_money(business_arrears.0),
                            format_money(business_arrears.1),
                        ),
                    ),
                    (
                        "POOR RELIEF".into(),
                        place.policies.as_ref().map_or_else(
                            || "Awaiting charter".into(),
                            |policy| {
                                format!(
                                    "{} / preserves {} full reserve days",
                                    policy.poor_relief.label(),
                                    policy.food_reserve_target_days,
                                )
                            },
                        ),
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
                    building.for_sale.map_or_else(
                        || building.owner.as_deref().unwrap_or("The settlement").into(),
                        |listing| {
                            format!(
                                "FOR SALE — {} coin / {}",
                                format_money(listing.asking_price),
                                listing.reason.label(),
                            )
                        },
                    ),
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
                            "Shared Hall exchange and expanded storage / staffing disabled".into()
                        }
                        SettlementBuildingKind::Tavern => {
                            "Food and lodging amenity / 2 work positions".into()
                        }
                        SettlementBuildingKind::Church => "Civic amenity / 1 work position".into(),
                        SettlementBuildingKind::Windmill => {
                            "Buys Wheat, produces Flour / 2 work positions".into()
                        }
                        SettlementBuildingKind::Bakery => {
                            "Buys 2 Flour, produces 4 Bread / 2 work positions".into()
                        }
                        SettlementBuildingKind::StorageHall => {
                            "Private company depot / 4 porter positions / 2,400 bulk storage".into()
                        }
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
                        "{} employed / {} open / {} max",
                        building.workers.len(),
                        building
                            .business
                            .and_then(|business| business.staffing)
                            .unwrap_or_else(|| {
                                BusinessStaffingPolicy::new(building.kind.positions())
                            })
                            .target_for(building.kind),
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
                if let Some(label) = building.kind.site_quality_label() {
                    rows.push((label.into(), format!("{:.0}%", building.quality * 100.0)));
                }
            }
            if let Some(business) = building.business {
                let account = business.account;
                let previous = account.previous_day;
                let previous_profit = previous.profit();
                let mut held_stock = [0; Good::COUNT];
                for (good, units) in &building.inventory {
                    held_stock[good.index()] = *units;
                }
                let protected = match (business.wage, business.management, business.procurement) {
                    (Some(wage), Some(management), Some(procurement)) => business_working_capital(
                        business
                            .staffing
                            .unwrap_or_else(|| {
                                BusinessStaffingPolicy::new(building.kind.positions())
                            })
                            .target_for(building.kind),
                        &wage,
                        &management,
                        &procurement,
                        Some(&held_stock),
                        place.market.as_ref(),
                    ),
                    _ => Default::default(),
                };
                rows.extend([
                    (
                        "BUSINESS STATUS".into(),
                        business.condition.map_or_else(
                            || "Operating record loading".into(),
                            |condition| {
                                let mut parts = vec![condition.state.label().to_string()];
                                if condition.cash_tight_days > 0 {
                                    parts
                                        .push(format!("cash-tight {}d", condition.cash_tight_days));
                                }
                                if condition.insolvent_days > 0 {
                                    parts.push(format!("insolvent {}d", condition.insolvent_days));
                                }
                                parts.join(" / ")
                            },
                        ),
                    ),
                    (
                        "MANAGEMENT".into(),
                        business.management.map_or_else(
                            || "Owner policy loading".into(),
                            |policy| {
                                format!(
                                    "{} / {} / {} payroll days protected",
                                    policy.strategy.label(),
                                    if policy.autopilot {
                                        "autopilot"
                                    } else {
                                        "manual"
                                    },
                                    policy.payroll_reserve_days,
                                )
                            },
                        ),
                    ),
                    (
                        "COMPANY TREASURY".into(),
                        business.company.map_or_else(
                            || "Company account loading".into(),
                            |company| format!("{} coin", format_money(company.cash)),
                        ),
                    ),
                    (
                        "SITE REQUIREMENT / COMPANY FREE".into(),
                        format!(
                            "{} / {} coin",
                            format_money(protected.total_with_liabilities(&account)),
                            format_money(business.company.map_or(0, |company| {
                                company
                                    .cash
                                    .saturating_sub(company.wage_arrears)
                                    .saturating_sub(company.tax_arrears)
                                    .saturating_sub(protected.total())
                            })),
                        ),
                    ),
                    (
                        "LIABILITIES".into(),
                        format!(
                            "{} wage / {} tax arrears · {} wage / {} tax defaulted",
                            format_money(account.wage_arrears),
                            format_money(account.tax_arrears),
                            format_money(account.defaulted_wages),
                            format_money(account.defaulted_taxes),
                        ),
                    ),
                    (
                        "YESTERDAY P&L".into(),
                        if previous.day == u32::MAX {
                            "First business day still open".into()
                        } else {
                            format!(
                                "revenue {} / costs {} / {}{} coin",
                                format_money(previous.gross_revenue),
                                format_money(previous.operating_expenses()),
                                if previous_profit < 0 { "-" } else { "+" },
                                format_money(previous_profit.unsigned_abs()),
                            )
                        },
                    ),
                    ("LIFETIME RESULT".into(), {
                        let profit = account.lifetime_profit();
                        format!(
                            "{}{} coin / {} withdrawn",
                            if profit < 0 { "-" } else { "+" },
                            format_money(profit.unsigned_abs()),
                            format_money(account.owner_withdrawals),
                        )
                    }),
                    (
                        "SALE POLICY".into(),
                        business.sale.map_or_else(
                            || "No offer configured".into(),
                            |policy| {
                                format!(
                                    "{} each / {} day company reserve ({} units) / collect {} / {} pricing",
                                    format_money(policy.asking_unit_price),
                                    policy.company_reserve_days,
                                    policy.company_reserve_units,
                                    policy.max_units_per_collection,
                                    if policy.automatic_pricing {
                                        "automatic"
                                    } else {
                                        "manual"
                                    },
                                )
                            },
                        ),
                    ),
                    (
                        "WAGE OFFER".into(),
                        business.wage.map_or_else(
                            || "No private wage".into(),
                            |policy| {
                                let mut value = format!(
                                    "{} coin/day / {}",
                                    format_money(policy.daily_wage),
                                    if policy.automatic {
                                        "automatic"
                                    } else {
                                        "owner-set"
                                    },
                                );
                                if policy.vacancy_days > 0 {
                                    value.push_str(&format!(" / vacant {}d", policy.vacancy_days));
                                }
                                value
                            },
                        ),
                    ),
                    (
                        "INPUT ORDERS".into(),
                        business.procurement.map_or_else(
                            || "No procurement policy".into(),
                            |policy| {
                                let orders = Good::ALL
                                    .into_iter()
                                    .filter_map(|good| {
                                        let rule = policy.rule(good);
                                        rule.enabled.then(|| {
                                            format!(
                                                "{}: {} day{} cover → {}-unit target (max {})",
                                                good.label(),
                                                rule.coverage_days,
                                                if rule.coverage_days == 1 { "" } else { "s" },
                                                rule.target_units,
                                                format_money(rule.maximum_unit_price),
                                            )
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                if orders.is_empty() {
                                    "No purchased inputs".into()
                                } else {
                                    orders.join(" / ")
                                }
                            },
                        ),
                    ),
                    (
                        "LOCAL PROFIT LEVY".into(),
                        place.policies.as_ref().map_or_else(
                            || "Settlement charter unavailable".into(),
                            |policy| {
                                format!(
                                    "{:.1}% of positive daily profit",
                                    policy.business_profit_tax_bps as f32 / 100.0,
                                )
                            },
                        ),
                    ),
                ]);
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
    use bevy::ecs::system::RunSystemOnce;

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
                id: shared::components::SettlementId::UNASSIGNED,
                name: name.to_string(),
                tier,
                hall_level: CivicHallLevel::for_tier(tier),
                position: Vec3::ZERO,
                residents: 0,
                treasury: 0,
                market: None,
                economy: None,
                administration: None,
                development: None,
                policies: None,
                opportunities: None,
                inventory: Vec::new(),
                inventory_used: 0,
                inventory_capacity: 0,
                buildings: Vec::new(),
                summary_buildings: [0; 6],
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
            id: shared::components::SettlementId(1),
            name: "Brackwater".into(),
            tier: SettlementTier::Village,
            hall_level: CivicHallLevel::Village,
            position: Vec3::new(120.0, 0.0, -80.0),
            residents: 7,
            treasury: 0,
            market: None,
            economy: None,
            administration: None,
            development: None,
            policies: None,
            opportunities: None,
            inventory: vec![(Good::Wood, 3)],
            inventory_used: 12,
            inventory_capacity: 200,
            buildings: vec![PlaceBuildingRecord {
                id: Some(shared::components::BuildingId(10)),
                kind: SettlementBuildingKind::Farmstead,
                position: Vec3::new(145.0, 0.0, -100.0),
                owner: Some("Ada".into()),
                for_sale: None,
                quality: 0.76,
                workers: vec!["Ada".into()],
                residents: Vec::new(),
                inventory: vec![(Good::Wheat, 4)],
                inventory_used: 8,
                inventory_capacity: 80,
                business: None,
            }],
            summary_buildings: [0, 1, 0, 0, 0, 0],
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
            .any(|(label, value)| label == "FARMLAND QUALITY" && value == "76%"));
        assert!(model
            .rows
            .iter()
            .any(|(label, value)| label == "STORE" && value.contains("Wheat 4")));
    }

    #[test]
    fn processor_detail_sheets_do_not_claim_land_controls_output() {
        let mut place = explorer_place();
        place.buildings[0].kind = SettlementBuildingKind::Windmill;
        let mill = place_detail_model(&place, SelectedPlaceEntry::Building(0), true);
        assert!(mill
            .rows
            .iter()
            .all(|(label, _)| !label.contains("QUALITY")));

        place.buildings[0].kind = SettlementBuildingKind::Bakery;
        let bakery = place_detail_model(&place, SelectedPlaceEntry::Building(0), true);
        assert!(bakery
            .rows
            .iter()
            .all(|(label, _)| !label.contains("QUALITY")));
    }

    #[test]
    fn expanded_business_shows_operating_finance_and_management() {
        let mut place = explorer_place();
        place.policies = Some(SettlementPolicies::default());
        let mut account = BusinessAccount::with_capital(2_000);
        account.record_sale(1, 800, 40, 4);
        account.incur_wages(1, 200);
        account.roll_to_day(2);
        place.buildings[0].business = Some(PlaceBusinessRecord {
            account,
            company: Some(shared::economy::CompanyAccount {
                cash: 2_000,
                ..default()
            }),
            company_id: Some(CompanyId(1)),
            sale: Some(BusinessSalePolicy::for_good(Good::Wheat)),
            wage: Some(BusinessWagePolicy::default()),
            staffing: Some(BusinessStaffingPolicy::new(1)),
            management: Some(BusinessManagementPolicy::default()),
            procurement: Some(BusinessProcurementPolicy::default()),
            condition: Some(BusinessCondition::default()),
        });

        let model = place_detail_model(&place, SelectedPlaceEntry::Building(0), true);
        for label in [
            "BUSINESS STATUS",
            "MANAGEMENT",
            "COMPANY TREASURY",
            "LIABILITIES",
            "YESTERDAY P&L",
            "SALE POLICY",
            "WAGE OFFER",
            "INPUT ORDERS",
            "LOCAL PROFIT LEVY",
        ] {
            assert!(
                model.rows.iter().any(|(actual, _)| actual == label),
                "missing {label}"
            );
        }
    }

    #[test]
    fn business_history_action_exists_only_for_a_stable_selected_business() {
        let mut place = explorer_place();
        place.buildings[0].business = Some(PlaceBusinessRecord {
            account: BusinessAccount::default(),
            company: Some(shared::economy::CompanyAccount::default()),
            company_id: Some(CompanyId(1)),
            sale: Some(BusinessSalePolicy::for_good(Good::Wheat)),
            wage: Some(BusinessWagePolicy::default()),
            staffing: Some(BusinessStaffingPolicy::new(1)),
            management: Some(BusinessManagementPolicy::default()),
            procurement: Some(BusinessProcurementPolicy::default()),
            condition: Some(BusinessCondition::default()),
        });

        let mut world = World::new();
        world.insert_resource(KnownPlaces {
            records: vec![place],
        });
        world.insert_resource(SelectedPlace(Some("Brackwater".into())));
        world.insert_resource(SelectedPlaceEntry::Building(0));
        world.spawn((
            Settlement {
                name: "Brackwater".into(),
                tier: SettlementTier::Village,
                residents: 7,
                treasury: 0,
            },
            shared::components::SettlementId(1),
        ));
        let button = world
            .spawn((
                PlaceBusinessHistoryAction,
                Button,
                Node {
                    display: Display::None,
                    ..default()
                },
                button_chrome(UiButtonVariant::Secondary),
            ))
            .id();

        world
            .run_system_once(sync_place_business_history_action)
            .unwrap();
        assert_eq!(world.get::<Node>(button).unwrap().display, Display::Flex);
        let history = world
            .get::<crate::ui::history::BusinessHistoryButton>(button)
            .unwrap();
        assert_eq!(history.place, "Brackwater");
        assert_eq!(history.business, shared::components::BuildingId(10));

        world.insert_resource(SelectedPlaceEntry::Overview);
        world
            .run_system_once(sync_place_business_history_action)
            .unwrap();
        assert_eq!(world.get::<Node>(button).unwrap().display, Display::None);
        assert!(world
            .get::<crate::ui::history::BusinessHistoryButton>(button)
            .is_none());
    }
}
