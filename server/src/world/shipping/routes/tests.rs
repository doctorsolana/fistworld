use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::economy::{Good, MarketTradeTier};

fn geometry(offset: f32) -> PortGeometry {
    PortGeometry {
        shore: Vec3::new(offset, 2.0, 0.0),
        pier_end: Vec3::new(offset, 1.0, 20.0),
        berth: Vec3::new(offset, 0.0, 26.0),
        departure: Vec3::new(offset + 15.0, 0.0, 26.0),
        yaw: -std::f32::consts::FRAC_PI_2,
        maximum_ship: ShipKind::Cog,
    }
}
fn route() -> CompanyTradeRoute {
    CompanyTradeRoute {
        company: CompanyId(1),
        warehouse: BuildingId(1),
        mode: TradeRouteMode::Merchant,
        origin: SettlementId(1),
        destination: SettlementId(2),
        good: Good::Wood,
        cargo_target: 6,
        maximum_purchase_price: 100,
        minimum_destination_price: 80,
        automatic: false,
        autonomous_management: false,
        expected_trip_profit: 0,
        decision_confidence: 100,
        active_contract: None,
        assigned_caravaner: None,
        current_stop: 0,
        status: TradeRouteStatus::Loading,
        completed_trips: 0,
        lifetime_units: 0,
        lifetime_delivery_revenue: 0,
        lifetime_purchase_cost: 0,
        lifetime_consigned_value: 0,
    }
}
fn routine(port: Entity) -> ShipRouteRoutine {
    ShipRouteRoutine {
        route: Entity::PLACEHOLDER,
        port,
        next_port: port,
        stop: 0,
        returning: false,
        phase: Phase::AtStop,
        departed_day: 0,
        started_at: 0.0,
        purchased_units: 0,
        purchase_cost: 0,
        market_fees: 0,
        consigned_value: 0,
        stops_visited: 0,
        retry_in: 0.0,
        departure_goal: None,
        holding_goal: None,
    }
}
fn hall(world: &mut World, id: SettlementId, units: u32, capacity: u32) -> Entity {
    let mut stock = GoodsInventory::new(capacity);
    stock.add(Good::Wood, units);
    let mut market = MootMarket::founding();
    market.unlock_trade_tier(MarketTradeTier::Marketplace);
    market.set_market_fee_bps(500);
    market.consign(MarketSeller::Treasury(id), Good::Wood, units, 50);
    world
        .spawn((
            id,
            Settlement {
                name: "Port town".into(),
                tier: SettlementTier::Town,
                residents: 0,
                treasury: 0,
            },
            stock,
            market,
        ))
        .id()
}

#[test]
fn ship_purchase_and_consignment_preserve_actual_town_stock_title_and_cash() {
    let mut world = World::new();
    world.init_resource::<BusinessEventQueue>();
    let company = world
        .spawn((
            CompanyId(1),
            CompanyAccount {
                cash: 1_000,
                ..default()
            },
        ))
        .id();
    let warehouse = world
        .spawn((
            BuildingId(1),
            OperatedBy(CompanyId(1)),
            BusinessAccount::default(),
        ))
        .id();
    let home = hall(&mut world, SettlementId(1), 6, 100);
    let away = hall(
        &mut world,
        SettlementId(2),
        0,
        2 * Good::Wood.bulk_per_unit(),
    );
    let port = world
        .spawn(SettlementPort {
            settlement: SettlementId(1),
            geometry: geometry(0.0),
            built: true,
        })
        .id();
    let ship = world
        .spawn((
            CompanyShip {
                company: CompanyId(1),
                kind: ShipKind::Coaster,
                home_port: BuildingId(3),
                assigned_route: None,
                status: ShipStatus::Loading,
            },
            PlayerPosition(geometry(0.0).berth),
            GoodsInventory::new(ShipKind::Coaster.capacity()),
        ))
        .id();
    let mut route = route();
    let mut routine = routine(port);
    let buy = TradeRouteStop {
        settlement: SettlementId(1),
        action: TradeRouteStopAction::Buy,
    };
    world.get_mut::<PlayerPosition>(ship).unwrap().0 += Vec3::X * 5.0;
    assert!(
        !transact(&mut world, ship, &mut route, buy, &mut routine, 0),
        "being near the town is not arrival at its berth"
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(home)
            .unwrap()
            .amount(Good::Wood),
        6
    );
    world.get_mut::<PlayerPosition>(ship).unwrap().0 = geometry(0.0).berth;
    assert!(transact(&mut world, ship, &mut route, buy, &mut routine, 0));
    world
        .run_system_once(crate::world::village::apply_business_events)
        .unwrap();
    assert_eq!(
        world
            .get::<GoodsInventory>(home)
            .unwrap()
            .amount(Good::Wood),
        0
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(ship)
            .unwrap()
            .amount(Good::Wood),
        6
    );
    assert_eq!(
        world.get::<CompanyAccount>(company).unwrap().cash
            + world.get::<Settlement>(home).unwrap().treasury,
        1_000
    );
    assert_eq!(
        world
            .get::<BusinessAccount>(warehouse)
            .unwrap()
            .operating_expenses,
        routine.purchase_cost
    );
    let cash = world.get::<CompanyAccount>(company).unwrap().cash;
    world.get_mut::<SettlementPort>(port).unwrap().settlement = SettlementId(2);
    let sell = TradeRouteStop {
        settlement: SettlementId(2),
        action: TradeRouteStopAction::Sell,
    };
    assert!(!transact(
        &mut world,
        ship,
        &mut route,
        sell,
        &mut routine,
        0
    ));
    assert_eq!(
        world
            .get::<GoodsInventory>(ship)
            .unwrap()
            .amount(Good::Wood),
        4
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(away)
            .unwrap()
            .amount(Good::Wood),
        2
    );
    assert_eq!(
        world
            .get::<MootMarket>(away)
            .unwrap()
            .seller_listed_units(MarketSeller::Business(BuildingId(1)), Good::Wood),
        2
    );
    assert_eq!(
        world.get::<CompanyAccount>(company).unwrap().cash,
        cash,
        "unsold consignment is never revenue"
    );
    world
        .get_mut::<GoodsInventory>(away)
        .unwrap()
        .resize_bulk_capacity(100);
    assert!(transact(
        &mut world,
        ship,
        &mut route,
        sell,
        &mut routine,
        0
    ));
    assert_eq!(
        world
            .get::<GoodsInventory>(ship)
            .unwrap()
            .amount(Good::Wood),
        0
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(away)
            .unwrap()
            .amount(Good::Wood),
        6
    );
    assert_eq!(
        world
            .get::<MootMarket>(away)
            .unwrap()
            .seller_listed_units(MarketSeller::Business(BuildingId(1)), Good::Wood),
        6
    );
    assert_eq!(route.lifetime_delivery_revenue, 0);
}

#[test]
fn berth_reservations_cannot_be_stolen_by_another_ship() {
    let mut world = World::new();
    let port = world.spawn_empty().id();
    assert!(reserve(&mut world, port, ShipId(1)));
    assert!(!reserve(&mut world, port, ShipId(2)));
    release_berth(&mut world, port, ShipId(2));
    assert!(!berth_available(&world, port, None));
    release_berth(&mut world, port, ShipId(1));
    assert!(reserve(&mut world, port, ShipId(2)));
}

#[test]
fn shipping_schedule_requires_matching_completed_ports_and_market_actions() {
    let stops = [
        TradeRouteStop {
            settlement: SettlementId(1),
            action: TradeRouteStopAction::Buy,
        },
        TradeRouteStop {
            settlement: SettlementId(2),
            action: TradeRouteStopAction::Sell,
        },
    ];
    let ports = [
        SettlementPort {
            settlement: SettlementId(1),
            geometry: geometry(0.0),
            built: true,
        },
        SettlementPort {
            settlement: SettlementId(2),
            geometry: geometry(100.0),
            built: true,
        },
    ];
    assert!(validate_stops(&stops, SettlementId(1), ShipKind::Cog, ports.into_iter()).is_ok());
    let mut blocked = ports;
    blocked[1].built = false;
    assert!(
        validate_stops(
            &stops,
            SettlementId(1),
            ShipKind::Coaster,
            blocked.into_iter()
        )
        .is_err()
    );
    blocked = ports;
    blocked[1].geometry.maximum_ship = ShipKind::Coaster;
    assert!(validate_stops(&stops, SettlementId(1), ShipKind::Cog, blocked.into_iter()).is_err());
    let mut private = stops;
    private[0].action = TradeRouteStopAction::Load;
    assert!(
        validate_stops(
            &private,
            SettlementId(1),
            ShipKind::Coaster,
            ports.into_iter()
        )
        .is_err()
    );
}

#[test]
fn captain_waiting_to_depart_can_restock_after_the_last_ration_is_eaten() {
    use crate::world::village::CompanyPorter;
    let mut world = World::new();
    world.init_resource::<BusinessEventQueue>();
    crew::tests::configure_movement(&mut world, 1.0);
    let home = hall(&mut world, SettlementId(1), 0, 100);
    world.entity_mut(home).insert((
        PlayerPosition(Vec3::new(-20.0, 0.0, 0.0)),
        PlayerRotation(0.0),
    ));
    world
        .get_mut::<GoodsInventory>(home)
        .unwrap()
        .add(Good::Bread, 10);
    world.get_mut::<MootMarket>(home).unwrap().consign(
        MarketSeller::Treasury(SettlementId(1)),
        Good::Bread,
        10,
        20,
    );
    let company = world
        .spawn((
            CompanyId(1),
            CompanyAccount {
                cash: 1_000,
                ..default()
            },
        ))
        .id();
    world.spawn((
        BuildingId(1),
        BusinessAccount::default(),
        OperatedBy(CompanyId(1)),
    ));
    let port_state = SettlementPort {
        settlement: SettlementId(1),
        geometry: geometry(0.),
        built: true,
    };
    let port = world
        .spawn((BuildingId(2), port_state, PortBerthReservation(ShipId(1))))
        .id();
    let route_entity = world
        .spawn((
            TradeRouteId(1),
            route(),
            MaritimeTradeRoute { ship: ShipId(1) },
        ))
        .id();
    let mut cargo = GoodsInventory::new(ShipKind::Coaster.capacity());
    cargo.add(Good::Wood, 6);
    let ship = world
        .spawn((
            ShipId(1),
            Vessel,
            CompanyShip {
                company: CompanyId(1),
                kind: ShipKind::Coaster,
                home_port: BuildingId(2),
                assigned_route: Some(TradeRouteId(1)),
                status: ShipStatus::WaitingForBerth,
            },
            PlayerPosition(port_state.geometry.berth),
            PlayerRotation(0.),
            cargo,
        ))
        .id();
    let person = world
        .spawn((
            PersonId(1),
            CharacterKind::Villager,
            CompanyPorter {
                company: CompanyId(1),
                storage_hall: BuildingId(1),
                settlement: home,
                settlement_id: SettlementId(1),
            },
            PlayerPosition(port_state.geometry.shore),
            PlayerRotation(0.0),
            shared::region::RegionCoord::from_world_pos(port_state.geometry.shore),
            GoodsInventory::new(24),
            CharacterActivity::Idle,
            CharacterMotion::STATIONARY,
        ))
        .id();
    crew::acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port_state);
    // The captain first collects real Hall provisions, then walks back to the
    // shore and boards. No actor position or inventory is granted on the way.
    for _ in 0..7_200 {
        crew::tests::tick(&mut world);
        if world.get::<AboardShip>(person).is_some() {
            break;
        }
    }
    assert_eq!(
        world.get::<AboardShip>(person).map(|aboard| aboard.ship),
        Some(ShipId(1)),
        "captain did not finish physical boarding"
    );
    // Acquisition recognizes the already supplied, physically boarded captain.
    assert!(crew::acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port_state
    ));
    // Represents the actual meals consumed while the berth's channel remains
    // blocked. No inventory is granted and the already-loaded freight stays put.
    assert_eq!(
        world
            .get_mut::<GoodsInventory>(person)
            .unwrap()
            .remove(Good::Bread, 3),
        3
    );
    let mut state = routine(port);
    state.route = route_entity;
    state.phase = Phase::Departing;
    world.entity_mut(ship).insert(state);
    let cash_before_restock = world.get::<CompanyAccount>(company).unwrap().cash;
    for _ in 0..7_200 {
        advance_shipping(&mut world);
        crew::tests::tick(&mut world);
        if crew::crew_ready(&world, ship) {
            break;
        }
        if world.get::<GoodsInventory>(person).unwrap().edible_amount() == 0 {
            assert_eq!(
                world.get::<CompanyAccount>(company).unwrap().cash,
                cash_before_restock
            );
            assert_eq!(
                world
                    .get::<GoodsInventory>(home)
                    .unwrap()
                    .amount(Good::Bread),
                7
            );
        }
    }
    assert!(crew::crew_ready(&world, ship));
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Bread),
        3
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(home)
            .unwrap()
            .amount(Good::Bread),
        4
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(ship)
            .unwrap()
            .amount(Good::Wood),
        6
    );
    assert_eq!(
        world.get::<PlayerPosition>(ship).unwrap().0,
        port_state.geometry.berth
    );
    assert_eq!(
        world.get::<CompanyAccount>(company).unwrap().cash
            + world.resource::<BusinessEventQueue>().pending_sale_gross(),
        1_000
    );
}
