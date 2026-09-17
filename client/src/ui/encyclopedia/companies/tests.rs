//! Company accounting presentation and selective-rebuild regressions.

use super::binding::{
    company_structure_key, CompanyBound, CompanyField, CompanyView, DayLedger, EditorField,
    PortfolioField, ResourceField, RetainStep, SiteField,
};
use super::controls::{
    CompanyBranchPolicyButton, CompanyManagementButton, CompanyRow, CompanySiteButton,
    EditTradeRouteButton, TradeRouteEditorButton, TradeRouteQuickActionButton,
};
use super::model::{
    CompanyBranchRecord, CompanyDirectory, CompanyFilter, CompanyHolderRecord, CompanyOfferRecord,
    CompanyPolicyFeedback, CompanyRecord, CompanyRouteRecord, CompanyRouteStopRecord,
    CompanySettlementRecord, CompanySiteRecord, CompanySort, CompanySortKey, TradeRouteDraft,
    TradeRouteEditorAction, TradeRouteEditorState, TradeRouteQuickAction,
};
use super::portfolio::CompanyRowStatus;
use super::route_editor::spawn_trade_route_editor;
use super::routes::spawn_route_card;
use super::sites::spawn_site_card;
use super::view::{compare_companies, visible_companies};
use crate::ui::encyclopedia::search::EncyclopediaSearch;
use crate::ui::encyclopedia::EncyclopediaTab;
use bevy::prelude::*;
use shared::components::{
    BuildingId, CompanyId, PersonId, SettlementBuildingKind, SettlementId, TradeRouteId,
    TradeRouteMode, TradeRouteStatus, TradeRouteStop, TradeRouteStopAction,
};
use shared::economy::{
    BusinessSourcingMode, BusinessState, CompanyAccount, CompanyManagementPolicy,
    CompanyResourcePolicy, Good,
};
use shared::protocol::HeroCompanyAction;

/// Owns the borrowed inputs of a [`CompanyView`] for spawn-helper tests.
struct ViewFixture {
    company: CompanyRecord,
    directory: CompanyDirectory,
    feedback: CompanyPolicyFeedback,
    editor: TradeRouteEditorState,
}

impl ViewFixture {
    fn new(company: CompanyRecord, directory: CompanyDirectory) -> Self {
        Self {
            company,
            directory,
            feedback: CompanyPolicyFeedback::default(),
            editor: TradeRouteEditorState::default(),
        }
    }

    /// The selected company as seen by its own master (every control shown).
    fn managed(company: CompanyRecord) -> Self {
        let directory = CompanyDirectory {
            records: vec![company.clone()],
            settlements: Vec::new(),
            local_person: Some(company.master),
            local_wallet: Some(1_000),
        };
        Self::new(company, directory)
    }

    fn view(&self) -> CompanyView<'_> {
        CompanyView {
            company: &self.company,
            directory: &self.directory,
            feedback: &self.feedback,
            editor: &self.editor,
        }
    }
}

fn bound_text(world: &mut World, key: CompanyBound) -> String {
    world
        .query::<(&CompanyBound, &Text)>()
        .iter(world)
        .find(|(candidate, _)| **candidate == key)
        .map(|(_, text)| text.0.clone())
        .unwrap_or_else(|| panic!("no bound text for {key:?}"))
}

fn image_nodes(world: &mut World) -> Vec<Entity> {
    let mut nodes: Vec<_> = world
        .query_filtered::<Entity, With<ImageNode>>()
        .iter(world)
        .collect();
    nodes.sort_unstable();
    nodes
}

#[test]
fn company_selection_styles_change_only_when_selection_changes() {
    use super::controls::{style_company_controls, CompanyFilterButton, CompanyRow};
    use super::model::SelectedCompany;
    use crate::ui::foundation::UiButtonStyle;

    #[derive(Resource, Default)]
    struct StyleChanges(usize);

    fn count_style_changes(
        changed: Query<(), Changed<UiButtonStyle>>,
        mut count: ResMut<StyleChanges>,
    ) {
        count.0 = changed.iter().count();
    }

    let mut app = App::new();
    app.init_resource::<CompanyFilter>();
    app.init_resource::<CompanySort>();
    app.insert_resource(SelectedCompany(Some(CompanyId(1))));
    app.init_resource::<StyleChanges>();
    app.add_systems(
        Update,
        (style_company_controls, count_style_changes).chain(),
    );
    for filter in [CompanyFilter::All, CompanyFilter::MyHoldings] {
        app.world_mut()
            .spawn((CompanyFilterButton(filter), UiButtonStyle::default()));
    }
    for id in [1, 2] {
        app.world_mut()
            .spawn((CompanyRow(CompanyId(id)), UiButtonStyle::default()));
    }
    app.update();
    assert_eq!(app.world().resource::<StyleChanges>().0, 4);

    app.update();
    assert_eq!(
        app.world().resource::<StyleChanges>().0,
        0,
        "idle frames must not dirty button styles"
    );

    *app.world_mut().resource_mut::<CompanyFilter>() = CompanyFilter::MyHoldings;
    app.world_mut().resource_mut::<SelectedCompany>().0 = Some(CompanyId(2));
    app.update();
    assert_eq!(app.world().resource::<StyleChanges>().0, 4);
    let world = app.world_mut();
    for (row, style) in world.query::<(&CompanyRow, &UiButtonStyle)>().iter(world) {
        assert_eq!(style.selected, row.0 == CompanyId(2));
    }
    for (filter, style) in world
        .query::<(&CompanyFilterButton, &UiButtonStyle)>()
        .iter(world)
    {
        assert_eq!(style.selected, filter.0 == CompanyFilter::MyHoldings);
    }
    app.update();
    assert_eq!(app.world().resource::<StyleChanges>().0, 0);
}

fn company(id: u64, owner: PersonId, cash: u64, assets: u64, debt: u64) -> CompanyRecord {
    CompanyRecord {
        id: CompanyId(id),
        name: format!("Company {id}"),
        founded_day: 1,
        master: owner,
        master_name: "Owner".to_string(),
        account: CompanyAccount {
            cash,
            wage_arrears: debt,
            book_value: assets,
            ..default()
        },
        policy: CompanyManagementPolicy::default(),
        capacity: None,
        ownership: shared::components::CompanyOwnership::sole(owner),
        holders: vec![CompanyHolderRecord {
            person: owner,
            name: "Owner".to_string(),
            shares: 1_000,
        }],
        offers: Vec::new(),
        decisions: Vec::new(),
        sites: Vec::new(),
        branches: Vec::new(),
        routes: Vec::new(),
        fleet: default(),
    }
}

#[test]
fn portfolio_interest_is_pro_rata_and_never_overdraws_debt() {
    let owner = PersonId(4);
    let healthy = company(1, owner, 1_000, 3_000, 500);
    assert_eq!(healthy.accounting_equity(), 3_500);
    assert_eq!(healthy.holding_book_interest(250), 875);

    let insolvent = company(2, owner, 100, 50, 1_000);
    assert_eq!(insolvent.accounting_equity(), 0);
}

#[test]
fn holdings_filter_supports_more_than_one_company() {
    let owner = PersonId(8);
    let outsider = PersonId(9);
    let directory = CompanyDirectory {
        records: vec![
            company(1, owner, 0, 0, 0),
            company(2, owner, 0, 0, 0),
            company(3, outsider, 0, 0, 0),
        ],
        settlements: Vec::new(),
        local_person: Some(owner),
        local_wallet: Some(2_000),
    };
    let visible = visible_companies(
        &directory,
        CompanyFilter::MyHoldings,
        &EncyclopediaSearch::default(),
        CompanySort::default(),
    );
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].id, CompanyId(1));
    assert_eq!(visible[1].id, CompanyId(2));
}

#[test]
fn company_search_matches_name_master_and_branch_settlement() {
    let owner = PersonId(8);
    let mut grain = company(1, owner, 0, 0, 0);
    grain.name = "Aldric Grain & Bread".into();
    grain.master_name = "Bryn".into();
    grain.branches = vec![CompanyBranchRecord {
        settlement: "Brackwater".into(),
        settlement_id: SettlementId(1),
        sites: 1,
        storage_halls: 0,
        used_bulk: 0,
        bulk_capacity: 0,
        resources: Vec::new(),
    }];
    let mut fish = company(2, owner, 0, 0, 0);
    fish.name = "Cassia River Fish".into();
    fish.master_name = "Cassia".into();
    let mut search = EncyclopediaSearch::default();
    assert!(
        search.matches_company(&grain) && search.matches_company(&fish),
        "an empty query hides nothing"
    );
    search.set_query(EncyclopediaTab::Companies, "  cAsSia ");
    assert!(!search.matches_company(&grain));
    assert!(search.matches_company(&fish), "name, any case, trimmed");
    search.set_query(EncyclopediaTab::Companies, "bryn");
    assert!(search.matches_company(&grain), "the Master's name");
    assert!(!search.matches_company(&fish));
    search.set_query(EncyclopediaTab::Companies, "brack");
    assert!(search.matches_company(&grain), "a town the company operates in");
    assert!(!search.matches_company(&fish));
    search.set_query(EncyclopediaTab::Companies, "bread brack");
    assert!(search.matches_company(&grain), "every term may hit a different field");
    search.set_query(EncyclopediaTab::Companies, "bread cassia");
    assert!(!search.matches_company(&grain) && !search.matches_company(&fish));
    // The People draft is a different field and never narrows this directory.
    search.clear_companies();
    search.set_query(EncyclopediaTab::People, "nobody");
    assert!(search.matches_company(&grain) && search.matches_company(&fish));
    assert!(!search.active(EncyclopediaTab::Companies));
}

#[test]
fn comparator_reverses_only_the_primary_key_and_breaks_ties_by_name_then_id() {
    use std::cmp::Ordering;
    let owner = PersonId(8);
    let mut alpha = company(1, owner, 500, 0, 0);
    alpha.name = "alpha works".into();
    let mut beta = company(2, owner, 500, 0, 0);
    beta.name = "Beta Works".into();
    let mut twin = company(3, owner, 500, 0, 0);
    twin.name = "alpha works".into();
    for key in CompanySortKey::ALL {
        for descending in [false, true] {
            let sort = CompanySort { key, descending };
            // Equal on every figure: the case-insensitive name decides, in
            // ascending order whichever way a figure key points. Only NAME
            // itself is the primary key and therefore reverses.
            let expected = if key == CompanySortKey::Name && descending {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            assert_eq!(
                compare_companies(&alpha, &beta, sort, Some(owner)),
                expected,
                "{sort:?}"
            );
            assert_eq!(
                compare_companies(&alpha, &twin, sort, Some(owner)),
                Ordering::Less,
                "equal names fall back to id under {sort:?}"
            );
        }
    }
    let richer = company(4, owner, 900, 0, 0);
    let cash = CompanySort::natural(CompanySortKey::Cash);
    assert!(cash.descending, "figures read largest first");
    assert_eq!(
        compare_companies(&richer, &alpha, cash, Some(owner)),
        Ordering::Less
    );
    let ascending = CompanySort {
        descending: false,
        ..cash
    };
    assert_eq!(
        compare_companies(&richer, &alpha, ascending, Some(owner)),
        Ordering::Greater
    );
    let name = CompanySort::natural(CompanySortKey::Name);
    assert!(!name.descending, "names read A to Z");
    // Without a local hero nobody holds anything: HOLDINGS degrades to name order.
    let holdings = CompanySort::default();
    assert_eq!(holdings, CompanySort::natural(CompanySortKey::Holdings));
    assert_eq!(
        compare_companies(&beta, &alpha, holdings, None),
        Ordering::Greater
    );
    assert_eq!(CompanySortKey::Sites.next(), CompanySortKey::Holdings);
    assert_eq!(CompanySortKey::Holdings.next(), CompanySortKey::Name);
}

#[test]
fn every_company_site_exposes_details_and_management_separately() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let site = CompanySiteRecord {
        entity: Entity::from_bits(41),
        id: BuildingId(42),
        settlement: "Oakfell".into(),
        settlement_id: SettlementId(1),
        kind: SettlementBuildingKind::Windmill,
        workers: 1,
        positions: 2,
        enabled_positions: 2,
        state: BusinessState::Operating,
        wage_arrears: 0,
        tax_arrears: 0,
        current_day: default(),
        previous_day: default(),
        output: Some(Good::Flour),
        output_stock: 4,
        asking_price: Some(100),
        input: Some(Good::Wheat),
        input_stock: 9,
        input_target: 18,
        input_coverage_days: 2,
        sourcing: Some(BusinessSourcingMode::PreferOwned),
        preferred_supplier: None,
        goods: vec![(Good::Flour, 4), (Good::Wheat, 9)],
        used_bulk: 13,
        bulk_capacity: 240,
    };
    let fixture = ViewFixture::managed(company(43, PersonId(8), 0, 0, 0));
    world
        .commands()
        .entity(parent)
        .with_children(|children| spawn_site_card(children, &fixture.view(), &site));
    world.flush();

    let mut details = world.query_filtered::<&CompanySiteButton, With<Button>>();
    let mut management = world.query_filtered::<&CompanyManagementButton, With<Button>>();
    assert_eq!(details.single(&world).unwrap().0, site.id);
    assert_eq!(
        management.single(&world).unwrap().target,
        crate::ui::business_management::BusinessManagementSelection::Site(site.entity)
    );
    assert_eq!(management.single(&world).unwrap().company, CompanyId(43));
}

fn storage_site(id: u64, settlement: SettlementId, name: &str) -> CompanySiteRecord {
    CompanySiteRecord {
        entity: Entity::from_bits(id + 100),
        id: BuildingId(id),
        settlement: name.into(),
        settlement_id: settlement,
        kind: SettlementBuildingKind::StorageHall,
        workers: 1,
        positions: 1,
        enabled_positions: 1,
        state: BusinessState::Operating,
        wage_arrears: 0,
        tax_arrears: 0,
        current_day: default(),
        previous_day: default(),
        output: None,
        output_stock: 0,
        asking_price: None,
        input: None,
        input_stock: 0,
        input_target: 0,
        input_coverage_days: 0,
        sourcing: None,
        preferred_supplier: None,
        goods: Vec::new(),
        used_bulk: 0,
        bulk_capacity: shared::economy::capacity::STORAGE_HALL,
    }
}

fn merchant_route() -> CompanyRouteRecord {
    CompanyRouteRecord {
        ship: None,
        id: TradeRouteId(50),
        warehouse: BuildingId(40),
        warehouse_name: "Oakfell Storage Hall #40".into(),
        mode: TradeRouteMode::Merchant,
        origin: "Oakfell".into(),
        destination: "Stonefield".into(),
        good: Good::Stone,
        cargo_target: 12,
        cargo_onboard: 0,
        maximum_purchase_price: 250,
        minimum_destination_price: 325,
        automatic: false,
        autonomous_management: false,
        expected_trip_profit: 0,
        decision_confidence: 100,
        assigned_caravaner: None,
        current_stop: 0,
        status: TradeRouteStatus::Idle,
        completed_trips: 0,
        lifetime_units: 0,
        lifetime_delivery_revenue: 0,
        lifetime_purchase_cost: 0,
        lifetime_consigned_value: 0,
        stops: vec![
            CompanyRouteStopRecord {
                settlement: SettlementId(1),
                settlement_name: "Oakfell".into(),
                action: TradeRouteStopAction::Buy,
            },
            CompanyRouteStopRecord {
                settlement: SettlementId(2),
                settlement_name: "Stonefield".into(),
                action: TradeRouteStopAction::Sell,
            },
            CompanyRouteStopRecord {
                settlement: SettlementId(3),
                settlement_name: "Meadowford".into(),
                action: TradeRouteStopAction::Unload,
            },
        ],
        trips: Vec::new(),
        latest_trip: None,
    }
}

fn company_with_route(id: u64, owner: PersonId, route: CompanyRouteRecord) -> CompanyRecord {
    let mut firm = company(id, owner, 5_000, 0, 0);
    firm.routes = vec![route];
    firm
}

#[test]
fn idle_merchant_route_card_exposes_building_style_management_actions() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let fixture = ViewFixture::managed(company_with_route(43, PersonId(8), merchant_route()));
    world
        .commands()
        .entity(parent)
        .with_children(|children| spawn_route_card(children, &fixture.view(), &merchant_route()));
    world.flush();

    let edit_count = world.query::<&EditTradeRouteButton>().iter(&world).count();
    let actions: Vec<_> = world
        .query::<&TradeRouteQuickActionButton>()
        .iter(&world)
        .map(|button| button.action)
        .collect();
    assert_eq!(edit_count, 1);
    assert!(actions.contains(&TradeRouteQuickAction::DispatchOnce));
    assert!(actions.contains(&TradeRouteQuickAction::Mothball));
}

#[test]
fn route_editor_exposes_ordered_three_town_timetable_controls() {
    let owner = PersonId(8);
    let mut firm = company(43, owner, 5_000, 0, 0);
    firm.sites = vec![
        storage_site(40, SettlementId(1), "Oakfell"),
        storage_site(41, SettlementId(3), "Meadowford"),
    ];
    let directory = CompanyDirectory {
        records: vec![firm.clone()],
        settlements: vec![
            CompanySettlementRecord {
                id: SettlementId(1),
                name: "Oakfell".into(),
                has_marketplace: true,
                port: None,
            },
            CompanySettlementRecord {
                id: SettlementId(2),
                name: "Stonefield".into(),
                has_marketplace: true,
                port: None,
            },
            CompanySettlementRecord {
                id: SettlementId(3),
                name: "Meadowford".into(),
                has_marketplace: true,
                port: None,
            },
        ],
        local_person: Some(owner),
        local_wallet: Some(1_000),
    };
    let route = merchant_route();
    let draft = TradeRouteDraft {
        ship: None,
        company: firm.id,
        route: Some(route.id),
        warehouse: route.warehouse,
        good: route.good,
        cargo_target: route.cargo_target,
        maximum_purchase_price: route.maximum_purchase_price,
        minimum_destination_price: route.minimum_destination_price,
        automatic: route.automatic,
        stops: route
            .stops
            .iter()
            .map(|stop| TradeRouteStop {
                settlement: stop.settlement,
                action: stop.action,
            })
            .collect(),
        pending: false,
    };

    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let mut fixture = ViewFixture::new(firm, directory);
    fixture.editor.draft = Some(draft.clone());
    world
        .commands()
        .entity(parent)
        .with_children(|children| spawn_trade_route_editor(children, &fixture.view(), &draft));
    world.flush();
    let actions: Vec<_> = world
        .query::<&TradeRouteEditorButton>()
        .iter(&world)
        .map(|button| button.0)
        .collect();
    assert!(actions.contains(&TradeRouteEditorAction::Save));
    assert!(actions.contains(&TradeRouteEditorAction::AddStop));
    assert!(actions.contains(&TradeRouteEditorAction::MoveStopLeft(2)));
    assert!(actions.contains(&TradeRouteEditorAction::PreviousStopSettlement(2)));
    assert!(actions.contains(&TradeRouteEditorAction::NextStopAction(2)));
}

/// The retained page over two wholly owned companies: `Company 1` (selected,
/// 100 cash) and `Company 2` (200 cash), with a real list viewport, count
/// text and detail viewport. Returns the detail viewport and content.
fn retained_company_app() -> (App, Entity, Entity) {
    use super::controls::{
        CompanyCountText, CompanyDetailContent, CompanyDetailViewport, CompanyListContent,
        CompanyListViewport, CompanyPortfolioContent,
    };
    use super::model::{CompanyPolicyFeedback, SelectedCompany};
    let mut app = App::new();
    let owner = PersonId(10);
    app.insert_resource(CompanyDirectory {
        records: vec![company(1, owner, 100, 0, 0), company(2, owner, 200, 0, 0)],
        local_person: Some(owner),
        local_wallet: Some(1_000),
        ..default()
    });
    app.init_resource::<CompanyFilter>()
        .init_resource::<CompanySort>()
        .init_resource::<EncyclopediaSearch>()
        .insert_resource(SelectedCompany(Some(CompanyId(1))))
        .init_resource::<CompanyPolicyFeedback>()
        .init_resource::<TradeRouteEditorState>()
        .init_resource::<crate::ui::perf::UiPerf>()
        .add_systems(Update, super::view::rebuild_company_view);
    app.world_mut()
        .spawn((CompanyPortfolioContent, Node::default()));
    let list_viewport = app
        .world_mut()
        .spawn((
            CompanyListViewport,
            Node::default(),
            ScrollPosition::default(),
        ))
        .id();
    app.world_mut()
        .spawn((CompanyListContent, Node::default(), ChildOf(list_viewport)));
    app.world_mut().spawn((CompanyCountText, Text::new("")));
    let viewport = app
        .world_mut()
        .spawn((
            CompanyDetailViewport,
            Node::default(),
            ScrollPosition::default(),
        ))
        .id();
    let detail = app
        .world_mut()
        .spawn((CompanyDetailContent, Node::default(), ChildOf(viewport)))
        .id();
    app.update();
    app.update();
    (app, viewport, detail)
}

#[test]
fn live_company_cash_without_hover_keeps_detail_entities_and_updates_text() {
    use crate::ui::history::CompanyHistoryButton;
    let (mut app, _, detail) = retained_company_app();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let images = image_nodes(app.world_mut());
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyHistoryButton>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        bound_text(app.world_mut(), CompanyBound::Company(CompanyField::Cash)),
        format!("{} coin", shared::economy::format_money(100))
    );
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .cash += 10;
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children,
        "a books tick must bind, not respawn"
    );
    assert!(app.world().get_entity(button).is_ok());
    assert_eq!(
        image_nodes(app.world_mut()),
        images,
        "no medallion or icon may be recreated by a cash tick"
    );
    assert_eq!(
        bound_text(app.world_mut(), CompanyBound::Company(CompanyField::Cash)),
        format!("{} coin", shared::economy::format_money(110))
    );
}

#[test]
fn live_company_cash_binds_in_place_while_hovered_and_never_respawns_on_pointer_leave() {
    use crate::ui::history::CompanyHistoryButton;
    let (mut app, viewport, detail) = retained_company_app();
    app.world_mut()
        .get_mut::<ScrollPosition>(viewport)
        .unwrap()
        .0
        .y = 420.0;
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyHistoryButton>>()
        .single(app.world())
        .unwrap();
    let mut cash = 100;
    for state in [Interaction::Hovered, Interaction::Pressed] {
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = state;
        let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
        directory.records[0].account.cash += 10;
        cash += 10;
        directory.local_wallet = Some(directory.local_wallet.unwrap() + 10);
        drop(directory);
        app.update();
        assert_eq!(
            app.world().get::<Children>(detail).unwrap().to_vec(),
            children
        );
        assert!(
            app.world().get_entity(button).is_ok(),
            "the user's target must survive live books"
        );
        assert_eq!(
            bound_text(app.world_mut(), CompanyBound::Company(CompanyField::Cash)),
            format!("{} coin", shared::economy::format_money(cash)),
            "values keep binding under the pointer"
        );
    }
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
    app.update();
    app.update();
    assert!(
        app.world().get_entity(button).is_ok(),
        "nothing was structural, so pointer leave must not respawn"
    );
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(
        app.world().get::<ScrollPosition>(viewport).unwrap().0.y,
        420.0
    );
}

#[test]
fn structural_change_still_rebuilds_and_defers_only_while_hovered() {
    use crate::ui::history::CompanyHistoryButton;
    let (mut app, viewport, detail) = retained_company_app();
    app.world_mut()
        .get_mut::<ScrollPosition>(viewport)
        .unwrap()
        .0
        .y = 420.0;
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyHistoryButton>>()
        .single(app.world())
        .unwrap();
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Hovered;
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].routes = vec![merchant_route()];
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children,
        "a structural change waits while a control is hovered"
    );
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
    app.update();
    assert!(app.world().get_entity(button).is_err());
    assert_ne!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(
        app.world_mut()
            .query::<&EditTradeRouteButton>()
            .iter(app.world())
            .count(),
        1,
        "the new route card appears after the deferred rebuild"
    );
    assert_eq!(
        app.world().get::<ScrollPosition>(viewport).unwrap().0.y,
        420.0,
        "a same-record rebuild keeps the reader's place"
    );
}

#[test]
fn site_stock_tick_does_not_respawn_detail_but_updates_site_text() {
    let (mut app, _, detail) = retained_company_app();
    let mut site = storage_site(40, SettlementId(1), "Oakfell");
    site.kind = SettlementBuildingKind::Windmill;
    site.output = Some(Good::Flour);
    site.input = Some(Good::Wheat);
    site.asking_price = Some(100);
    site.output_stock = 4;
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].sites = vec![site];
    app.update();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let stock = CompanyBound::Site(BuildingId(40), SiteField::LedgerStock);
    let staff = CompanyBound::Site(BuildingId(40), SiteField::Staff);
    assert!(bound_text(app.world_mut(), stock).contains("site stock 4 /"));
    assert_eq!(bound_text(app.world_mut(), staff), "Staff 1 / 1");
    {
        let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
        let site = &mut directory.records[0].sites[0];
        site.output_stock = 9;
        site.workers = 0;
        site.current_day.gross_revenue += 300;
    }
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert!(bound_text(app.world_mut(), stock).contains("site stock 9 /"));
    assert_eq!(bound_text(app.world_mut(), staff), "Staff 0 / 1");
}

#[test]
fn site_entity_churn_rebinds_the_site_settings_target_without_respawn() {
    use crate::ui::business_management::BusinessManagementSelection;
    let (mut app, _, detail) = retained_company_app();
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].sites =
        vec![storage_site(40, SettlementId(1), "Oakfell")];
    app.update();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let buttons: Vec<Entity> = app
        .world_mut()
        .query::<(Entity, &CompanyManagementButton)>()
        .iter(app.world())
        .filter(|(_, button)| {
            button.target == BusinessManagementSelection::Site(Entity::from_bits(140))
        })
        .map(|(entity, _)| entity)
        .collect();
    assert_eq!(
        buttons.len(),
        2,
        "site card and site ledger each expose SITE SETTINGS"
    );
    let replaced = Entity::from_bits(9_140);
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].sites[0].entity = replaced;
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    for button in buttons {
        assert_eq!(
            app.world()
                .get::<CompanyManagementButton>(button)
                .unwrap()
                .target,
            BusinessManagementSelection::Site(replaced),
            "replication handed the building a new entity; the payload must follow"
        );
    }
}

fn wheat_branch(retain_units: u32, sell_excess: bool) -> CompanyBranchRecord {
    CompanyBranchRecord {
        settlement: "Oakfell".into(),
        settlement_id: SettlementId(1),
        sites: 1,
        storage_halls: 1,
        used_bulk: 0,
        bulk_capacity: shared::economy::capacity::STORAGE_HALL,
        resources: vec![(
            Good::Wheat,
            0,
            CompanyResourcePolicy {
                retain_units,
                sell_excess,
            },
        )],
    }
}

#[test]
fn branch_policy_buttons_rebind_their_payload_after_retain_units_change() {
    let (mut app, _, detail) = retained_company_app();
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].branches =
        vec![wheat_branch(5, false)];
    app.update();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let plus_one = CompanyBound::Resource(
        SettlementId(1),
        Good::Wheat,
        ResourceField::Step(RetainStep::Plus1),
    );
    let toggle = CompanyBound::Resource(SettlementId(1), Good::Wheat, ResourceField::Toggle);
    let payload = |world: &mut World, key: CompanyBound| {
        world
            .query::<(Entity, &CompanyBound, &CompanyBranchPolicyButton)>()
            .iter(world)
            .find(|(_, candidate, _)| **candidate == key)
            .map(|(entity, _, button)| (entity, button.action))
            .unwrap()
    };
    let (plus_entity, action) = payload(app.world_mut(), plus_one);
    assert_eq!(
        action,
        HeroCompanyAction::SetRetainUnits {
            settlement: SettlementId(1),
            good: Good::Wheat,
            units: 6
        }
    );
    let (toggle_entity, action) = payload(app.world_mut(), toggle);
    assert_eq!(
        action,
        HeroCompanyAction::SetSellExcess {
            settlement: SettlementId(1),
            good: Good::Wheat,
            enabled: true
        }
    );
    // The server accepted +1 and now reports 20 retained with excess sold.
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].branches =
        vec![wheat_branch(20, true)];
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(
        payload(app.world_mut(), plus_one),
        (
            plus_entity,
            HeroCompanyAction::SetRetainUnits {
                settlement: SettlementId(1),
                good: Good::Wheat,
                units: 21
            }
        ),
        "the +1 button must never send the stale absolute value 6"
    );
    assert_eq!(
        payload(app.world_mut(), toggle),
        (
            toggle_entity,
            HeroCompanyAction::SetSellExcess {
                settlement: SettlementId(1),
                good: Good::Wheat,
                enabled: false
            }
        )
    );
    assert_eq!(
        bound_text(
            app.world_mut(),
            CompanyBound::Resource(SettlementId(1), Good::Wheat, ResourceField::ToggleLabel)
        ),
        "HOLD ALL"
    );
    let meter_width = app
        .world_mut()
        .query::<(&CompanyBound, &Node)>()
        .iter(app.world())
        .find(|(key, _)| {
            **key == CompanyBound::Resource(SettlementId(1), Good::Wheat, ResourceField::Meter)
        })
        .map(|(_, node)| node.width)
        .unwrap();
    let capacity = shared::economy::capacity::STORAGE_HALL / Good::Wheat.bulk_per_unit();
    assert_eq!(meter_width, Val::Percent(20.0 * 100.0 / capacity as f32));
}

#[test]
fn portfolio_cards_bind_in_place_when_wallet_changes() {
    use super::controls::CompanyPortfolioContent;
    let (mut app, _, _) = retained_company_app();
    let portfolio = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyPortfolioContent>>()
        .single(app.world())
        .unwrap();
    let children = app.world().get::<Children>(portfolio).unwrap().to_vec();
    assert_eq!(
        bound_text(
            app.world_mut(),
            CompanyBound::Portfolio(PortfolioField::Wallet)
        ),
        format!("{} coin", shared::economy::format_money(1_000))
    );
    let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
    directory.local_wallet = Some(1_010);
    directory.records[1].account.cash += 500;
    drop(directory);
    app.update();
    assert_eq!(
        app.world().get::<Children>(portfolio).unwrap().to_vec(),
        children,
        "the strip is spawned once and bound"
    );
    assert_eq!(
        bound_text(
            app.world_mut(),
            CompanyBound::Portfolio(PortfolioField::Wallet)
        ),
        format!("{} coin", shared::economy::format_money(1_010))
    );
    assert_eq!(
        bound_text(
            app.world_mut(),
            CompanyBound::Portfolio(PortfolioField::Interest)
        ),
        format!("{} coin", shared::economy::format_money(800))
    );
}

#[test]
fn feedback_receipt_binds_the_note_instead_of_respawning() {
    let (mut app, _, detail) = retained_company_app();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let note = |world: &mut World| {
        world
            .query::<(&CompanyBound, &Text, &Node)>()
            .iter(world)
            .find(|(key, ..)| **key == CompanyBound::Company(CompanyField::NoteFeedback))
            .map(|(_, text, node)| (text.0.clone(), node.display))
            .unwrap()
    };
    assert_eq!(note(app.world_mut()), (String::new(), Display::None));
    *app.world_mut().resource_mut::<CompanyPolicyFeedback>() = CompanyPolicyFeedback {
        company: Some(CompanyId(1)),
        message: "Retention updated".into(),
        success: true,
    };
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(
        note(app.world_mut()),
        ("UPDATED: Retention updated".to_string(), Display::Flex)
    );
}

#[test]
fn internal_memo_row_toggles_display_in_place() {
    let (mut app, _, detail) = retained_company_app();
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .current_day
        .day = 3;
    app.update();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let memo_row = |world: &mut World| {
        world
            .query::<(&CompanyBound, &Node)>()
            .iter(world)
            .find(|(key, _)| {
                **key == CompanyBound::Company(CompanyField::DayMemoRow(DayLedger::Today))
            })
            .map(|(_, node)| node.display)
            .unwrap()
    };
    assert_eq!(memo_row(app.world_mut()), Display::None);
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .current_day
        .internal_revenue = 40;
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(memo_row(app.world_mut()), Display::Flex);
    assert!(bound_text(
        app.world_mut(),
        CompanyBound::Company(CompanyField::DayMemo(DayLedger::Today))
    )
    .starts_with("0.40"));
}

#[test]
fn company_rows_keep_entities_when_profit_sign_flips() {
    let (mut app, _, _) = retained_company_app();
    let rows: Vec<Entity> = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyRow>>()
        .iter(app.world())
        .collect();
    assert_eq!(rows.len(), 2);
    let status = |world: &mut World| {
        world
            .query::<(&CompanyRowStatus, &Text)>()
            .iter(world)
            .find(|(row, _)| row.0 == CompanyId(1))
            .map(|(_, text)| text.0.clone())
            .unwrap()
    };
    assert_eq!(
        status(app.world_mut()),
        "100.0% yours · SITES UNOBSERVED · Master"
    );
    {
        let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
        let company = &mut directory.records[0];
        company.sites = vec![storage_site(40, SettlementId(1), "Oakfell")];
        company.account.current_day.external_revenue = 500;
    }
    app.update();
    let after: Vec<Entity> = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyRow>>()
        .iter(app.world())
        .collect();
    assert_eq!(after, rows, "a status flip rewrites the row in place");
    assert_eq!(
        status(app.world_mut()),
        "100.0% yours · PROFITABLE · Master"
    );
}

#[test]
fn rows_rebuild_when_visible_order_changes() {
    let (mut app, _, _) = retained_company_app();
    let order = |world: &mut World| -> Vec<CompanyId> {
        use super::controls::CompanyListContent;
        let list = world
            .query_filtered::<Entity, With<CompanyListContent>>()
            .single(world)
            .unwrap();
        world
            .get::<Children>(list)
            .unwrap()
            .iter()
            .map(|child| world.get::<CompanyRow>(child).unwrap().0)
            .collect()
    };
    let rows_before = order(app.world_mut());
    assert_eq!(rows_before, vec![CompanyId(1), CompanyId(2)]);
    // Selling most of company 1 drops it below company 2 in the owned-first order.
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].holders[0].shares = 10;
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .holders
        .push(CompanyHolderRecord {
            person: PersonId(77),
            name: "Buyer".into(),
            shares: 990,
        });
    app.update();
    assert_eq!(
        order(app.world_mut()),
        vec![CompanyId(2), CompanyId(1)],
        "a change of visible order must respawn the rows in the new order"
    );
}

/// The list rows in display order.
fn list_order(world: &mut World) -> Vec<CompanyId> {
    row_entities(world)
        .into_iter()
        .map(|row| world.get::<CompanyRow>(row).unwrap().0)
        .collect()
}

fn row_entities(world: &mut World) -> Vec<Entity> {
    use super::controls::CompanyListContent;
    let list = world
        .query_filtered::<Entity, With<CompanyListContent>>()
        .single(world)
        .unwrap();
    world
        .get::<Children>(list)
        .map(|children| {
            children
                .iter()
                .filter(|child| world.get::<CompanyRow>(*child).is_some())
                .collect()
        })
        .unwrap_or_default()
}

fn count_text(world: &mut World) -> String {
    use super::controls::CompanyCountText;
    world
        .query_filtered::<&Text, With<CompanyCountText>>()
        .single(world)
        .unwrap()
        .0
        .clone()
}

fn list_shows_text(world: &mut World, wanted: &str) -> bool {
    use super::controls::CompanyListContent;
    let list = world
        .query_filtered::<Entity, With<CompanyListContent>>()
        .single(world)
        .unwrap();
    world.get::<Children>(list).is_some_and(|children| {
        children
            .iter()
            .any(|child| world.get::<Text>(child).is_some_and(|text| text.0 == wanted))
    })
}

fn set_company_query(app: &mut App, query: &str) {
    app.world_mut()
        .resource_mut::<EncyclopediaSearch>()
        .set_query(EncyclopediaTab::Companies, query);
}

#[test]
fn search_filters_rows_once_and_keeps_selection_and_detail() {
    use super::model::SelectedCompany;
    let (mut app, _, detail) = retained_company_app();
    app.world_mut().resource_mut::<CompanyDirectory>().records[1].name =
        "Cassia River Fish".into();
    app.update();
    // Equal holdings: "Cassia River Fish" sorts before "Company 1" by name.
    assert_eq!(list_order(app.world_mut()), vec![CompanyId(2), CompanyId(1)]);
    assert_eq!(count_text(app.world_mut()), "2 companies");
    let detail_children = app.world().get::<Children>(detail).unwrap().to_vec();

    set_company_query(&mut app, "cassia");
    app.update();
    assert_eq!(list_order(app.world_mut()), vec![CompanyId(2)]);
    assert_eq!(count_text(app.world_mut()), "1 of 2");
    assert_eq!(
        app.world().resource::<SelectedCompany>().0,
        Some(CompanyId(1)),
        "a text search never revokes the selection"
    );
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        detail_children,
        "the detail pane is not structure of the list"
    );
    let rows = row_entities(app.world_mut());
    app.update();
    app.update();
    assert_eq!(
        row_entities(app.world_mut()),
        rows,
        "one query rebuilds the rows once, not every frame"
    );

    app.world_mut()
        .resource_mut::<EncyclopediaSearch>()
        .clear_companies();
    app.update();
    assert_eq!(list_order(app.world_mut()), vec![CompanyId(2), CompanyId(1)]);
    assert_eq!(count_text(app.world_mut()), "2 companies");
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        detail_children
    );
}

#[test]
fn sort_key_and_direction_reorder_rows_and_ties_break_by_name() {
    use super::controls::{
        handle_company_sort_buttons, style_company_controls, CompanySortDirectionButton,
        CompanySortDirectionLabel, CompanySortKeyButton, CompanySortKeyLabel,
    };
    use crate::ui::encyclopedia::ClickGuard;
    use bevy::ecs::system::RunSystemOnce;

    #[derive(Resource, Default)]
    struct LabelChanges(usize);
    fn count_label_changes(
        changed: Query<
            (),
            (
                Changed<Text>,
                Or<(With<CompanySortKeyLabel>, With<CompanySortDirectionLabel>)>,
            ),
        >,
        mut count: ResMut<LabelChanges>,
    ) {
        count.0 = changed.iter().count();
    }

    let (mut app, _, _) = retained_company_app();
    app.init_resource::<LabelChanges>().add_systems(
        Update,
        (style_company_controls, count_label_changes)
            .chain()
            .after(super::view::rebuild_company_view),
    );
    app.world_mut()
        .spawn((CompanySortKeyLabel, Text::new("HOLDINGS")));
    app.world_mut()
        .spawn((CompanySortDirectionLabel, Text::new("v")));
    {
        let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
        directory.records[0].sites = vec![
            storage_site(40, SettlementId(1), "Oakfell"),
            storage_site(41, SettlementId(1), "Oakfell"),
        ];
        directory.records[1].sites = vec![storage_site(42, SettlementId(1), "Oakfell")];
        directory.records[0].account.current_day.external_revenue = 50;
        directory.records[1].account.current_day.external_revenue = 500;
    }
    app.update();
    app.update();
    assert_eq!(
        list_order(app.world_mut()),
        vec![CompanyId(1), CompanyId(2)],
        "equal holdings tie, so the default order is by name"
    );
    assert_eq!(app.world().resource::<LabelChanges>().0, 0);

    let expectations = [
        (CompanySort::natural(CompanySortKey::Cash), [2, 1], "CASH", "v"),
        (
            CompanySort {
                key: CompanySortKey::Cash,
                descending: false,
            },
            [1, 2],
            "CASH",
            "^",
        ),
        (CompanySort::natural(CompanySortKey::Sites), [1, 2], "SITES", "v"),
        (CompanySort::natural(CompanySortKey::Profit), [2, 1], "PROFIT", "v"),
        (CompanySort::natural(CompanySortKey::Name), [1, 2], "NAME", "^"),
        (
            CompanySort {
                key: CompanySortKey::Name,
                descending: true,
            },
            [2, 1],
            "NAME",
            "v",
        ),
    ];
    for (sort, order, key_label, direction_label) in expectations {
        *app.world_mut().resource_mut::<CompanySort>() = sort;
        app.update();
        assert_eq!(
            list_order(app.world_mut()),
            order.map(CompanyId).to_vec(),
            "{sort:?}"
        );
        let world = app.world_mut();
        let key = world
            .query_filtered::<&Text, With<CompanySortKeyLabel>>()
            .single(world)
            .unwrap()
            .0
            .clone();
        let direction = world
            .query_filtered::<&Text, With<CompanySortDirectionLabel>>()
            .single(world)
            .unwrap()
            .0
            .clone();
        assert_eq!((key.as_str(), direction.as_str()), (key_label, direction_label));
        app.update();
        assert_eq!(
            app.world().resource::<LabelChanges>().0,
            0,
            "labels are rewritten only when they differ"
        );
    }
    // Equal cash: the direction cannot shuffle a tie, which stays by name.
    app.world_mut().resource_mut::<CompanyDirectory>().records[1]
        .account
        .cash = 100;
    for descending in [true, false] {
        *app.world_mut().resource_mut::<CompanySort>() = CompanySort {
            key: CompanySortKey::Cash,
            descending,
        };
        app.update();
        assert_eq!(list_order(app.world_mut()), vec![CompanyId(1), CompanyId(2)]);
    }

    // The key button cycles through every key and resets the direction to
    // the new key's natural one; the toggle flips it. Mouse presses only.
    app.insert_resource(ClickGuard(true));
    let mut mouse = ButtonInput::<MouseButton>::default();
    mouse.press(MouseButton::Left);
    app.insert_resource(mouse);
    *app.world_mut().resource_mut::<CompanySort>() = CompanySort {
        key: CompanySortKey::Holdings,
        descending: false,
    };
    let key_button = app
        .world_mut()
        .spawn((CompanySortKeyButton, Interaction::Pressed))
        .id();
    app.world_mut()
        .run_system_once(handle_company_sort_buttons)
        .unwrap();
    assert_eq!(
        *app.world().resource::<CompanySort>(),
        CompanySort::natural(CompanySortKey::Name),
        "a new key starts in its natural direction"
    );
    app.world_mut().despawn(key_button);
    let direction_button = app
        .world_mut()
        .spawn((CompanySortDirectionButton, Interaction::Pressed))
        .id();
    app.world_mut()
        .run_system_once(handle_company_sort_buttons)
        .unwrap();
    assert_eq!(
        *app.world().resource::<CompanySort>(),
        CompanySort {
            key: CompanySortKey::Name,
            descending: true,
        }
    );
    app.world_mut().resource_mut::<ClickGuard>().0 = false;
    app.world_mut()
        .run_system_once(handle_company_sort_buttons)
        .unwrap();
    assert_eq!(
        *app.world().resource::<CompanySort>(),
        CompanySort {
            key: CompanySortKey::Name,
            descending: true,
        },
        "an unarmed click guard ignores the press"
    );
    app.world_mut().despawn(direction_button);
}

#[test]
fn a_books_tick_that_does_not_reorder_keeps_row_entities_under_a_cash_sort() {
    use super::portfolio::CompanyRowLedger;
    let (mut app, _, _) = retained_company_app();
    *app.world_mut().resource_mut::<CompanySort>() = CompanySort::natural(CompanySortKey::Cash);
    app.update();
    assert_eq!(list_order(app.world_mut()), vec![CompanyId(2), CompanyId(1)]);
    let rows = row_entities(app.world_mut());
    app.world_mut().resource_mut::<CompanyDirectory>().records[1]
        .account
        .cash = 250;
    app.update();
    assert_eq!(
        row_entities(app.world_mut()),
        rows,
        "a tick that keeps the order rewrites the ledger line in place"
    );
    let ledger = app
        .world_mut()
        .query::<(&CompanyRowLedger, &Text)>()
        .iter(app.world())
        .find(|(row, _)| row.0 == CompanyId(2))
        .map(|(_, text)| text.0.clone())
        .unwrap();
    assert!(
        ledger.contains(&format!("{} cash", shared::economy::format_money(250))),
        "{ledger}"
    );
    // Crossing the other company's cash changes the order: a real rebuild.
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .cash = 300;
    app.update();
    assert_eq!(list_order(app.world_mut()), vec![CompanyId(1), CompanyId(2)]);
    assert!(
        row_entities(app.world_mut())
            .iter()
            .all(|row| !rows.contains(row)),
        "a reorder respawns the rows in the new order"
    );
}

#[test]
fn no_match_search_shows_its_own_empty_state_and_clears() {
    use super::model::SelectedCompany;
    let (mut app, _, _) = retained_company_app();
    set_company_query(&mut app, "zz-no-such-company");
    app.update();
    assert!(list_order(app.world_mut()).is_empty());
    assert_eq!(count_text(app.world_mut()), "0 of 2");
    assert!(list_shows_text(
        app.world_mut(),
        "No companies match this search"
    ));
    assert_eq!(
        app.world().resource::<SelectedCompany>().0,
        Some(CompanyId(1)),
        "the selection outlives a fruitless query"
    );
    app.world_mut()
        .resource_mut::<EncyclopediaSearch>()
        .clear_companies();
    app.update();
    assert_eq!(list_order(app.world_mut()), vec![CompanyId(1), CompanyId(2)]);
    assert!(!list_shows_text(
        app.world_mut(),
        "No companies match this search"
    ));
    assert_eq!(count_text(app.world_mut()), "2 companies");
}

#[test]
fn list_scroll_resets_on_search_and_sort_changes_but_not_on_ticks() {
    use super::controls::CompanyListViewport;
    let (mut app, detail_viewport, _) = retained_company_app();
    let list_viewport = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyListViewport>>()
        .single(app.world())
        .unwrap();
    let list_scroll = |app: &App| app.world().get::<ScrollPosition>(list_viewport).unwrap().y;
    let scroll_to = |app: &mut App, y: f32| {
        app.world_mut()
            .get_mut::<ScrollPosition>(list_viewport)
            .unwrap()
            .y = y;
    };
    scroll_to(&mut app, 300.0);
    app.world_mut()
        .get_mut::<ScrollPosition>(detail_viewport)
        .unwrap()
        .y = 420.0;
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .cash += 10;
    app.update();
    assert_eq!(list_scroll(&app), 300.0, "a books tick keeps the reader's place");

    set_company_query(&mut app, "company");
    app.update();
    assert_eq!(list_scroll(&app), 0.0, "a new query starts at the top");
    assert_eq!(
        app.world().get::<ScrollPosition>(detail_viewport).unwrap().y,
        420.0,
        "the detail pane's scroll is not the list's business"
    );

    scroll_to(&mut app, 300.0);
    app.world_mut()
        .resource_mut::<EncyclopediaSearch>()
        .set_query(EncyclopediaTab::People, "someone");
    app.update();
    assert_eq!(list_scroll(&app), 300.0, "another tab's draft is not an input here");

    *app.world_mut().resource_mut::<CompanySort>() = CompanySort::natural(CompanySortKey::Cash);
    app.update();
    assert_eq!(list_scroll(&app), 0.0, "a new order starts at the top");
}

#[test]
fn structural_key_covers_every_button_gate() {
    use shared::components::*;
    let owner = PersonId(8);
    let (mut base, mut directory) = fleet_fixture(ShipKind::Coaster);
    let mut route = merchant_route();
    route.ship = None;
    route.trips = vec![shared::components::TradeRouteTrip::default(); 3];
    base.routes = vec![route];
    base.fleet.orders.push((
        ShipOrderId(93),
        ShipConstructionOrder {
            company: base.id,
            port: BuildingId(71),
            kind: ShipKind::Coaster,
            status: ShipOrderStatus::Hauling,
            delivered: [0; 3],
            progress: 100,
        },
    ));
    base.account.current_day.day = 4;
    base.account.previous_day.day = 3;
    base.branches = vec![wheat_branch(5, false)];
    // Two holders: the master keeps executive control, so a share sale
    // inside the class is a value (the contribute buttons belong to every
    // holder, so sole ownership is not a class of its own).
    base.holders = vec![
        CompanyHolderRecord {
            person: owner,
            name: "Owner".into(),
            shares: 900,
        },
        CompanyHolderRecord {
            person: PersonId(12),
            name: "Minor".into(),
            shares: 100,
        },
    ];
    directory.local_person = Some(owner);
    let editor = TradeRouteEditorState::default();
    let key = |company: &CompanyRecord, directory: &CompanyDirectory| {
        company_structure_key(Some(company), directory, &editor)
    };
    let baseline = key(&base, &directory);

    let structural: Vec<(&str, Box<dyn Fn(&mut CompanyRecord, &mut CompanyDirectory)>)> = vec![
        (
            "route automatic (RUN ONE CIRCUIT)",
            Box::new(|c, _| c.routes[0].automatic = true),
        ),
        (
            "assigned caravaner (EDIT TIMETABLE)",
            Box::new(|c, _| c.routes[0].assigned_caravaner = Some("Bo".into())),
        ),
        (
            "route leaves Idle (MOTHBALL)",
            Box::new(|c, _| c.routes[0].status = TradeRouteStatus::InTransit),
        ),
        (
            "route mothballed (REOPEN)",
            Box::new(|c, _| c.routes[0].status = TradeRouteStatus::Mothballed),
        ),
        (
            "route gains ship (STOP AFTER VOYAGE)",
            Box::new(|c, _| c.routes[0].ship = Some((ShipId(91), ShipKind::Coaster))),
        ),
        (
            "route mode (merchant controls)",
            Box::new(|c, _| c.routes[0].mode = TradeRouteMode::ContractCarrier),
        ),
        (
            "route stop count",
            Box::new(|c, _| c.routes[0].stops.pop().map(drop).unwrap()),
        ),
        (
            "trip rows appear (three -> none)",
            Box::new(|c, _| c.routes[0].trips.clear()),
        ),
        (
            "order completed (CANCEL ORDER)",
            Box::new(|c, _| c.fleet.orders[0].1.status = ShipOrderStatus::Completed),
        ),
        (
            "port unbuilt (ORDER buttons)",
            Box::new(|_, d| d.settlements[0].port.as_mut().unwrap().built = false),
        ),
        (
            "port class (ORDER COG)",
            Box::new(|_, d| {
                d.settlements[0].port.as_mut().unwrap().maximum_ship = ShipKind::Coaster
            }),
        ),
        (
            "ship assigned (NEW SHIP ROUTE / ASSIGN)",
            Box::new(|c, _| c.fleet.ships[0].1.assigned_route = Some(TradeRouteId(50))),
        ),
        (
            "ship added",
            Box::new(|c, _| c.fleet.ships.push(c.fleet.ships[0].clone())),
        ),
        (
            "today's ledger recorded",
            Box::new(|c, _| c.account.current_day.day = u32::MAX),
        ),
        (
            "previous day recorded",
            Box::new(|c, _| c.account.previous_day.day = u32::MAX),
        ),
        (
            "site added",
            Box::new(|c, _| c.sites.push(storage_site(41, SettlementId(2), "Quay"))),
        ),
        (
            "warehouse unstaffed (NEW CARAVAN ROUTE)",
            Box::new(|c, _| c.sites[0].workers = 0),
        ),
        (
            "second settlement (NEW CARAVAN ROUTE)",
            Box::new(|_, d| d.settlements.truncate(1)),
        ),
        (
            "holder count",
            Box::new(|c, _| {
                c.holders.push(CompanyHolderRecord {
                    person: PersonId(9),
                    name: "B".into(),
                    shares: 1,
                })
            }),
        ),
        (
            "offer count",
            Box::new(|c, _| {
                c.offers.push(CompanyOfferRecord {
                    seller: PersonId(8),
                    seller_name: "A".into(),
                    shares: 5,
                    unit_price: 10,
                    listed_day: 1,
                })
            }),
        ),
        (
            "decision count",
            Box::new(|c, _| {
                c.decisions.push(shared::economy::CompanyDecisionRecord {
                    day: 1,
                    master: PersonId(8),
                    from: default(),
                    to: default(),
                    reason: shared::economy::CompanyDecisionReason::FinancialStress,
                })
            }),
        ),
        (
            "ownership class (master -> no shares)",
            Box::new(|_, d| d.local_person = Some(PersonId(999))),
        ),
        ("no local hero", Box::new(|_, d| d.local_person = None)),
        (
            "branch added",
            Box::new(|c, _| {
                c.branches.push(CompanyBranchRecord {
                    settlement_id: SettlementId(2),
                    settlement: "Quay".into(),
                    ..wheat_branch(0, true)
                })
            }),
        ),
        ("master changed", Box::new(|c, _| c.master = PersonId(999))),
    ];
    for (name, mutate) in structural {
        let (mut company, mut directory) = (base.clone(), directory.clone());
        mutate(&mut company, &mut directory);
        assert_ne!(
            key(&company, &directory),
            baseline,
            "{name} decides which controls exist and must change the structure key"
        );
    }

    let values: Vec<(&str, Box<dyn Fn(&mut CompanyRecord, &mut CompanyDirectory)>)> = vec![
        ("cash", Box::new(|c, _| c.account.cash += 1)),
        (
            "today's revenue",
            Box::new(|c, _| c.account.current_day.external_revenue += 1),
        ),
        (
            "internal memo",
            Box::new(|c, _| c.account.current_day.internal_revenue += 1),
        ),
        ("site stock", Box::new(|c, _| c.sites[0].used_bulk += 1)),
        (
            "site entity",
            Box::new(|c, _| c.sites[0].entity = Entity::from_bits(4_242)),
        ),
        (
            "route cargo",
            Box::new(|c, _| c.routes[0].cargo_onboard += 1),
        ),
        (
            "route status in transit -> returning",
            Box::new(|c, _| c.routes[0].status = TradeRouteStatus::Returning),
        ),
        (
            "route current stop",
            Box::new(|c, _| c.routes[0].current_stop = 1),
        ),
        (
            "fourth trip",
            Box::new(|c, _| c.routes[0].trips = vec![TradeRouteTrip::default(); 4]),
        ),
        (
            "retain units",
            Box::new(|c, _| c.branches[0].resources[0].2.retain_units = 9),
        ),
        (
            "order progress",
            Box::new(|c, _| c.fleet.orders[0].1.progress = 500),
        ),
        ("holder shares", Box::new(|c, _| c.holders[0].shares -= 1)),
        (
            "names",
            Box::new(|c, _| {
                c.name = "Renamed".into();
                c.master_name = "Renamed".into();
            }),
        ),
        ("wallet", Box::new(|_, d| d.local_wallet = Some(5))),
        (
            "dividend capacity published",
            Box::new(|c, _| {
                c.capacity = Some(shared::economy::CompanyDividendCapacity {
                    day: 4,
                    distributable: 30_000,
                    protected_reserves: 1_850,
                    retained_profit: 46_200,
                    last_paid_day: 3,
                    last_paid: 20_000,
                })
            }),
        ),
    ];
    let mut in_transit = base.clone();
    in_transit.routes[0].status = TradeRouteStatus::InTransit;
    for (name, mutate) in values {
        let (mut company, mut directory) = (base.clone(), directory.clone());
        if name.starts_with("route status") {
            company = in_transit.clone();
        }
        let before = key(&company, &directory);
        mutate(&mut company, &mut directory);
        assert_eq!(
            key(&company, &directory),
            before,
            "{name} is a value and must bind without a rebuild"
        );
    }
}

#[test]
fn changing_company_resets_deep_scroll_even_when_previous_detail_is_hovered() {
    use super::model::SelectedCompany;
    use crate::ui::history::CompanyHistoryButton;
    let (mut app, viewport, _) = retained_company_app();
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<CompanyHistoryButton>>()
        .single(app.world())
        .unwrap();
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Hovered;
    app.world_mut()
        .get_mut::<ScrollPosition>(viewport)
        .unwrap()
        .0
        .y = 850.0;
    app.world_mut().resource_mut::<SelectedCompany>().0 = Some(CompanyId(2));
    app.update();
    assert_eq!(
        app.world().get::<ScrollPosition>(viewport).unwrap().0,
        Vec2::ZERO
    );
    assert!(app.world().get_entity(button).is_err());
    let history = app
        .world_mut()
        .query::<&CompanyHistoryButton>()
        .single(app.world())
        .unwrap();
    assert_eq!(history.company, CompanyId(2));
}

#[test]
fn route_draft_survives_live_snapshot_and_stepper_binds_in_place_while_hovered() {
    let (mut app, _, detail) = retained_company_app();
    let route = merchant_route();
    let draft = TradeRouteDraft {
        ship: None,
        company: CompanyId(1),
        route: Some(route.id),
        warehouse: route.warehouse,
        good: route.good,
        cargo_target: 37,
        maximum_purchase_price: route.maximum_purchase_price,
        minimum_destination_price: route.minimum_destination_price,
        automatic: route.automatic,
        stops: route
            .stops
            .iter()
            .map(|stop| TradeRouteStop {
                settlement: stop.settlement,
                action: stop.action,
            })
            .collect(),
        pending: false,
    };
    app.world_mut()
        .resource_mut::<TradeRouteEditorState>()
        .draft = Some(draft.clone());
    app.update();
    let button = app
        .world_mut()
        .query::<(Entity, &TradeRouteEditorButton)>()
        .iter(app.world())
        .find(|(_, button)| button.0 == TradeRouteEditorAction::CargoUp(1))
        .unwrap()
        .0;
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Hovered;
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .cash += 5;
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(
        app.world()
            .resource::<TradeRouteEditorState>()
            .draft
            .as_ref(),
        Some(&draft)
    );
    assert!(
        bound_text(app.world_mut(), CompanyBound::Editor(EditorField::Cargo))
            .contains("TARGET 37 UNITS")
    );
    app.world_mut()
        .resource_mut::<TradeRouteEditorState>()
        .draft
        .as_mut()
        .unwrap()
        .cargo_target += 1;
    app.update();
    assert!(
        app.world().get_entity(button).is_ok(),
        "the stepper under the pointer survives its own press"
    );
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert!(
        bound_text(app.world_mut(), CompanyBound::Editor(EditorField::Cargo))
            .contains("TARGET 38 UNITS"),
        "the stepped value shows immediately, in place"
    );
    // Adding a stop is structural: the lane gains a card and its controls.
    app.world_mut()
        .resource_mut::<TradeRouteEditorState>()
        .draft
        .as_mut()
        .unwrap()
        .stops
        .push(TradeRouteStop {
            settlement: SettlementId(1),
            action: TradeRouteStopAction::Sell,
        });
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
    app.update();
    assert!(app
        .world_mut()
        .query::<&TradeRouteEditorButton>()
        .iter(app.world())
        .any(|b| b.0 == TradeRouteEditorAction::RemoveStop(3)));
}

#[test]
fn company_settings_remain_addressable_without_an_observed_workplace() {
    use crate::ui::business_management::BusinessManagementSelection;
    let (mut app, _, _) = retained_company_app();
    let buttons: Vec<_> = app
        .world_mut()
        .query::<&CompanyManagementButton>()
        .iter(app.world())
        .map(|button| (button.target, button.company))
        .collect();
    assert_eq!(
        buttons,
        vec![(
            BusinessManagementSelection::Company(CompanyId(1)),
            CompanyId(1)
        )]
    );
}

fn fleet_fixture(kind: shared::components::ShipKind) -> (CompanyRecord, CompanyDirectory) {
    use shared::components::*;
    let mut company = company(43, PersonId(8), 50_000, 0, 0);
    company.fleet.ships.push((
        ShipId(91),
        CompanyShip {
            company: company.id,
            kind,
            home_port: BuildingId(71),
            assigned_route: None,
            status: ShipStatus::Moored,
        },
    ));
    company
        .sites
        .push(storage_site(40, SettlementId(1), "Oakfell"));
    let directory = CompanyDirectory {
        records: vec![company.clone()],
        local_person: Some(PersonId(8)),
        local_wallet: Some(500),
        settlements: vec![
            CompanySettlementRecord {
                id: SettlementId(1),
                name: "Oakfell".into(),
                has_marketplace: true,
                port: Some(SettlementPortSummary {
                    port: BuildingId(71),
                    maximum_ship: ShipKind::Cog,
                    built: true,
                }),
            },
            CompanySettlementRecord {
                id: SettlementId(2),
                name: "Small Quay".into(),
                has_marketplace: true,
                port: Some(SettlementPortSummary {
                    port: BuildingId(72),
                    maximum_ship: ShipKind::Coaster,
                    built: true,
                }),
            },
            CompanySettlementRecord {
                id: SettlementId(3),
                name: "Unfinished Port".into(),
                has_marketplace: true,
                port: Some(SettlementPortSummary {
                    port: BuildingId(73),
                    maximum_ship: ShipKind::Cog,
                    built: false,
                }),
            },
        ],
    };
    (company, directory)
}
#[test]
fn ship_timetable_uses_hull_capacity_and_only_compatible_finished_ports() {
    use shared::components::*;
    let (coaster, directory) = fleet_fixture(ShipKind::Coaster);
    let mut draft = super::fleet::new_ship_draft(&coaster, &directory, ShipId(91)).unwrap();
    assert_eq!(
        draft.stops.iter().map(|s| s.settlement).collect::<Vec<_>>(),
        [SettlementId(1), SettlementId(2)]
    );
    assert_eq!(
        draft.stop_actions(),
        [TradeRouteStopAction::Buy, TradeRouteStopAction::Sell]
    );
    draft.good = shared::economy::Good::Iron;
    assert_eq!(
        draft.cargo_capacity(),
        ShipKind::Coaster.capacity() / draft.good.bulk_per_unit()
    );
    assert!(
        draft.cargo_capacity() > shared::economy::capacity::PORTER / draft.good.bulk_per_unit()
    );
    assert!(!draft.accepts_settlement(&directory.settlements[2]));
    let (cog, directory) = fleet_fixture(ShipKind::Cog);
    assert!(
        super::fleet::new_ship_draft(&cog, &directory, ShipId(91)).is_err(),
        "large hull has no valid second port"
    );
}
#[test]
fn native_ship_cargo_control_respects_ship_capacity_and_preserves_draft() {
    use bevy::ecs::system::RunSystemOnce;
    use shared::components::*;
    let (company, directory) = fleet_fixture(ShipKind::Coaster);
    let mut draft = super::fleet::new_ship_draft(&company, &directory, ShipId(91)).unwrap();
    draft.good = shared::economy::Good::Wood;
    let capacity = draft.cargo_capacity();
    let mut world = World::new();
    world.insert_resource(directory);
    world.insert_resource(crate::ui::encyclopedia::ClickGuard(true));
    let mut mouse = ButtonInput::<MouseButton>::default();
    mouse.press(MouseButton::Left);
    world.insert_resource(mouse);
    world.insert_resource(TradeRouteEditorState {
        draft: Some(draft),
        ..default()
    });
    world.spawn((
        Interaction::Pressed,
        TradeRouteEditorButton(TradeRouteEditorAction::CargoUp(u32::MAX)),
    ));
    world
        .run_system_once(super::route_actions::handle_trade_route_editor_buttons)
        .unwrap();
    let draft = world
        .resource::<TradeRouteEditorState>()
        .draft
        .as_ref()
        .unwrap();
    assert_eq!(draft.cargo_target, capacity);
    assert_eq!(draft.ship, Some((ShipId(91), ShipKind::Coaster)));
    assert!(!draft.pending);
    assert_eq!(draft.stops.len(), 2);
}
#[test]
fn fleet_order_buttons_follow_company_authority_and_port_class() {
    use shared::components::*;
    let (company, mut directory) = fleet_fixture(ShipKind::Coaster);
    let mut world = World::new();
    let host = world.spawn_empty().id();
    let fixture = ViewFixture::new(company.clone(), directory.clone());
    world
        .commands()
        .entity(host)
        .with_children(|p| super::fleet::spawn_fleet(p, &fixture.view()));
    world.flush();
    let actions: Vec<_> = world
        .query::<&super::fleet::FleetButton>()
        .iter(&world)
        .map(|b| b.action)
        .collect();
    assert!(actions.contains(&super::fleet::FleetAction::NewRoute(ShipId(91))));
    assert!(actions.contains(&super::fleet::FleetAction::Order {
        port: BuildingId(71),
        kind: ShipKind::Cog
    }));
    assert!(!actions.contains(&super::fleet::FleetAction::Order {
        port: BuildingId(72),
        kind: ShipKind::Cog
    }));
    assert!(!actions.iter().any(|a| matches!(
        a,
        super::fleet::FleetAction::Order {
            port: BuildingId(73),
            ..
        }
    )));
    let mut readonly = World::new();
    let host = readonly.spawn_empty().id();
    directory.local_person = Some(PersonId(999));
    let fixture = ViewFixture::new(company, directory);
    readonly
        .commands()
        .entity(host)
        .with_children(|p| super::fleet::spawn_fleet(p, &fixture.view()));
    readonly.flush();
    assert_eq!(
        readonly
            .query::<&super::fleet::FleetButton>()
            .iter(&readonly)
            .count(),
        0
    );
    assert!(
        readonly
            .query::<&Text>()
            .iter(&readonly)
            .any(|text| text.0.contains("480 bulk")),
        "read-only observers still see fleet facts"
    );
}

#[test]
fn active_ship_route_exposes_graceful_stop_without_offering_unsafe_edit() {
    use shared::components::*;
    let mut route = merchant_route();
    route.ship = Some((ShipId(91), ShipKind::Coaster));
    route.status = TradeRouteStatus::InTransit;
    route.assigned_caravaner = Some("Sailor".into());
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let fixture = ViewFixture::managed(company_with_route(43, PersonId(8), route.clone()));
    world
        .commands()
        .entity(parent)
        .with_children(|p| spawn_route_card(p, &fixture.view(), &route));
    world.flush();
    assert_eq!(
        world.query::<&EditTradeRouteButton>().iter(&world).count(),
        0
    );
    assert!(world
        .query::<&TradeRouteQuickActionButton>()
        .iter(&world)
        .any(|button| button.action == TradeRouteQuickAction::Mothball));
    assert!(world
        .query::<&Text>()
        .iter(&world)
        .any(|text| text.0 == "STOP AFTER VOYAGE"));
}

#[test]
fn company_details_show_distributable_and_last_paid_dividend() {
    use shared::economy::CompanyDividendCapacity;
    let (mut app, _, detail) = retained_company_app();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    let text = |world: &mut World, field: CompanyField| {
        world
            .query::<(&CompanyBound, &Text, &Node)>()
            .iter(world)
            .find(|(key, ..)| **key == CompanyBound::Company(field))
            .map(|(_, text, node)| (text.0.clone(), node.display))
            .unwrap_or_else(|| panic!("no bound text for {field:?}"))
    };
    // No snapshot yet: the row says so and the position note stays hidden.
    let (capacity, _) = text(app.world_mut(), CompanyField::DividendCapacity);
    assert!(
        capacity.contains("awaiting the first finance review"),
        "{capacity}"
    );
    assert_eq!(
        text(app.world_mut(), CompanyField::PositionDividend).1,
        Display::None
    );

    // The finance pass publishes: 300.00 coin available, 200.00 paid on day 11.
    app.world_mut().resource_mut::<CompanyDirectory>().records[0].capacity =
        Some(CompanyDividendCapacity {
            day: 12,
            distributable: 30_000,
            protected_reserves: 1_850,
            retained_profit: 46_200,
            last_paid_day: 11,
            last_paid: 20_000,
        });
    app.update();
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children,
        "a capacity snapshot is a value and must bind in place"
    );
    let (capacity, _) = text(app.world_mut(), CompanyField::DividendCapacity);
    assert!(
        capacity.contains("300.00 coin available now (day 12)")
            && capacity.contains("18.50 coin reserved")
            && capacity.contains("last paid 200.00 coin on day 11"),
        "{capacity}"
    );
    // The sole owner of company 1 takes the whole distribution.
    let (position, display) = text(app.world_mut(), CompanyField::PositionDividend);
    assert_eq!(display, Display::Flex);
    assert!(
        position.contains("300.00 coin") && position.contains("your 1000 shares 300.00 coin"),
        "{position}"
    );

    // A 600 / 400 cap table: the local master's take comes from the shared
    // split of the exact cap table, not from the display-sorted holders.
    let owner = PersonId(8);
    let minor = PersonId(3);
    let mut split_company = company(43, owner, 0, 0, 0);
    split_company.ownership = shared::components::CompanyOwnership::from_shares(vec![
        shared::components::CompanyShare {
            shareholder: owner,
            shares: 600,
        },
        shared::components::CompanyShare {
            shareholder: minor,
            shares: 400,
        },
    ])
    .unwrap();
    split_company.holders = vec![
        CompanyHolderRecord {
            person: owner,
            name: "Owner".into(),
            shares: 600,
        },
        CompanyHolderRecord {
            person: minor,
            name: "Minor".into(),
            shares: 400,
        },
    ];
    split_company.capacity = Some(CompanyDividendCapacity {
        day: 12,
        distributable: 1_001,
        protected_reserves: 0,
        retained_profit: 1_001,
        last_paid_day: u32::MAX,
        last_paid: 0,
    });
    let fixture = ViewFixture::managed(split_company);
    let view = fixture.view();
    let mut split = Vec::new();
    shared::economy::pro_rata_split(1_001, &fixture.company.ownership, &mut split);
    // PersonId 3 sorts first on the cap table and takes the penny remainder.
    assert_eq!(split, vec![(minor, 401), (owner, 600)]);
    let position = view
        .value(CompanyBound::Company(CompanyField::PositionDividend))
        .and_then(|value| value.text)
        .unwrap();
    assert!(position.contains("your 600 shares 6.00 coin"), "{position}");
    let capacity = view
        .value(CompanyBound::Company(CompanyField::DividendCapacity))
        .and_then(|value| value.text)
        .unwrap();
    assert!(capacity.contains("never paid"), "{capacity}");
}

#[test]
fn contribute_buttons_follow_share_ownership_not_sole_mastery() {
    let (mut app, _, _) = retained_company_app();
    let contribute_buttons = |world: &mut World| {
        world
            .query::<&CompanyBranchPolicyButton>()
            .iter(world)
            .filter(|button| matches!(button.action, HeroCompanyAction::ContributeCapital { .. }))
            .count()
    };
    // The sole master sees the two donation presets.
    assert_eq!(contribute_buttons(app.world_mut()), 2);

    // A co-owned company: the master is still a shareholder and may donate.
    let owner = PersonId(10);
    let minor = PersonId(11);
    {
        let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
        let record = &mut directory.records[0];
        record.ownership = shared::components::CompanyOwnership::from_shares(vec![
            shared::components::CompanyShare {
                shareholder: owner,
                shares: 600,
            },
            shared::components::CompanyShare {
                shareholder: minor,
                shares: 400,
            },
        ])
        .unwrap();
        record.holders = vec![
            CompanyHolderRecord {
                person: owner,
                name: "Owner".into(),
                shares: 600,
            },
            CompanyHolderRecord {
                person: minor,
                name: "Minor".into(),
                shares: 400,
            },
        ];
    }
    app.update();
    app.update();
    assert_eq!(
        contribute_buttons(app.world_mut()),
        2,
        "a co-owning master may still donate"
    );

    // So may the minority holder.
    app.world_mut()
        .resource_mut::<CompanyDirectory>()
        .local_person = Some(minor);
    app.update();
    app.update();
    assert_eq!(
        contribute_buttons(app.world_mut()),
        2,
        "any shareholder may donate"
    );

    // Someone without shares has nothing to donate into.
    app.world_mut()
        .resource_mut::<CompanyDirectory>()
        .local_person = Some(PersonId(999));
    app.update();
    app.update();
    assert_eq!(contribute_buttons(app.world_mut()), 0);
}

/// A page (company settings, ledger) covering the tab body stops the bind
/// pass; the gate reads the page resources, not the `TabBody` display that
/// other systems rewrite during the frame, so it never flaps.
#[test]
fn a_covering_page_stops_binding_until_it_closes() {
    use crate::ui::business_management::{BusinessManagementSelection, BusinessManagementTarget};
    let (mut app, _, detail) = retained_company_app();
    let children = app.world().get::<Children>(detail).unwrap().to_vec();
    assert_eq!(
        bound_text(app.world_mut(), CompanyBound::Company(CompanyField::Cash)),
        format!("{} coin", shared::economy::format_money(100))
    );

    app.insert_resource(BusinessManagementTarget(Some(
        BusinessManagementSelection::Company(CompanyId(1)),
    )));
    app.world_mut().resource_mut::<CompanyDirectory>().records[0]
        .account
        .cash += 10;
    for _ in 0..3 {
        app.update();
        assert_eq!(
            bound_text(app.world_mut(), CompanyBound::Company(CompanyField::Cash)),
            format!("{} coin", shared::economy::format_money(100)),
            "nothing binds while a page covers the tab body"
        );
    }

    app.world_mut().resource_mut::<BusinessManagementTarget>().0 = None;
    app.update();
    assert_eq!(
        bound_text(app.world_mut(), CompanyBound::Company(CompanyField::Cash)),
        format!("{} coin", shared::economy::format_money(110)),
        "the first visible frame binds everything that moved"
    );
    assert_eq!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children,
        "revealing the tab binds; it does not respawn"
    );
}

#[test]
fn the_directory_snapshot_condition_follows_the_page_resources() {
    use crate::ui::business_management::{BusinessManagementSelection, BusinessManagementTarget};
    use crate::ui::encyclopedia::companies_body_visible;
    use bevy::ecs::system::RunSystemOnce;
    let mut world = World::new();
    assert!(
        world.run_system_once(companies_body_visible).unwrap(),
        "no page resources at all means nothing covers the tab"
    );
    world.insert_resource(BusinessManagementTarget(None));
    assert!(world.run_system_once(companies_body_visible).unwrap());
    world.resource_mut::<BusinessManagementTarget>().0 =
        Some(BusinessManagementSelection::Company(CompanyId(1)));
    assert!(!world.run_system_once(companies_body_visible).unwrap());
}
