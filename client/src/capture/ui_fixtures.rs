//! Deterministic selection, company and retained-UI capture fixtures.

use super::history_fixtures::synthetic_company_history;
use bevy::prelude::*;

/// `FISTFORCE_CAPTURE_WORLD_MAP=1` opens the real modal world map during an
/// offline capture. Combine it with `FISTFORCE_CAPTURE_HERO=default` and
/// `FISTFORCE_CAPTURE_SELECT=1` to verify the owned-hero arrow and camera
/// viewport without mouse automation or a live server.
pub(super) fn open_capture_world_map(
    mut map_open: ResMut<crate::ui::world_map::MapOpen>,
    mut handled: Local<bool>,
) {
    if *handled {
        return;
    }
    *handled = true;
    if std::env::var("FISTFORCE_CAPTURE_WORLD_MAP").is_ok_and(|value| value == "1") {
        map_open.0 = true;
    }
}

/// Put the capture camera on the local branch controls without baking a
/// special layout into the real encyclopedia. This keeps the scrollable
/// directory itself under visual regression coverage.
pub(super) fn position_capture_company_scroll(
    mut viewports: Query<
        &mut ScrollPosition,
        With<crate::ui::encyclopedia::companies::CompanyDetailViewport>,
    >,
) {
    if std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA").as_deref() != Ok("company-stock") {
        return;
    }
    for mut position in viewports.iter_mut() {
        position.y = 410.0;
    }
}

pub(super) fn stage_capture_companies(commands: &mut Commands) {
    use shared::components::{
        BuildingId, BuildingOf, Company, CompanyId, CompanyLeadership, CompanyOwnership,
        CompanyShare, CompanyShareMarket, OperatedBy, PersonId, SettlementBuilding,
        SettlementBuildingKind, SettlementId,
    };
    use shared::economy::{
        BusinessAccount, BusinessCondition, BusinessInputRule, BusinessManagementPolicy,
        BusinessPrivateInputRule, BusinessProcurementPolicy, BusinessSalePolicy,
        BusinessStaffingPolicy, BusinessSupplyPolicy, BusinessWagePolicy, CompanyAccount,
        CompanyBranchPolicies, CompanyDayLedger, CompanyDecisionHistory, CompanyDecisionReason,
        CompanyDecisionRecord, CompanyManagementPolicy, CompanyResourcePolicy, Good,
        GoodsInventory,
    };

    let aldric =
        if std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA").is_ok_and(|mode| mode == "business") {
            // `spawn_capture_heroes` gives the first local stand-in this stable id.
            // Making that person the Master exposes the real authoritative controls.
            PersonId(10_000)
        } else {
            PersonId(1)
        };
    let bryn = PersonId(2);
    let cassia = PersonId(3);
    let first_ownership = CompanyOwnership::from_shares(vec![
        CompanyShare {
            shareholder: aldric,
            shares: 720,
        },
        CompanyShare {
            shareholder: bryn,
            shares: 280,
        },
    ])
    .expect("capture cap table totals 1,000");
    let mut first_market = CompanyShareMarket::default();
    assert!(first_market.list(&first_ownership, bryn, 80, 145, 11));
    let mut decisions = CompanyDecisionHistory::default();
    decisions.push(CompanyDecisionRecord {
        day: 8,
        master: aldric,
        from: shared::economy::BusinessStrategy::Balanced,
        to: shared::economy::BusinessStrategy::Growth,
        reason: CompanyDecisionReason::ProfitableExpansion,
    });
    let brackwater = SettlementId(701);
    let high_meadow = SettlementId(702);
    let rivermeet = SettlementId(703);
    let mut first_branches = CompanyBranchPolicies::default();
    first_branches.set_resource(
        brackwater,
        Good::Wheat,
        CompanyResourcePolicy {
            retain_units: 24,
            sell_excess: true,
        },
    );
    first_branches.set_resource(
        brackwater,
        Good::Flour,
        CompanyResourcePolicy {
            retain_units: 18,
            sell_excess: true,
        },
    );
    first_branches.set_resource(
        high_meadow,
        Good::Bread,
        CompanyResourcePolicy {
            retain_units: 8,
            sell_excess: false,
        },
    );
    commands.spawn((
        CompanyId(501),
        Company {
            name: "Aldric Grain & Bread".to_string(),
            founded_day: 2,
        },
        first_ownership,
        CompanyLeadership { master: aldric },
        first_market,
        CompanyAccount {
            cash: 8_950,
            contributed_capital: 4_000,
            capital_expenditures: 2_700,
            book_value: 2_700,
            owner_withdrawals: 1_250,
            current_day: CompanyDayLedger {
                day: 12,
                external_revenue: 2_480,
                wage_expense: 700,
                external_input_expense: 220,
                market_fees: 124,
                delivery_fees: 40,
                profit_taxes: 140,
                owner_withdrawals: 350,
                capital_expenditures: 0,
                internal_revenue: 1_100,
                internal_input_expense: 1_100,
            },
            previous_day: CompanyDayLedger {
                day: 11,
                external_revenue: 2_150,
                wage_expense: 700,
                external_input_expense: 180,
                market_fees: 108,
                delivery_fees: 40,
                profit_taxes: 112,
                owner_withdrawals: 250,
                capital_expenditures: 0,
                internal_revenue: 900,
                internal_input_expense: 900,
            },
            ..default()
        },
        CompanyManagementPolicy {
            strategy: shared::economy::BusinessStrategy::Growth,
            ..default()
        },
        first_branches,
        decisions,
    ));

    let second_ownership = CompanyOwnership::from_shares(vec![
        CompanyShare {
            shareholder: cassia,
            shares: 880,
        },
        CompanyShare {
            shareholder: aldric,
            shares: 120,
        },
    ])
    .expect("capture cap table totals 1,000");
    commands.spawn((
        CompanyId(502),
        Company {
            name: "Cassia River Fish".to_string(),
            founded_day: 5,
        },
        second_ownership,
        CompanyLeadership { master: cassia },
        CompanyShareMarket::default(),
        CompanyAccount {
            cash: 2_240,
            book_value: 900,
            wage_arrears: 125,
            current_day: CompanyDayLedger {
                day: 12,
                external_revenue: 650,
                wage_expense: 300,
                market_fees: 32,
                delivery_fees: 25,
                ..CompanyDayLedger::empty(12)
            },
            ..default()
        },
        CompanyManagementPolicy::default(),
        CompanyBranchPolicies::default(),
        CompanyDecisionHistory::default(),
    ));

    let sites = [
        (
            BuildingId(601),
            SettlementBuildingKind::Farmstead,
            "Brackwater",
            brackwater,
            Good::Wheat,
            18,
            4_100,
        ),
        (
            BuildingId(602),
            SettlementBuildingKind::Windmill,
            "Brackwater",
            brackwater,
            Good::Flour,
            50,
            2_450,
        ),
        (
            BuildingId(603),
            SettlementBuildingKind::Bakery,
            "High Meadow",
            high_meadow,
            Good::Bread,
            12,
            2_400,
        ),
    ];
    for (index, (id, kind, settlement, settlement_id, output, stock, cash)) in
        sites.into_iter().enumerate()
    {
        let mut inventory = GoodsInventory::new(kind.storage_bulk_capacity());
        inventory.add(output, stock);
        let mut procurement = BusinessProcurementPolicy::none();
        let mut supply = BusinessSupplyPolicy::none();
        if let Some(recipe) = match kind {
            SettlementBuildingKind::Windmill => Some((Good::Wheat, 9, 18, 250)),
            SettlementBuildingKind::Bakery => Some((Good::Flour, 15, 30, 300)),
            _ => None,
        } {
            procurement.set_rule(
                recipe.0,
                BusinessInputRule {
                    enabled: true,
                    coverage_days: 2,
                    reorder_below: recipe.1,
                    target_units: recipe.2,
                    maximum_unit_price: recipe.3,
                },
            );
            supply.set_rule(
                recipe.0,
                BusinessPrivateInputRule {
                    enabled: true,
                    ..default()
                },
            );
            inventory.add(recipe.0, recipe.1);
        }
        let mut account = BusinessAccount::with_capital(cash);
        account.current_day = shared::economy::BusinessDayLedger {
            day: 12,
            gross_revenue: 700 + index as u64 * 240,
            internal_revenue: if index < 2 { 350 } else { 0 },
            wage_expense: 200,
            market_fees: 35,
            profit_taxes: 45,
            ..shared::economy::BusinessDayLedger::empty(12)
        };
        let sale = BusinessSalePolicy::for_good(output);
        commands.spawn((
            id,
            BuildingOf(settlement_id),
            OperatedBy(CompanyId(501)),
            SettlementBuilding {
                kind,
                settlement: settlement.to_string(),
                owner: Some("Aldric".to_string()),
                quality: 0.8,
                workers: vec!["Worker".to_string(); usize::from(kind.positions())],
            },
            inventory,
            account,
            BusinessCondition {
                state: shared::economy::BusinessState::Operating,
                ..default()
            },
            sale,
            BusinessManagementPolicy::default(),
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(kind.positions()),
            procurement,
            supply,
        ));
    }

    let mut depot_stock = GoodsInventory::new(shared::economy::capacity::STORAGE_HALL);
    depot_stock.add(Good::Wheat, 70);
    depot_stock.add(Good::Flour, 32);
    commands.spawn((
        BuildingId(605),
        BuildingOf(brackwater),
        OperatedBy(CompanyId(501)),
        SettlementBuilding {
            kind: SettlementBuildingKind::StorageHall,
            settlement: "Brackwater".to_string(),
            owner: Some("Aldric".to_string()),
            quality: 0.5,
            workers: vec!["Company Porter".to_string()],
        },
        depot_stock,
        BusinessAccount::with_capital(0),
        BusinessCondition::default(),
        BusinessSalePolicy::default(),
        BusinessManagementPolicy::default(),
        BusinessWagePolicy::default(),
        BusinessStaffingPolicy::new(1),
        BusinessProcurementPolicy::none(),
        BusinessSupplyPolicy::none(),
    ));

    let mut fish =
        GoodsInventory::new(SettlementBuildingKind::FishermansHut.storage_bulk_capacity());
    fish.add(Good::Food, 9);
    commands.spawn((
        BuildingId(604),
        BuildingOf(rivermeet),
        OperatedBy(CompanyId(502)),
        SettlementBuilding {
            kind: SettlementBuildingKind::FishermansHut,
            settlement: "Rivermeet".to_string(),
            owner: Some("Cassia".to_string()),
            quality: 0.7,
            workers: vec!["Fisher".to_string()],
        },
        fish,
        BusinessAccount::with_capital(2_240),
        BusinessCondition::default(),
        BusinessSalePolicy::for_good(Good::Food),
        BusinessManagementPolicy::default(),
        BusinessWagePolicy::default(),
        BusinessStaffingPolicy::new(1),
        BusinessProcurementPolicy::none(),
        BusinessSupplyPolicy::default(),
    ));

    commands.queue(|world: &mut World| {
        world
            .resource_mut::<crate::ui::history::SettlementHistoryCache>()
            .companies
            .insert(CompanyId(501), synthetic_company_history(CompanyId(501)));
    });
}

/// Open the actual site-controls page over the staged company directory.
/// This is a rendering fixture only; it does not invent a second UI model.
pub(super) fn open_capture_business_management(
    heroes: Query<(&shared::components::Hero, &shared::components::PersonId)>,
    sites: Query<(Entity, &shared::components::BuildingId)>,
    mut target: ResMut<crate::ui::business_management::BusinessManagementTarget>,
    mut return_to: ResMut<crate::ui::business_management::BusinessManagementReturn>,
    mut encyclopedia: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut opened: Local<bool>,
    mut commands: Commands,
) {
    if *opened
        || !std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA").is_ok_and(|mode| mode == "business")
    {
        return;
    }
    let Some((hero, _)) = heroes.iter().next() else {
        return;
    };
    let Some((site, _)) = sites.iter().find(|(_, id)| id.0 == 602) else {
        return;
    };
    commands.insert_resource(crate::camera_rts::LocalPeerId(
        shared::player::peer_id_to_u64(hero.owner),
    ));
    target.0 = Some(site);
    return_to.0 = Some(shared::components::CompanyId(501));
    // The controls are an encyclopedia page: keep the window open so the page
    // host exists (closing it would clear the target again).
    encyclopedia.0 = true;
    *opened = true;
}

/// `FISTFORCE_CAPTURE_BUSINESS_SCROLL=<px>` scrolls the site-controls page so
/// its lower sections (meters, input rows) can be photographed.
pub(super) fn scroll_capture_business_page(
    mut bodies: Query<&mut ScrollPosition, With<crate::ui::business_management::BodyScroll>>,
) {
    let Some(offset) = std::env::var("FISTFORCE_CAPTURE_BUSINESS_SCROLL")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
    else {
        return;
    };
    for mut scroll in bodies.iter_mut() {
        if scroll.0.y != offset {
            scroll.0.y = offset;
        }
    }
}

/// Open history after the commander camera has rendered ordinary world frames.
/// A modal present on the very first offline capture frame prevents the capture
/// harness's camera from completing its initial convergence; real players can
/// only open this after entering the world, so the delay mirrors actual use.
pub(super) fn open_capture_history(
    settlements: Query<(Entity, &shared::components::Settlement)>,
    market: Res<crate::ui::market::MarketPageTarget>,
    mut target: ResMut<crate::ui::history::HistoryPanelTarget>,
    mut frames: Local<u8>,
) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HISTORY") else {
        return;
    };
    if target.0.is_some() {
        return;
    }
    *frames = frames.saturating_add(1);
    if *frames < 30 {
        return;
    }
    let hall = settlements
        .iter()
        .find(|(_, settlement)| settlement.name == "Brackwater")
        .map(|(entity, _)| entity);
    let view = match mode.as_str() {
        "market" => crate::ui::history::HistoryView::Market(shared::economy::Good::Wood),
        "business" => {
            crate::ui::history::HistoryView::Business(shared::components::BuildingId(100))
        }
        "world" => crate::ui::history::HistoryView::World,
        "company" => crate::ui::history::HistoryView::Company(shared::components::CompanyId(501)),
        _ => crate::ui::history::HistoryView::Village,
    };
    let global_view = matches!(
        view,
        crate::ui::history::HistoryView::World | crate::ui::history::HistoryView::Company(_)
    );
    let settlement = if global_view { None } else { hall };
    if !global_view && settlement.is_none() {
        return;
    }
    target.0 = Some(crate::ui::history::HistoryTarget {
        settlement,
        place: match view {
            crate::ui::history::HistoryView::World => "World".to_string(),
            crate::ui::history::HistoryView::Company(_) => "Aldric Grain & Bread".to_string(),
            _ => "Brackwater".to_string(),
        },
        view,
        return_to_market: matches!(view, crate::ui::history::HistoryView::Market(_))
            && market.0.is_some(),
    });
}

/// FISTFORCE_CAPTURE_DRAG_BOX="x0,y0,x1,y1[,ui_scale]" pins the drag-select
/// marquee to a known rectangle in WINDOW pixels, so where it actually lands on
/// screen can be measured instead of eyeballed.
///
/// The optional ui_scale reproduces the macOS setup, where the window takes a
/// scale-factor override of 1.0 and the Retina factor lives in `UiScale` -- the
/// exact condition under which cursor pixels and UI pixels stop being the same
/// unit, which is what put the marquee in the wrong place.
pub(super) fn force_capture_drag_box(
    mut drag: ResMut<crate::selection::DragBox>,
    mut ui_scale: ResMut<bevy::ui::UiScale>,
) {
    let Ok(spec) = std::env::var("FISTFORCE_CAPTURE_DRAG_BOX") else {
        return;
    };
    let parts: Vec<f32> = spec
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() < 4 {
        return;
    }
    if let Some(scale) = parts.get(4) {
        if ui_scale.0 != *scale {
            ui_scale.0 = *scale;
        }
    }
    drag.start = Some(Vec2::new(parts[0], parts[1]));
    drag.current = Vec2::new(parts[2], parts[3]);
    drag.active = true;
}

/// FISTFORCE_CAPTURE_SELECT_PERSON=<name> selects that person in the
/// encyclopedia once they actually exist.
///
/// Retried rather than set once: `rebuild_people_list` drops a selection that is
/// not in the visible list, and at startup the list is empty, so a one-shot set
/// is cleared before the characters have even been learned.
/// FISTFORCE_CAPTURE_SELECT_PLACE=<name>, retried for the same reason as the
/// person selector: the list is empty at startup and drops a selection it does
/// not contain.
pub(super) fn select_capture_place(
    places: Res<crate::ui::encyclopedia::places::KnownPlaces>,
    mut selected: ResMut<crate::ui::encyclopedia::places::SelectedPlace>,
    mut entry: ResMut<crate::ui::encyclopedia::places::SelectedPlaceEntry>,
    mut commands: Commands,
    mut staged_worksite: Local<Option<Entity>>,
    sites: Query<Entity, With<shared::components::ConstructionSite>>,
) {
    let Ok(wanted) = std::env::var("FISTFORCE_CAPTURE_SELECT_PLACE") else {
        return;
    };
    let Some(place) = places.find(&wanted) else {
        return;
    };
    if selected.0 != Some(place.id) {
        selected.0 = Some(place.id);
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
    }
    let Ok(building) = std::env::var("FISTFORCE_CAPTURE_SELECT_BUILDING") else {
        return;
    };
    if building.eq_ignore_ascii_case("worksite") {
        // The staged places snapshot has no live site entities; spawn one so
        // the worksite page (status, materials, SEND MY HERO) can be
        // photographed offline.
        let site = sites.iter().next().or(*staged_worksite).unwrap_or_else(|| {
            let mut inventory =
                shared::economy::GoodsInventory::new(shared::economy::capacity::VILLAGER);
            inventory.add(shared::economy::Good::Wood, 4);
            let site = commands
                .spawn((
                    shared::components::ConstructionSite {
                        kind: shared::components::SettlementBuildingKind::House,
                        settlement: place.name.clone(),
                        raising: false,
                        rotation: 0.0,
                        stand: Vec3::ZERO,
                    },
                    inventory,
                ))
                .id();
            *staged_worksite = Some(site);
            site
        });
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Worksite(site);
    } else if building.eq_ignore_ascii_case("overview") {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
    } else if building.eq_ignore_ascii_case("hall")
        || building.eq_ignore_ascii_case(shared::components::SettlementBuildingKind::Hall.label())
    {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Hall;
    } else if let Some((index, _)) = place
        .buildings
        .iter()
        .enumerate()
        .find(|(_, record)| record.kind.label().eq_ignore_ascii_case(&building))
    {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Building(index);
    }
}

pub(super) fn select_capture_person(
    people: Res<crate::ui::encyclopedia::KnownPeople>,
    mut selected: ResMut<crate::ui::encyclopedia::SelectedPerson>,
) {
    if selected.0.is_some() {
        return;
    }
    let Ok(wanted) = std::env::var("FISTFORCE_CAPTURE_SELECT_PERSON") else {
        return;
    };
    if wanted.eq_ignore_ascii_case("first") {
        if let Some(record) = people.records.iter().find(|record| record.known) {
            selected.0 = Some(record.id);
        }
        return;
    }
    if wanted.eq_ignore_ascii_case("hungry") {
        if let Some(record) = people.records.iter().find(|record| {
            record
                .nutrition
                .is_some_and(|nutrition| nutrition.is_hungry())
        }) {
            selected.0 = Some(record.id);
        }
        return;
    }
    if let Some(record) = people.find(&wanted) {
        selected.0 = Some(record.id);
    }
}
