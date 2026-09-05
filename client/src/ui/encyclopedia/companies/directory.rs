//! Bounded snapshots of replicated company, branch, site and route data.

use super::model::{
    CompanyBranchRecord, CompanyDirectory, CompanyHolderRecord, CompanyOfferRecord, CompanyRecord,
    CompanyRouteRecord, CompanyRouteStopRecord, CompanySettlementRecord, CompanySiteRecord,
};
use crate::ui::encyclopedia::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::{
    BuildingId, BuildingOf, CharacterName, Company, CompanyId, CompanyLeadership, CompanyOwnership,
    CompanyShareMarket, CompanyTradeRoute, Hero, OperatedBy, PersonId, SettlementBuilding,
    SettlementBuildingKind, SettlementId, SettlementSummary, TradeRouteHistory, TradeRouteId,
    TradeRouteSchedule,
};
use shared::economy::{
    BusinessAccount, BusinessCondition, BusinessProcurementPolicy, BusinessStaffingPolicy,
    BusinessSupplyPolicy, CompanyAccount, CompanyBranchPolicies, CompanyDecisionHistory,
    CompanyManagementPolicy, CompanyResourcePolicy, Good, GoodsInventory, Wallet,
};

pub(in crate::ui::encyclopedia) fn company_tab_active(tab: Res<EncyclopediaTab>) -> bool {
    *tab == EncyclopediaTab::Companies
}

#[allow(clippy::type_complexity)]
pub(in crate::ui::encyclopedia) fn refresh_company_directory(
    companies: Query<(
        &CompanyId,
        &Company,
        &CompanyOwnership,
        &CompanyLeadership,
        &CompanyAccount,
        &CompanyManagementPolicy,
        Option<&CompanyBranchPolicies>,
        &CompanyDecisionHistory,
        &CompanyShareMarket,
    )>,
    sites: Query<(
        Entity,
        &BuildingId,
        &OperatedBy,
        &BuildingOf,
        &SettlementBuilding,
        Option<&BusinessAccount>,
        Option<&BusinessCondition>,
        Option<&GoodsInventory>,
        Option<&shared::economy::BusinessSalePolicy>,
        Option<&BusinessProcurementPolicy>,
        Option<&BusinessSupplyPolicy>,
        Option<&BusinessStaffingPolicy>,
    )>,
    routes: Query<(
        &TradeRouteId,
        &CompanyTradeRoute,
        &TradeRouteSchedule,
        &TradeRouteHistory,
    )>,
    settlements: Query<&SettlementSummary>,
    people: Query<(&PersonId, &CharacterName, Option<&GoodsInventory>)>,
    heroes: Query<(&Hero, &PersonId, Option<&Wallet>)>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    known_people: Res<KnownPeople>,
    mut directory: ResMut<CompanyDirectory>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
    time: Res<Time>,
    mut last_run: Local<Option<f32>>,
) {
    let mut _ui_scope = ui_perf.scope("refresh_company_directory");
    // A snapshot clones every company's books. Twice a second is plenty for
    // a ledger view and drops the per-frame allocation churn at scale.
    let now = time.elapsed_secs();
    if last_run.is_some_and(|last| now - last < COMPANY_SNAPSHOT_INTERVAL_SECS) {
        return;
    }
    *last_run = Some(now);
    let mut names: HashMap<PersonId, String> = known_people
        .records
        .iter()
        .filter(|record| record.id.is_assigned())
        .map(|record| (record.id, record.name.clone()))
        .collect();
    for (id, name, _) in people.iter() {
        names.insert(*id, name.0.clone());
    }
    let person_name = |person: PersonId| {
        names
            .get(&person)
            .cloned()
            .unwrap_or_else(|| format!("Person #{}", person.0))
    };
    let replicated_local = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            .map(|(_, person, wallet)| (*person, wallet.map(|wallet| wallet.balance())))
    });
    let roster_local = known_people
        .records
        .iter()
        .find(|record| record.is_self && record.id.is_assigned())
        .map(|record| (record.id, record.wallet));
    let (local_person, local_wallet) = replicated_local.or(roster_local).unzip();
    let local_wallet = local_wallet.flatten();

    let settlement_names: HashMap<SettlementId, (String, bool)> = settlements
        .iter()
        .map(|settlement| {
            (
                settlement.id,
                (settlement.name.clone(), settlement.has_marketplace),
            )
        })
        .collect();
    let settlement_name = |settlement: SettlementId| {
        settlement_names
            .get(&settlement)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| format!("Settlement #{}", settlement.0))
    };
    let mut known_settlements: Vec<_> = settlement_names
        .iter()
        .map(|(id, (name, has_marketplace))| CompanySettlementRecord {
            id: *id,
            name: name.clone(),
            has_marketplace: *has_marketplace,
        })
        .collect();
    known_settlements.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    let warehouse_names: HashMap<BuildingId, String> = sites
        .iter()
        .filter_map(|(_, id, _, _, building, ..)| {
            (building.kind == SettlementBuildingKind::StorageHall).then_some((
                *id,
                format!("Storage Hall #{} / {}", id.0, building.settlement),
            ))
        })
        .collect();
    let mut routes_by_company: HashMap<CompanyId, Vec<CompanyRouteRecord>> = HashMap::new();
    for (id, route, schedule, history) in routes.iter() {
        let cargo_onboard = route.assigned_caravaner.map_or(0, |assigned| {
            people
                .iter()
                .find(|(person, ..)| **person == assigned)
                .and_then(|(_, _, inventory)| inventory)
                .map_or(0, |inventory| inventory.amount(route.good))
        });
        routes_by_company
            .entry(route.company)
            .or_default()
            .push(CompanyRouteRecord {
                id: *id,
                warehouse: route.warehouse,
                warehouse_name: warehouse_names
                    .get(&route.warehouse)
                    .cloned()
                    .unwrap_or_else(|| format!("Storage Hall #{}", route.warehouse.0)),
                mode: route.mode,
                origin: settlement_name(route.origin),
                destination: settlement_name(route.destination),
                good: route.good,
                cargo_target: route.cargo_target,
                cargo_onboard,
                maximum_purchase_price: route.maximum_purchase_price,
                minimum_destination_price: route.minimum_destination_price,
                automatic: route.automatic,
                autonomous_management: route.autonomous_management,
                expected_trip_profit: route.expected_trip_profit,
                decision_confidence: route.decision_confidence,
                assigned_caravaner: route.assigned_caravaner.map(&person_name),
                current_stop: route.current_stop,
                status: route.status,
                completed_trips: route.completed_trips,
                lifetime_units: route.lifetime_units,
                lifetime_delivery_revenue: route.lifetime_delivery_revenue,
                lifetime_purchase_cost: route.lifetime_purchase_cost,
                lifetime_consigned_value: route.lifetime_consigned_value,
                stops: schedule
                    .stops()
                    .iter()
                    .map(|stop| CompanyRouteStopRecord {
                        settlement: stop.settlement,
                        settlement_name: settlement_name(stop.settlement),
                        action: stop.action,
                    })
                    .collect(),
                trips: history.trips().to_vec(),
                latest_trip: history.trips().last().copied(),
            });
    }
    for routes in routes_by_company.values_mut() {
        routes.sort_by_key(|route| route.id);
    }

    let mut sites_by_company: HashMap<CompanyId, Vec<CompanySiteRecord>> = HashMap::new();
    for (
        entity,
        building_id,
        operated_by,
        building_of,
        building,
        site_account,
        condition,
        inventory,
        sale,
        procurement,
        supply,
        staffing,
    ) in sites.iter()
    {
        let output = output_good(building.kind);
        let input = input_good(building.kind);
        let sale = sale.copied();
        let output_stock = output.map_or(0, |good| {
            inventory.map_or(0, |inventory| inventory.amount(good))
        });
        let input_stock = input.map_or(0, |good| {
            inventory.map_or(0, |inventory| inventory.amount(good))
        });
        let input_rule = input.and_then(|good| procurement.map(|policy| policy.rule(good)));
        let private_rule = input.and_then(|good| supply.map(|policy| policy.rule(good)));
        let account = site_account.copied().unwrap_or_default();
        sites_by_company
            .entry(operated_by.0)
            .or_default()
            .push(CompanySiteRecord {
                entity,
                id: *building_id,
                settlement: building.settlement.clone(),
                settlement_id: building_of.0,
                kind: building.kind,
                workers: building.workers.len(),
                positions: building.kind.positions(),
                enabled_positions: staffing
                    .copied()
                    .unwrap_or_default()
                    .target_for(building.kind),
                state: condition.copied().unwrap_or_default().state,
                wage_arrears: account.wage_arrears,
                tax_arrears: account.tax_arrears,
                current_day: account.current_day,
                previous_day: account.previous_day,
                output,
                output_stock,
                asking_price: sale.map(|policy| policy.asking_unit_price),
                input,
                input_stock,
                input_target: input_rule.map_or(0, |rule| rule.target_units),
                input_coverage_days: input_rule.map_or(0, |rule| rule.coverage_days),
                sourcing: private_rule
                    .filter(|rule| rule.enabled)
                    .map(|rule| rule.sourcing),
                preferred_supplier: private_rule.and_then(|rule| rule.preferred_supplier),
                goods: inventory.map_or_else(Vec::new, |inventory| {
                    Good::ALL
                        .into_iter()
                        .filter_map(|good| {
                            let amount = inventory.amount(good);
                            (amount > 0).then_some((good, amount))
                        })
                        .collect()
                }),
                used_bulk: inventory.map_or(0, GoodsInventory::used_bulk),
                bulk_capacity: inventory.map_or(0, GoodsInventory::bulk_capacity),
            });
    }

    let mut records = Vec::new();
    for (
        id,
        company,
        ownership,
        leadership,
        account,
        policy,
        branch_policies,
        decisions,
        share_market,
    ) in companies.iter()
    {
        let mut holders: Vec<_> = ownership
            .shares()
            .iter()
            .map(|share| CompanyHolderRecord {
                person: share.shareholder,
                name: person_name(share.shareholder),
                shares: share.shares,
            })
            .collect();
        holders.sort_by(|a, b| b.shares.cmp(&a.shares).then_with(|| a.name.cmp(&b.name)));

        let mut offers: Vec<_> = share_market
            .offers()
            .iter()
            .map(|offer| CompanyOfferRecord {
                seller: offer.seller,
                seller_name: person_name(offer.seller),
                shares: offer.shares,
                unit_price: offer.unit_price,
                listed_day: offer.listed_day,
            })
            .collect();
        offers.sort_by_key(|offer| (offer.unit_price, offer.listed_day, offer.seller));

        let mut company_sites = sites_by_company.remove(id).unwrap_or_default();
        company_sites.sort_by_key(|site| (site.settlement.clone(), site.kind.label(), site.id));
        let mut branches_by_settlement: HashMap<SettlementId, CompanyBranchRecord> = HashMap::new();
        for site in &company_sites {
            let branch = branches_by_settlement
                .entry(site.settlement_id)
                .or_insert_with(|| CompanyBranchRecord {
                    settlement: site.settlement.clone(),
                    settlement_id: site.settlement_id,
                    sites: 0,
                    storage_halls: 0,
                    used_bulk: 0,
                    bulk_capacity: 0,
                    resources: Good::ALL
                        .into_iter()
                        .map(|good| {
                            (
                                good,
                                0,
                                branch_policies
                                    .map_or_else(CompanyResourcePolicy::default, |policies| {
                                        policies.resource(site.settlement_id, good)
                                    }),
                            )
                        })
                        .collect(),
                });
            branch.sites += 1;
            branch.storage_halls += usize::from(site.kind == SettlementBuildingKind::StorageHall);
            branch.used_bulk = branch.used_bulk.saturating_add(site.used_bulk);
            branch.bulk_capacity = branch.bulk_capacity.saturating_add(site.bulk_capacity);
            for (good, amount) in &site.goods {
                if let Some((_, total, _)) = branch
                    .resources
                    .iter_mut()
                    .find(|(candidate, ..)| candidate == good)
                {
                    *total = total.saturating_add(*amount);
                }
            }
        }
        let mut branches: Vec<_> = branches_by_settlement.into_values().collect();
        branches.sort_by(|a, b| a.settlement.cmp(&b.settlement));

        records.push(CompanyRecord {
            id: *id,
            name: company.name.clone(),
            founded_day: company.founded_day,
            master: leadership.master,
            master_name: person_name(leadership.master),
            account: *account,
            policy: *policy,
            holders,
            offers,
            decisions: decisions.entries().to_vec(),
            sites: company_sites,
            branches,
            routes: routes_by_company.remove(id).unwrap_or_default(),
        });
    }
    records.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });

    let next = CompanyDirectory {
        records,
        settlements: known_settlements,
        local_person,
        local_wallet,
    };
    if *directory != next {
        _ui_scope.rebuilt();
        *directory = next;
    }
}

/// How often the company directory re-snapshots the replicated books.
pub(super) const COMPANY_SNAPSHOT_INTERVAL_SECS: f32 = 0.5;

pub(super) fn output_good(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::Windmill => Some(Good::Flour),
        SettlementBuildingKind::Bakery => Some(Good::Bread),
        SettlementBuildingKind::StoneQuarry => Some(Good::Stone),
        SettlementBuildingKind::LivestockFarm => Some(Good::Meat),
        _ => None,
    }
}

pub(super) fn input_good(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Windmill => Some(Good::Wheat),
        SettlementBuildingKind::Bakery => Some(Good::Flour),
        _ => None,
    }
}
