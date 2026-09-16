use super::*;
use shared::components::{
    BuildingId, BuildingOf, CharacterDayPlan, EmployedAt, HouseholdId, HouseholdMembers, PersonId,
    PlannedLeisure, PlannedLeisureStatus, ResidentOf, SettlementId,
};

const TOWN: SettlementId = SettlementId(700);
const TAVERN: BuildingId = BuildingId(701);

fn fixture() -> (App, Entity) {
    let mut app = App::new();
    let mut clock = WorldTime::new_default();
    clock.set_normalized_time(18.5 / 24.0);
    app.world_mut().spawn(clock);
    let tavern = app
        .world_mut()
        .spawn((
            TAVERN,
            BuildingOf(TOWN),
            SettlementBuilding {
                kind: SettlementBuildingKind::Tavern,
                settlement: "Activityford".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
            TavernService::default(),
            GoodsInventory::new(shared::economy::capacity::TAVERN),
            BusinessSalePolicy::default(),
            BusinessAccount::default(),
            shared::components::OperatedBy(shared::components::CompanyId(700)),
        ))
        .id();
    (app, tavern)
}

fn visitor(app: &mut App, id: u64) -> Entity {
    app.world_mut()
        .spawn((
            PersonId(id),
            CharacterKind::Villager,
            ResidentOf(TOWN),
            PlayerPosition(Vec3::new(30.0, 0.0, 30.0)),
            CharacterActivity::Idle,
            Wallet::new(1_000),
            Nutrition::default(),
            WorkStatus::LookingForWork,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
            CharacterDayPlan {
                day: 0,
                wake_minute: 360,
                work_minutes: None,
                meal_minute: 1080,
                leisure_minutes: (1080, 1290),
                sleep_minute: 1320,
                leisure: PlannedLeisure::TavernMeal,
                leisure_status: PlannedLeisureStatus::Planned,
                planned_work_status: WorkStatus::LookingForWork,
            },
        ))
        .id()
}

fn shopping(place: Entity) -> HouseholdShoppingRoutine {
    HouseholdShoppingRoutine {
        account: place,
        household: HouseholdId(700),
        home: place,
        hall: place,
        counter: Vec3::ZERO,
        phase: HouseholdShoppingPhase::ReturningHome,
        cargo: [0; Good::COUNT],
    }
}

fn freight(place: Entity) -> MarketCollectionRoutine {
    MarketCollectionRoutine {
        business: place,
        seller: TAVERN,
        hall: place,
        counter: Vec3::ZERO,
        good: Good::Bread,
        reserved_units: 1,
        unit_price: 20,
        phase: MarketCollectionPhase::ReturningToHall,
        fallback_counter_attempted: false,
    }
}

#[test]
fn tavern_leisure_waits_for_shopping_freight_and_the_last_harvest_load() {
    let (mut app, tavern) = fixture();
    app.add_systems(Update, tavern::assign_tavern_routines);
    let shopper = visitor(&mut app, 1);
    let porter = visitor(&mut app, 2);
    let farmer = visitor(&mut app, 3);
    let destination = Vec3::new(90.0, 0.0, 90.0);
    app.world_mut()
        .entity_mut(shopper)
        .insert((shopping(tavern), MoveTarget(destination)));
    app.world_mut()
        .entity_mut(porter)
        .insert((freight(tavern), MoveTarget(destination)));
    app.world_mut().entity_mut(farmer).insert((
        FarmerRoutine {
            farmstead: tavern,
            field: tavern,
            hall: tavern,
            work_stand: Vec3::ZERO,
            harvest_seconds: 20.0,
            failed_workplace_routes: 0,
            production_day: 0,
            produced_today: 1,
            phase: FarmerPhase::ReturningToFarmstead,
        },
        MoveTarget(destination),
    ));
    app.world_mut()
        .get_mut::<GoodsInventory>(farmer)
        .unwrap()
        .add(Good::Wheat, 1);
    app.update();
    for person in [shopper, porter, farmer] {
        assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
        assert_eq!(
            app.world().get::<MoveTarget>(person).unwrap().0,
            destination
        );
    }
    app.world_mut()
        .entity_mut(shopper)
        .remove::<HouseholdShoppingRoutine>();
    app.update();
    assert!(app.world().get::<TavernVisitRoutine>(shopper).is_some());
    assert!(app.world().get::<TavernVisitRoutine>(porter).is_none());
}

#[derive(Component)]
struct StartShopping;

fn grant_shopping(mut commands: Commands, candidates: Query<Entity, With<StartShopping>>) {
    for candidate in &candidates {
        commands
            .entity(candidate)
            .remove::<StartShopping>()
            .insert(shopping(candidate));
    }
}

#[test]
fn an_ordered_earlier_shopping_grant_prevents_same_update_tavern_admission() {
    let (mut app, _) = fixture();
    let person = visitor(&mut app, 1);
    app.world_mut().entity_mut(person).insert(StartShopping);
    app.add_systems(
        Update,
        (grant_shopping, tavern::assign_tavern_routines).chain(),
    );
    app.update();
    assert!(
        app.world()
            .get::<HouseholdShoppingRoutine>(person)
            .is_some()
    );
    assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
}

#[test]
fn newly_assigned_innkeeper_does_not_also_receive_a_stale_leisure_plan() {
    let (mut app, _) = fixture();
    let person = visitor(&mut app, 1);
    app.world_mut()
        .entity_mut(person)
        .insert(EmployedAt(TAVERN));
    app.add_systems(Update, tavern::assign_tavern_routines);
    app.update();
    assert!(app.world().get::<TavernWorkerRoutine>(person).is_some());
    assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
}

#[test]
fn an_existing_tavern_visit_yields_its_destination_to_committed_freight() {
    let (mut app, tavern) = fixture();
    let person = visitor(&mut app, 1);
    app.add_systems(
        Update,
        (tavern::assign_tavern_routines, tavern::run_tavern_routines).chain(),
    );
    app.update();
    assert!(app.world().get::<TavernVisitRoutine>(person).is_some());
    let destination = Vec3::new(90.0, 0.0, 90.0);
    app.world_mut()
        .entity_mut(person)
        .insert((freight(tavern), MoveTarget(destination)));
    app.update();
    assert_eq!(
        app.world().get::<MoveTarget>(person).unwrap().0,
        destination
    );
}

#[test]
fn real_household_provisioning_waits_for_innkeepers_and_tavern_visitors() {
    let (mut app, _) = fixture();
    let worker = visitor(&mut app, 1);
    let guest = visitor(&mut app, 2);
    app.world_mut()
        .entity_mut(worker)
        .insert(EmployedAt(TAVERN));
    app.init_resource::<BusinessEventQueue>();
    app.init_resource::<MootQueueClock>();
    app.init_resource::<RegionRegistry>();
    let region = RegionCoord::new(0, 0);
    app.world_mut()
        .resource_mut::<RegionRegistry>()
        .set_observers_for_test(region, 1);
    let mut inventory = GoodsInventory::new(shared::economy::capacity::HALL);
    inventory.add(Good::Bread, 20);
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Treasury(TOWN),
        Good::Bread,
        20,
        20,
    );
    app.world_mut().spawn((
        TOWN,
        Settlement {
            name: "Activityford".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 2,
            treasury: 0,
        },
        PlayerPosition(Vec3::ZERO),
        PlayerRotation(0.0),
        inventory,
        market,
    ));
    let home_id = BuildingId(702);
    let home = app
        .world_mut()
        .spawn((
            home_id,
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Activityford".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            PlayerPosition(Vec3::new(30.0, 0.0, 30.0)),
            PlayerRotation(0.0),
            GoodsInventory::new(shared::economy::capacity::HOUSE),
        ))
        .id();
    app.world_mut().spawn((
        HouseholdId(700),
        HouseholdMembers {
            resident_ids: vec![PersonId(1), PersonId(2)],
            settlement: TOWN,
            dwelling: Some(home_id),
        },
        HouseholdEconomy {
            fuel_target_days: 0,
            ..default()
        },
    ));
    for person in [worker, guest] {
        app.world_mut()
            .entity_mut(person)
            .insert((HomeAssignment { home }, region));
    }
    app.add_systems(
        Update,
        (
            tavern::assign_tavern_routines,
            update_household_budgets_and_pantries,
        )
            .chain(),
    );
    app.update();
    assert!(app.world().get::<TavernWorkerRoutine>(worker).is_some());
    assert!(app.world().get::<TavernVisitRoutine>(guest).is_some());
    for person in [worker, guest] {
        assert!(
            app.world()
                .get::<HouseholdShoppingRoutine>(person)
                .is_none()
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 1_000);
    }
    app.world_mut()
        .entity_mut(guest)
        .remove::<TavernVisitRoutine>();
    app.world_mut()
        .get_mut::<CharacterDayPlan>(guest)
        .unwrap()
        .leisure_status = PlannedLeisureStatus::Completed;
    let world = app.world_mut();
    world
        .query::<&mut WorldTime>()
        .single_mut(world)
        .unwrap()
        .set_normalized_time(20.0 / 24.0);
    app.update();
    assert!(app.world().get::<HouseholdShoppingRoutine>(guest).is_some());
    assert!(
        app.world()
            .get::<HouseholdShoppingRoutine>(worker)
            .is_none()
    );
}
