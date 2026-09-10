use super::*;
use std::time::Duration;

fn fixture(count: usize) -> (App, Entity, Entity, Vec<Entity>) {
    let mut app = App::new();
    app.init_resource::<Time>();
    app.add_systems(Update, run_tavern_routines);
    let mut clock = WorldTime::new_default();
    clock.set_normalized_time(18.0 / 24.0);
    app.world_mut().spawn(clock);
    let company_id = shared::components::CompanyId(1);
    let company = app
        .world_mut()
        .spawn((company_id, shared::economy::CompanyAccount::default()))
        .id();
    let building_id = shared::components::BuildingId(1);
    let mut inventory = GoodsInventory::new(SettlementBuildingKind::Tavern.storage_bulk_capacity());
    inventory.add(Good::Bread, 24);
    let tavern = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Tavern,
                settlement: "Test".into(),
                owner: None,
                quality: 1.0,
                workers: vec!["Worker".into()],
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            building_id,
            inventory,
            BusinessSalePolicy {
                asking_unit_price: 10,
                ..Default::default()
            },
            BusinessAccount::default(),
            BusinessCondition {
                state: BusinessState::Operating,
                ..Default::default()
            },
            shared::components::OperatedBy(company_id),
            TavernService {
                innkeepers_on_duty: 1,
                ..Default::default()
            },
        ))
        .id();
    app.world_mut().spawn((
        CharacterKind::Villager,
        shared::components::EmployedAt(building_id),
    ));
    let people = (0..count)
        .map(|i| {
            app.world_mut()
                .spawn((
                    PlayerPosition(Vec3::ZERO),
                    PlayerRotation(0.0),
                    CharacterActivity::Indoors,
                    Wallet::new(10_000),
                    Nutrition::default(),
                    WorkStatus::Chilling,
                    CharacterDayPlan {
                        day: 0,
                        wake_minute: 360,
                        work_minutes: None,
                        meal_minute: 1080,
                        leisure_minutes: (1080, 1290),
                        sleep_minute: 1380,
                        leisure: PlannedLeisure::TavernMeal,
                        leisure_status: PlannedLeisureStatus::InProgress,
                        planned_work_status: WorkStatus::Chilling,
                    },
                    TavernVisitRoutine {
                        tavern,
                        queue_order: i as u128,
                        phase: TavernVisitPhase::Entering,
                        dining_seconds: 0.0,
                        failed_routes: 0,
                        served: false,
                        outdoor_seat: None,
                        progress_position: Vec3::ZERO,
                        progress_world_seconds: 0.0,
                    },
                ))
                .id()
        })
        .collect();
    (app, tavern, company, people)
}

fn seat_everyone(app: &mut App, people: &[Entity]) {
    app.update(); // Buy the meal and reserve a seat.
    for &person in people {
        app.world_mut()
            .entity_mut(person)
            .remove::<WorkplaceDoorTransit>();
    }
    app.update(); // Door exit completed; normal movement gets a bench destination.
    for &person in people {
        let target = app.world().get::<MoveTarget>(person).unwrap().0;
        app.world_mut()
            .entity_mut(person)
            .insert(PlayerPosition(target));
    }
    app.update(); // Arrival uses the actual routine to sit and face the table.
}

#[test]
fn eight_tavern_guests_pay_once_use_distinct_seats_and_leave_on_ground() {
    let (mut app, tavern, company, people) = fixture(8);
    seat_everyone(&mut app, &people);
    let mut indices = std::collections::BTreeSet::new();
    for &person in &people {
        let routine = app.world().get::<TavernVisitRoutine>(person).unwrap();
        assert_eq!(routine.phase, TavernVisitPhase::OutdoorDining);
        indices.insert(routine.outdoor_seat.unwrap());
        assert_eq!(
            *app.world().get::<CharacterActivity>(person).unwrap(),
            CharacterActivity::Sitting
        );
        assert!((app.world().get::<PlayerPosition>(person).unwrap().0.y - 0.5).abs() < 1e-5);
        assert!(app.world().get::<MoveTarget>(person).is_none());
    }
    assert_eq!(indices.len(), 8);
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(tavern)
            .unwrap()
            .amount(Good::Bread),
        16
    );
    assert_eq!(
        app.world()
            .get::<shared::economy::CompanyAccount>(company)
            .unwrap()
            .cash,
        80
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(46));
    app.update();
    for &person in &people {
        assert_eq!(app.world().get::<PlayerPosition>(person).unwrap().0.y, 0.0);
        let target = app.world().get::<MoveTarget>(person).unwrap().0;
        app.world_mut()
            .entity_mut(person)
            .insert(PlayerPosition(target));
    }
    app.update();
    for &person in &people {
        assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
        assert_eq!(
            app.world()
                .get::<CharacterDayPlan>(person)
                .unwrap()
                .leisure_status,
            PlannedLeisureStatus::Completed
        );
        assert_eq!(app.world().get::<Wallet>(person).unwrap().balance(), 9_990);
    }
    assert_eq!(
        app.world()
            .get::<shared::economy::CompanyAccount>(company)
            .unwrap()
            .cash,
        80
    );
}

#[test]
fn a_new_job_keeps_todays_completed_meal_but_a_new_day_resets_it() {
    let (mut app, _, _, people) = fixture(1);
    app.add_systems(
        Update,
        refresh_character_day_plans.after(run_tavern_routines),
    );
    let person = people[0];
    app.world_mut()
        .entity_mut(person)
        .remove::<TavernVisitRoutine>()
        .insert((
            shared::components::PersonId(42),
            shared::components::ResidentOf(shared::components::SettlementId(1)),
            WorkStatus::Employed,
        ));
    app.world_mut()
        .get_mut::<CharacterDayPlan>(person)
        .unwrap()
        .leisure_status = PlannedLeisureStatus::Completed;
    app.update();
    let plan = app.world().get::<CharacterDayPlan>(person).unwrap();
    assert_eq!(plan.planned_work_status, WorkStatus::Employed);
    assert!(plan.work_minutes.is_some());
    assert_eq!(plan.leisure_status, PlannedLeisureStatus::Completed);
    app.world_mut()
        .query::<&mut WorldTime>()
        .single_mut(app.world_mut())
        .unwrap()
        .day += 1;
    app.update();
    assert_eq!(
        app.world()
            .get::<CharacterDayPlan>(person)
            .unwrap()
            .leisure_status,
        PlannedLeisureStatus::Planned
    );
}

#[test]
fn tavern_removal_or_strategic_demotion_releases_seated_visitors() {
    for demote in [false, true] {
        let (mut app, tavern, _, people) = fixture(1);
        seat_everyone(&mut app, &people);
        let person = people[0];
        if demote {
            app.world_mut()
                .entity_mut(person)
                .insert(strategic::StrategicPerson);
        } else {
            app.world_mut().despawn(tavern);
        }
        app.update();
        assert!(app.world().get::<TavernVisitRoutine>(person).is_none());
        assert_eq!(app.world().get::<PlayerPosition>(person).unwrap().0.y, 0.0);
        assert_eq!(
            *app.world().get::<CharacterActivity>(person).unwrap(),
            CharacterActivity::Idle
        );
    }
}

#[test]
fn civic_hiring_waits_until_a_tavern_visit_finishes() {
    let (mut app, _, _, people) = fixture(8);
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Test".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 8,
                treasury: 100_000,
            },
            shared::components::SettlementId(1),
        ))
        .id();
    for (index, &person) in people.iter().enumerate() {
        app.world_mut().entity_mut(person).insert((
            shared::components::PersonId(index as u64 + 1),
            CharacterName(format!("Guest {index}")),
            VillagerIntent::Resident { settlement: hall },
            Occupation(None),
        ));
    }
    app.add_systems(
        Update,
        (
            crate::world::village_roads::ensure_moot_administrations,
            crate::world::village_roads::staff_moot_stewards,
            crate::world::village_roads::staff_public_positions,
        )
            .chain(),
    );
    app.update();
    for &person in &people {
        assert!(app
            .world()
            .get::<shared::components::CivicEmployment>(person)
            .is_none());
    }
    app.world_mut()
        .entity_mut(people[0])
        .remove::<TavernVisitRoutine>();
    app.update();
    assert!(app
        .world()
        .get::<shared::components::CivicEmployment>(people[0])
        .is_some());
}
