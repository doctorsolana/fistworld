use super::*;
use crate::world::village::{BuildStage, BusinessEventQueue};
use shared::economy::MarketSeller;

struct Fixture {
    world: World,
    hall: Entity,
    clock: Entity,
    homes: Vec<Entity>,
    owners: Vec<Entity>,
    groups: Vec<Entity>,
}

fn fixture(houses: usize, arriving: usize) -> Fixture {
    let mut world = World::new();
    world.init_resource::<HouseUpgradeDecisions>();
    world.init_resource::<super::super::HouseUpgradeProjects>();
    world.init_resource::<BusinessEventQueue>();
    let clock = world.spawn(WorldTime::new_default()).id();
    let mut market = MootMarket::founding();
    market.consign(MarketSeller::Treasury(SettlementId(1)), Good::Wood, 100, 50);
    market.consign(
        MarketSeller::Treasury(SettlementId(1)),
        Good::Bread,
        100,
        100,
    );
    let mut stock = GoodsInventory::new(4000);
    stock.add(Good::Wood, 100);
    stock.add(Good::Bread, 100);
    let hall = world
        .spawn((
            SettlementId(1),
            Settlement {
                name: "Village".into(),
                tier: SettlementTier::Village,
                residents: (houses * 4) as u32,
                treasury: 0,
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            market,
            stock,
        ))
        .id();
    let mut homes = Vec::new();
    let mut owners = Vec::new();
    let mut groups = Vec::new();
    for index in 0..houses {
        let id = BuildingId(index as u64 + 1);
        let household = HouseholdId(id.0);
        let members: Vec<_> = (0..4)
            .map(|offset| PersonId(index as u64 * 4 + offset + 1))
            .collect();
        let at = Vec3::new(
            100.0 + (index % 50) as f32 * 20.0,
            0.0,
            100.0 + (index / 50) as f32 * 20.0,
        );
        let mut pantry = GoodsInventory::new(80);
        pantry.add(Good::Bread, 12);
        pantry.add(Good::Wood, 2);
        let home = world
            .spawn((
                id,
                BuildingOf(SettlementId(1)),
                HouseAppearance::default(),
                OwnedBy(members[0]),
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Village".into(),
                    owner: Some("Owner".into()),
                    quality: 1.0,
                    workers: Vec::new(),
                },
                PlayerPosition(at),
                PlayerRotation(0.0),
                Household {
                    resident_ids: members.clone(),
                    residents: vec!["Resident".into(); 4],
                },
                OccupiedByHousehold(household),
                pantry,
            ))
            .id();
        homes.push(home);
        groups.push(
            world
                .spawn((
                    household,
                    HouseholdMembers {
                        resident_ids: members.clone(),
                        settlement: SettlementId(1),
                        dwelling: Some(id),
                    },
                    HouseholdEconomy::default(),
                ))
                .id(),
        );
        for (offset, person) in members.into_iter().enumerate() {
            let entity = world
                .spawn((
                    person,
                    CharacterName("Resident".into()),
                    CharacterKind::Villager,
                    Wallet::new(3000),
                    Health::default(),
                    LivesAt(id),
                    HouseholdMember(household),
                    ResidentOf(SettlementId(1)),
                    VillagerIntent::Resident { settlement: hall },
                    PlayerPosition(at + Vec3::Z * 8.0),
                    PlayerRotation(0.0),
                ))
                .id();
            if offset == 0 {
                owners.push(entity);
            }
        }
    }
    for person in 0..arriving {
        world.spawn((
            PersonId((houses * 4 + person + 1) as u64),
            VillagerIntent::ArrivingBySea { settlement: hall },
        ));
    }
    Fixture {
        world,
        hall,
        clock,
        homes,
        owners,
        groups,
    }
}

fn review(f: &mut Fixture, day: u32) {
    f.world.get_mut::<WorldTime>(f.clock).unwrap().day = day;
    review_house_upgrades(&mut f.world);
}

fn pending(f: &Fixture) -> usize {
    f.world
        .resource::<super::super::HouseUpgradeProjects>()
        .pending_in_settlement(SettlementId(1))
}

fn pending_house(f: &mut Fixture) {
    f.world.spawn((
        UnderConstruction {
            kind: SettlementBuildingKind::House,
            position: Vec3::new(1500.0, 0.0, 1500.0),
            rotation: 0.0,
            owner: None,
            owner_id: None,
            builder: None,
            settlement: f.hall,
            settlement_id: SettlementId(1),
            stand: Vec3::ZERO,
            failed_stand_routes: 0,
            stage: BuildStage::Supplying,
            quality: 1.0,
        },
        HouseAppearance::default(),
        GoodsInventory::new(60),
    ));
}

fn displaced_fixture() -> Fixture {
    let mut f = fixture(1, 0);
    let people: Vec<_> = f
        .world
        .query::<(Entity, &PersonId)>()
        .iter(&f.world)
        .map(|(entity, _)| entity)
        .collect();
    for person in people {
        f.world.entity_mut(person).remove::<LivesAt>();
    }
    f.world
        .entity_mut(f.homes[0])
        .remove::<OccupiedByHousehold>();
    f.world
        .entity_mut(f.homes[0])
        .insert((Household::default(), GoodsInventory::new(80)));
    *f.world.get_mut::<HouseholdMembers>(f.groups[0]).unwrap() = HouseholdMembers {
        resident_ids: (1..=8).map(PersonId).collect(),
        settlement: SettlementId(1),
        dwelling: None,
    };
    f.world
        .get_mut::<HouseholdEconomy>(f.groups[0])
        .unwrap()
        .pennies = 2550;
    for id in 5..=8 {
        f.world.spawn((
            PersonId(id),
            CharacterName("Displaced".into()),
            HouseholdMember(HouseholdId(1)),
            ResidentOf(SettlementId(1)),
            VillagerIntent::Resident { settlement: f.hall },
            PlayerPosition(Vec3::new(100.0, 0.0, 100.0)),
        ));
    }
    f.world.get_mut::<Settlement>(f.hall).unwrap().residents = 8;
    f
}

#[test]
fn displaced_owner_extends_for_own_eight_member_group_without_splitting() {
    use bevy::ecs::system::RunSystemOnce;
    let mut f = displaced_fixture();
    review(&mut f, 1);
    assert_eq!(pending(&f), 0);
    review(&mut f, 2);
    assert_eq!(pending(&f), 1);
    assert_eq!(
        f.world.get::<Wallet>(f.owners[0]).unwrap().balance(),
        3000 - super::super::required_escrow_pennies()
    );
    assert_eq!(
        f.world
            .get::<HouseholdEconomy>(f.groups[0])
            .unwrap()
            .pennies,
        2550
    );
    assert_eq!(
        f.world
            .get::<HouseholdMembers>(f.groups[0])
            .unwrap()
            .dwelling,
        None
    );
    // The normal membership pass, awakened by the completed appearance,
    // admits the intact group. Neither escrow nor relocation spends or
    // transports the reserved domestic money or the physical pantry.
    f.world
        .get_mut::<HouseAppearance>(f.homes[0])
        .unwrap()
        .level = HouseLevel::UpperStorey;
    f.world
        .init_resource::<crate::world::identity::WorldIdAllocator>();
    f.world
        .run_system_once(crate::world::village::assign_households)
        .unwrap();
    assert_eq!(
        f.world.get::<HouseholdId>(f.groups[0]),
        Some(&HouseholdId(1))
    );
    assert_eq!(
        f.world
            .get::<HouseholdMembers>(f.groups[0])
            .unwrap()
            .dwelling,
        Some(BuildingId(1))
    );
    assert_eq!(
        f.world
            .get::<Household>(f.homes[0])
            .unwrap()
            .resident_ids
            .len(),
        8
    );
    assert_eq!(
        f.world
            .get::<HouseholdEconomy>(f.groups[0])
            .unwrap()
            .pennies,
        2550
    );
    assert_eq!(
        f.world
            .get::<GoodsInventory>(f.homes[0])
            .unwrap()
            .used_bulk(),
        0
    );
}

#[test]
fn displaced_group_uses_adequate_existing_or_pending_home_before_extension() {
    for pending_upgrade in [false, true] {
        let mut f = displaced_fixture();
        let appearance = HouseAppearance {
            line: HouseLine::Cabin,
            level: if pending_upgrade {
                HouseLevel::Ground
            } else {
                HouseLevel::UpperStorey
            },
        };
        f.world.spawn((
            BuildingId(2),
            BuildingOf(SettlementId(1)),
            appearance,
            Household::default(),
        ));
        if pending_upgrade {
            f.world.spawn((
                BuildingOf(SettlementId(1)),
                HouseUpgradeWorksite {
                    house: BuildingId(2),
                    owner: PersonId(9),
                    target: HouseAppearance {
                        level: HouseLevel::UpperStorey,
                        ..appearance
                    },
                    wood_required: shared::components::HOUSE_UPGRADE_WOOD_REQUIRED,
                },
            ));
        }
        review(&mut f, 1);
        review(&mut f, 2);
        assert_eq!(pending(&f), 0);
        assert!(
            !f.world.resource::<HouseUpgradeDecisions>().towns[&SettlementId(1)]
                .unplaced_groups
                .contains(&HouseholdId(1))
        );
    }
}

#[test]
fn funded_resident_owner_invests_after_two_days_without_raiding_necessities() {
    let mut f = fixture(1, 2);
    review(&mut f, 1);
    assert_eq!(pending(&f), 0);
    // Ordinary expenditure is allowed while both savings observations
    // independently cover escrow and the protected personal reserve.
    f.world.get_mut::<Wallet>(f.owners[0]).unwrap().debit(10);
    review(&mut f, 2);
    assert_eq!(pending(&f), 1);
    assert_eq!(
        f.world.get::<Wallet>(f.owners[0]).unwrap().balance(),
        2990 - super::super::required_escrow_pennies()
    );
    assert_eq!(
        f.world
            .get::<HouseholdEconomy>(f.groups[0])
            .unwrap()
            .pennies,
        0
    );
    let pantry = f.world.get::<GoodsInventory>(f.homes[0]).unwrap();
    assert_eq!(
        (pantry.amount(Good::Bread), pantry.amount(Good::Wood)),
        (12, 2)
    );
    assert_eq!(
        f.world.get::<HouseAppearance>(f.homes[0]).unwrap().level,
        HouseLevel::Ground
    );
}

#[test]
fn existing_homeless_group_is_not_misclassified_as_future_lodgers() {
    let mut f = fixture(2, 2);
    f.world
        .get_mut::<HouseAppearance>(f.homes[1])
        .unwrap()
        .level = HouseLevel::UpperStorey;
    // Four waiting people cannot merge into the second household, but
    // its four spare beds already suffice for the two incoming newcomers.
    let members: Vec<_> = (20..24).map(PersonId).collect();
    f.world.spawn((
        HouseholdId(50),
        HouseholdMembers {
            resident_ids: members.clone(),
            settlement: SettlementId(1),
            dwelling: None,
        },
        HouseholdEconomy::default(),
    ));
    for id in members {
        f.world.spawn((
            id,
            HouseholdMember(HouseholdId(50)),
            VillagerIntent::Resident { settlement: f.hall },
        ));
    }
    f.world.get_mut::<Settlement>(f.hall).unwrap().residents += 4;
    review(&mut f, 1);
    review(&mut f, 2);
    let town = &f.world.resource::<HouseUpgradeDecisions>().towns[&SettlementId(1)];
    assert_eq!(
        (town.beds, town.admission_beds, town.expected_arrivals),
        (12, 4, 2)
    );
    assert_eq!(pending(&f), 0);
}

#[test]
fn ordinary_pending_house_prevents_duplicate_capacity_without_building_of() {
    let mut f = fixture(1, 2);
    pending_house(&mut f);
    review(&mut f, 1);
    review(&mut f, 2);
    let town = &f.world.resource::<HouseUpgradeDecisions>().towns[&SettlementId(1)];
    assert_eq!(
        (town.beds, town.pending_beds, town.admission_beds),
        (4, 4, 4)
    );
    assert_eq!(pending(&f), 0);
}

#[test]
fn reserve_funding_covers_food_and_fuel_together_before_owner_investment() {
    let mut f = fixture(1, 2);
    f.world
        .get_mut::<GoodsInventory>(f.homes[0])
        .unwrap()
        .remove(Good::Wood, 2);
    f.world
        .get_mut::<GoodsInventory>(f.homes[0])
        .unwrap()
        .remove(Good::Bread, 8);
    // Today's meals remain physical. Tomorrow's food and fuel must share
    // one funded communal budget, not each count the same cash twice.
    review(&mut f, 1);
    review(&mut f, 2);
    assert_eq!(pending(&f), 0);
    f.world
        .get_mut::<HouseholdEconomy>(f.groups[0])
        .unwrap()
        .pennies = 800;
    review(&mut f, 3);
    assert_eq!(pending(&f), 0);
    review(&mut f, 4);
    assert_eq!(pending(&f), 0);
    f.world
        .get_mut::<HouseholdEconomy>(f.groups[0])
        .unwrap()
        .pennies = 900;
    review(&mut f, 5);
    assert_eq!(pending(&f), 0);
    review(&mut f, 6);
    assert_eq!(pending(&f), 1);
    assert_eq!(
        f.world
            .get::<HouseholdEconomy>(f.groups[0])
            .unwrap()
            .pennies,
        900
    );
}

#[test]
fn cheap_materials_do_not_turn_refundable_escrow_into_a_fictitious_cost() {
    let mut f = fixture(1, 2);
    f.world.get_mut::<MootMarket>(f.hall).unwrap().reprice(
        MarketSeller::Treasury(SettlementId(1)),
        Good::Wood,
        10,
    );
    review(&mut f, 1);
    review(&mut f, 2);
    assert_eq!(pending(&f), 1);
}

#[test]
fn quiet_village_and_skipped_days_do_not_invent_sustained_growth() {
    let mut quiet = fixture(1, 0);
    for day in 1..=4 {
        review(&mut quiet, day);
    }
    assert_eq!(pending(&quiet), 0);
    let mut growing = fixture(1, 2);
    review(&mut growing, 1);
    review(&mut growing, 8);
    assert_eq!(pending(&growing), 0);
    review(&mut growing, 9);
    assert_eq!(pending(&growing), 1);
}

#[test]
fn new_owner_must_establish_their_own_savings_and_need_history() {
    let mut f = fixture(1, 2);
    review(&mut f, 1);
    f.world.get_mut::<OwnedBy>(f.homes[0]).unwrap().0 = PersonId(2);
    review(&mut f, 2);
    assert_eq!(pending(&f), 0);
    review(&mut f, 3);
    assert_eq!(pending(&f), 1);
    assert_eq!(f.world.get::<Wallet>(f.owners[0]).unwrap().balance(), 3000);
}

#[test]
fn real_snapshot_queues_ready_houses_but_attempts_only_one_per_tick() {
    let mut f = fixture(2, 8);
    review(&mut f, 1);
    review(&mut f, 2);
    assert_eq!(pending(&f), 1);
    assert_eq!(f.world.resource::<HouseUpgradeDecisions>().queue.len(), 1);
    let escrow = f
        .world
        .resource::<super::super::HouseUpgradeProjects>()
        .total_escrow_pennies();
    review(&mut f, 2);
    assert!(f.world.resource::<HouseUpgradeDecisions>().queue.is_empty());
    assert_eq!(
        f.world
            .resource::<super::super::HouseUpgradeProjects>()
            .total_escrow_pennies(),
        escrow
    );
}

#[test]
#[ignore = "runtime benchmark: run optimized with --ignored --nocapture"]
fn decision_snapshot_and_idle_ticks_at_5000_people() {
    use std::time::Instant;
    let mut f = fixture(1250, 0);
    assert_eq!(f.world.query::<&PersonId>().iter(&f.world).count(), 5000);
    review(&mut f, 0);
    let entities = f.world.entities().len();
    let mut daily = Vec::new();
    for day in 1..=30 {
        let started = Instant::now();
        review(&mut f, day);
        daily.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let idle = Instant::now();
    for _ in 0..10000 {
        review_house_upgrades(&mut f.world);
    }
    let idle_us = idle.elapsed().as_secs_f64() * 1_000_000.0 / 10000.0;
    daily.sort_by(f64::total_cmp);
    let p95 = daily[(daily.len() * 95 / 100).min(daily.len() - 1)];
    println!(
        "house upgrade decision 5000 people/1250 homes: daily p95={p95:.3}ms idle={idle_us:.3}us"
    );
    assert_eq!(f.world.entities().len(), entities);
    assert_eq!(
        f.world
            .resource::<HouseUpgradeDecisions>()
            .observations
            .len(),
        1250
    );
    assert_eq!(pending(&f), 0);
    assert!(
        p95 < 16.0,
        "daily snapshot exceeded a 16ms guardrail: {p95:.3}ms"
    );
    assert!(
        idle_us < 100.0,
        "idle decision exceeds 0.1ms per tick: {idle_us:.3}us"
    );
}
