use super::*;

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<crate::world::identity::WorldIdAllocator>()
        .add_systems(PreUpdate, crate::world::identity::assign_stable_world_ids)
        .add_systems(
            Update,
            (
                ensure_households,
                ensure_house_appearances,
                assign_households,
            )
                .chain(),
        );
    app
}

fn settlement(app: &mut App, id: u64) -> Entity {
    app.world_mut()
        .spawn((
            SettlementId(id),
            Settlement {
                name: format!("Town {id}"),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(Vec3::new(id as f32 * 100., 0., 0.)),
        ))
        .id()
}

fn house(app: &mut App, id: u64, place: u64) -> Entity {
    app.world_mut()
        .spawn((
            BuildingId(id),
            BuildingOf(SettlementId(place)),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: format!("Town {place}"),
                owner: None,
                quality: 0.5,
                workers: vec![],
            },
            PlayerPosition(Vec3::new(id as f32 * 10., 0., 0.)),
            GoodsInventory::new(200),
        ))
        .id()
}

fn person(app: &mut App, id: u64, place: u64, hall: Entity) -> Entity {
    app.world_mut()
        .spawn((
            PersonId(id),
            CharacterName("Robin".into()),
            ResidentOf(SettlementId(place)),
            VillagerIntent::Resident { settlement: hall },
            PlayerPosition(Vec3::new(id as f32, 0., 0.)),
        ))
        .id()
}

fn group(app: &mut App, person: Entity) -> (Entity, HouseholdId) {
    let id = app.world().get::<HouseholdMember>(person).unwrap().0;
    let entity = app
        .world_mut()
        .query::<(Entity, &HouseholdId)>()
        .iter(app.world())
        .find_map(|(entity, candidate)| (*candidate == id).then_some(entity))
        .unwrap();
    (entity, id)
}

#[test]
fn canonical_appearance_preserves_authored_upper_storeys_and_explicit_state() {
    use shared::building::{BuildingType, PlacedBuilding};
    let mut app = App::new();
    app.add_systems(
        Update,
        (ensure_households, ensure_house_appearances).chain(),
    );
    let mut authored = Vec::new();
    for (id, art) in [
        BuildingType::LogCabin,
        BuildingType::CabinL2,
        BuildingType::LongCabin,
        BuildingType::LongCabinL2,
    ]
    .into_iter()
    .enumerate()
    {
        let home = house(&mut app, id as u64 + 1, 1);
        app.world_mut().entity_mut(home).insert(PlacedBuilding {
            building_type: art,
            rotation: 0.0,
        });
        authored.push((home, art));
    }
    let explicit = house(&mut app, 5, 1);
    app.world_mut().entity_mut(explicit).insert((
        HouseAppearance::default(),
        PlacedBuilding {
            building_type: BuildingType::LongCabinL2,
            rotation: 0.0,
        },
    ));
    let fallback = house(&mut app, 6, 1);
    app.update();
    for (home, art) in authored {
        assert_eq!(
            app.world().get::<HouseAppearance>(home).copied(),
            HouseAppearance::from_building_type(art)
        );
    }
    assert_eq!(
        app.world().get::<HouseAppearance>(explicit),
        Some(&HouseAppearance::default())
    );
    assert_eq!(
        app.world()
            .get::<HouseAppearance>(fallback)
            .unwrap()
            .level
            .housing_capacity(),
        4
    );
}

#[test]
fn upper_storey_adds_four_beds_without_replacing_or_merging_households() {
    use shared::components::{HouseLevel, HouseLine};
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    app.world_mut().entity_mut(home).insert(HouseAppearance {
        line: HouseLine::LongCabin,
        level: HouseLevel::Ground,
    });
    let mut people: Vec<_> = (1..=4).map(|id| person(&mut app, id, 1, hall)).collect();
    app.update();
    people.extend((5..=8).map(|id| person(&mut app, id, 1, hall)));
    app.update();
    let (housed, household_id) = group(&mut app, people[0]);
    let (displaced, displaced_id) = group(&mut app, people[4]);
    assert_ne!(household_id, displaced_id);
    app.world_mut()
        .get_mut::<HouseholdEconomy>(housed)
        .unwrap()
        .pennies = 983;
    app.world_mut()
        .get_mut::<HouseholdEconomy>(displaced)
        .unwrap()
        .pennies = 541;
    app.world_mut()
        .get_mut::<GoodsInventory>(home)
        .unwrap()
        .add(Good::Bread, 7);
    app.update();
    app.world_mut()
        .get_mut::<HouseAppearance>(home)
        .unwrap()
        .level = HouseLevel::UpperStorey;
    app.update();
    assert_eq!(
        app.world()
            .get::<Household>(home)
            .unwrap()
            .resident_ids
            .len(),
        4
    );
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(displaced)
            .unwrap()
            .dwelling,
        None
    );
    for id in 9..=12 {
        let newcomer = person(&mut app, id, 1, hall);
        app.update();
        assert_eq!(group(&mut app, newcomer), (housed, household_id));
        assert_eq!(
            app.world().get::<LivesAt>(newcomer),
            Some(&LivesAt(BuildingId(1)))
        );
    }
    assert_eq!(
        app.world()
            .get::<Household>(home)
            .unwrap()
            .resident_ids
            .len(),
        8
    );
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(housed)
            .unwrap()
            .resident_ids
            .len(),
        8
    );
    assert_eq!(group(&mut app, people[0]), (housed, household_id));
    assert_eq!(group(&mut app, people[4]), (displaced, displaced_id));
    assert_eq!(
        app.world().get::<HouseholdEconomy>(housed).unwrap().pennies,
        983
    );
    assert_eq!(
        app.world()
            .get::<HouseholdEconomy>(displaced)
            .unwrap()
            .pennies,
        541
    );
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(home)
            .unwrap()
            .amount(Good::Bread),
        7
    );
    assert_eq!(
        app.world().get::<OccupiedByHousehold>(home),
        Some(&OccupiedByHousehold(household_id))
    );
}

#[test]
fn completed_capacity_change_alone_rehouses_a_large_displaced_group() {
    use shared::components::HouseLevel;
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    app.world_mut()
        .entity_mut(home)
        .insert(HouseAppearance::default());
    let household_id = HouseholdId(44);
    let members: Vec<_> = (1..=8).map(PersonId).collect();
    let household = app
        .world_mut()
        .spawn((
            household_id,
            HouseholdMembers {
                resident_ids: members,
                settlement: SettlementId(1),
                dwelling: None,
            },
            HouseholdEconomy {
                pennies: 731,
                ..default()
            },
        ))
        .id();
    for id in 1..=8 {
        let person = person(&mut app, id, 1, hall);
        app.world_mut()
            .entity_mut(person)
            .insert(HouseholdMember(household_id));
    }
    app.update();
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(household)
            .unwrap()
            .dwelling,
        None
    );
    app.world_mut()
        .get_mut::<HouseAppearance>(home)
        .unwrap()
        .level = HouseLevel::UpperStorey;
    app.update();
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(household)
            .unwrap()
            .dwelling,
        Some(BuildingId(1))
    );
    assert_eq!(
        app.world()
            .get::<Household>(home)
            .unwrap()
            .resident_ids
            .len(),
        8
    );
    assert_eq!(
        app.world()
            .get::<HouseholdEconomy>(household)
            .unwrap()
            .pennies,
        731
    );
}

#[test]
fn relocation_keeps_group_and_purse_but_never_moves_physical_stock() {
    let mut app = app();
    let first_hall = settlement(&mut app, 1);
    let second_hall = settlement(&mut app, 2);
    let first_house = house(&mut app, 1, 1);
    let second_house = house(&mut app, 2, 2);
    let first = person(&mut app, 1, 1, first_hall);
    let second = person(&mut app, 2, 1, first_hall);
    app.world_mut()
        .entity_mut(first_house)
        .insert(HouseholdEconomy {
            pennies: 735,
            pantry_target_days: 5,
            ..default()
        });
    {
        let mut stock = app
            .world_mut()
            .get_mut::<GoodsInventory>(first_house)
            .unwrap();
        assert_eq!(stock.add(Good::Food, 9), 9);
        assert_eq!(stock.add(Good::Wood, 4), 4);
    }
    app.update();
    let (entity, id) = group(&mut app, first);
    assert_eq!(
        app.world().get::<HouseholdMember>(second),
        Some(&HouseholdMember(id))
    );
    assert!(app.world().get::<HouseholdEconomy>(first_house).is_none());
    assert_eq!(
        app.world().get::<HouseholdEconomy>(entity).unwrap().pennies,
        735
    );
    assert_eq!(
        app.world()
            .get::<HouseholdEconomy>(entity)
            .unwrap()
            .pantry_target_days,
        5
    );

    for person in [first, second] {
        app.world_mut().entity_mut(person).insert((
            ResidentOf(SettlementId(2)),
            VillagerIntent::Resident {
                settlement: second_hall,
            },
        ));
    }
    app.update();
    app.update(); // Exact adoption and relocation cannot run the transfer twice.
    assert_eq!(group(&mut app, first), (entity, id));
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(entity)
            .unwrap()
            .dwelling,
        Some(BuildingId(2))
    );
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(entity)
            .unwrap()
            .settlement,
        SettlementId(2)
    );
    assert_eq!(
        app.world().get::<HouseholdEconomy>(entity).unwrap().pennies,
        735
    );
    assert_eq!(
        app.world().get::<OccupiedByHousehold>(second_house),
        Some(&OccupiedByHousehold(id))
    );
    assert!(app
        .world()
        .get::<OccupiedByHousehold>(first_house)
        .is_none());
    assert!(app
        .world()
        .get::<Household>(first_house)
        .unwrap()
        .resident_ids
        .is_empty());
    let stock = app.world().get::<GoodsInventory>(first_house).unwrap();
    assert_eq!((stock.amount(Good::Food), stock.amount(Good::Wood)), (9, 4));
    assert_eq!(
        app.world()
            .get::<GoodsInventory>(second_house)
            .unwrap()
            .edible_amount(),
        0
    );
}

#[test]
fn demolished_home_does_not_disband_or_impoverish_its_household() {
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    let first = person(&mut app, 1, 1, hall);
    let second = person(&mut app, 2, 1, hall);
    app.update();
    let (entity, id) = group(&mut app, first);
    app.world_mut()
        .get_mut::<HouseholdEconomy>(entity)
        .unwrap()
        .pennies = 913;
    app.world_mut().despawn(home);
    app.update();
    assert_eq!(group(&mut app, first), (entity, id));
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(entity)
            .unwrap()
            .dwelling,
        None
    );
    assert_eq!(
        app.world().get::<HouseholdEconomy>(entity).unwrap().pennies,
        913
    );
    for person in [first, second] {
        assert!(app.world().get::<HomeAssignment>(person).is_none());
        assert!(app.world().get::<LivesAt>(person).is_none());
    }
    let replacement = house(&mut app, 2, 1);
    app.update();
    assert_eq!(group(&mut app, first), (entity, id));
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(entity)
            .unwrap()
            .dwelling,
        Some(BuildingId(2))
    );
    assert_eq!(
        app.world().get::<HouseholdEconomy>(entity).unwrap().pennies,
        913
    );
    assert_eq!(
        app.world()
            .get::<Household>(replacement)
            .unwrap()
            .resident_ids
            .len(),
        2
    );
}

#[test]
fn losing_a_sleeping_residents_home_releases_the_hidden_body_and_door_routine() {
    let mut app = app();
    app.add_systems(Update, run_household_schedules.after(assign_households));
    app.world_mut().spawn(WorldTime::new_default());
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    app.world_mut().entity_mut(home).insert(PlayerRotation(0.0));
    let resident = person(&mut app, 1, 1, hall);
    app.world_mut().entity_mut(resident).insert((
        CharacterKind::Villager,
        PlayerRotation(0.0),
        CharacterActivity::Idle,
    ));
    app.update();
    let home_position = app.world().get::<PlayerPosition>(home).unwrap().0;
    app.world_mut().entity_mut(resident).insert((
        CharacterActivity::Indoors,
        HomeRoutine {
            home,
            phase: HomePhase::Sleeping,
            failed_routes: 0,
        },
        BuildingDoorUse {
            building: home_position,
        },
        MoveTarget(home_position),
        NavigationRouteFailed {
            goal: home_position,
        },
    ));
    app.world_mut().despawn(home);
    app.update();
    let resident = app.world().entity(resident);
    assert!(resident.get::<HomeAssignment>().is_none());
    assert!(resident.get::<HomeRoutine>().is_none());
    assert!(resident.get::<BuildingDoorUse>().is_none());
    assert!(resident.get::<MoveTarget>().is_none());
    assert!(resident.get::<NavigationRouteFailed>().is_none());
    assert_eq!(
        resident.get::<CharacterActivity>(),
        Some(&CharacterActivity::Idle)
    );
    assert!(resident.get::<HouseholdMember>().is_some());
}

#[test]
fn sequential_unhoused_arrivals_share_capacity_then_use_two_houses_for_five() {
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let first = person(&mut app, 1, 1, hall);
    app.update();
    let (first_group, first_id) = group(&mut app, first);
    for id in 2..=5 {
        person(&mut app, id, 1, hall);
        app.update();
    }
    assert_eq!(
        app.world_mut()
            .query::<&HouseholdId>()
            .iter(app.world())
            .count(),
        2
    );
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(first_group)
            .unwrap()
            .resident_ids
            .len(),
        4
    );
    let homes = [house(&mut app, 1, 1), house(&mut app, 2, 1)];
    app.update();
    assert_eq!(group(&mut app, first), (first_group, first_id));
    assert_eq!(
        homes
            .iter()
            .map(|home| app
                .world()
                .get::<Household>(*home)
                .unwrap()
                .resident_ids
                .len())
            .sum::<usize>(),
        5
    );
    assert_eq!(
        app.world_mut()
            .query::<&HomeAssignment>()
            .iter(app.world())
            .count(),
        5
    );
}

#[test]
fn newcomer_uses_existing_spare_bed_without_replacing_household_identity() {
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    let first = person(&mut app, 1, 1, hall);
    app.update();
    let (entity, id) = group(&mut app, first);
    for number in 2..=4 {
        let newcomer = person(&mut app, number, 1, hall);
        app.update();
        assert_eq!(
            app.world().get::<HouseholdMember>(newcomer),
            Some(&HouseholdMember(id))
        );
    }
    assert_eq!(group(&mut app, first), (entity, id));
    assert_eq!(
        app.world()
            .get::<Household>(home)
            .unwrap()
            .resident_ids
            .len(),
        4
    );
    assert_eq!(
        app.world_mut()
            .query::<&HouseholdId>()
            .iter(app.world())
            .count(),
        1
    );
}

#[test]
fn absent_member_keeps_group_but_does_not_occupy_a_bed() {
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    let first = person(&mut app, 1, 1, hall);
    let second = person(&mut app, 2, 1, hall);
    app.update();
    let (entity, id) = group(&mut app, first);
    app.world_mut()
        .entity_mut(second)
        .insert(VillagerIntent::Travelling { settlement: hall });
    app.update();
    assert_eq!(
        app.world().get::<HouseholdMember>(second),
        Some(&HouseholdMember(id))
    );
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(entity)
            .unwrap()
            .resident_ids
            .len(),
        2
    );
    assert_eq!(
        app.world().get::<Household>(home).unwrap().resident_ids,
        vec![PersonId(1)]
    );
    assert!(app.world().get::<HomeAssignment>(second).is_none());
}

#[test]
fn conscription_keeps_domestic_identity_and_purse_without_assigning_a_bed() {
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let home = house(&mut app, 1, 1);
    let recruit = person(&mut app, 1, 1, hall);
    app.update();
    let (entity, id) = group(&mut app, recruit);
    app.world_mut()
        .get_mut::<HouseholdEconomy>(entity)
        .unwrap()
        .pennies = 421;
    {
        let mut commands = app.world_mut().commands();
        crate::player::army::discharge_from_village_life(&mut commands.entity(recruit));
    }
    app.world_mut().flush();
    app.update();
    assert_eq!(group(&mut app, recruit), (entity, id));
    assert_eq!(
        app.world().get::<HouseholdEconomy>(entity).unwrap().pennies,
        421
    );
    assert_eq!(
        app.world()
            .get::<HouseholdMembers>(entity)
            .unwrap()
            .resident_ids,
        vec![PersonId(1)]
    );
    assert!(app.world().get::<HomeAssignment>(recruit).is_none());
    assert!(app
        .world()
        .get::<Household>(home)
        .unwrap()
        .resident_ids
        .is_empty());
    assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 0);
}

#[derive(Resource, Default)]
struct ReplicatedChanges {
    groups: usize,
    houses: usize,
    homes: usize,
}

#[test]
fn settled_and_homeless_groups_do_not_dirty_replicated_relationships() {
    let mut app = app();
    app.init_resource::<ReplicatedChanges>().add_systems(
        PostUpdate,
        |groups: Query<(), Changed<HouseholdMembers>>,
         houses: Query<(), Changed<Household>>,
         homes: Query<(), Changed<LivesAt>>,
         mut counts: ResMut<ReplicatedChanges>| {
            counts.groups = groups.iter().count();
            counts.houses = houses.iter().count();
            counts.homes = homes.iter().count();
        },
    );
    let hall = settlement(&mut app, 1);
    house(&mut app, 1, 1);
    for id in 1..=5 {
        person(&mut app, id, 1, hall);
    }
    app.update();
    assert_eq!(app.world().resource::<ReplicatedChanges>().groups, 2);
    for _ in 0..3 {
        app.update();
        let changes = app.world().resource::<ReplicatedChanges>();
        assert_eq!((changes.groups, changes.houses, changes.homes), (0, 0, 0));
    }
}

#[test]
#[ignore = "runtime benchmark: run optimized with --ignored --nocapture"]
fn appearance_change_reassigns_5000_people_without_replacing_households() {
    use std::time::Instant;
    let mut app = app();
    let hall = settlement(&mut app, 1);
    let mut homes = Vec::new();
    let mut accounts = Vec::new();
    for id in 1..=1250u64 {
        let home = house(&mut app, id, 1);
        homes.push(home);
        app.world_mut()
            .entity_mut(home)
            .insert(HouseAppearance::default());
        let household = HouseholdId(id);
        let members: Vec<_> = (id * 4 - 3..=id * 4).map(PersonId).collect();
        for member in &members {
            let person = person(&mut app, member.0, 1, hall);
            app.world_mut()
                .entity_mut(person)
                .insert((HouseholdMember(household), LivesAt(BuildingId(id))));
        }
        accounts.push(
            app.world_mut()
                .spawn((
                    household,
                    HouseholdMembers {
                        resident_ids: members,
                        settlement: SettlementId(1),
                        dwelling: Some(BuildingId(id)),
                    },
                    HouseholdEconomy {
                        pennies: id + 1000,
                        ..default()
                    },
                ))
                .id(),
        );
    }
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world_mut()
            .query::<&PersonId>()
            .iter(app.world())
            .count(),
        5000
    );
    let mut durations = Vec::new();
    for sample in 0..30 {
        // The real changed-component event wakes the complete assignment pass;
        // this includes identity, appearance and deferred membership commands.
        app.world_mut()
            .get_mut::<HouseAppearance>(homes[0])
            .unwrap()
            .level = if sample % 2 == 0 {
            shared::components::HouseLevel::UpperStorey
        } else {
            shared::components::HouseLevel::Ground
        };
        let start = Instant::now();
        app.update();
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(f64::total_cmp);
    let p95 = durations[durations.len() * 95 / 100];
    println!("house appearance assignment 5000 people/1250 homes: event p95={p95:.3}ms");
    for (index, &entity) in accounts.iter().enumerate() {
        let id = index as u64 + 1;
        assert_eq!(
            app.world().get::<HouseholdId>(entity),
            Some(&HouseholdId(id))
        );
        assert_eq!(
            app.world().get::<HouseholdEconomy>(entity).unwrap().pennies,
            id + 1000
        );
        assert_eq!(
            app.world()
                .get::<HouseholdMembers>(entity)
                .unwrap()
                .resident_ids
                .len(),
            4
        );
        assert_eq!(
            app.world()
                .get::<Household>(homes[index])
                .unwrap()
                .resident_ids
                .len(),
            4
        );
    }
    assert!(
        p95 < 16.0,
        "appearance assignment exceeded 16ms guardrail: {p95:.3}ms"
    );
}

#[test]
fn displaced_owner_extension_reserves_the_home_through_completion_or_cancellation() {
    use shared::components::{
        HouseLevel, HouseUpgradeWorksite, OwnedBy, HOUSE_UPGRADE_WOOD_REQUIRED,
    };
    for complete in [true, false] {
        let mut app = app();
        let hall = settlement(&mut app, 1);
        let home = house(&mut app, 1, 1);
        app.world_mut()
            .entity_mut(home)
            .insert((HouseAppearance::default(), OwnedBy(PersonId(9))));
        let mut owner_account = None;
        // The other existing group sorts before the owner's group, so title
        // priority must decide the completed home rather than iteration order.
        for (group_id, ids) in [
            (1u64, (1..=4).collect::<Vec<_>>()),
            (20, (9..=16).collect()),
        ] {
            for &id in &ids {
                let person = person(&mut app, id, 1, hall);
                app.world_mut()
                    .entity_mut(person)
                    .insert(HouseholdMember(HouseholdId(group_id)));
            }
            let entity = app
                .world_mut()
                .spawn((
                    HouseholdId(group_id),
                    HouseholdMembers {
                        resident_ids: ids.into_iter().map(PersonId).collect(),
                        settlement: SettlementId(1),
                        dwelling: None,
                    },
                    HouseholdEconomy {
                        pennies: 983,
                        ..default()
                    },
                ))
                .id();
            if group_id == 20 {
                owner_account = Some(entity);
            }
        }
        let site = app
            .world_mut()
            .spawn(HouseUpgradeWorksite {
                house: BuildingId(1),
                owner: PersonId(9),
                target: HouseAppearance {
                    level: HouseLevel::UpperStorey,
                    ..default()
                },
                wood_required: HOUSE_UPGRADE_WOOD_REQUIRED,
            })
            .id();
        app.update();
        for id in 17..=20 {
            person(&mut app, id, 1, hall);
        }
        for _ in 0..3 {
            app.update();
        }
        assert!(app
            .world()
            .get::<Household>(home)
            .unwrap()
            .resident_ids
            .is_empty());
        app.world_mut().despawn(site);
        if complete {
            app.world_mut()
                .get_mut::<HouseAppearance>(home)
                .unwrap()
                .level = HouseLevel::UpperStorey;
        }
        app.update();
        let expected = if complete {
            HouseholdId(20)
        } else {
            HouseholdId(1)
        };
        assert_eq!(
            app.world().get::<OccupiedByHousehold>(home),
            Some(&OccupiedByHousehold(expected))
        );
        let account = owner_account.unwrap();
        assert_eq!(
            app.world()
                .get::<HouseholdEconomy>(account)
                .unwrap()
                .pennies,
            983
        );
        assert_eq!(
            app.world()
                .get::<HouseholdMembers>(account)
                .unwrap()
                .resident_ids
                .len(),
            8
        );
        assert_eq!(
            app.world()
                .get::<HouseholdMembers>(account)
                .unwrap()
                .dwelling,
            complete.then_some(BuildingId(1))
        );
    }
}
