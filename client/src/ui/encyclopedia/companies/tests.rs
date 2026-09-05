//! Company accounting presentation and selective-rebuild regressions.

use super::controls::{
    CompanyManagementButton, CompanySiteButton, EditTradeRouteButton, TradeRouteEditorButton,
    TradeRouteQuickActionButton,
};
use super::model::{
    CompanyDirectory, CompanyFilter, CompanyHolderRecord, CompanyRecord, CompanyRouteRecord,
    CompanyRouteStopRecord, CompanySettlementRecord, CompanySiteRecord, TradeRouteDraft,
    TradeRouteEditorAction, TradeRouteEditorState, TradeRouteQuickAction,
};
use super::route_editor::spawn_trade_route_editor;
use super::routes::spawn_route_card;
use super::sites::spawn_site_card;
use super::view::visible_companies;
use bevy::prelude::*;
use shared::components::{
    BuildingId, CompanyId, PersonId, SettlementBuildingKind, SettlementId, TradeRouteId,
    TradeRouteMode, TradeRouteStatus, TradeRouteStop, TradeRouteStopAction,
};
use shared::economy::{
    BusinessSourcingMode, BusinessState, CompanyAccount, CompanyManagementPolicy, Good,
};

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
    let visible = visible_companies(&directory, CompanyFilter::MyHoldings);
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].id, CompanyId(1));
    assert_eq!(visible[1].id, CompanyId(2));
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
    world
        .commands()
        .entity(parent)
        .with_children(|children| spawn_site_card(children, CompanyId(43), &site));
    world.flush();

    let mut details = world.query_filtered::<&CompanySiteButton, With<Button>>();
    let mut management = world.query_filtered::<&CompanyManagementButton, With<Button>>();
    assert_eq!(details.single(&world).unwrap().0, site.id);
    assert_eq!(management.single(&world).unwrap().site, site.entity);
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

#[test]
fn idle_merchant_route_card_exposes_building_style_management_actions() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    world.commands().entity(parent).with_children(|children| {
        spawn_route_card(children, CompanyId(43), &merchant_route(), true)
    });
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
            },
            CompanySettlementRecord {
                id: SettlementId(2),
                name: "Stonefield".into(),
                has_marketplace: true,
            },
            CompanySettlementRecord {
                id: SettlementId(3),
                name: "Meadowford".into(),
                has_marketplace: true,
            },
        ],
        local_person: Some(owner),
        local_wallet: Some(1_000),
    };
    let route = merchant_route();
    let draft = TradeRouteDraft {
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
    world.commands().entity(parent).with_children(|children| {
        spawn_trade_route_editor(
            children,
            &firm,
            &directory,
            &draft,
            &TradeRouteEditorState::default(),
        )
    });
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
