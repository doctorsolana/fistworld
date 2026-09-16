use bevy::prelude::*;
use shared::components::*;
use shared::economy::{
    CarriedLoad, Good, GoodsInventory, HouseholdEconomy, MarketSeller, MootMarket, Wallet,
};

use super::project::Phase;
use super::*;
use crate::player::hero::MoveTarget;
use crate::world::village::{BusinessEventQueue, VillagerIntent};

const HOUSE: BuildingId = BuildingId(10);
const OWNER: PersonId = PersonId(1);

struct Fixture {
    world: World,
    clock: Entity,
    hall: Entity,
    home: Entity,
    owner: Entity,
    worker: Entity,
    household: Entity,
}

fn fixture() -> Fixture {
    let mut world = World::new();
    let clock = world.spawn(WorldTime::new_default()).id();
    let hall = world
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Test Village".into(),
                tier: SettlementTier::Village,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            MootMarket::founding(),
            GoodsInventory::new(1000),
        ))
        .id();
    let mut pantry = GoodsInventory::new(80);
    pantry.add(Good::Bread, 7);
    let home = world
        .spawn((
            HOUSE,
            BuildingOf(SettlementId(1)),
            OwnedBy(OWNER),
            HouseAppearance::default(),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Test Village".into(),
                owner: Some("Owner".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(30.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            Household {
                resident_ids: vec![OWNER],
                residents: vec!["Owner".into()],
            },
            OccupiedByHousehold(HouseholdId(5)),
            pantry,
        ))
        .id();
    let mut hearth = crate::world::village::HearthState::default();
    hearth.credit = 5;
    world.entity_mut(home).insert(hearth);
    let household = world
        .spawn((HouseholdId(5), HouseholdEconomy::default()))
        .id();
    let owner = world
        .spawn((
            OWNER,
            CharacterKind::Hero,
            Health::default(),
            Wallet::new(1000),
            PlayerPosition(Vec3::new(30.0, 0.0, 8.0)),
            ResidentOf(SettlementId(1)),
        ))
        .id();
    let worker = world
        .spawn((
            PersonId(2),
            CharacterKind::Villager,
            Health::default(),
            Wallet::new(0),
            PlayerPosition(Vec3::new(15.0, 0.0, 0.0)),
            PlayerRotation(0.0),
            ResidentOf(SettlementId(1)),
            VillagerIntent::Resident { settlement: hall },
            GoodsInventory::new(24),
            CarriedLoad::default(),
        ))
        .id();
    Fixture {
        world,
        clock,
        hall,
        home,
        owner,
        worker,
        household,
    }
}

impl Fixture {
    fn wood(&mut self, units: u32, price: u64) {
        self.world
            .get_mut::<GoodsInventory>(self.hall)
            .unwrap()
            .add(Good::Wood, units);
        self.world
            .get_mut::<MootMarket>(self.hall)
            .unwrap()
            .consign(
                MarketSeller::Treasury(SettlementId(1)),
                Good::Wood,
                units,
                price,
            );
    }
    fn tick(&mut self, seconds: f32) {
        let mut clock = self.world.get_mut::<WorldTime>(self.clock).unwrap();
        clock.seconds_in_cycle += seconds;
        drop(clock);
        run_house_upgrade_projects(&mut self.world);
    }
    fn travel_to_goal(&mut self) {
        let goal = self
            .world
            .get::<MoveTarget>(self.worker)
            .expect("builder travel goal")
            .0;
        self.world.get_mut::<PlayerPosition>(self.worker).unwrap().0 = goal;
        self.tick(1.0);
    }
    fn supply(&mut self) {
        self.wood(8, 20);
        request_upgrade(&mut self.world, HOUSE, OWNER).unwrap();
        self.tick(0.0);
        self.travel_to_goal();
        assert_eq!(self.project().cargo.amount(Good::Wood), 6);
        self.travel_to_goal();
        assert_eq!(self.project().delivered, 6);
        assert_eq!(self.project().phase, Phase::ToMarket);
        self.travel_to_goal();
        assert_eq!(self.project().cargo.amount(Good::Wood), 2);
        self.travel_to_goal();
        assert_eq!(self.project().delivered, 8);
        assert_eq!(self.project().phase, Phase::Working);
    }
    fn project(&self) -> &super::project::UpgradeProject {
        &self.world.resource::<HouseUpgradeProjects>().entries[&HOUSE]
    }
    fn total_cash(&mut self) -> u64 {
        crate::world::village_lab::total_money(&mut self.world)
    }
    fn total_wood(&mut self) -> u32 {
        let cargo = self
            .world
            .get_resource::<HouseUpgradeProjects>()
            .map_or(0, |book| book.transit_wood());
        cargo
            + self
                .world
                .query::<&GoodsInventory>()
                .iter(&self.world)
                .map(|inv| inv.amount(Good::Wood))
                .sum::<u32>()
    }
}

#[test]
fn request_is_paid_atomic_and_preserves_the_existing_dwelling() {
    let mut f = fixture();
    assert!(request_upgrade(&mut f.world, HOUSE, PersonId(99)).is_err());
    f.world.get_mut::<Settlement>(f.hall).unwrap().tier = SettlementTier::Hamlet;
    assert!(request_upgrade(&mut f.world, HOUSE, OWNER).is_err());
    f.world.get_mut::<Settlement>(f.hall).unwrap().tier = SettlementTier::Village;
    f.world.get_mut::<Wallet>(f.owner).unwrap().debit(501);
    assert!(request_upgrade(&mut f.world, HOUSE, OWNER).is_err());
    assert_eq!(f.world.get::<Wallet>(f.owner).unwrap().balance(), 499);
    f.world.get_mut::<Wallet>(f.owner).unwrap().credit(501);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    assert!(request_upgrade(&mut f.world, HOUSE, OWNER).is_err());
    assert_eq!(f.world.get::<Wallet>(f.owner).unwrap().balance(), 500);
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(
        f.world.get::<HouseAppearance>(f.home).unwrap().level,
        HouseLevel::Ground
    );
    assert!(
        f.world
            .get::<crate::world::village::UnderConstruction>(f.home)
            .is_none()
    );
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.home)
            .unwrap()
            .amount(Good::Bread),
        7
    );
    assert_eq!(
        f.world
            .get::<HouseholdEconomy>(f.household)
            .unwrap()
            .pennies,
        0
    );
}

#[test]
fn paid_wood_requires_real_arrivals_and_labor_before_capacity_changes() {
    let mut f = fixture();
    f.wood(8, 20);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.tick(0.0);
    assert_eq!(f.project().worker, Some(f.worker));
    f.tick(60.0);
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.hall)
            .unwrap()
            .amount(Good::Wood),
        8
    );
    assert_eq!(f.project().work_done, 0.0);
    f.travel_to_goal();
    assert_eq!(f.project().cargo.amount(Good::Wood), 6);
    assert_eq!(
        f.world
            .resource::<HouseUpgradeProjects>()
            .carried_load_for_worker(f.worker)
            .unwrap()
            .good,
        Some(Good::Wood)
    );
    assert_eq!(
        f.world
            .resource::<BusinessEventQueue>()
            .pending_sale_gross(),
        120
    );
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 8);
    f.tick(30.0);
    assert_eq!(f.project().work_done, 0.0);
    f.travel_to_goal();
    assert_eq!(f.project().delivered, 6);
    assert_eq!(f.project().work_done, 0.0);
    f.travel_to_goal();
    assert_eq!(f.project().cargo.amount(Good::Wood), 2);
    f.travel_to_goal();
    assert_eq!(f.project().delivered, 8);
    f.tick(30.0);
    assert_eq!(f.world.get::<Wallet>(f.worker).unwrap().balance(), 50);
    assert_eq!(
        f.world.get::<HouseAppearance>(f.home).unwrap().level,
        HouseLevel::Ground
    );
    f.tick(30.0);
    assert!(
        !f.world
            .resource::<HouseUpgradeProjects>()
            .contains_house(HOUSE)
    );
    assert_eq!(
        f.world.get::<HouseAppearance>(f.home).unwrap().level,
        HouseLevel::UpperStorey
    );
    assert_eq!(f.world.get::<BuildingId>(f.home), Some(&HOUSE));
    assert_eq!(
        f.world.get::<OccupiedByHousehold>(f.home).unwrap().0,
        HouseholdId(5)
    );
    assert_eq!(f.world.get::<OwnedBy>(f.home).unwrap().0, OWNER);
    assert_eq!(
        f.world.get::<Household>(f.home).unwrap().resident_ids,
        vec![OWNER]
    );
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.home)
            .unwrap()
            .amount(Good::Bread),
        7
    );
    assert_eq!(f.world.get::<Wallet>(f.owner).unwrap().balance(), 740);
    assert_eq!(f.world.get::<Wallet>(f.worker).unwrap().balance(), 100);
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 0);
    assert_eq!(
        f.world
            .get::<crate::world::village::HearthState>(f.home)
            .unwrap()
            .credit,
        5
    );
    assert!(
        f.world
            .get::<HouseUpgradeBuilderRoutine>(f.worker)
            .is_none()
    );
}

#[test]
fn empty_market_and_busy_workers_cannot_create_free_progress() {
    let mut f = fixture();
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.tick(120.0);
    assert!(f.project().worker.is_none());
    assert_eq!(f.project().work_done, 0.0);
    assert_eq!(f.project().escrow, 500);
    f.wood(8, 20);
    f.world.entity_mut(f.worker).remove::<VillagerIntent>();
    f.tick(20.0);
    assert!(f.project().worker.is_none());
    assert!(f.world.get::<HouseUpgradeBuilderRoutine>(f.owner).is_none());
    assert_eq!(f.total_cash(), 1000);
}

fn reserve_builder_meal(f: &mut Fixture) {
    use crate::world::village::moot_services::{MootQueueClock, MootServiceKind, reserve_meal};
    use bevy::ecs::system::RunSystemOnce;
    let worker = f.worker;
    let hall = f.hall;
    f.world
        .run_system_once(move |mut commands: Commands| {
            reserve_meal(
                &mut commands,
                &mut MootQueueClock::default(),
                worker,
                hall,
                MootServiceKind::PersonalMeal,
                Good::Bread,
                0,
            );
        })
        .unwrap();
    assert!(
        f.world
            .get::<crate::world::village::MootMealRoutine>(worker)
            .is_some()
    );
}

#[test]
fn personal_meal_owns_the_builder_trip_and_work_resumes_only_at_the_site() {
    use crate::world::village::{MootMealRoutine, MootQueueTicket};
    use crate::world::village_roads::NavigationRouteFailed;

    for phase in [Phase::ToMarket, Phase::ToSite, Phase::Working] {
        let mut f = fixture();
        if phase == Phase::Working {
            f.supply();
            f.tick(12.0);
        } else {
            f.wood(8, 20);
            request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
            f.tick(0.0);
            if phase == Phase::ToSite {
                f.travel_to_goal();
            }
        }
        assert_eq!(f.project().phase, phase);
        let project_goal = if phase == Phase::ToMarket {
            f.project().market_stand
        } else {
            f.project().stand
        };
        let state = |f: &Fixture| {
            let project = f.project();
            (
                project.phase,
                project.work_done,
                project.paid_labor,
                project.escrow,
                project.delivered,
                project.cargo.amount(Good::Wood),
                project.worker,
                f.world.get::<Wallet>(f.worker).unwrap().balance(),
            )
        };
        let before = state(&f);
        reserve_builder_meal(&mut f);
        let meal_goal = project_goal + Vec3::new(20.0, 0.0, 15.0);
        // Even at the upgrade's exact target, it must not buy, unload or work
        // while the real meal component owns the worker's next movement.
        f.world
            .entity_mut(f.worker)
            .insert((PlayerPosition(project_goal), MoveTarget(meal_goal)));
        f.tick(45.0);
        assert_eq!(state(&f), before, "paused {phase:?}");
        assert_eq!(f.world.get::<MoveTarget>(f.worker).unwrap().0, meal_goal);
        assert_eq!(
            *f.world.get::<CharacterActivity>(f.worker).unwrap(),
            CharacterActivity::Idle
        );

        f.world.entity_mut(f.worker).insert((
            PlayerPosition(meal_goal),
            NavigationRouteFailed { goal: meal_goal },
        ));
        f.tick(60.0);
        assert_eq!(
            state(&f),
            before,
            "meal route failure must not cancel {phase:?}"
        );
        assert_eq!(f.world.get::<MoveTarget>(f.worker).unwrap().0, meal_goal);
        assert_eq!(f.total_cash(), 1000);
        assert_eq!(f.total_wood(), 8);

        // Isolate the ownership boundary: simulate the food system finishing
        // its errand. This fixture tests physical arrival gating, not pathfinding
        // or the separate meal purchase/queue lifecycle.
        f.world.entity_mut(f.worker).remove::<(
            MootMealRoutine,
            MootQueueTicket,
            NavigationRouteFailed,
            MoveTarget,
        )>();
        f.tick(1.0);
        assert_eq!(state(&f), before, "cannot resume remotely after {phase:?}");
        assert_eq!(f.world.get::<MoveTarget>(f.worker).unwrap().0, project_goal);
        f.tick(45.0);
        assert_eq!(state(&f), before, "walking time is not work");
        f.travel_to_goal();
        match phase {
            Phase::ToMarket => {
                assert_eq!(f.project().cargo.amount(Good::Wood), 6);
                assert_eq!(f.project().phase, Phase::ToSite);
            }
            Phase::ToSite => {
                assert_eq!(f.project().delivered, 6);
                assert_eq!(f.project().cargo.amount(Good::Wood), 0);
            }
            Phase::Working => assert_eq!(f.project().work_done, before.1 + 1.0),
            _ => unreachable!(),
        }
        assert_eq!(f.total_cash(), 1000);
        assert_eq!(f.total_wood(), 8);
    }
}

#[test]
fn cancelling_an_upgrade_during_a_meal_preserves_the_food_movement_owner() {
    let mut f = fixture();
    f.supply();
    f.tick(12.0);
    reserve_builder_meal(&mut f);
    let meal_goal = f.project().stand + Vec3::X * 25.0;
    f.world.entity_mut(f.worker).insert(MoveTarget(meal_goal));
    cancel_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    assert!(
        f.world
            .get::<HouseUpgradeBuilderRoutine>(f.worker)
            .is_none()
    );
    assert!(
        f.world
            .get::<crate::world::village::MootMealRoutine>(f.worker)
            .is_some()
    );
    assert_eq!(f.world.get::<MoveTarget>(f.worker).unwrap().0, meal_goal);
    assert_eq!(
        *f.world.get::<CharacterActivity>(f.worker).unwrap(),
        CharacterActivity::Idle
    );
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 8);
}

#[test]
fn cancellation_returns_purchased_goods_cash_and_releases_worker_once() {
    let mut f = fixture();
    f.supply();
    f.tick(30.0);
    cancel_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    assert_eq!(f.world.get::<Wallet>(f.owner).unwrap().balance(), 790);
    assert_eq!(f.world.get::<Wallet>(f.worker).unwrap().balance(), 50);
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.home)
            .unwrap()
            .amount(Good::Wood),
        8
    );
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 8);
    assert!(
        f.world
            .get::<HouseUpgradeBuilderRoutine>(f.worker)
            .is_none()
    );
    assert!(f.world.get::<MoveTarget>(f.worker).is_none());
    assert!(cancel_upgrade(&mut f.world, HOUSE, OWNER).is_err());
    f.tick(30.0);
    assert_eq!(f.total_cash(), 1000);
}

#[test]
fn destroyed_worksite_recovers_owned_pile_without_losing_materials() {
    let mut f = fixture();
    f.supply();
    let site = f.project().worksite;
    f.world.despawn(site);
    f.tick(1.0);
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 8);
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.home)
            .unwrap()
            .amount(Good::Wood),
        8
    );
    assert!(
        f.world
            .get::<HouseUpgradeBuilderRoutine>(f.worker)
            .is_none()
    );
}

#[test]
fn dead_owner_and_destroyed_house_settle_to_real_treasury_and_hall_stock() {
    let mut f = fixture();
    f.wood(8, 20);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.tick(0.0);
    f.travel_to_goal();
    f.world.get_mut::<Health>(f.owner).unwrap().current = 0.0;
    f.world.despawn(f.home);
    f.tick(1.0);
    assert_eq!(f.world.get::<Settlement>(f.hall).unwrap().treasury, 380);
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.hall)
            .unwrap()
            .amount(Good::Wood),
        8
    );
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 8);
    assert!(
        !f.world
            .resource::<HouseUpgradeProjects>()
            .contains_house(HOUSE)
    );
}

#[test]
fn unobserved_house_builder_requires_physical_arrival_and_never_teleports_on_elapsed_time() {
    let mut f = fixture();
    f.wood(8, 20);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    let opening = f.world.get::<PlayerPosition>(f.worker).unwrap().0;
    f.tick(0.0);
    assert!(f.world.get::<MoveTarget>(f.worker).is_some());
    f.tick(60.0);
    assert_eq!(f.world.get::<PlayerPosition>(f.worker).unwrap().0, opening);
    assert_eq!(f.project().cargo.amount(Good::Wood), 0);
    assert_eq!(f.project().work_done, 0.0);
    f.travel_to_goal();
    assert_eq!(f.project().cargo.amount(Good::Wood), 6);
    let pickup = f.world.get::<PlayerPosition>(f.worker).unwrap().0;
    f.tick(60.0);
    assert_eq!(f.world.get::<PlayerPosition>(f.worker).unwrap().0, pickup);
    assert_eq!(f.project().delivered, 0);
    assert_eq!(f.project().work_done, 0.0);
    // Supply each actual arrival independently of the work timer. This tests
    // handoffs and labor, not the separate navigation solver.
    f.travel_to_goal();
    assert_eq!(f.project().delivered, 6);
    f.travel_to_goal();
    assert_eq!(f.project().cargo.amount(Good::Wood), 2);
    f.travel_to_goal();
    assert_eq!(f.project().delivered, 8);
    assert_eq!(f.project().work_done, 0.0);
    f.tick(30.0);
    assert_eq!(
        f.world.get::<HouseAppearance>(f.home).unwrap().level,
        HouseLevel::Ground
    );
    f.tick(30.0);
    assert_eq!(
        f.world.get::<HouseAppearance>(f.home).unwrap().level,
        HouseLevel::UpperStorey
    );
    assert!(f.world.get::<MoveTarget>(f.worker).is_none());
    assert_eq!(f.total_cash(), 1000);
}

#[test]
fn house_upgrade_preserves_existing_route_and_foreign_construction_commitment() {
    use crate::world::village_roads::{RouteWaypoint, TravelRoute};
    let mut f = fixture();
    f.wood(8, 20);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    let destination = Vec3::Z * 20.0;
    f.world.entity_mut(f.worker).insert((
        MoveTarget(destination),
        TravelRoute {
            goal: destination,
            waypoints: vec![RouteWaypoint {
                position: destination,
                on_road: false,
            }],
            next: 0,
            geometry_version: 0,
        },
    ));
    f.tick(0.0);
    assert!(f.project().worker.is_none());
    assert_eq!(
        f.world.get::<TravelRoute>(f.worker).unwrap().goal,
        destination
    );
    // An existing port construction contract also blocks assignment after the
    // route ends. This is one of the owners missing from the old manual list.
    f.world
        .entity_mut(f.worker)
        .remove::<(MoveTarget, TravelRoute)>();
    let port = f.world.spawn_empty().id();
    f.world
        .entity_mut(f.worker)
        .insert(crate::world::ports::PortBuilder { project: port });
    f.tick(16.0);
    assert!(f.project().worker.is_none());
    assert_eq!(
        f.world
            .get::<crate::world::ports::PortBuilder>(f.worker)
            .unwrap()
            .project,
        port
    );
    f.world
        .entity_mut(f.worker)
        .remove::<crate::world::ports::PortBuilder>();
    f.tick(16.0);
    assert_eq!(f.project().worker, Some(f.worker));
    assert!(
        f.world
            .get::<HouseUpgradeBuilderRoutine>(f.worker)
            .is_some()
    );
    assert_eq!(f.total_cash(), 1000);
}

#[test]
fn occupied_or_invalid_expansion_never_debits_owner_or_moves_bystanders() {
    let mut f = fixture();
    let original = *f.world.get::<HouseAppearance>(f.home).unwrap();
    let upgraded = HouseAppearance {
        level: HouseLevel::UpperStorey,
        ..original
    };
    let position = f.world.get::<PlayerPosition>(f.home).unwrap().0;
    let old = original.building_type().definition();
    let new = upgraded.building_type().definition();
    let annulus = (0..100).find_map(|step| {
        let x = (step as f32 / 99.0 - 0.5) * new.footprint.x;
        let p = new.world_footprint_center(position, 0.0) + Vec2::new(x, 0.0);
        let old_delta = (p - old.world_footprint_center(position, 0.0)).abs();
        old_delta
            .cmpgt(old.footprint * 0.5 + Vec2::splat(0.01))
            .any()
            .then_some(p)
    });
    // Some authored variants add height without widening. Only test the extra
    // collision space if this family actually has one.
    if let Some(annulus) = annulus {
        let body = f
            .world
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(annulus.x, 0.0, annulus.y)),
            ))
            .id();
        f.supply();
        f.tick(60.0);
        assert_eq!(
            f.world.get::<HouseAppearance>(f.home).unwrap().level,
            HouseLevel::Ground
        );
        assert_eq!(f.world.get::<PlayerPosition>(body).unwrap().0.xz(), annulus);
        f.world.despawn(body);
        f.tick(1.0);
        assert_eq!(
            f.world.get::<HouseAppearance>(f.home).unwrap().level,
            HouseLevel::UpperStorey
        );
    }
    let mut f = fixture();
    let blocker = f
        .world
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Test Village".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(30.0, 0.0, 0.0)),
        ))
        .id();
    assert!(request_upgrade(&mut f.world, HOUSE, OWNER).is_err());
    assert_eq!(f.world.get::<Wallet>(f.owner).unwrap().balance(), 1000);
    assert!(f.world.get_entity(blocker).is_ok());
}

#[test]
fn blocked_refund_keeps_cargo_until_real_storage_is_available() {
    let mut f = fixture();
    f.supply();
    let home_free = f
        .world
        .get::<GoodsInventory>(f.home)
        .unwrap()
        .free_units(Good::Bread);
    f.world
        .get_mut::<GoodsInventory>(f.home)
        .unwrap()
        .add(Good::Bread, home_free);
    let hall_free = f
        .world
        .get::<GoodsInventory>(f.hall)
        .unwrap()
        .free_units(Good::Bread);
    f.world
        .get_mut::<GoodsInventory>(f.hall)
        .unwrap()
        .add(Good::Bread, hall_free);
    cancel_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    assert_eq!(f.project().phase, Phase::Refunding);
    assert_eq!(f.project().cargo.amount(Good::Wood), 8);
    assert_eq!(f.project().escrow, 0);
    assert_eq!(f.total_cash(), 1000);
    assert!(
        f.world
            .get::<HouseUpgradeBuilderRoutine>(f.worker)
            .is_none()
    );
    f.tick(1.0);
    assert_eq!(f.project().cargo.amount(Good::Wood), 8);
    f.world.get_mut::<GoodsInventory>(f.home).unwrap().remove(
        Good::Bread,
        HOUSE_UPGRADE_WOOD_REQUIRED * Good::Wood.bulk_per_unit(),
    );
    f.tick(15.0);
    assert!(
        !f.world
            .resource::<HouseUpgradeProjects>()
            .contains_house(HOUSE)
    );
    assert_eq!(f.total_wood(), 8);
    assert_eq!(f.total_cash(), 1000);
}

#[test]
fn a_worker_with_private_cargo_or_military_orders_is_not_commandeered() {
    let mut f = fixture();
    f.wood(8, 20);
    f.world
        .get_mut::<GoodsInventory>(f.worker)
        .unwrap()
        .add(Good::Bread, 1);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.tick(0.0);
    assert!(f.project().worker.is_none());
    f.world
        .get_mut::<GoodsInventory>(f.worker)
        .unwrap()
        .remove(Good::Bread, 1);
    f.world
        .entity_mut(f.worker)
        .insert(CommandedBy("Captain".into()));
    f.tick(20.0);
    assert!(f.project().worker.is_none());
    assert!(f.world.get::<CommandedBy>(f.worker).is_some());
    f.world.entity_mut(f.worker).remove::<CommandedBy>();
    f.tick(20.0);
    assert_eq!(f.project().worker, Some(f.worker));
}

#[test]
#[ignore = "Manual focused scale probe: 50 active projects, 5,000 employed residents"]
fn waiting_project_search_budget_is_fair_at_five_thousand_people() {
    let mut f = fixture();
    f.world
        .entity_mut(f.worker)
        .insert(EmployedAt(BuildingId(9999)));
    for person in 3..5002 {
        f.world.spawn((
            PersonId(person),
            ResidentOf(SettlementId(1)),
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(3000.0, 0.0, 2000.0)),
            Health::default(),
            Wallet::new(1),
            GoodsInventory::new(24),
            VillagerIntent::Resident { settlement: f.hall },
            EmployedAt(BuildingId(9999)),
        ));
    }
    f.world.get_mut::<Wallet>(f.owner).unwrap().credit(50_000);
    f.wood(8, 20);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    let mut ids = vec![HOUSE];
    for index in 1..50 {
        let id = BuildingId(100 + index);
        f.world.spawn((
            id,
            BuildingOf(SettlementId(1)),
            OwnedBy(OWNER),
            HouseAppearance::default(),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Test Village".into(),
                owner: Some("Owner".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(100.0 + index as f32 * 25.0, 0.0, 100.0)),
            PlayerRotation(0.0),
        ));
        request_upgrade(&mut f.world, id, OWNER).unwrap();
        ids.push(id);
    }
    let mut samples = Vec::new();
    for iteration in 0..500 {
        // Keep observations in daylight and advance a day at a time so every
        // project's retry is due, even under extreme simulation acceleration.
        let mut clock = f.world.get_mut::<WorldTime>(f.clock).unwrap();
        clock.day += 1;
        drop(clock);
        let start = std::time::Instant::now();
        f.tick(0.0);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(
            f.world
                .resource::<HouseUpgradeProjects>()
                .last_worker_reviewed,
            Some(ids[iteration % ids.len()])
        );
        assert!(
            f.world
                .resource::<HouseUpgradeProjects>()
                .entries
                .values()
                .all(|project| project.worker.is_none())
        );
    }
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "HouseUpgradeWaitingBench people=5000 projects=50 samples=500 p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
        samples[250], samples[475], samples[499]
    );
    assert_eq!(
        f.world
            .resource::<HouseUpgradeProjects>()
            .pending_in_settlement(SettlementId(1)),
        50
    );
}

#[test]
fn owner_death_returns_unused_capital_and_materials_to_surviving_household() {
    let mut f = fixture();
    f.world
        .entity_mut(f.owner)
        .insert(HouseholdMember(HouseholdId(5)));
    f.world.entity_mut(f.household).insert(HouseholdMembers {
        settlement: SettlementId(1),
        dwelling: Some(HOUSE),
        resident_ids: vec![OWNER, PersonId(2)],
    });
    f.supply();
    f.world.get_mut::<Health>(f.owner).unwrap().current = 0.0;
    f.world.entity_mut(f.home).remove::<OwnedBy>();
    f.world
        .get_mut::<HouseholdMembers>(f.household)
        .unwrap()
        .resident_ids = vec![PersonId(2)];
    f.tick(1.0);
    assert_eq!(
        f.world
            .get::<HouseholdEconomy>(f.household)
            .unwrap()
            .pennies,
        340
    );
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.home)
            .unwrap()
            .amount(Good::Wood),
        8
    );
    assert_eq!(f.world.get::<Settlement>(f.hall).unwrap().treasury, 0);
    assert_eq!(f.total_cash(), 1000);
    assert_eq!(f.total_wood(), 8);
}

#[test]
fn market_claims_use_whole_contract_affordability_and_actually_listed_stock() {
    let mut f = fixture();
    f.wood(8, 20);
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.tick(0.0);
    assert_eq!(
        f.world
            .get::<MootMarket>(f.hall)
            .unwrap()
            .pool(Good::Wood)
            .day
            .unaffordable_units,
        0
    );
    cancel_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.world.entity_mut(f.hall).insert(MootMarket::founding());
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.tick(0.0);
    let flow = &f
        .world
        .get::<MootMarket>(f.hall)
        .unwrap()
        .pool(Good::Wood)
        .day;
    assert_eq!(
        flow.unavailable_units, 8,
        "private, unlisted Hall stock is not an offer"
    );
    assert_eq!(flow.unaffordable_units, 0);
    assert_eq!(flow.funded_unmet_units, 8);
}

#[test]
fn daily_history_keeps_reserved_private_upgrade_capital_in_local_money() {
    use crate::world::village::history::{SettlementHistoryRuntime, capture_settlement_history};
    use bevy::ecs::system::RunSystemOnce;
    let mut f = fixture();
    f.world
        .init_resource::<crate::world::village::SettlementEconomyRuntime>();
    f.world.init_resource::<SettlementHistoryRuntime>();
    f.world
        .entity_mut(f.hall)
        .insert(shared::economy::SettlementEconomy::default());
    request_upgrade(&mut f.world, HOUSE, OWNER).unwrap();
    f.world.run_system_once(capture_settlement_history).unwrap();
    f.world.get_mut::<WorldTime>(f.clock).unwrap().day = 1;
    f.world.run_system_once(capture_settlement_history).unwrap();
    let history = f.world.resource::<SettlementHistoryRuntime>().archive(
        f.hall,
        SettlementId(1),
        "Test Village",
    );
    assert_eq!(history.days.len(), 1);
    assert_eq!(history.days[0].total_local_coin, 1000);
    assert_eq!(history.days[0].resident_wallet_money, 500);
    assert_eq!(history.days[0].household_cash, 0);
}

#[test]
fn extension_preserves_its_accepted_doorstep_without_swallowing_public_roads() {
    let original = HouseAppearance::default();
    let target = HouseAppearance {
        level: HouseLevel::UpperStorey,
        ..original
    };
    let position = Vec3::new(-123.0, 18.59834, 103.0);
    let rotation = -std::f32::consts::FRAC_PI_2;
    // Actual connected acceptance fixture: a six-metre future road corridor
    // starts at the accepted doorstep and turns away toward its Hall.
    let mut road = VillageRoad {
        settlement: "Lab Meadow".into(),
        builder: "Builder".into(),
        points: vec![
            Vec2::new(-118.7, 103.0),
            Vec2::new(-116.45, 103.0),
            Vec2::new(-115.467, 103.675),
            Vec2::new(-114.484, 104.349),
            Vec2::new(-113.501, 105.024),
            Vec2::new(-100.0, 112.55),
            Vec2::new(-100.0, 114.8),
        ],
        built_through: 7,
        width: 1.5,
        reserved_width: 6.0,
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    let new = target.building_type().definition();
    assert!(
        road.intersects_rotated_rect(
            new.world_footprint_center(position, rotation),
            new.footprint * 0.5,
            rotation,
            0.15
        ),
        "the old whole-reservation check rejects an ordinary doorstep"
    );
    assert!(!super::placement::road_blocks_extension(
        &road, original, target, position, rotation
    ));
    road.points.reverse();
    assert!(!super::placement::road_blocks_extension(
        &road, original, target, position, rotation
    ));
    // A through-road has no exemption, even when the current house is close.
    road.points = vec![Vec2::new(-120.0, 95.0), Vec2::new(-120.0, 111.0)];
    assert!(super::placement::road_blocks_extension(
        &road, original, target, position, rotation
    ));
    // Neither may the house swallow its own entrance's actual walking line.
    road.points = vec![Vec2::new(-118.7, 103.0), Vec2::new(-123.0, 103.0)];
    assert!(super::placement::road_blocks_extension(
        &road, original, target, position, rotation
    ));
    // Owning the endpoint does not exempt later portions of a looping road.
    road.points = vec![
        Vec2::new(-118.7, 103.0),
        Vec2::new(-113.0, 103.0),
        Vec2::new(-113.0, 115.0),
        Vec2::new(-127.0, 115.0),
        Vec2::new(-127.0, 103.0),
    ];
    assert!(super::placement::road_blocks_extension(
        &road, original, target, position, rotation
    ));
}
