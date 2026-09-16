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
    use super::controls::{CompanyFilterButton, CompanyRow, style_company_controls};
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

fn retained_company_app() -> (App, Entity, Entity) {
    use super::controls::{
        CompanyDetailContent, CompanyDetailViewport, CompanyListContent, CompanyPortfolioContent,
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
        .insert_resource(SelectedCompany(Some(CompanyId(1))))
        .init_resource::<CompanyPolicyFeedback>()
        .init_resource::<TradeRouteEditorState>()
        .init_resource::<crate::ui::perf::UiPerf>()
        .add_systems(Update, super::view::rebuild_company_view);
    app.world_mut()
        .spawn((CompanyPortfolioContent, Node::default()));
    app.world_mut().spawn((CompanyListContent, Node::default()));
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
fn live_company_cash_retains_hovered_and_pressed_controls_then_flushes_without_another_snapshot() {
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
    for state in [Interaction::Hovered, Interaction::Pressed] {
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = state;
        let mut directory = app.world_mut().resource_mut::<CompanyDirectory>();
        directory.records[0].account.cash += 10;
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
    }
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
    app.update();
    assert!(
        app.world().get_entity(button).is_err(),
        "the deferred snapshot must flush on pointer leave"
    );
    assert_ne!(
        app.world().get::<Children>(detail).unwrap().to_vec(),
        children
    );
    assert_eq!(
        app.world().get::<ScrollPosition>(viewport).unwrap().0.y,
        420.0
    );
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
fn route_draft_survives_live_snapshot_and_explicit_draft_actions_update_while_hovered() {
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
    app.world_mut()
        .resource_mut::<TradeRouteEditorState>()
        .draft
        .as_mut()
        .unwrap()
        .cargo_target += 1;
    app.update();
    assert!(
        app.world().get_entity(button).is_err(),
        "an explicit draft action must refresh immediately"
    );
    assert_eq!(
        app.world()
            .resource::<TradeRouteEditorState>()
            .draft
            .as_ref()
            .unwrap()
            .cargo_target,
        38
    );
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
    world
        .commands()
        .entity(host)
        .with_children(|p| super::fleet::spawn_fleet(p, &company, &directory));
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
    readonly
        .commands()
        .entity(host)
        .with_children(|p| super::fleet::spawn_fleet(p, &company, &directory));
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
    world
        .commands()
        .entity(parent)
        .with_children(|p| spawn_route_card(p, CompanyId(43), &route, true));
    world.flush();
    assert_eq!(
        world.query::<&EditTradeRouteButton>().iter(&world).count(),
        0
    );
    assert!(
        world
            .query::<&TradeRouteQuickActionButton>()
            .iter(&world)
            .any(|button| button.action == TradeRouteQuickAction::Mothball)
    );
    assert!(
        world
            .query::<&Text>()
            .iter(&world)
            .any(|text| text.0 == "STOP AFTER VOYAGE")
    );
}
