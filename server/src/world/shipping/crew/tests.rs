use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::economy::{MarketSeller, PorterCartState, capacity};

#[derive(bevy::ecs::schedule::ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct CrewMovement;

/// Retain the real planner's incremental state across test ticks, just like
/// the server schedule; run_system_once would discard its retained searches.
pub(crate) fn configure_movement(world: &mut World, warp: f32) {
    use shared::map::{HeightmapData, LoadedMap, MapBounds, MapDefinition, MapTerrain};
    let bounds = MapBounds {
        min: [-128.0; 2],
        max: [128.0; 2],
    };
    world.insert_resource(shared::terrain::WorldTerrain::from_loaded_map(LoadedMap {
        definition: MapDefinition {
            map_id: "crew-physical-test".into(),
            bounds,
            terrain: MapTerrain {
                heightmap: "test".into(),
                minimap: None,
                water_level: Some(-10.0),
                height_min: 0.0,
                height_max: 0.0,
            },
            generated: None,
            player_spawn: None,
            objects: vec![],
            blockers: vec![],
        },
        heightmap: HeightmapData::new(bounds, 2, 2, vec![0.0; 4], Some(-10.0)),
        edits: default(),
        terrain_deltas_by_chunk: default(),
        objects_by_chunk: default(),
        biome_field: None,
        rivers: std::sync::Arc::new(vec![]),
        river_segments_by_chunk: default(),
        content_hash: 1,
        map_dir: default(),
    }));
    world.init_resource::<Time>();
    world.init_resource::<SimulationDelta>();
    world.init_resource::<crate::world::pathfinding::PathfindingBudgetSettings>();
    world.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    world.spawn((WorldTime::new_default(), TimeWarp::clamped(warp)));
    let mut schedule = Schedule::new(CrewMovement);
    schedule.add_systems(
        (
            crate::world::simulation_time::refresh_simulation_delta,
            crate::world::village::run_workplace_door_transits,
            advance_crew,
            crate::world::village_roads::queue_villager_travel_routes,
            crate::world::village_roads::retry_failed_routes_after_obstacle_change,
            crate::world::village_roads::plan_villager_travel_routes,
            crate::player::hero::step_units,
            crate::player::hero::settle_characters_without_targets,
        )
            .chain(),
    );
    world.add_schedule(schedule);
}

pub(crate) fn tick(world: &mut World) {
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f64(1.0 / 60.0));
    world.run_schedule(CrewMovement);
}

fn wait_until(world: &mut World, predicate: impl Fn(&World) -> bool) {
    for _ in 0..7_200 {
        if predicate(world) {
            return;
        }
        tick(world);
    }
    assert!(
        predicate(world),
        "crew lifecycle did not reach its next physical boundary within 120 real seconds"
    );
}

fn setup() -> (World, Entity, Entity, SettlementPort) {
    let mut world = World::new();
    world.init_resource::<BusinessEventQueue>();
    configure_movement(&mut world, 1.0);
    let port = SettlementPort {
        settlement: SettlementId(1),
        built: true,
        geometry: PortGeometry {
            shore: Vec3::new(0.0, 1.0, 0.0),
            pier_end: Vec3::new(0.0, 1.0, 20.0),
            berth: Vec3::new(0.0, 0.0, 26.0),
            departure: Vec3::new(15.0, 0.0, 26.0),
            yaw: 0.0,
            maximum_ship: ShipKind::Cog,
        },
    };
    let mut stock = GoodsInventory::new(100);
    stock.add(Good::Bread, 10);
    let mut market = MootMarket::founding();
    market.consign(MarketSeller::Treasury(SettlementId(1)), Good::Bread, 10, 20);
    let hall = world
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Harbour".into(),
                tier: SettlementTier::Town,
                residents: 1,
                treasury: 0,
            },
            stock,
            market,
            PlayerPosition(Vec3::new(-20.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ))
        .id();
    world.spawn((
        CompanyId(1),
        CompanyAccount {
            cash: 1_000,
            ..default()
        },
    ));
    world.spawn((BuildingId(1), BusinessAccount::default()));
    let ship = world
        .spawn((
            ShipId(1),
            CompanyShip {
                company: CompanyId(1),
                kind: ShipKind::Coaster,
                home_port: BuildingId(2),
                assigned_route: None,
                status: ShipStatus::Moored,
            },
            PlayerPosition(port.geometry.berth),
            PlayerRotation(0.0),
        ))
        .id();
    let person = world
        .spawn((
            PersonId(1),
            CharacterKind::Villager,
            CompanyPorter {
                company: CompanyId(1),
                storage_hall: BuildingId(1),
                settlement: hall,
                settlement_id: SettlementId(1),
            },
            crate::world::village::VillagerIntent::Resident { settlement: hall },
            PlayerPosition(port.geometry.shore),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(port.geometry.shore),
            GoodsInventory::new(capacity::VILLAGER),
            CharacterMotion::STATIONARY,
            CharacterActivity::Idle,
        ))
        .id();
    (world, ship, person, port)
}

#[test]
fn captain_boards_walks_the_gangway_buys_real_food_and_returns_to_shore() {
    let (mut world, ship, person, port) = setup();
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert!(world.get::<ShipCrew>(person).is_some());
    let from = world.get::<PlayerPosition>(person).unwrap().0;
    wait_until(&mut world, |world| crew_ready(world, ship));
    assert!(world.get::<AboardShip>(person).is_some());
    assert!(
        world
            .get::<PlayerPosition>(person)
            .unwrap()
            .0
            .distance(from)
            > 10.0
    );
    assert!(acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Bread),
        3
    );
    let cash = world
        .query::<&CompanyAccount>()
        .single(&world)
        .unwrap()
        .cash;
    assert!(cash < 1_000);
    assert_eq!(
        cash + world.resource::<BusinessEventQueue>().pending_sale_gross(),
        1_000
    );
    let hall = world
        .query_filtered::<&GoodsInventory, With<Settlement>>()
        .single(&world)
        .unwrap();
    assert_eq!(hall.amount(Good::Bread), 7);
    world.entity_mut(person).insert(PorterCartState::default());
    world
        .run_system_once(crate::world::village::sync_porter_cart_state)
        .unwrap();
    assert!(world.get::<PorterCartState>(person).is_none());
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Bread),
        3
    );
    release_crew(&mut world, ship, &port);
    wait_until(&mut world, |world| world.get::<ShipCrew>(person).is_none());
    assert!(world.get::<ShipCrew>(person).is_none());
    assert!(world.get::<AboardShip>(person).is_none());
    assert!(
        world
            .get::<PlayerPosition>(person)
            .unwrap()
            .0
            .distance(port.geometry.shore)
            < 0.05
    );
    assert!(world.get::<CompanyPorter>(person).is_some());
    // The same released captain keeps the provisions already bought. Those
    // meals must not make a second manually dispatched voyage unstaffable.
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert!(world.get::<ShipCrew>(person).is_some());
    wait_until(&mut world, |world| crew_ready(world, ship));
    assert!(acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert_eq!(
        world
            .query::<&CompanyAccount>()
            .single(&world)
            .unwrap()
            .cash,
        cash
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Bread),
        3
    );
}

#[test]
fn reserved_personal_or_freight_cargo_is_not_taken_aboard() {
    let (mut world, ship, person, port) = setup();
    world
        .get_mut::<GoodsInventory>(person)
        .unwrap()
        .add(Good::Wood, 1);
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert!(world.get::<ShipCrew>(person).is_none());
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Wood),
        1
    );
}

#[test]
fn cancellation_during_boarding_returns_to_connected_shore() {
    let (mut world, ship, person, port) = setup();
    acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
    wait_until(&mut world, |world| {
        world
            .get::<ShipCrew>(person)
            .is_some_and(|crew| crew.phase == CrewPhase::BoardingPier)
    });
    for _ in 0..100 {
        tick(&mut world);
    }
    assert_eq!(
        world.get::<ShipCrew>(person).unwrap().phase,
        CrewPhase::BoardingPier
    );
    release_crew(&mut world, ship, &port);
    for _ in 0..120 {
        tick(&mut world);
    }
    assert!(world.get::<ShipCrew>(person).is_none());
    assert!(
        world
            .get::<PlayerPosition>(person)
            .unwrap()
            .0
            .distance(port.geometry.shore)
            < 0.05
    );
}

#[test]
fn ready_meals_do_not_override_an_existing_service_owner_or_admit_raw_food() {
    use crate::world::village::moot_services::{
        MootQueueClock, MootQueueTicket, MootServiceKind, enqueue_moot_service,
    };
    let (mut world, ship, person, port) = setup();
    world
        .get_mut::<GoodsInventory>(person)
        .unwrap()
        .add(Good::Bread, 2);
    let hall = world
        .query_filtered::<Entity, With<Settlement>>()
        .single(&world)
        .unwrap();
    enqueue_moot_service(
        &mut world.commands(),
        &mut MootQueueClock::default(),
        person,
        hall,
        MootServiceKind::HouseholdShopping,
    );
    world.flush();
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert!(world.get::<ShipCrew>(person).is_none());
    assert!(world.get::<MootQueueTicket>(person).is_some());
    world.entity_mut(person).remove::<MootQueueTicket>();
    world
        .get_mut::<GoodsInventory>(person)
        .unwrap()
        .add(Good::Flour, 1);
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert!(world.get::<ShipCrew>(person).is_none());
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Bread),
        2
    );
}

#[test]
fn requested_layoff_finishes_the_existing_gangway_before_releasing_the_captain() {
    use crate::world::village::worker_activity::EmploymentReleaseRequested;
    let (mut world, ship, person, port) = setup();
    acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
    wait_until(&mut world, |world| crew_ready(world, ship));
    assert!(acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    let before = world.get::<PlayerPosition>(person).unwrap().0;
    world.entity_mut(person).insert(EmploymentReleaseRequested);
    assert!(!crew_ready(&world, ship));
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert_eq!(
        world.get::<ShipCrew>(person).unwrap().phase,
        CrewPhase::LeavingHull
    );
    assert_eq!(world.get::<PlayerPosition>(person).unwrap().0, before);
    wait_until(&mut world, |world| world.get::<ShipCrew>(person).is_none());
    assert!(world.get::<ShipCrew>(person).is_none());
    assert!(world.get::<CrewAssignment>(ship).is_none());
    assert!(
        world
            .get::<PlayerPosition>(person)
            .unwrap()
            .0
            .distance(port.geometry.shore)
            < 0.05
    );
    assert_eq!(
        world
            .get::<GoodsInventory>(person)
            .unwrap()
            .amount(Good::Bread),
        3
    );
    assert!(!acquire_crew(
        &mut world,
        ship,
        CompanyId(1),
        BuildingId(1),
        &port
    ));
    assert!(world.get::<ShipCrew>(person).is_none());
}

#[test]
fn crew_follows_the_authored_port_height_profile_at_normal_and_high_warp() {
    let (_, _, _, mut port) = setup();
    port.geometry.pier_end.y = 3.;
    for step in [0.05, 7., 100.] {
        let mut position = port.geometry.shore;
        let mut finished = false;
        for _ in 0..2000 {
            (position, finished) = pier_step(port.geometry, position, true, step);
            let along = (position.xz() - port.geometry.shore.xz()).dot(port.geometry.seaward());
            assert!((position.y - port.geometry.deck_height(along)).abs() < 0.0001);
            if finished {
                break;
            }
        }
        assert!(finished);
        assert_eq!(position, port.geometry.pier_end);
        finished = false;
        for _ in 0..2000 {
            (position, finished) = pier_step(port.geometry, position, false, step);
            let along = (position.xz() - port.geometry.shore.xz()).dot(port.geometry.seaward());
            assert!((position.y - port.geometry.deck_height(along)).abs() < 0.0001);
            if finished {
                break;
            }
        }
        assert!(finished);
        assert_eq!(position, port.geometry.shore);
    }
}

fn set_warp(world: &mut World, warp: f32) {
    world.query::<&mut TimeWarp>().single_mut(world).unwrap().0 = warp;
}

fn company_cash(world: &mut World) -> u64 {
    world.query::<&CompanyAccount>().single(world).unwrap().cash
}

fn hall_entity(world: &mut World) -> Entity {
    world
        .query_filtered::<Entity, With<Settlement>>()
        .single(world)
        .unwrap()
}

/// Enter the real warehouse doorway before asking it to hand over its porter.
/// Only the initial fixture pose is authored; entry, exit and the counter trip
/// all use the production door runner, retained planner and person mover.
fn enter_warehouse(world: &mut World, person: Entity) -> Vec3 {
    use crate::world::village::{WorkplaceDoorTransit, WorkplaceInterior, begin_workplace_entry};
    use shared::building::{BuildingPosition, PlacedBuilding};
    use shared::spatial::{ObstacleEntry, SpatialObstacleGrid};
    let kind = SettlementBuildingKind::StorageHall;
    let building = Vec3::new(10.0, 0.0, -12.0);
    let door = kind.entrance_position(building, 0.0);
    let inside = kind.interior_door_position(building, 0.0);
    let definition = kind.placement_definition();
    let warehouse = world
        .query::<(Entity, &BuildingId)>()
        .iter(world)
        .find(|(_, id)| **id == BuildingId(1))
        .unwrap()
        .0;
    // Keep the planner's authored-building snapshot and the mover's live
    // collision grid consistent, as the ordinary building spawn does. A
    // grid-only wall is invisible to route search and rejected only during
    // final certification, which does not represent a real warehouse.
    world.entity_mut(warehouse).insert((
        PlacedBuilding {
            building_type: kind.art(),
            rotation: 0.0,
        },
        BuildingPosition(building),
        PlayerPosition(building),
        PlayerRotation(0.0),
    ));
    world.init_resource::<SpatialObstacleGrid>();
    world
        .resource_mut::<SpatialObstacleGrid>()
        .insert(ObstacleEntry {
            center: definition.world_footprint_center(building, 0.0),
            half_extents: definition.footprint * 0.5
                + Vec2::splat(shared::physics::CHARACTER_NAV_RADIUS),
            rotation: 0.0,
            obstacle_type: kind.art() as u32,
        });
    world
        .entity_mut(person)
        .insert((PlayerPosition(door), RegionCoord::from_world_pos(door)));
    begin_workplace_entry(&mut world.commands(), person, building, door, inside);
    world.flush();
    wait_until(world, |world| {
        world.get::<WorkplaceDoorTransit>(person).is_none()
    });
    assert!(world.get::<WorkplaceInterior>(person).is_some());
    assert_eq!(
        *world.get::<CharacterActivity>(person).unwrap(),
        CharacterActivity::Indoors
    );
    assert!(
        world
            .get::<PlayerPosition>(person)
            .unwrap()
            .0
            .distance(door)
            > 0.5
    );
    door
}

#[test]
fn captain_physically_exits_an_occupied_warehouse_before_the_counter_trip_at_both_warps() {
    use crate::world::village::{WorkplaceDoorDirection, WorkplaceDoorTransit, WorkplaceInterior};
    for warp in [1.0, 25.0] {
        let (mut world, ship, person, port) = setup();
        set_warp(&mut world, warp);
        let door = enter_warehouse(&mut world, person);
        let inside = world.get::<PlayerPosition>(person).unwrap().0;
        let cash = company_cash(&mut world);
        acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
        assert!(world.get::<ShipCrew>(person).is_some());
        assert_eq!(world.get::<PlayerPosition>(person).unwrap().0, inside);
        assert_eq!(
            world.get::<WorkplaceDoorTransit>(person).unwrap().direction,
            WorkplaceDoorDirection::Leaving
        );
        assert!(world.get::<MoveTarget>(person).is_none());
        assert_eq!(
            *world.get::<CharacterMotion>(person).unwrap(),
            CharacterMotion::STATIONARY
        );
        let counter = world
            .get::<ShipCrew>(person)
            .unwrap()
            .diagnostic_provision_counter(&world)
            .unwrap();
        let mut exited = false;
        let mut saw_land_walking = false;
        let mut saw_purchase = false;
        for _ in 0..7_200 {
            let before = world.get::<PlayerPosition>(person).unwrap().0;
            let inside_before = world.get::<WorkplaceInterior>(person).is_some();
            let food_before = world.get::<GoodsInventory>(person).unwrap().edible_amount();
            tick(&mut world);
            let position = world.get::<PlayerPosition>(person).unwrap().0;
            if world.get::<WorkplaceInterior>(person).is_some() {
                assert_eq!(company_cash(&mut world), cash);
                assert_eq!(
                    world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                    0
                );
                if let Some(target) = world.get::<MoveTarget>(person) {
                    assert!(
                        target.0.distance(counter) > 5.0,
                        "counter cannot replace the doorway goal"
                    );
                }
            } else if !exited {
                assert!(inside_before);
                // The crossing must reach the exterior side of the authored
                // entrance before ordinary navigation can start the next leg.
                assert!(before.z < door.z);
                assert!(
                    !world
                        .resource::<shared::spatial::SpatialObstacleGrid>()
                        .point_blocked(before.xz())
                );
                exited = true;
            }
            if exited
                && world.get::<ShipCrew>(person).is_some_and(|crew| {
                    matches!(
                        crew.phase,
                        CrewPhase::CollectingProvisions | CrewPhase::Approaching
                    )
                })
            {
                assert_eq!(
                    *world.get::<CharacterActivity>(person).unwrap(),
                    CharacterActivity::Idle
                );
                saw_land_walking |= position.distance_squared(before) > 0.00001
                    && world.get::<CharacterMotion>(person).unwrap().is_moving();
            }
            let food = world.get::<GoodsInventory>(person).unwrap().edible_amount();
            if food > food_before {
                assert!(exited && at_counter(before, counter));
                saw_purchase = true;
            }
            if crew_ready(&world, ship) {
                break;
            }
        }
        assert!(
            exited && saw_land_walking && saw_purchase && crew_ready(&world, ship),
            "warp={warp}, exited={exited}, walking={saw_land_walking}, purchase={saw_purchase}, \
             ready={}, crew={:?}, position={:?}, target={:?}, motion={:?}, \
             failed={}, pending={}, route={:?}, interior={:?}, door={:?}",
            crew_ready(&world, ship),
            world.get::<ShipCrew>(person),
            world.get::<PlayerPosition>(person),
            world.get::<MoveTarget>(person),
            world.get::<CharacterMotion>(person),
            world.get::<NavigationRouteFailed>(person).is_some(),
            world.get::<NavigationRoutePending>(person).is_some(),
            world.get::<TravelRoute>(person),
            world.get::<WorkplaceInterior>(person),
            world.get::<WorkplaceDoorTransit>(person),
        );
        assert_eq!(
            world.get::<GoodsInventory>(person).unwrap().edible_amount(),
            3
        );
        assert_eq!(
            company_cash(&mut world) + world.resource::<BusinessEventQueue>().pending_sale_gross(),
            cash
        );
    }
}

#[test]
fn captain_waiting_at_an_unaffordable_counter_has_no_stale_walking_motion_at_both_warps() {
    for warp in [1.0, 25.0] {
        let (mut world, ship, person, port) = setup();
        set_warp(&mut world, warp);
        world
            .query::<&mut CompanyAccount>()
            .single_mut(&mut world)
            .unwrap()
            .cash = 0;
        // Idle porters also use Indoors when no retained workplace is present.
        world.entity_mut(person).insert(CharacterActivity::Indoors);
        acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
        assert_eq!(
            *world.get::<CharacterActivity>(person).unwrap(),
            CharacterActivity::Idle
        );
        let counter = world
            .get::<ShipCrew>(person)
            .unwrap()
            .diagnostic_provision_counter(&world)
            .unwrap();
        wait_until(&mut world, |world| {
            at_counter(world.get::<PlayerPosition>(person).unwrap().0, counter)
        });
        tick(&mut world);
        let waiting = world.get::<PlayerPosition>(person).unwrap().0;
        for _ in 0..120 {
            assert!(!acquire_crew(
                &mut world,
                ship,
                CompanyId(1),
                BuildingId(1),
                &port
            ));
            tick(&mut world);
            assert_eq!(
                world.get::<ShipCrew>(person).unwrap().phase,
                CrewPhase::CollectingProvisions
            );
            assert_eq!(world.get::<PlayerPosition>(person).unwrap().0, waiting);
            assert_eq!(
                *world.get::<CharacterActivity>(person).unwrap(),
                CharacterActivity::Idle
            );
            assert_eq!(
                *world.get::<CharacterMotion>(person).unwrap(),
                CharacterMotion::STATIONARY
            );
            assert!(world.get::<MoveTarget>(person).is_none());
            assert_eq!(
                world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                0
            );
        }
        assert_eq!(company_cash(&mut world), 0);
        assert_eq!(
            world.resource::<BusinessEventQueue>().pending_sale_gross(),
            0
        );
    }
}

#[test]
fn cancelling_captain_assignment_preserves_an_in_progress_workplace_exit_at_both_warps() {
    use crate::world::village::{WorkplaceDoorTransit, WorkplaceInterior};
    for warp in [1.0, 25.0] {
        let (mut world, ship, person, port) = setup();
        set_warp(&mut world, warp);
        let door = enter_warehouse(&mut world, person);
        acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
        wait_until(&mut world, |world| {
            world.get::<MoveTarget>(person).is_some()
        });
        let position = world.get::<PlayerPosition>(person).unwrap().0;
        release_crew(&mut world, ship, &port);
        assert_eq!(world.get::<PlayerPosition>(person).unwrap().0, position);
        assert!(world.get::<ShipCrew>(person).is_none());
        assert!(world.get::<WorkplaceInterior>(person).is_some());
        assert!(
            world
                .get::<WorkplaceDoorTransit>(person)
                .unwrap()
                .destination_after_exit
                .is_none()
        );
        wait_until(&mut world, |world| {
            world.get::<WorkplaceDoorTransit>(person).is_none()
        });
        assert!(world.get::<WorkplaceInterior>(person).is_none());
        assert!(world.get::<MoveTarget>(person).is_none());
        assert!(world.get::<PlayerPosition>(person).unwrap().0.z < door.z);
        assert_eq!(
            *world.get::<CharacterMotion>(person).unwrap(),
            CharacterMotion::STATIONARY
        );
        assert_eq!(company_cash(&mut world), 1_000);
        assert_eq!(
            world.get::<GoodsInventory>(person).unwrap().edible_amount(),
            0
        );
    }
}

#[test]
fn provisions_are_collected_at_the_real_counter_before_boarding_and_after_resupply_at_both_warps() {
    for warp in [1.0, 25.0] {
        let (mut world, ship, person, port) = setup();
        set_warp(&mut world, warp);
        let hall = hall_entity(&mut world);
        assert!(!acquire_crew(
            &mut world,
            ship,
            CompanyId(1),
            BuildingId(1),
            &port
        ));
        let pickup = world.get::<ShipCrew>(person).unwrap().provisions.unwrap();
        let counter = provision_counter(&world, pickup).unwrap();
        assert!(
            world
                .get::<PlayerPosition>(person)
                .unwrap()
                .0
                .distance(counter)
                > 10.0
        );
        // Even a direct caller cannot spend or take stock remotely.
        provision(
            &mut world,
            person,
            CompanyId(1),
            BuildingId(1),
            SettlementId(1),
            pickup,
        );
        assert_eq!(company_cash(&mut world), 1_000);
        assert_eq!(
            world
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Bread),
            10
        );
        assert_eq!(
            world.get::<GoodsInventory>(person).unwrap().edible_amount(),
            0
        );

        for visit in 0..2 {
            let mut pickups = 0;
            let mut saw_land_trip = false;
            let mut saw_disembark = false;
            for _ in 0..7_200 {
                let before_position = world.get::<PlayerPosition>(person).unwrap().0;
                let before_food = world.get::<GoodsInventory>(person).unwrap().edible_amount();
                let phase = world.get::<ShipCrew>(person).unwrap().phase;
                saw_land_trip |= phase == CrewPhase::CollectingProvisions
                    && before_position.xz().distance(port.geometry.shore.xz()) > 2.0;
                saw_disembark |= matches!(
                    phase,
                    CrewPhase::ResupplyLeavingHull | CrewPhase::ResupplyLeavingPier
                );
                tick(&mut world);
                let food = world.get::<GoodsInventory>(person).unwrap().edible_amount();
                if food > before_food {
                    pickups += 1;
                    assert_eq!(phase, CrewPhase::CollectingProvisions);
                    assert!(
                        at_counter(before_position, counter),
                        "{warp}× food changed away from the actual pickup"
                    );
                    assert!(world.get::<AboardShip>(person).is_none());
                }
                assert_eq!(
                    world
                        .get::<GoodsInventory>(hall)
                        .unwrap()
                        .amount(Good::Bread)
                        + food
                        + visit * 3,
                    10
                );
                assert_eq!(
                    company_cash(&mut world)
                        + world.resource::<BusinessEventQueue>().pending_sale_gross(),
                    1_000
                );
                if crew_ready(&world, ship) {
                    break;
                }
            }
            assert!(
                crew_ready(&world, ship),
                "{warp}× visit {visit} did not finish the real market and boarding trip"
            );
            assert_eq!(pickups, 1);
            assert!(saw_land_trip);
            assert_eq!(
                world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                3
            );
            if visit == 0 {
                // Consume the already collected meals while waiting at berth.
                assert_eq!(
                    world
                        .get_mut::<GoodsInventory>(person)
                        .unwrap()
                        .remove(Good::Bread, 3),
                    3
                );
                let before = world.get::<PlayerPosition>(person).unwrap().0;
                let cash = company_cash(&mut world);
                assert!(!acquire_crew(
                    &mut world,
                    ship,
                    CompanyId(1),
                    BuildingId(1),
                    &port
                ));
                assert_eq!(world.get::<PlayerPosition>(person).unwrap().0, before);
                assert_eq!(company_cash(&mut world), cash);
                assert_eq!(
                    world
                        .get::<GoodsInventory>(hall)
                        .unwrap()
                        .amount(Good::Bread),
                    7
                );
                assert_eq!(
                    world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                    0
                );
            } else {
                assert!(
                    saw_disembark,
                    "resupply must walk off the hull and pier first"
                );
            }
        }
        assert_eq!(
            world
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Bread),
            4
        );
    }
}

#[test]
fn cancelling_a_provision_trip_preserves_body_money_and_collected_food_at_both_warps() {
    for warp in [1.0, 25.0] {
        for after_purchase in [false, true] {
            let (mut world, ship, person, port) = setup();
            set_warp(&mut world, warp);
            let hall = hall_entity(&mut world);
            acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
            if after_purchase {
                wait_until(&mut world, |world| {
                    world.get::<GoodsInventory>(person).unwrap().edible_amount() == 3
                });
                assert_eq!(
                    world.get::<ShipCrew>(person).unwrap().phase,
                    CrewPhase::Approaching
                );
            } else {
                for _ in 0..10 {
                    tick(&mut world);
                }
                assert_eq!(
                    world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                    0
                );
            }
            let before = world.get::<PlayerPosition>(person).unwrap().0;
            let cash = company_cash(&mut world);
            let food = world.get::<GoodsInventory>(person).unwrap().edible_amount();
            release_crew(&mut world, ship, &port);
            assert!(world.get::<ShipCrew>(person).is_none());
            assert!(world.get::<CrewAssignment>(ship).is_none());
            assert!(world.get::<MoveTarget>(person).is_none());
            tick(&mut world);
            assert_eq!(world.get::<PlayerPosition>(person).unwrap().0, before);
            assert_eq!(company_cash(&mut world), cash);
            assert_eq!(
                world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                food
            );
            assert_eq!(
                world
                    .get::<GoodsInventory>(hall)
                    .unwrap()
                    .amount(Good::Bread)
                    + food,
                10
            );
            assert_eq!(
                cash + world.resource::<BusinessEventQueue>().pending_sale_gross(),
                1_000
            );
        }
    }
}

#[test]
fn partial_provisions_do_not_repeat_the_counter_trip_during_one_berth_visit_at_both_warps() {
    for warp in [1.0, 25.0] {
        for (stock, cash, expected_food) in [(2, 1_000, 2), (10, 20, 1)] {
            let (mut world, ship, person, port) = setup();
            set_warp(&mut world, warp);
            let hall = hall_entity(&mut world);
            let mut initial_stock = GoodsInventory::new(100);
            initial_stock.add(Good::Bread, stock);
            let mut initial_market = MootMarket::founding();
            initial_market.consign(
                MarketSeller::Treasury(SettlementId(1)),
                Good::Bread,
                stock,
                20,
            );
            world
                .entity_mut(hall)
                .insert((initial_stock, initial_market));
            world
                .query::<&mut CompanyAccount>()
                .single_mut(&mut world)
                .unwrap()
                .cash = cash;
            acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port);
            wait_until(&mut world, |world| crew_ready(world, ship));
            assert_eq!(
                world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                expected_food
            );
            assert!(
                world
                    .get::<ShipCrew>(person)
                    .unwrap()
                    .provisioned_this_berth
            );
            let cash_after = company_cash(&mut world);
            for _ in 0..120 {
                assert!(
                    acquire_crew(&mut world, ship, CompanyId(1), BuildingId(1), &port),
                    "{warp}× partial provisions must permit departure"
                );
                tick(&mut world);
                assert_eq!(
                    world.get::<ShipCrew>(person).unwrap().phase,
                    CrewPhase::Aboard
                );
                assert_eq!(
                    world.get::<PlayerPosition>(person).unwrap().0,
                    helm(&world, ship).unwrap(),
                    "an aboard captain stays at the authoritative helm"
                );
                assert_eq!(company_cash(&mut world), cash_after);
                assert_eq!(
                    world
                        .get::<GoodsInventory>(hall)
                        .unwrap()
                        .amount(Good::Bread)
                        + expected_food,
                    stock
                );
            }

            // This unit fixture changes the hull's authoritative location to
            // exercise visit identity; all provision trips still use the real
            // person mover. It does not claim to test water-route execution.
            world.get_mut::<PlayerPosition>(ship).unwrap().0 += Vec3::X * 4.0;
            tick(&mut world);
            assert!(
                !world
                    .get::<ShipCrew>(person)
                    .unwrap()
                    .provisioned_this_berth
            );
            world.get_mut::<PlayerPosition>(ship).unwrap().0 = port.geometry.berth;
            tick(&mut world);
            assert!(!acquire_crew(
                &mut world,
                ship,
                CompanyId(1),
                BuildingId(1),
                &port
            ));
            assert_eq!(
                world.get::<ShipCrew>(person).unwrap().phase,
                CrewPhase::ResupplyLeavingHull
            );
            let mut saw_counter = false;
            for _ in 0..7_200 {
                saw_counter |=
                    world.get::<ShipCrew>(person).unwrap().phase == CrewPhase::CollectingProvisions;
                tick(&mut world);
                if crew_ready(&world, ship) {
                    break;
                }
            }
            assert!(saw_counter);
            assert!(crew_ready(&world, ship));
            assert_eq!(
                world.get::<GoodsInventory>(person).unwrap().edible_amount(),
                expected_food
            );
            assert_eq!(company_cash(&mut world), cash_after);
            assert!(acquire_crew(
                &mut world,
                ship,
                CompanyId(1),
                BuildingId(1),
                &port
            ));
            assert_eq!(
                company_cash(&mut world)
                    + world.resource::<BusinessEventQueue>().pending_sale_gross(),
                cash
            );
        }
    }
}
