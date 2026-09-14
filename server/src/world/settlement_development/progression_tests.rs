use super::*;
use crate::world::village::development_evidence::SettlementDevelopmentSample;
use shared::economy::{MootMarket, SettlementEconomy};

fn fixture(tier: SettlementTier) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<SettlementDevelopmentSamples>()
        .add_systems(Update, update_settlement_developments);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let hall = app
        .world_mut()
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Workingstead".into(),
                tier,
                residents: 30,
                treasury: 271,
            },
            SettlementDevelopment::from_seed(19, 0),
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            // Deliberately hungry and poor. These are meaningful wellbeing data,
            // but neither should veto the structural development contract.
            SettlementEconomy {
                prosperity: 0.0,
                unmet_food: 30,
                food_secure_days: 0,
                ..default()
            },
            MootMarket::founding(),
        ))
        .id();
    (app, clock, hall)
}

fn village_evidence() -> SettlementDevelopmentEvidence {
    SettlementDevelopmentEvidence {
        residents: 12,
        housed_residents: 8,
        occupied_homes: 2,
        ..default()
    }
}

fn town_evidence() -> SettlementDevelopmentEvidence {
    SettlementDevelopmentEvidence {
        residents: 30,
        housed_residents: 20,
        occupied_homes: 3,
        operating_business_types: 2,
        market_accessible: true,
        paid_trade_pennies: 17,
    }
}

fn observe(
    app: &mut App,
    clock: Entity,
    hall: Entity,
    day: u32,
    current: SettlementDevelopmentEvidence,
    completed: Option<(u32, SettlementDevelopmentEvidence)>,
) {
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
    app.world_mut()
        .get_mut::<Settlement>(hall)
        .unwrap()
        .residents = current.residents;
    app.world_mut()
        .resource_mut::<SettlementDevelopmentSamples>()
        .by_settlement
        .insert(
            SettlementId(1),
            SettlementDevelopmentSample { current, completed },
        );
    app.update();
}

fn projects(app: &mut App) -> usize {
    app.world_mut()
        .query::<&CivicHallUpgradeWorksite>()
        .iter(app.world())
        .count()
}

#[test]
fn hungry_growing_hamlet_qualifies_on_two_of_three_dates_and_still_owes_real_wood() {
    let (mut app, clock, hall) = fixture(SettlementTier::Hamlet);
    let first = village_evidence();
    observe(&mut app, clock, hall, 1, first, Some((0, first)));
    assert_eq!(projects(&mut app), 0);
    let mut missed = first;
    missed.housed_residents = 7;
    observe(&mut app, clock, hall, 2, missed, Some((1, missed)));
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .progress_days,
        1
    );
    let mut grown = first;
    grown.residents = 17;
    observe(&mut app, clock, hall, 3, grown, Some((2, grown)));
    assert_eq!(projects(&mut app), 1);
    assert_eq!(
        app.world().get::<Settlement>(hall).unwrap().tier,
        SettlementTier::Hamlet
    );
    assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 271);
    let (project, inventory) = app
        .world_mut()
        .query::<(&CivicHallUpgradeWorksite, &GoodsInventory)>()
        .single(app.world())
        .unwrap();
    assert_eq!(project.target, CivicHallLevel::Village);
    assert_eq!(project.material, Good::Wood);
    assert_eq!(project.material_required, VILLAGE_HALL_WOOD_REQUIRED);
    assert_eq!(inventory.amount(Good::Wood), 0);
    let development = app.world().get::<SettlementDevelopment>(hall).unwrap();
    assert_eq!(development.progress_days, 2);
    assert_eq!(development.required_days, 2);
    assert_eq!(development.material_required, 12);
}

#[test]
fn bootstrap_repeated_updates_and_skipped_days_do_not_invent_qualification() {
    let (mut app, clock, hall) = fixture(SettlementTier::Hamlet);
    let evidence = village_evidence();
    observe(&mut app, clock, hall, 0, evidence, None);
    assert_eq!(projects(&mut app), 0);
    observe(&mut app, clock, hall, 5, evidence, Some((4, evidence)));
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .progress_days,
        1
    );
    observe(&mut app, clock, hall, 8, evidence, Some((7, evidence)));
    assert_eq!(projects(&mut app), 0);
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .progress_days,
        1
    );
    observe(&mut app, clock, hall, 9, evidence, Some((8, evidence)));
    assert_eq!(projects(&mut app), 1);
}

#[test]
fn outdated_or_future_evidence_cannot_be_relabelled_as_yesterday() {
    let (mut app, clock, hall) = fixture(SettlementTier::Hamlet);
    let evidence = village_evidence();
    observe(&mut app, clock, hall, 1, evidence, Some((0, evidence)));
    observe(&mut app, clock, hall, 2, evidence, Some((0, evidence)));
    observe(&mut app, clock, hall, 3, evidence, Some((30, evidence)));
    assert_eq!(projects(&mut app), 0);
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .progress_days,
        1
    );
    observe(&mut app, clock, hall, 4, evidence, None);
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .progress_days,
        0
    );
}

#[test]
fn current_population_or_lost_housing_blocks_commissioning_despite_good_history() {
    let (mut app, clock, hall) = fixture(SettlementTier::Hamlet);
    let evidence = village_evidence();
    observe(&mut app, clock, hall, 1, evidence, Some((0, evidence)));
    let mut current = evidence;
    current.housed_residents = 0;
    current.occupied_homes = 0;
    observe(&mut app, clock, hall, 2, current, Some((1, evidence)));
    assert_eq!(projects(&mut app), 0);
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .next_gate,
        SettlementProgressGate::Housing
    );
    // The two real prior days still qualify when today's snapshot establishes
    // repaired homes; the latest observation itself need not qualify as well.
    observe(&mut app, clock, hall, 3, evidence, Some((2, current)));
    assert_eq!(projects(&mut app), 1);
}

#[test]
fn town_needs_access_diverse_activity_and_recent_paid_trade_without_a_tavern_gate() {
    let (mut app, clock, hall) = fixture(SettlementTier::Village);
    let evidence = town_evidence();
    for (day, expected) in [
        (1, SettlementProgressGate::Marketplace),
        (2, SettlementProgressGate::BusinessActivity),
        (3, SettlementProgressGate::Trade),
    ] {
        let mut current = evidence;
        match expected {
            SettlementProgressGate::Marketplace => current.market_accessible = false,
            SettlementProgressGate::BusinessActivity => current.operating_business_types = 1,
            SettlementProgressGate::Trade => current.paid_trade_pennies = 0,
            _ => unreachable!(),
        }
        observe(
            &mut app,
            clock,
            hall,
            day,
            current,
            Some((day - 1, current)),
        );
        assert_eq!(
            app.world()
                .get::<SettlementDevelopment>(hall)
                .unwrap()
                .next_gate,
            expected
        );
        assert_eq!(projects(&mut app), 0);
    }
    observe(&mut app, clock, hall, 4, evidence, Some((3, evidence)));
    observe(&mut app, clock, hall, 5, evidence, Some((4, evidence)));
    assert_eq!(projects(&mut app), 1);
    let project = app
        .world_mut()
        .query::<&CivicHallUpgradeWorksite>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        (project.target, project.material, project.material_required),
        (CivicHallLevel::Town, Good::Stone, TOWN_HALL_STONE_REQUIRED)
    );
}

#[test]
fn commissioned_project_survives_deterioration_and_materials_do_not_replace_day_counts() {
    let (mut app, clock, hall) = fixture(SettlementTier::Hamlet);
    let evidence = village_evidence();
    observe(&mut app, clock, hall, 1, evidence, Some((0, evidence)));
    observe(&mut app, clock, hall, 2, evidence, Some((1, evidence)));
    let project_entity = app
        .world_mut()
        .query_filtered::<Entity, With<CivicHallUpgradeWorksite>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .get_mut::<GoodsInventory>(project_entity)
        .unwrap()
        .add(Good::Wood, 5);
    observe(
        &mut app,
        clock,
        hall,
        5,
        SettlementDevelopmentEvidence::default(),
        None,
    );
    assert_eq!(projects(&mut app), 1);
    let development = app.world().get::<SettlementDevelopment>(hall).unwrap();
    assert_eq!(
        (development.progress_days, development.required_days),
        (2, 2)
    );
    assert_eq!(
        (development.material_staged, development.material_required),
        (5, 12)
    );
    assert_eq!(
        development.next_gate,
        SettlementProgressGate::CivicHallMaterials
    );
    app.world_mut()
        .get_mut::<ConstructionSite>(project_entity)
        .unwrap()
        .raising = true;
    app.update();
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .next_gate,
        SettlementProgressGate::CivicHallConstruction
    );
}

#[test]
fn occupied_beds_and_total_residents_cannot_replace_distinct_homes_or_housed_people() {
    let (mut app, clock, hall) = fixture(SettlementTier::Hamlet);
    let mut evidence = village_evidence();
    evidence.residents = 500;
    evidence.occupied_homes = 1;
    observe(&mut app, clock, hall, 1, evidence, Some((0, evidence)));
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .next_gate,
        SettlementProgressGate::OccupiedHomes
    );
    let (mut app, clock, hall) = fixture(SettlementTier::Village);
    let mut evidence = town_evidence();
    evidence.residents = 500;
    evidence.housed_residents = 19;
    observe(&mut app, clock, hall, 1, evidence, Some((0, evidence)));
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .next_gate,
        SettlementProgressGate::Housing
    );
}

#[test]
fn road_summaries_stay_settlement_scoped_and_refresh_without_daily_resampling() {
    let (mut app, clock, hall) = fixture(SettlementTier::Town);
    let other = app
        .world_mut()
        .spawn((
            SettlementId(2),
            Settlement {
                name: "Otherstead".into(),
                tier: SettlementTier::Town,
                residents: 0,
                treasury: 0,
            },
            SettlementDevelopment::from_seed(20, 0),
            PlayerPosition(Vec3::X * 100.0),
        ))
        .id();
    let road = app
        .world_mut()
        .spawn((
            RoadOf(SettlementId(1)),
            VillageRoad {
                settlement: "Workingstead".into(),
                builder: "Builder".into(),
                points: vec![Vec2::ZERO, Vec2::X * 20.0],
                built_through: 2,
                width: 2.6,
                reserved_width: RoadClass::Main.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Main,
                stone_committed: 0,
            },
        ))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(hall)
            .unwrap()
            .dirt_roads,
        1
    );
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(other)
            .unwrap()
            .dirt_roads,
        0
    );
    app.world_mut()
        .get_mut::<VillageRoad>(road)
        .unwrap()
        .surface = RoadSurface::Stone;
    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .seconds_in_cycle += 1.0;
    app.update();
    let development = app.world().get::<SettlementDevelopment>(hall).unwrap();
    assert_eq!(
        (
            development.dirt_roads,
            development.stone_roads,
            development.stone_needed
        ),
        (0, 1, 0)
    );
    assert_eq!(
        app.world()
            .get::<SettlementDevelopment>(other)
            .unwrap()
            .stone_roads,
        0
    );
}
