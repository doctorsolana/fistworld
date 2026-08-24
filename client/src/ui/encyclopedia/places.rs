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
    CivicHallLevel, CivicHallUpgradeWorksite, CompanyId, ConstructionSite, FarmField, FishingPier,
    Household, MootAdministration, OperatedBy, PlayerPosition, Settlement, SettlementBuilding,
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
use crate::ui::styles::{EMBER, INK, INK_MUTED};
use lightyear::prelude::{Connected, MessageSender};
use shared::protocol::{HeroConstructionOrder, ReliableChannel};

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
    pub material: Good,
    pub delivered_material: u32,
    pub required_material: u32,
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

fn economy_from_summary(summary: &shared::components::SettlementSummary) -> SettlementEconomy {
    let mut economy = SettlementEconomy::default();
    apply_summary_economy(&mut economy, summary);
    economy
}

fn apply_summary_economy(
    economy: &mut SettlementEconomy,
    summary: &shared::components::SettlementSummary,
) {
    economy.prosperity = summary.prosperity;
    economy.reserve_days = summary.reserve_days;
    economy.recent_food_production = summary.recent_food_production;
    economy.recent_food_consumption = summary.recent_food_consumption;
    economy.unmet_food = summary.hungry;
    economy.housing_capacity = summary.housing_capacity;
    economy.homeless_residents = summary.homeless;
    economy.job_seekers = summary.job_seekers;
    economy.unpaid_workers = summary.unpaid_workers;
    economy.unrest = summary.unrest;
    economy.unrest_change = summary.unrest_change;
    economy.unrest_target = summary.unrest_target;
    economy.unrest_hunger_pressure = summary.unrest_hunger_pressure;
    economy.unrest_housing_pressure = summary.unrest_housing_pressure;
    economy.unrest_wage_pressure = summary.unrest_wage_pressure;
}

fn economy_matches_summary(
    economy: Option<&SettlementEconomy>,
    summary: &shared::components::SettlementSummary,
) -> bool {
    economy.is_some_and(|economy| {
        economy.prosperity == summary.prosperity
            && economy.reserve_days == summary.reserve_days
            && economy.recent_food_production == summary.recent_food_production
            && economy.recent_food_consumption == summary.recent_food_consumption
            && economy.unmet_food == summary.hungry
            && economy.housing_capacity == summary.housing_capacity
            && economy.homeless_residents == summary.homeless
            && economy.job_seekers == summary.job_seekers
            && economy.unpaid_workers == summary.unpaid_workers
            && economy.unrest == summary.unrest
            && economy.unrest_change == summary.unrest_change
            && economy.unrest_target == summary.unrest_target
            && economy.unrest_hunger_pressure == summary.unrest_hunger_pressure
            && economy.unrest_housing_pressure == summary.unrest_housing_pressure
            && economy.unrest_wage_pressure == summary.unrest_wage_pressure
    })
}

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
                && economy_matches_summary(record.economy.as_ref(), summary)
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
            apply_summary_economy(record.economy.get_or_insert_default(), summary);
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
                economy: Some(economy_from_summary(summary)),
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
    /// A construction site, addressed by its replicated entity: worksites are
    /// transient and have no stable index in the place snapshot.
    Worksite(Entity),
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
pub struct PlaceDetailTile(pub usize);

#[derive(Component, Clone, Copy)]
pub struct PlaceDetailTileLabel(pub usize);

#[derive(Component, Clone, Copy)]
pub struct PlaceDetailTileValue(pub usize);

#[derive(Component, Clone, Copy)]
pub struct PlaceBackToCompanyLabel;

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
        Option<&CivicHallUpgradeWorksite>,
    )>,
    mut places: ResMut<KnownPlaces>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
    time: Res<Time>,
    mut last_run: Local<Option<f32>>,
) {
    let mut _ui_scope = ui_perf.scope("learn_settlements");
    // This pass snapshots every building of every settlement. Twice a second
    // keeps the places tab and compact panel current without a per-frame tax.
    let now = time.elapsed_secs();
    if last_run.is_some_and(|last| now - last < 0.5) {
        return;
    }
    *last_run = Some(now);
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
                .filter(|(site, owner, _, _, _)| {
                    settlement_id.map_or_else(
                        || site.settlement == settlement.name,
                        |id| owner.is_some_and(|owner| owner.0 == *id),
                    )
                })
                .map(|(site, _, inventory, for_sale, hall_upgrade)| {
                    let (material, required_material) = hall_upgrade.map_or(
                        (Good::Wood, site.kind.construction_wood_required()),
                        |upgrade| (upgrade.material, upgrade.material_required),
                    );
                    PlacePermitRecord {
                        kind: site.kind,
                        raising: site.raising,
                        material,
                        delivered_material: inventory.map_or(0, |store| store.amount(material)),
                        required_material,
                        for_sale: for_sale.copied(),
                    }
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
                permit.delivered_material,
                permit.required_material
            );
            let state = format!("{state} {}", permit.material.label());
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
    mut last: Local<Option<u64>>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
) {
    let mut _ui_scope = ui_perf.scope("rebuild_place_list");
    // Same rule as the people list: rows render a handful of slow facts
    // (name, tier, building count, and the expanded place's building labels),
    // while `KnownPlaces` moves every time a storehouse count ticks. Rebuild
    // on the ROW projection, not on the registry. A fresh container always fills.
    let fresh = existing.is_empty();
    if !places.is_changed() && !selected.is_changed() && !fresh && last.is_some() {
        return;
    }
    let ordered = places.ordered();
    let signature = place_rows_signature(&ordered, selected.0.as_deref());
    if !fresh && *last == Some(signature) {
        return;
    }

    let Ok(content_entity) = content.single() else {
        return;
    };
    *last = Some(signature);
    _ui_scope.rebuilt();
    for row in existing.iter() {
        commands.entity(row).despawn();
    }

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

/// Hash of exactly what the place rows render: every row's name, tier and
/// building count, plus the expanded place's building labels and summaries.
fn place_rows_signature(ordered: &[&PlaceRecord], selected: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    selected.hash(&mut hasher);
    ordered.len().hash(&mut hasher);
    for record in ordered {
        record.name.hash(&mut hasher);
        record.tier.label().hash(&mut hasher);
        record.buildings.len().hash(&mut hasher);
        if selected == Some(record.name.as_str()) {
            record.hall_level.label().hash(&mut hasher);
            for (index, building) in record.buildings.iter().enumerate() {
                building_label(record, index).hash(&mut hasher);
                building_tree_summary(building).hash(&mut hasher);
            }
        }
    }
    hasher.finish()
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
                font_size: FontSize::Px(14.0),
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
                font_size: FontSize::Px(16.0),
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
                font_size: FontSize::Px(12.5),
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
                font_size: FontSize::Px(14.0),
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
                font_size: FontSize::Px(11.5),
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
    directory: Res<companies::CompanyDirectory>,
    mut buttons: Query<&mut Node, With<PlaceBackToCompanyAction>>,
    mut labels: Query<&mut Text, With<PlaceBackToCompanyLabel>>,
) {
    for mut node in buttons.iter_mut() {
        node.display = if return_to.0.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    // Name the destination: "BACK TO ALDRIC GRAIN & BREAD" says where you are
    // in a way "BACK TO COMPANY" never could.
    let label = return_to
        .0
        .and_then(|id| directory.records.iter().find(|company| company.id == id))
        .map_or_else(
            || "BACK TO COMPANY".to_string(),
            |company| format!("BACK TO {}", company.name.to_uppercase()),
        );
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.clone();
        }
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
    worksites: Query<(
        &shared::components::ConstructionSite,
        Option<&shared::economy::GoodsInventory>,
        Option<&shared::components::CivicHallUpgradeWorksite>,
        Option<&shared::economy::BusinessForSale>,
    )>,
    mut assign_buttons: Query<
        (&mut Node, &mut AssignHeroToWorksite),
        (
            With<WorksiteAssignButton>,
            Without<PlaceDetailCard>,
            Without<PlaceDetailEmptyState>,
            Without<PlaceDetailLine>,
            Without<PlaceDetailTile>,
        ),
    >,
    mut action_rows: Query<
        &mut Node,
        (
            With<PlaceActionsRow>,
            Without<WorksiteAssignButton>,
            Without<PlaceDetailCard>,
            Without<PlaceDetailEmptyState>,
            Without<PlaceDetailLine>,
            Without<PlaceDetailTile>,
        ),
    >,
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
            &mut TextFont,
            &mut TextColor,
            Option<&PlaceDetailLabel>,
            Option<&PlaceDetailValue>,
        ),
        (
            Or<(With<PlaceDetailLabel>, With<PlaceDetailValue>)>,
            Without<PlaceDetailName>,
            Without<PlaceDetailSubtitle>,
        ),
    >,
    mut tiles: Query<
        (&PlaceDetailTile, &mut Node),
        (
            Without<PlaceDetailCard>,
            Without<PlaceDetailEmptyState>,
            Without<PlaceDetailLine>,
        ),
    >,
    mut tile_text: Query<
        (
            &mut Text,
            Option<&PlaceDetailTileLabel>,
            Option<&PlaceDetailTileValue>,
        ),
        (
            Or<(With<PlaceDetailTileLabel>, With<PlaceDetailTileValue>)>,
            Without<PlaceDetailLabel>,
            Without<PlaceDetailValue>,
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

    let live_worksite = match *selected_entry {
        SelectedPlaceEntry::Worksite(site) => worksites.get(site).ok().map(|data| (site, data)),
        _ => None,
    };
    let model = match live_worksite {
        Some((_, (site, inventory, upgrade, for_sale))) => {
            let (good, required) = upgrade.map_or(
                (
                    shared::economy::Good::Wood,
                    site.kind.construction_wood_required(),
                ),
                |upgrade| (upgrade.material, upgrade.material_required),
            );
            let delivered = inventory.map_or(0, |inventory| inventory.amount(good));
            worksite_detail_model(site, delivered, required, good, for_sale)
        }
        // A finished (despawned) worksite falls back to the place overview.
        None if matches!(*selected_entry, SelectedPlaceEntry::Worksite(_)) => {
            place_detail_model(record, SelectedPlaceEntry::Overview, god.0)
        }
        None => place_detail_model(record, *selected_entry, god.0),
    };
    let on_worksite = matches!(*selected_entry, SelectedPlaceEntry::Worksite(_));
    for mut node in action_rows.iter_mut() {
        let display = if on_worksite {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (mut node, mut assign) in assign_buttons.iter_mut() {
        let display = if live_worksite.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let target = live_worksite.map(|(site, _)| site);
        if assign.0 != target {
            assign.0 = target;
        }
    }
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
    for (PlaceDetailTile(index), mut node) in tiles.iter_mut() {
        let display = if *index < model.tiles.len() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (mut text, label, value) in tile_text.iter_mut() {
        let index = label
            .map(|part| part.0)
            .or_else(|| value.map(|part| part.0));
        let Some((tile_label, tile_value)) = index.and_then(|index| model.tiles.get(index)) else {
            continue;
        };
        let next = if label.is_some() {
            tile_label
        } else {
            tile_value
        };
        if text.0 != *next {
            text.0 = next.clone();
        }
    }
    for (PlaceDetailLine(index), mut node) in lines.iter_mut() {
        let row = model.rows.get(*index);
        let display = if row.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        // A section heading sits on its own, with air above it and no rule.
        let section = matches!(row, Some(DetailRow::Section(_)));
        let padding = if section {
            UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(22.0), Val::Px(4.0))
        } else {
            UiRect::vertical(Val::Px(9.0))
        };
        if node.padding != padding {
            node.padding = padding;
        }
        let border = UiRect::bottom(Val::Px(if section { 0.0 } else { 1.0 }));
        if node.border != border {
            node.border = border;
        }
    }
    for (mut text, mut font, mut color, label, value) in line_text.iter_mut() {
        let index = label
            .map(|part| part.0)
            .or_else(|| value.map(|part| part.0));
        let Some(row) = index.and_then(|index| model.rows.get(index)) else {
            continue;
        };
        let (next, next_size, next_color) = match (row, label.is_some()) {
            (DetailRow::Section(name), true) => (name.clone(), 13.0, EMBER),
            (DetailRow::Section(_), false) => (String::new(), 16.0, INK),
            (DetailRow::Line(row_label, _), true) => (row_label.clone(), 13.5, INK_MUTED),
            (DetailRow::Line(_, row_value), false) => (row_value.clone(), 16.0, INK),
        };
        if text.0 != next {
            text.0 = next;
        }
        if font.font_size != FontSize::Px(next_size) {
            font.font_size = FontSize::Px(next_size);
        }
        if color.0 != next_color {
            color.0 = next_color;
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

/// The place-level action strip (MARKET, SETTLEMENT HISTORY, ...) at the top
/// of the detail card; hidden while a worksite entry is open - those actions
/// belong to the place, and with the short worksite tile row they would
/// overlap the tiles.
#[derive(Component)]
pub(crate) struct PlaceActionsRow;

/// The pre-spawned SEND MY HERO button on the place detail card; visible only
/// while a worksite entry is open. The payload is bound in place.
#[derive(Component)]
pub(crate) struct WorksiteAssignButton;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AssignHeroToWorksite(pub Option<Entity>);

fn worksite_detail_model(
    site: &shared::components::ConstructionSite,
    delivered: u32,
    required: u32,
    good: shared::economy::Good,
    for_sale: Option<&shared::economy::BusinessForSale>,
) -> PlaceDetailModel {
    let status = if site.raising {
        "Raising the frame"
    } else if delivered >= required {
        "Ready to build"
    } else {
        "Awaiting materials"
    };
    let mut rows = vec![
        DetailRow::Section("WORKSITE".into()),
        DetailRow::Line(
            "MATERIALS".into(),
            format!("{delivered} of {required} {} delivered", good.label()),
        ),
        DetailRow::Line(
            "CREW".into(),
            "The village drafts a free resident; send your own hero below to supply and build it yourself".into(),
        ),
    ];
    if let Some(listing) = for_sale {
        rows.push(DetailRow::Line(
            "FOR SALE".into(),
            format!(
                "{} coin / {}",
                shared::economy::format_money(listing.asking_price),
                listing.reason.label(),
            ),
        ));
    }
    PlaceDetailModel {
        title: format!("{} WORKSITE", site.kind.label().to_uppercase()),
        subtitle: format!(
            "{} / {}",
            site.settlement.to_uppercase(),
            status.to_uppercase()
        ),
        tiles: vec![
            ("STATUS".into(), status.to_string()),
            (
                good.label().to_uppercase(),
                format!("{delivered} / {required}"),
            ),
        ],
        rows,
    }
}

/// Send the local hero to supply and raise the open worksite. The server
/// validates ownership and reachability and answers through the permit
/// notice ([`HeroConstructionResult`]).
pub(super) fn handle_worksite_assign_button(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &AssignHeroToWorksite), Changed<Interaction>>,
    mut clients: Query<
        &mut MessageSender<HeroConstructionOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, assign) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let (Some(site), Ok(mut sender)) = (assign.0, clients.single_mut()) else {
            continue;
        };
        sender.send::<ReliableChannel>(HeroConstructionOrder { site });
    }
}

struct PlaceDetailModel {
    title: String,
    subtitle: String,
    /// The numbers a player scans first: big, few, on top.
    tiles: Vec<(String, String)>,
    rows: Vec<DetailRow>,
}

impl PlaceDetailModel {
    /// Label / value pairs only, section headings skipped.
    #[cfg(test)]
    fn lines(&self) -> impl Iterator<Item = (&str, &str)> {
        self.rows.iter().filter_map(|row| match row {
            DetailRow::Line(label, value) => Some((label.as_str(), value.as_str())),
            DetailRow::Section(_) => None,
        })
    }
}

/// One row of the detail ledger: a section heading, or a label / value pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DetailRow {
    Section(String),
    Line(String, String),
}

/// Order the flat ledger into named sections. Each table entry names a
/// section and the labels that belong to it; labels keep their original
/// relative order, and anything the table does not mention lands in a final
/// DETAILS section so no fact is ever silently dropped.
fn group_rows(rows: Vec<(String, String)>, sections: &[(&str, &[&str])]) -> Vec<DetailRow> {
    let mut remaining: Vec<Option<(String, String)>> = rows.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for (section, labels) in sections {
        let mut members = Vec::new();
        for slot in remaining.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|(label, _)| labels.contains(&label.as_str()))
            {
                let (label, value) = slot.take().unwrap();
                members.push(DetailRow::Line(label, value));
            }
        }
        if !members.is_empty() {
            out.push(DetailRow::Section((*section).to_string()));
            out.extend(members);
        }
    }
    let leftovers: Vec<DetailRow> = remaining
        .into_iter()
        .flatten()
        .map(|(label, value)| DetailRow::Line(label, value))
        .collect();
    if !leftovers.is_empty() {
        out.push(DetailRow::Section("DETAILS".to_string()));
        out.extend(leftovers);
    }
    out
}

const OVERVIEW_SECTIONS: &[(&str, &[&str])] = &[
    (
        "PEOPLE",
        &[
            "RESIDENTS",
            "HOUSING",
            "UNEMPLOYMENT",
            "HUNGER",
            "UNPAID WORKERS",
        ],
    ),
    (
        "STABILITY",
        &["UNREST", "UNREST PRESSURES", "FOOD SECURITY"],
    ),
    ("ECONOMY", &["TREASURY", "COMMON STORE"]),
    ("GROWTH", &["STRUCTURES", "TO ADVANCE", "LOCATION"]),
];

const HALL_SECTIONS: &[(&str, &[&str])] = &[
    (
        "OFFICE",
        &[
            "OWNER",
            "SERVICES",
            "REEVE",
            "MOOT STEWARDS",
            "PUBLIC POSITIONS",
            "CIVIC WAGE ARREARS",
        ],
    ),
    (
        "POLICY",
        &[
            "CIVIC POLICY",
            "SOCIAL / GROWTH POLICY",
            "PERMIT POLICY",
            "POOR RELIEF",
        ],
    ),
    (
        "PERMITS & WORKS",
        &[
            "PERMIT MARKET",
            "APPROVED WORKS",
            "LAYOUT CHARTER",
            "ROAD AUDIT",
            "ROAD MATERIALS",
            "DEFENCE RESERVES",
        ],
    ),
    (
        "MARKET",
        &[
            "TREASURY",
            "COMMON STORE",
            "MARKET MODEL",
            "LIFETIME TRADE",
            "LABOUR MARKET",
            "PURCHASABLE / AT BUSINESSES",
            "LOCAL COMPANY TREASURIES",
            "BUSINESS ARREARS",
        ],
    ),
    ("LOCATION", &["LOCATION"]),
];

const BUILDING_SECTIONS: &[(&str, &[&str])] = &[
    (
        "OWNERSHIP",
        &[
            "OWNER",
            "PURPOSE",
            "COMPANY TREASURY",
            "SITE REQUIREMENT / COMPANY FREE",
        ],
    ),
    (
        "PEOPLE",
        &["BEDS", "HOUSEHOLD", "STAFFING", "WORKERS", "WAGE OFFER"],
    ),
    (
        "BUSINESS",
        &[
            "BUSINESS STATUS",
            "MANAGEMENT",
            "LIABILITIES",
            "YESTERDAY P&L",
            "LIFETIME RESULT",
        ],
    ),
    ("TRADE", &["SALE POLICY", "INPUT ORDERS"]),
    ("LOCATION", &["LOCATION"]),
];

/// The six figures that describe a town at a glance.
fn town_tiles(place: &PlaceRecord) -> Vec<(String, String)> {
    let economy = place.economy.as_ref();
    let population = if place.residents == 0 {
        "None yet".to_string()
    } else {
        place.residents.to_string()
    };
    let unemployment = economy.map_or_else(
        || "-".to_string(),
        |economy| {
            format!(
                "{:.0}%",
                resident_share(u32::from(economy.job_seekers), place.residents)
            )
        },
    );
    let prosperity = economy.map_or_else(
        || "-".to_string(),
        |economy| format!("{:.0}", economy.prosperity),
    );
    let food = economy.map_or_else(
        || "-".to_string(),
        |economy| {
            let state = if economy.unmet_food > 0 {
                "Crisis"
            } else if economy.reserve_days < 1.0 {
                "Short"
            } else if economy.reserve_days < shared::economy::FOOD_SECURITY_TARGET_DAYS {
                "Fragile"
            } else {
                "Secure"
            };
            format!("{state}, {:.1} days", economy.reserve_days)
        },
    );
    let unrest = economy.map_or_else(
        || "-".to_string(),
        |economy| format!("{:.0}  {}", economy.unrest, economy.unrest_label()),
    );
    let treasury = format!("{} coin", format_money(place.treasury));
    [
        ("POPULATION", population),
        ("UNEMPLOYMENT", unemployment),
        ("PROSPERITY", prosperity),
        ("FOOD", food),
        ("UNREST", unrest),
        ("TREASURY", treasury),
    ]
    .into_iter()
    .map(|(label, value)| (label.to_string(), value))
    .collect()
}

fn resident_share(count: u32, residents: u32) -> f32 {
    if residents == 0 {
        0.0
    } else {
        count as f32 / residents as f32 * 100.0
    }
}

fn unrest_summary(economy: Option<&SettlementEconomy>) -> String {
    economy.map_or_else(
        || "Awaiting first daily reading".to_string(),
        |economy| {
            let trend = if economy.unrest_change.abs() <= 0.05 {
                "steady last day".to_string()
            } else {
                format!(
                    "{} {:+.1} last day",
                    economy.unrest_trend_label(),
                    economy.unrest_change,
                )
            };
            format!(
                "{:.0} / 100 / {} / {} / pressure {:.0}",
                economy.unrest,
                economy.unrest_label(),
                trend,
                economy.unrest_target,
            )
        },
    )
}

fn unrest_pressure_summary(economy: Option<&SettlementEconomy>) -> String {
    economy.map_or_else(
        || "No causes recorded".to_string(),
        |economy| {
            format!(
                "Hunger +{:.1} / homelessness +{:.1} / unpaid workers +{:.1}",
                economy.unrest_hunger_pressure,
                economy.unrest_housing_pressure,
                economy.unrest_wage_pressure,
            )
        },
    )
}

fn food_security_summary(economy: Option<&SettlementEconomy>) -> String {
    economy.map_or_else(
        || "Awaiting first daily reading".to_string(),
        |economy| {
            let state = if economy.unmet_food > 0 {
                "CRISIS"
            } else if economy.reserve_days < 1.0 {
                "SHORTAGE RISK"
            } else if economy.reserve_days < shared::economy::FOOD_SECURITY_TARGET_DAYS {
                "FRAGILE"
            } else {
                "SECURE"
            };
            format!(
                "{state} / {:.1} reserve days / {:.1} produced vs {:.1} consumed daily",
                economy.reserve_days,
                economy.recent_food_production,
                economy.recent_food_consumption,
            )
        },
    )
}

fn hunger_summary(economy: Option<&SettlementEconomy>, residents: u32) -> String {
    economy.map_or_else(
        || "Awaiting first daily reading".to_string(),
        |economy| {
            if economy.unmet_food == 0 {
                "Everyone ate after the last daily settlement".to_string()
            } else {
                format!(
                    "{} of {} residents unfed after the last meal ({:.0}%)",
                    economy.unmet_food,
                    residents,
                    resident_share(economy.unmet_food, residents),
                )
            }
        },
    )
}

fn housing_summary(economy: Option<&SettlementEconomy>, residents: u32) -> String {
    economy.map_or_else(
        || "Awaiting first housing audit".to_string(),
        |economy| {
            let housed = residents.saturating_sub(economy.homeless_residents);
            format!(
                "{housed} housed / {} homeless ({:.0}%) / {} completed beds",
                economy.homeless_residents,
                resident_share(economy.homeless_residents, residents),
                economy.housing_capacity,
            )
        },
    )
}

fn work_seekers_summary(economy: Option<&SettlementEconomy>, residents: u32) -> String {
    economy.map_or_else(
        || "Awaiting first labour reading".to_string(),
        |economy| {
            format!(
                "{} actively seeking work ({:.0}%) / {} private + {} civic vacancies",
                economy.job_seekers,
                resident_share(u32::from(economy.job_seekers), residents),
                economy.private_vacant_jobs,
                economy.civic_vacant_jobs,
            )
        },
    )
}

fn unpaid_workers_summary(economy: Option<&SettlementEconomy>) -> String {
    economy.map_or_else(
        || "Awaiting first payroll audit".to_string(),
        |economy| match economy.unpaid_workers {
            0 => "No current workers at an employer owing wages".to_string(),
            1 => "1 current worker at an employer owing wages".to_string(),
            count => format!("{count} current workers at employers owing wages"),
        },
    )
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
                tiles: town_tiles(place),
                rows: group_rows(
                    vec![
                        ("RESIDENTS".into(), residents),
                        ("UNREST".into(), unrest_summary(place.economy.as_ref())),
                        (
                            "UNREST PRESSURES".into(),
                            unrest_pressure_summary(place.economy.as_ref()),
                        ),
                        (
                            "FOOD SECURITY".into(),
                            food_security_summary(place.economy.as_ref()),
                        ),
                        (
                            "HUNGER".into(),
                            hunger_summary(place.economy.as_ref(), place.residents),
                        ),
                        (
                            "UNEMPLOYMENT".into(),
                            work_seekers_summary(place.economy.as_ref(), place.residents),
                        ),
                        (
                            "HOUSING".into(),
                            housing_summary(place.economy.as_ref(), place.residents),
                        ),
                        (
                            "UNPAID WORKERS".into(),
                            unpaid_workers_summary(place.economy.as_ref()),
                        ),
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
                    OVERVIEW_SECTIONS,
                ),
            }
        }
        SelectedPlaceEntry::Hall => {
            let moot_stewards = place.administration.as_ref().map_or_else(
                || "Vacant".to_string(),
                |office| {
                    if office.city_workers.is_empty() {
                        office
                            .lead_steward
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
                            .lead_steward
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
                tiles: town_tiles(place),
                rows: group_rows(vec![
                    ("OWNER".into(), "The settlement common".into()),
                    (
                        "SERVICES".into(),
                        "Permits / market / common storage".into(),
                    ),
                    ("UNREST".into(), unrest_summary(place.economy.as_ref())),
                    (
                        "UNREST PRESSURES".into(),
                        unrest_pressure_summary(place.economy.as_ref()),
                    ),
                    (
                        "FOOD SECURITY".into(),
                        food_security_summary(place.economy.as_ref()),
                    ),
                    (
                        "HUNGER".into(),
                        hunger_summary(place.economy.as_ref(), place.residents),
                    ),
                    (
                        "HOUSING".into(),
                        housing_summary(place.economy.as_ref(), place.residents),
                    ),
                    (
                        "UNEMPLOYMENT".into(),
                        work_seekers_summary(place.economy.as_ref(), place.residents),
                    ),
                    (
                        "UNPAID WORKERS".into(),
                        unpaid_workers_summary(place.economy.as_ref()),
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
                ], HALL_SECTIONS),
            }
        }
        // Live worksite entries are rendered by `worksite_detail_model` in
        // `sync_place_detail`; a stale entity reaching this pure fn falls back
        // to the overview.
        SelectedPlaceEntry::Worksite(_) => {
            place_detail_model(place, SelectedPlaceEntry::Overview, exact_location)
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
                        SettlementBuildingKind::StoneQuarry => {
                            "Extracts Stone / 2 work positions".into()
                        }
                        SettlementBuildingKind::FishermansHut => {
                            "Produces Food at its fishing pier / 2 work positions".into()
                        }
                        SettlementBuildingKind::LivestockFarm => {
                            "Produces Meat and Wool from its pasture / 2 work positions".into()
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
                // A building page must never be mistaken for the company that
                // runs it: the subtitle names the owner and the town, and the
                // company page is one click back.
                subtitle: match building.owner.as_deref() {
                    Some(owner) if building.for_sale.is_none() => format!(
                        "ONE SITE OF {}  /  IN {}",
                        owner.to_uppercase(),
                        place.name.to_uppercase()
                    ),
                    _ => format!("BUILDING IN {}", place.name.to_uppercase()),
                },
                tiles: Vec::new(),
                rows: group_rows(rows, BUILDING_SECTIONS),
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
    fn a_worksite_page_reports_materials_and_reads_as_a_site_not_a_place() {
        let site = shared::components::ConstructionSite {
            kind: shared::components::SettlementBuildingKind::House,
            settlement: "Brackwater".to_string(),
            raising: false,
            rotation: 0.0,
            stand: Vec3::ZERO,
        };
        let model = worksite_detail_model(&site, 4, 10, shared::economy::Good::Wood, None);
        assert_eq!(model.title, "HOUSE WORKSITE");
        assert_eq!(model.subtitle, "BRACKWATER / AWAITING MATERIALS");
        assert!(model
            .tiles
            .iter()
            .any(|(label, value)| label == "WOOD" && value == "4 / 10"));
        let raising = shared::components::ConstructionSite {
            raising: true,
            ..site
        };
        let model = worksite_detail_model(&raising, 10, 10, shared::economy::Good::Wood, None);
        assert_eq!(model.subtitle, "BRACKWATER / RAISING THE FRAME");
    }

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
            .lines()
            .find(|(label, _)| *label == "STRUCTURES")
            .map(|(_, value)| value);
        assert_eq!(
            structures,
            Some("2 completed / 1 wheat field / 0 fishing piers")
        );
        assert!(model.lines().all(|(_, value)| !value.contains("Ada")));
    }

    #[test]
    fn place_overview_makes_each_public_hardship_a_visible_fact() {
        let mut place = explorer_place();
        place.residents = 20;
        place.economy = Some(SettlementEconomy {
            reserve_days: 0.6,
            recent_food_production: 12.0,
            recent_food_consumption: 20.0,
            unmet_food: 10,
            housing_capacity: 18,
            homeless_residents: 4,
            job_seekers: 5,
            private_vacant_jobs: 2,
            civic_vacant_jobs: 1,
            unpaid_workers: 2,
            unrest: 34.5,
            unrest_target: 34.5,
            unrest_change: 10.0,
            unrest_hunger_pressure: 27.5,
            unrest_housing_pressure: 5.0,
            unrest_wage_pressure: 2.0,
            ..default()
        });

        let model = place_detail_model(&place, SelectedPlaceEntry::Overview, true);
        for label in [
            "UNREST",
            "UNREST PRESSURES",
            "FOOD SECURITY",
            "HUNGER",
            "UNEMPLOYMENT",
            "HOUSING",
            "UNPAID WORKERS",
        ] {
            assert!(
                model.lines().any(|(actual, _)| actual == label),
                "missing {label}"
            );
        }
        assert!(model.lines().any(|(label, value)| {
            label == "UNREST" && value.contains("Uneasy") && value.contains("rising")
        }));
        assert!(model
            .lines()
            .any(|(label, value)| label == "HUNGER" && value.contains("50%")));
    }

    #[test]
    fn selecting_a_building_produces_its_own_detail_sheet() {
        let model = place_detail_model(&explorer_place(), SelectedPlaceEntry::Building(0), true);
        assert_eq!(model.title, "FARMSTEAD");
        // The subtitle names the owner and the town, so a site page can never
        // be mistaken for the company page it was opened from.
        assert_eq!(model.subtitle, "ONE SITE OF ADA  /  IN BRACKWATER");
        assert!(model
            .lines()
            .any(|(label, value)| label == "OWNER" && value == "Ada"));
        assert!(model
            .lines()
            .any(|(label, value)| label == "FARMLAND QUALITY" && value == "76%"));
        assert!(model
            .lines()
            .any(|(label, value)| label == "STORE" && value.contains("Wheat 4")));
    }

    #[test]
    fn processor_detail_sheets_do_not_claim_land_controls_output() {
        let mut place = explorer_place();
        place.buildings[0].kind = SettlementBuildingKind::Windmill;
        let mill = place_detail_model(&place, SelectedPlaceEntry::Building(0), true);
        assert!(mill.lines().all(|(label, _)| !label.contains("QUALITY")));

        place.buildings[0].kind = SettlementBuildingKind::Bakery;
        let bakery = place_detail_model(&place, SelectedPlaceEntry::Building(0), true);
        assert!(bakery.lines().all(|(label, _)| !label.contains("QUALITY")));
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
                model.lines().any(|(actual, _)| actual == label),
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
