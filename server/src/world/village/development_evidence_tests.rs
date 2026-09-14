use super::*;
use shared::components::{
    CompanyId, HouseAppearance, HouseLevel, RoadClass, RoadSurface, SettlementTier,
};
use shared::economy::{BusinessDayLedger, BusinessState};

struct Fixture {
    app: App,
    clock: Entity,
    hall: Entity,
    settlement: SettlementId,
}

impl Fixture {
    fn new() -> Self {
        let mut app = App::new();
        app.init_resource::<SettlementDevelopmentSamples>()
            .add_systems(Update, aggregate_settlement_development);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let settlement = SettlementId(1);
        let hall = spawn_hall(app.world_mut(), settlement, Vec3::ZERO);
        Self {
            app,
            clock,
            hall,
            settlement,
        }
    }

    fn day(&mut self, day: u32) {
        self.app
            .world_mut()
            .get_mut::<WorldTime>(self.clock)
            .unwrap()
            .day = day;
        self.app.update();
    }

    fn sample(&self) -> SettlementDevelopmentSample {
        self.app
            .world()
            .resource::<SettlementDevelopmentSamples>()
            .by_settlement[&self.settlement]
    }

    fn building(&mut self, id: u64, kind: SettlementBuildingKind) -> Entity {
        self.app
            .world_mut()
            .spawn((
                BuildingId(id),
                BuildingOf(self.settlement),
                building(kind),
                PlayerPosition(Vec3::new(id as f32 * 10.0, 0.0, 0.0)),
                PlayerRotation(0.0),
            ))
            .id()
    }

    fn person(&mut self, id: u64, home: Option<Entity>) -> Entity {
        let mut person = self.app.world_mut().spawn((
            PersonId(id),
            Health::default(),
            CharacterKind::Villager,
            ResidentOf(self.settlement),
            VillagerIntent::Resident {
                settlement: self.hall,
            },
        ));
        if let Some(home) = home {
            person.insert(HomeAssignment::new(home));
        }
        person.id()
    }

    fn business(
        &mut self,
        id: u64,
        kind: SettlementBuildingKind,
        ledger: BusinessDayLedger,
    ) -> Entity {
        let business = self.building(id, kind);
        self.app.world_mut().entity_mut(business).insert((
            BusinessAccount {
                current_day: ledger,
                ..default()
            },
            BusinessCondition::default(),
            OperatedBy(CompanyId(id)),
        ));
        let worker = self.person(id + 10_000, None);
        self.app
            .world_mut()
            .entity_mut(worker)
            .insert(EmployedAt(BuildingId(id)));
        business
    }
}

fn building(kind: SettlementBuildingKind) -> SettlementBuilding {
    SettlementBuilding {
        kind,
        settlement: "Evidence".into(),
        owner: None,
        quality: 1.0,
        workers: Vec::new(),
    }
}

fn spawn_hall(world: &mut World, id: SettlementId, position: Vec3) -> Entity {
    world
        .spawn((
            id,
            Settlement {
                name: "Evidence".into(),
                tier: SettlementTier::Village,
                residents: 999,
                treasury: 0,
            },
            PlayerPosition(position),
        ))
        .id()
}

fn road(points: Vec<Vec2>, complete: bool) -> VillageRoad {
    VillageRoad {
        settlement: "Display names do not define membership".into(),
        builder: "Road crew".into(),
        built_through: if complete { points.len() as u16 } else { 1 },
        points,
        width: 2.6,
        reserved_width: RoadClass::Lane.initial_reserved_width(),
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    }
}

#[test]
fn arrivals_and_a_house_extension_do_not_remove_existing_housed_residents() {
    let mut f = Fixture::new();
    let first = f.building(1, SettlementBuildingKind::House);
    let second = f.building(2, SettlementBuildingKind::House);
    for id in 1..=8 {
        f.person(id, Some(if id <= 4 { first } else { second }));
    }
    f.day(0);
    assert!(f.sample().completed.is_none());
    assert_eq!(f.sample().current.housed_residents, 8);
    for id in 9..=14 {
        f.person(id, None);
    }
    f.app.world_mut().entity_mut(first).insert(HouseAppearance {
        level: HouseLevel::UpperStorey,
        ..default()
    });
    f.day(1);
    let sample = f.sample();
    assert_eq!(sample.completed.unwrap().0, 0);
    assert_eq!(sample.current.residents, 14);
    assert_eq!(sample.current.housed_residents, 8);
    assert_eq!(sample.current.occupied_homes, 2);
}

#[test]
fn only_distinct_living_residents_of_real_completed_same_settlement_homes_count() {
    let mut f = Fixture::new();
    let valid = f.building(1, SettlementBuildingKind::House);
    let wrong_town = f.building(2, SettlementBuildingKind::House);
    f.app
        .world_mut()
        .entity_mut(wrong_town)
        .insert(BuildingOf(SettlementId(2)));
    let unfinished = f.building(3, SettlementBuildingKind::House);
    f.app
        .world_mut()
        .entity_mut(unfinished)
        .insert(UnderConstruction {
            kind: SettlementBuildingKind::House,
            position: Vec3::ZERO,
            rotation: 0.0,
            owner: None,
            owner_id: None,
            builder: None,
            settlement: f.hall,
            settlement_id: f.settlement,
            stand: Vec3::ZERO,
            failed_stand_routes: 0,
            stage: super::super::BuildStage::Supplying,
            quality: 1.0,
        });
    let site_only = f.building(4, SettlementBuildingKind::House);
    f.app
        .world_mut()
        .entity_mut(site_only)
        .insert(ConstructionSite {
            kind: SettlementBuildingKind::House,
            settlement: "Evidence".into(),
            raising: true,
            stand: Vec3::ZERO,
            rotation: 0.0,
        });
    let workplace = f.building(5, SettlementBuildingKind::Bakery);
    f.person(1, Some(valid));
    f.person(1, Some(valid)); // Malformed duplicate durable id must not add a resident.
    for (id, home) in [
        (2, wrong_town),
        (3, unfinished),
        (4, site_only),
        (5, workplace),
    ] {
        f.person(id, Some(home));
    }
    let dead = f.person(6, Some(valid));
    f.app.world_mut().get_mut::<Health>(dead).unwrap().current = 0.0;
    let sailing = f.person(7, Some(valid));
    f.app
        .world_mut()
        .entity_mut(sailing)
        .insert(VillagerIntent::ArrivingBySea { settlement: f.hall });
    let travelling = f.person(8, Some(valid));
    f.app
        .world_mut()
        .entity_mut(travelling)
        .insert(VillagerIntent::Travelling { settlement: f.hall });
    let departed = f.person(9, Some(valid));
    f.app
        .world_mut()
        .entity_mut(departed)
        .insert(ResidentOf(SettlementId(2)));
    let removed = f.building(6, SettlementBuildingKind::House);
    f.person(10, Some(removed));
    f.app.world_mut().despawn(removed);
    f.day(0);
    assert_eq!(f.sample().current.residents, 6);
    assert_eq!(f.sample().current.housed_residents, 1);
    assert_eq!(f.sample().current.occupied_homes, 1);
}

#[test]
fn production_and_paid_specialist_services_qualify_without_local_food_production() {
    let mut f = Fixture::new();
    let mut quarry = BusinessDayLedger::empty(0);
    quarry.produced_units = 5;
    f.business(1, SettlementBuildingKind::StoneQuarry, quarry);
    let mut freight = BusinessDayLedger::empty(0);
    freight.gross_revenue = 75;
    f.business(2, SettlementBuildingKind::StorageHall, freight);
    // A second quarry does not create a third distinct business type.
    f.business(3, SettlementBuildingKind::StoneQuarry, quarry);
    f.day(0);
    f.day(1);
    let evidence = f.sample().completed.unwrap().1;
    assert_eq!(evidence.operating_business_types, 2);
    assert_eq!(evidence.paid_trade_pennies, 75);
}

#[test]
fn closed_unstaffed_stale_and_internal_transfer_only_businesses_do_not_qualify() {
    let mut f = Fixture::new();
    let mut active = BusinessDayLedger::empty(0);
    active.produced_units = 8;
    active.gross_revenue = 40;
    let closed = f.business(1, SettlementBuildingKind::Bakery, active);
    f.app
        .world_mut()
        .get_mut::<BusinessCondition>(closed)
        .unwrap()
        .state = BusinessState::Closed;
    let empty = f.business(2, SettlementBuildingKind::Tavern, active);
    let employed = f
        .app
        .world_mut()
        .query::<(Entity, &EmployedAt)>()
        .iter(f.app.world())
        .find_map(|(entity, at)| (at.0 == BuildingId(2)).then_some(entity))
        .unwrap();
    f.app
        .world_mut()
        .entity_mut(employed)
        .remove::<EmployedAt>();
    // A display roster is not proof of a real living employee.
    f.app
        .world_mut()
        .get_mut::<SettlementBuilding>(empty)
        .unwrap()
        .workers
        .push("Ghost".into());
    let mut internal = BusinessDayLedger::empty(0);
    internal.internal_revenue = 900;
    internal.sold_units = 20;
    f.business(3, SettlementBuildingKind::StorageHall, internal);
    let stale = f.business(4, SettlementBuildingKind::LumberjackHut, active);
    f.app
        .world_mut()
        .get_mut::<BusinessAccount>(stale)
        .unwrap()
        .current_day
        .day = 99;
    f.business(5, SettlementBuildingKind::Market, active);
    f.business(6, SettlementBuildingKind::Hall, active);
    let mut idle = BusinessDayLedger::empty(0);
    idle.wage_expense = 100;
    f.business(7, SettlementBuildingKind::Windmill, idle);
    f.day(0);
    f.day(1);
    assert_eq!(f.sample().current.operating_business_types, 0);
    assert_eq!(f.sample().current.paid_trade_pennies, 0);
}

#[test]
fn exact_completed_ledger_is_used_and_cannot_repeat_on_the_next_date() {
    let mut f = Fixture::new();
    let mut ledger = BusinessDayLedger::empty(0);
    ledger.gross_revenue = 45;
    let site = f.business(
        1,
        SettlementBuildingKind::Tavern,
        BusinessDayLedger::empty(1),
    );
    f.app
        .world_mut()
        .get_mut::<BusinessAccount>(site)
        .unwrap()
        .previous_day = ledger;
    f.day(0);
    f.day(1);
    assert_eq!(f.sample().current.paid_trade_pennies, 45);
    f.day(2);
    assert_eq!(f.sample().current.paid_trade_pennies, 0);
    assert_eq!(f.sample().current.operating_business_types, 0);
}

#[test]
fn a_market_requires_its_own_completed_hall_connected_connector() {
    let mut f = Fixture::new();
    let market = f.building(1, SettlementBuildingKind::Market);
    let position = f.app.world().get::<PlayerPosition>(market).unwrap().0;
    let door = SettlementBuildingKind::Market
        .entrance_position(position, 0.0)
        .xz();
    let hall_door = SettlementBuildingKind::Hall
        .entrance_position(Vec3::ZERO, 0.0)
        .xz();
    let connector = f
        .app
        .world_mut()
        .spawn((RoadOf(f.settlement), road(vec![door, hall_door], false)))
        .id();
    f.day(0);
    assert!(!f.sample().current.market_accessible);
    f.app
        .world_mut()
        .get_mut::<VillageRoad>(connector)
        .unwrap()
        .built_through = 2;
    f.day(1);
    assert!(f.sample().current.market_accessible);
    f.app
        .world_mut()
        .get_mut::<VillageRoad>(connector)
        .unwrap()
        .points[1] += Vec2::X * 30.0;
    f.day(2);
    assert!(!f.sample().current.market_accessible);
    f.app
        .world_mut()
        .get_mut::<VillageRoad>(connector)
        .unwrap()
        .points[1] = hall_door;
    f.app
        .world_mut()
        .entity_mut(connector)
        .insert(RoadOf(SettlementId(2)));
    f.day(3);
    assert!(!f.sample().current.market_accessible);
}

#[test]
fn foundation_and_skipped_dates_do_not_invent_completed_observations() {
    let mut f = Fixture::new();
    f.day(12);
    assert!(f.sample().completed.is_none());
    f.day(15);
    assert!(f.sample().completed.is_none());
    f.day(16);
    assert_eq!(f.sample().completed.unwrap().0, 15);
    let newcomer = SettlementId(2);
    spawn_hall(f.app.world_mut(), newcomer, Vec3::X * 100.0);
    f.day(17);
    assert!(f
        .app
        .world()
        .resource::<SettlementDevelopmentSamples>()
        .by_settlement[&newcomer]
        .completed
        .is_none());
}

#[test]
fn repeated_ticks_keep_the_sample_until_the_next_daily_review() {
    let mut f = Fixture::new();
    f.person(1, None);
    f.day(0);
    f.person(2, None);
    f.day(0);
    assert_eq!(f.sample().current.residents, 1);
    f.day(1);
    assert_eq!(f.sample().current.residents, 2);
}

#[test]
#[ignore = "timing probe; run alone with --ignored --nocapture"]
fn development_evidence_5000_resident_scale_probe() {
    use std::time::Instant;
    let mut f = Fixture::new();
    f.app.world_mut().despawn(f.hall);
    for town in 0..10_u64 {
        let id = SettlementId(town + 1);
        let position = Vec3::X * (town as f32 * 1000.0);
        let hall = spawn_hall(f.app.world_mut(), id, position);
        let hall_door = SettlementBuildingKind::Hall
            .entrance_position(position, 0.0)
            .xz();
        for home in 0..125_u64 {
            let building_id = BuildingId(town * 200 + home);
            let home_position = position
                + Vec3::new(
                    (home % 12) as f32 * 12.0 + 20.0,
                    0.0,
                    (home / 12) as f32 * 15.0,
                );
            let house = f
                .app
                .world_mut()
                .spawn((
                    building_id,
                    BuildingOf(id),
                    building(SettlementBuildingKind::House),
                    PlayerPosition(home_position),
                ))
                .id();
            let door = SettlementBuildingKind::House
                .entrance_position(home_position, 0.0)
                .xz();
            f.app.world_mut().spawn((
                RoadOf(id),
                road(vec![door, Vec2::new(door.x, hall_door.y), hall_door], true),
            ));
            for occupant in 0..4_u64 {
                let person = town * 500 + home * 4 + occupant;
                f.app.world_mut().spawn((
                    PersonId(person),
                    Health::default(),
                    CharacterKind::Villager,
                    ResidentOf(id),
                    VillagerIntent::Resident { settlement: hall },
                    HomeAssignment::new(house),
                ));
            }
        }
        for site in 0..20_u64 {
            let building_id = BuildingId(town * 200 + 125 + site);
            let kind = if site == 0 {
                SettlementBuildingKind::Market
            } else {
                SettlementBuildingKind::StoneQuarry
            };
            let site_position = position + Vec3::new(20.0 + site as f32 * 10.0, 0.0, -30.0);
            f.app.world_mut().spawn((
                building_id,
                BuildingOf(id),
                building(kind),
                PlayerPosition(site_position),
                BusinessCondition::default(),
                BusinessAccount::default(),
                OperatedBy(CompanyId(building_id.0)),
            ));
            let door = kind.entrance_position(site_position, 0.0).xz();
            f.app.world_mut().spawn((
                RoadOf(id),
                road(vec![door, Vec2::new(door.x, hall_door.y), hall_door], true),
            ));
        }
    }
    f.day(0);
    let mut durations = Vec::new();
    for day in 1..=50 {
        f.app.world_mut().get_mut::<WorldTime>(f.clock).unwrap().day = day;
        let start = Instant::now();
        f.app.update();
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(f64::total_cmp);
    let start = Instant::now();
    for _ in 0..10_000 {
        f.app.update();
    }
    let no_op_us = start.elapsed().as_secs_f64() * 1_000_000.0 / 10_000.0;
    assert_eq!(
        f.app
            .world()
            .resource::<SettlementDevelopmentSamples>()
            .by_settlement
            .values()
            .map(|s| s.current.residents)
            .sum::<u32>(),
        5000
    );
    println!("Development evidence: 5000 residents, 1250 homes, 200 firms, 1450 roads, 10 settlements; daily p50={:.3}ms p95={:.3}ms max={:.3}ms; same-day schedule mean={no_op_us:.3}us", durations[25], durations[47], durations[49]);
}
