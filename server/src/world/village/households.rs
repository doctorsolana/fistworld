//! Household membership, shared necessities and visible home routines.
//!
//! Durable PersonId and BuildingId relationships are authoritative; readable
//! resident names remain derived inspection data only.

use super::*;

/// Give every completed cabin a bounded, inspectable household roster.
pub fn ensure_households(
    mut commands: Commands,
    houses: Query<(
        Entity,
        &SettlementBuilding,
        Option<&Household>,
        Option<&HouseholdEconomy>,
    )>,
) {
    for (entity, building, household, economy) in houses.iter() {
        if building.kind == SettlementBuildingKind::House {
            let mut entity_commands = commands.entity(entity);
            if household.is_none() {
                entity_commands.insert(Household::default());
            }
            if economy.is_none() {
                entity_commands.insert(HouseholdEconomy::default());
            }
        }
    }
}

/// Fund each house's shared necessities purse and restock its bounded pantry.
///
/// This is intentionally one decision per household per world day. Tactical
/// households send the named shopper on a visible trip; strategic settlements
/// settle the same purchase directly so 5,000 people do not need 5,000 paths.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_household_budgets_and_pantries(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    regions: Option<Res<RegionRegistry>>,
    mut halls: Query<
        (
            Entity,
            &Settlement,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut houses: Query<
        (
            Entity,
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &Household,
            &mut HouseholdEconomy,
            &mut GoodsInventory,
        ),
        Without<CharacterKind>,
    >,
    mut residents: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &mut Wallet,
        Option<&WorkStatus>,
        Option<&HomeAssignment>,
        Option<&RegionCoord>,
        Option<&HouseholdShoppingRoutine>,
    )>,
    busy: Query<
        (),
        Or<(
            With<ConstructionMaterialRoutine>,
            With<RoadBuilderRoutine>,
            With<HomeRoutine>,
            With<WorkplaceDoorTransit>,
            With<MarketCollectionRoutine>,
        )>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let hall_by_id: HashMap<shared::components::SettlementId, Entity> = halls
        .iter()
        .map(|(entity, _, settlement_id, ..)| (*settlement_id, entity))
        .collect();
    let resident_by_id: HashMap<shared::components::PersonId, Entity> = residents
        .iter()
        .map(|(entity, person_id, ..)| (*person_id, entity))
        .collect();
    for (house_entity, building, building_of, household, mut economy, mut pantry) in
        houses.iter_mut()
    {
        if building.kind != SettlementBuildingKind::House || economy.last_budget_day == day {
            continue;
        }
        let mut members: Vec<(shared::components::PersonId, String, WorkStatus)> = household
            .resident_ids
            .iter()
            .filter_map(|person_id| {
                let entity = resident_by_id.get(person_id).copied()?;
                let Ok((_, _, name, _, status, assignment, _, _)) = residents.get(entity) else {
                    return None;
                };
                assignment
                    .is_some_and(|assignment| assignment.home == house_entity)
                    .then_some((
                        *person_id,
                        name.0.clone(),
                        status.copied().unwrap_or_default(),
                    ))
            })
            .collect();
        members.sort_by(|a, b| {
            let rank = |status: WorkStatus| match status {
                WorkStatus::Chilling => 0,
                WorkStatus::LookingForWork => 1,
                WorkStatus::Employed => 2,
            };
            rank(a.2)
                .cmp(&rank(b.2))
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        economy.shopper = members.first().map(|(person_id, ..)| *person_id);
        if members.is_empty() {
            economy.last_budget_day = day;
            continue;
        }
        let target_units =
            (members.len() as u32).saturating_mul(u32::from(economy.pantry_target_days));
        let deficit = target_units.saturating_sub(pantry.edible_amount());
        if deficit == 0 {
            economy.last_budget_day = day;
            continue;
        }
        let Some(hall_entity) = hall_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let Ok((_, _, _, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(hall_entity)
        else {
            continue;
        };
        let estimated_unit_price = [Good::Food, Good::Wheat]
            .into_iter()
            .filter(|good| hall_store.amount(*good) > 0)
            .map(|good| market.pool(good).ask)
            .min()
            .unwrap_or(0);
        let wanted_budget = u64::from(deficit).saturating_mul(estimated_unit_price);
        let mut needed = wanted_budget.saturating_sub(economy.pennies);
        if needed > 0 {
            // All earners contribute toward the same concrete pantry target,
            // while retaining two coins as personal discretionary money.
            let mut contribution_order = members.clone();
            contribution_order.sort_by_key(|(person_id, ..)| *person_id);
            for (person_id, _, _) in contribution_order {
                let Some(member_entity) = resident_by_id.get(&person_id).copied() else {
                    continue;
                };
                let Ok((_, _, _, mut wallet, _, _, _, _)) = residents.get_mut(member_entity) else {
                    continue;
                };
                let available = wallet.balance().saturating_sub(2 * PENNIES_PER_COIN);
                let contribution = available.min(needed);
                if contribution > 0 && wallet.debit(contribution) {
                    economy.pennies = economy.pennies.saturating_add(contribution);
                    needed -= contribution;
                }
                if needed == 0 {
                    break;
                }
            }
        }

        let shopper_entity = economy
            .shopper
            .and_then(|shopper| resident_by_id.get(&shopper).copied());
        let tactical_shopper = shopper_entity.is_some_and(|shopper| {
            let Ok((_, _, _, _, _, _, region, routine)) = residents.get(shopper) else {
                return false;
            };
            routine.is_none()
                && busy.get(shopper).is_err()
                && region.is_some_and(|region| {
                    regions.as_ref().is_some_and(|registry| {
                        registry
                            .get(*region)
                            .is_some_and(|state| state.sim_level == SimLevel::Tactical)
                    })
                })
        });
        if tactical_shopper && clock.is_day() {
            if let Some(shopper) = shopper_entity {
                let entrance = SettlementBuildingKind::Hall.entrance_position(
                    hall_position.0,
                    hall_rotation.map_or(0.0, |rotation| rotation.0),
                );
                commands.entity(shopper).insert((
                    HouseholdShoppingRoutine {
                        home: house_entity,
                        hall: hall_entity,
                        phase: HouseholdShoppingPhase::GoingToMarket,
                    },
                    MoveTarget(entrance),
                ));
                economy.last_budget_day = day;
                continue;
            }
        }

        let mut remaining = deficit;
        for good in [Good::Food, Good::Wheat] {
            if remaining == 0 {
                break;
            }
            let room = pantry.free_bulk() / good.bulk_per_unit();
            let requested = remaining.min(room).min(hall_store.amount(good));
            let trade =
                market.sell_to_consumer(good, hall_store.amount(good), requested, economy.pennies);
            if trade.units == 0 {
                continue;
            }
            if economy.pennies < trade.pennies {
                continue;
            }
            economy.pennies -= trade.pennies;
            let moved = hall_store.transfer_to(&mut pantry, good, trade.units);
            debug_assert_eq!(moved, trade.units);
            remaining = remaining.saturating_sub(moved);
        }
        economy.last_budget_day = day;
    }
}

/// Animate the tactical half of household restocking. The shopper pays from
/// the house purse at the Moot, carries only what fits, and unloads at their
/// own cabin; off-screen households use the aggregate branch above.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_household_shopping(
    mut commands: Commands,
    mut halls: Query<
        (
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (
            With<Settlement>,
            Without<SettlementBuilding>,
            Without<CharacterKind>,
        ),
    >,
    mut houses: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &Household,
            &mut HouseholdEconomy,
            &mut GoodsInventory,
        ),
        Without<CharacterKind>,
    >,
    mut shoppers: Query<
        (
            Entity,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            &mut HouseholdShoppingRoutine,
            Option<&MoveTarget>,
            Option<&HomeRoutine>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    for (shopper, position, mut activity, mut carrier, mut routine, move_target, home) in
        shoppers.iter_mut()
    {
        if home.is_some() {
            continue;
        }
        let Ok((hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(routine.hall)
        else {
            commands
                .entity(shopper)
                .remove::<HouseholdShoppingRoutine>()
                .remove::<MoveTarget>();
            continue;
        };
        let Ok((building, home_position, home_rotation, household, mut economy, mut pantry)) =
            houses.get_mut(routine.home)
        else {
            commands
                .entity(shopper)
                .remove::<HouseholdShoppingRoutine>()
                .remove::<MoveTarget>();
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        let home_entrance = building
            .kind
            .entrance_position(home_position.0, home_rotation.0);
        *activity = CharacterActivity::Idle;
        match routine.phase {
            HouseholdShoppingPhase::GoingToMarket => {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, shopper, move_target, hall_entrance);
                    continue;
                }
                let target = (household.resident_ids.len() as u32)
                    .saturating_mul(u32::from(economy.pantry_target_days));
                let mut remaining = target.saturating_sub(pantry.edible_amount());
                for good in [Good::Food, Good::Wheat] {
                    if remaining == 0 {
                        break;
                    }
                    let requested = remaining
                        .min(carrier.free_bulk() / good.bulk_per_unit())
                        .min(hall_store.amount(good));
                    let trade = market.sell_to_consumer(
                        good,
                        hall_store.amount(good),
                        requested,
                        economy.pennies,
                    );
                    if trade.units == 0 || economy.pennies < trade.pennies {
                        continue;
                    }
                    economy.pennies -= trade.pennies;
                    let moved = hall_store.transfer_to(&mut carrier, good, trade.units);
                    debug_assert_eq!(moved, trade.units);
                    remaining = remaining.saturating_sub(moved);
                }
                if carrier.edible_amount() == 0 {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                commands.entity(shopper).insert(MoveTarget(home_entrance));
                routine.phase = HouseholdShoppingPhase::ReturningHome;
            }
            HouseholdShoppingPhase::ReturningHome => {
                if ground_distance(position.0, home_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, shopper, move_target, home_entrance);
                    continue;
                }
                for good in [Good::Food, Good::Wheat] {
                    carrier.transfer_to(&mut pantry, good, u32::MAX);
                }
                commands
                    .entity(shopper)
                    .remove::<HouseholdShoppingRoutine>()
                    .remove::<MoveTarget>();
            }
        }
    }
}

/// Assign every resident to one cabin in their settlement, up to its real bed
/// capacity. Nearest-first keeps a household spatially coherent without yet
/// inventing family relationships.
pub fn assign_households(
    mut commands: Commands,
    settlements: Query<(&shared::components::SettlementId, &Settlement)>,
    mut houses: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &mut Household,
        Option<&shared::components::BuildingId>,
        &shared::components::BuildingOf,
    )>,
    villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &shared::components::ResidentOf,
        &PlayerPosition,
        Option<&HomeAssignment>,
    )>,
    changed_assignments: Query<(), Changed<HomeAssignment>>,
    changed_names: Query<(), Changed<CharacterName>>,
) {
    let house_info: Vec<(
        Entity,
        shared::components::SettlementId,
        Vec3,
        usize,
        Option<shared::components::BuildingId>,
    )> = houses
        .iter()
        .filter(|(_, building, _, _, _, _)| building.kind == SettlementBuildingKind::House)
        .map(
            |(entity, building, position, _, building_id, building_of)| {
                (
                    entity,
                    building_of.0,
                    position.0,
                    building.kind.housing_capacity() as usize,
                    building_id.copied(),
                )
            },
        )
        .collect();
    let house_lookup: HashMap<Entity, usize> = house_info
        .iter()
        .enumerate()
        .map(|(index, (entity, ..))| (*entity, index))
        .collect();
    let settlement_names: HashMap<shared::components::SettlementId, String> = settlements
        .iter()
        .map(|(settlement_id, settlement)| (*settlement_id, settlement.name.clone()))
        .collect();

    // The common case is a fully assigned, unchanged village. Validate it in
    // O(people + houses) and leave every replicated roster untouched. The old
    // implementation rebuilt names and searched every house for every person
    // on all 60 server ticks, even though housing changes only when a person or
    // cabin changes.
    let mut occupancy: HashMap<Entity, usize> = HashMap::new();
    let mut needs_rebuild = !changed_assignments.is_empty() || !changed_names.is_empty();
    for (_, _, _, intent, resident_of, _, assignment) in villagers.iter() {
        if !intent.counts_as_resident() {
            // Travelling villagers used to consume beds before they arrived,
            // while recount_residents correctly excluded them. Remove any
            // legacy assignment and rebuild its old cabin roster.
            if assignment.is_some() {
                needs_rebuild = true;
            }
            continue;
        }
        let Some(assignment) = assignment else {
            needs_rebuild = true;
            continue;
        };
        let Some(index) = house_lookup.get(&assignment.home).copied() else {
            needs_rebuild = true;
            continue;
        };
        let (_, place, _, capacity, _) = &house_info[index];
        let same_settlement = resident_of.0 == *place;
        let occupied = occupancy.entry(assignment.home).or_default();
        if !same_settlement || *occupied >= *capacity {
            needs_rebuild = true;
            continue;
        }
        *occupied += 1;
    }
    if !needs_rebuild {
        needs_rebuild = houses.iter().any(|(house, _, _, household, _, _)| {
            occupancy.get(&house).copied().unwrap_or(0) != household.resident_ids.len()
        });
    }
    if !needs_rebuild {
        return;
    }

    let mut rosters: HashMap<Entity, Vec<(shared::components::PersonId, String)>> = HashMap::new();
    let mut assigned: HashSet<Entity> = HashSet::new();
    for (villager, person_id, name, intent, resident_of, _, assignment) in villagers.iter() {
        let Some(assignment) = assignment else {
            continue;
        };
        let valid = intent.counts_as_resident()
            && house_lookup
                .get(&assignment.home)
                .and_then(|index| house_info.get(*index))
                .is_some_and(|(house, place, _, capacity, _)| {
                    resident_of.0 == *place && rosters.get(house).map_or(0, Vec::len) < *capacity
                });
        if valid {
            assigned.insert(villager);
            rosters
                .entry(assignment.home)
                .or_default()
                .push((*person_id, name.0.clone()));
            if let Some((_, _, _, _, Some(building_id))) = house_lookup
                .get(&assignment.home)
                .and_then(|index| house_info.get(*index))
            {
                commands
                    .entity(villager)
                    .insert(shared::components::LivesAt(*building_id));
            }
        } else {
            commands
                .entity(villager)
                .remove::<HomeAssignment>()
                .remove::<shared::components::LivesAt>();
        }
    }

    for (house, place, house_position, capacity, building_id) in &house_info {
        while rosters.get(house).map_or(0, Vec::len) < *capacity {
            let resident = villagers
                .iter()
                .filter(|(entity, _, _, intent, resident_of, _, _)| {
                    !assigned.contains(entity)
                        && intent.counts_as_resident()
                        && resident_of.0 == *place
                })
                .min_by(|a, b| {
                    a.5 .0
                        .distance_squared(*house_position)
                        .total_cmp(&b.5 .0.distance_squared(*house_position))
                })
                .map(|(entity, person_id, name, _, _, _, _)| (entity, *person_id, name.0.clone()));
            let Some((resident, person_id, name)) = resident else {
                break;
            };
            assigned.insert(resident);
            rosters
                .entry(*house)
                .or_default()
                .push((person_id, name.clone()));
            let mut resident_commands = commands.entity(resident);
            resident_commands.insert(HomeAssignment { home: *house });
            if let Some(building_id) = building_id {
                resident_commands.insert(shared::components::LivesAt(*building_id));
            }
            let place_name = settlement_names
                .get(place)
                .map_or("unknown settlement", String::as_str);
            info!("Village '{place_name}': {name} was assigned a bed in a cabin");
        }
    }

    for (house, _, _, mut household, _, _) in houses.iter_mut() {
        let next = rosters.remove(&house).unwrap_or_default();
        let resident_ids: Vec<_> = next.iter().map(|(person_id, _)| *person_id).collect();
        let resident_names: Vec<_> = next.into_iter().map(|(_, name)| name).collect();
        if household.resident_ids != resident_ids || household.residents != resident_names {
            household.resident_ids = resident_ids;
            household.residents = resident_names;
        }
    }
}

/// Send housed villagers through their cabin door at sunset and back out at
/// sunrise. Work state is reset to its workplace approach, so morning resumes
/// with a real journey instead of farming or chopping beside the bed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_household_schedules(
    simulation_time: crate::world::simulation_time::SimulationTime,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    homes: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &Household,
        ),
        Without<CharacterKind>,
    >,
    workplaces: Query<
        (&SettlementBuilding, &PlayerPosition, &PlayerRotation),
        Without<CharacterKind>,
    >,
    roads: Query<&shared::components::VillageRoad>,
    mut villagers: Query<
        (
            Entity,
            &shared::components::PersonId,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &HomeAssignment,
            Option<&MoveTarget>,
            Option<&mut HomeRoutine>,
            Option<&mut FarmerRoutine>,
            Option<&mut LumberjackRoutine>,
            Option<&mut FishingRoutine>,
            Option<&mut RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let is_day = clock.is_day();
    let dt = simulation_time.world_seconds();

    for (
        villager,
        person_id,
        position,
        mut facing,
        mut activity,
        assignment,
        move_target,
        routine,
        mut farmer,
        mut lumberjack,
        mut fisher,
        mut road_builder,
        workplace_transit,
    ) in villagers.iter_mut()
    {
        let assigned_home = homes.get(assignment.home).ok();

        let Some(mut routine) = routine else {
            if is_day {
                continue;
            }
            let Some((home, _, home_position, home_rotation, _)) = assigned_home else {
                continue;
            };
            let door =
                SettlementBuildingKind::House.entrance_position(home_position.0, home_rotation.0);
            *activity = CharacterActivity::Idle;
            let workplace_threshold = workplace_transit
                .map(|transit| (transit.building, transit.door, transit.inside))
                .or_else(|| {
                    farmer.as_deref().and_then(|farmer| {
                        matches!(farmer.phase, FarmerPhase::Inside { .. })
                            .then(|| workplaces.get(farmer.farmstead).ok())
                            .flatten()
                            .map(|(building, position, rotation)| {
                                (
                                    position.0,
                                    building.kind.entrance_position(position.0, rotation.0),
                                    building.kind.interior_door_position(position.0, rotation.0),
                                )
                            })
                    })
                })
                .or_else(|| {
                    lumberjack.as_deref().and_then(|lumberjack| {
                        matches!(lumberjack.phase, LumberjackPhase::Inside { .. })
                            .then(|| workplaces.get(lumberjack.hut).ok())
                            .flatten()
                            .map(|(building, position, rotation)| {
                                (
                                    position.0,
                                    building.kind.entrance_position(position.0, rotation.0),
                                    building.kind.interior_door_position(position.0, rotation.0),
                                )
                            })
                    })
                })
                .or_else(|| {
                    fisher.as_deref().and_then(|fisher| {
                        matches!(fisher.phase, FishingPhase::Inside { .. })
                            .then(|| workplaces.get(fisher.hut).ok())
                            .flatten()
                            .map(|(building, position, rotation)| {
                                (
                                    position.0,
                                    building.kind.entrance_position(position.0, rotation.0),
                                    building.kind.interior_door_position(position.0, rotation.0),
                                )
                            })
                    })
                });

            // Unfinished work waits for morning. Resetting the spatial phase is
            // essential: otherwise an interrupted Chopping phase resumes at
            // the cabin and the villager swings an axe beside their bed.
            if let Some(farmer) = farmer.as_deref_mut() {
                farmer.phase = FarmerPhase::GoingToFarmstead;
            }
            if let Some(lumberjack) = lumberjack.as_deref_mut() {
                lumberjack.phase = LumberjackPhase::GoingToHut;
            }
            if let Some(fisher) = fisher.as_deref_mut() {
                fisher.phase = FishingPhase::GoingToHut;
                commands
                    .entity(villager)
                    .remove::<PierTraversal>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>();
            }
            if let Some(road_builder) = road_builder.as_deref_mut() {
                if let Ok(road) = roads.get(road_builder.road) {
                    road_builder.restart_from_built_road(road);
                }
            }

            // Night takes ownership of the destination. A failed daytime
            // route must not remain attached: movement deliberately sleeps
            // while NavigationRouteFailed exists, including during a doorway
            // transit that is otherwise allowed to ignore the wall collider.
            commands
                .entity(villager)
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();

            let phase =
                if let Some((building, workplace_door, workplace_inside)) = workplace_threshold {
                    begin_workplace_exit(
                        &mut commands,
                        villager,
                        building,
                        workplace_door,
                        workplace_inside,
                        door,
                    );
                    HomePhase::LeavingWorkplace
                } else {
                    commands.entity(villager).insert(MoveTarget(door));
                    HomePhase::GoingToDoor
                };
            commands
                .entity(villager)
                .insert(HomeRoutine { home, phase });
            continue;
        };

        let Ok((_, building, home_position, home_rotation, household)) = homes.get(routine.home)
        else {
            *activity = CharacterActivity::Idle;
            commands
                .entity(villager)
                .remove::<HomeRoutine>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };
        if assignment.home != routine.home
            || !household
                .resident_ids
                .iter()
                .any(|resident| resident == person_id)
        {
            *activity = CharacterActivity::Idle;
            commands
                .entity(villager)
                .remove::<HomeRoutine>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        }

        let door = building
            .kind
            .entrance_position(home_position.0, home_rotation.0);
        let inside = building
            .kind
            .interior_door_position(home_position.0, home_rotation.0);
        let outside = exterior_door_clearance_position(home_position.0, door);
        let door_use = BuildingDoorUse {
            building: home_position.0,
        };

        if is_day {
            match routine.phase {
                HomePhase::LeavingWorkplace => {
                    // Finish crossing the workplace threshold first. Its
                    // transit changes this to GoingToDoor, which releases the
                    // villager back to work on the following daylight tick.
                }
                HomePhase::GoingToDoor | HomePhase::OpeningToEnter { .. } => {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(villager)
                        .remove::<HomeRoutine>()
                        .remove::<BuildingDoorUse>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                HomePhase::Entering => {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(villager)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert((door_use, MoveTarget(outside)));
                    routine.phase = HomePhase::Leaving;
                }
                HomePhase::Sleeping => {
                    *activity = CharacterActivity::Indoors;
                    commands
                        .entity(villager)
                        .insert(door_use)
                        .remove::<MoveTarget>();
                    routine.phase = HomePhase::OpeningToLeave {
                        seconds_left: DOOR_OPEN_SECONDS,
                    };
                }
                HomePhase::OpeningToLeave { seconds_left } => {
                    *activity = CharacterActivity::Indoors;
                    commands
                        .entity(villager)
                        .insert(door_use)
                        .remove::<MoveTarget>();
                    let left = seconds_left - dt;
                    if left > 0.0 {
                        routine.phase = HomePhase::OpeningToLeave { seconds_left: left };
                    } else {
                        *activity = CharacterActivity::Idle;
                        commands.entity(villager).insert(MoveTarget(outside));
                        routine.phase = HomePhase::Leaving;
                    }
                }
                HomePhase::Leaving => {
                    *activity = CharacterActivity::Idle;
                    commands.entity(villager).insert(door_use);
                    let still_inside_blocker = obstacles.as_deref().is_some_and(|grid| {
                        grid.point_blocked(Vec2::new(position.0.x, position.0.z))
                    });
                    if ground_distance(position.0, outside) <= DOOR_REACH && !still_inside_blocker {
                        commands
                            .entity(villager)
                            .remove::<HomeRoutine>()
                            .remove::<BuildingDoorUse>()
                            .remove::<MoveTarget>()
                            .remove::<TravelRoute>()
                            .remove::<NavigationRoutePending>()
                            .remove::<NavigationRouteFailed>();
                    } else {
                        ensure_move_target(&mut commands, villager, move_target, outside);
                    }
                }
            }
            continue;
        }

        match routine.phase {
            HomePhase::LeavingWorkplace => {
                // `run_workplace_door_transits` owns this threshold and changes
                // the phase after the worker reaches the exterior door.
                *activity = CharacterActivity::Idle;
            }
            HomePhase::GoingToDoor => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, door) <= DOOR_REACH {
                    commands
                        .entity(villager)
                        .remove::<MoveTarget>()
                        .insert(door_use);
                    let to_house = home_position.0 - door;
                    if to_house.length_squared() > 1e-4 {
                        facing.0 = build_clip_facing(to_house);
                    }
                    routine.phase = HomePhase::OpeningToEnter {
                        seconds_left: DOOR_OPEN_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, villager, move_target, door);
                }
            }
            HomePhase::OpeningToEnter { seconds_left } => {
                *activity = CharacterActivity::Idle;
                commands
                    .entity(villager)
                    .insert(door_use)
                    .remove::<MoveTarget>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = HomePhase::OpeningToEnter { seconds_left: left };
                } else {
                    commands.entity(villager).insert(MoveTarget(inside));
                    routine.phase = HomePhase::Entering;
                }
            }
            HomePhase::Entering => {
                *activity = CharacterActivity::Idle;
                commands.entity(villager).insert(door_use);
                if ground_distance(position.0, inside) <= DOOR_REACH {
                    *activity = CharacterActivity::Indoors;
                    commands
                        .entity(villager)
                        .remove::<BuildingDoorUse>()
                        .remove::<MoveTarget>();
                    routine.phase = HomePhase::Sleeping;
                } else {
                    ensure_move_target(&mut commands, villager, move_target, inside);
                }
            }
            HomePhase::Sleeping => {
                *activity = CharacterActivity::Indoors;
                commands
                    .entity(villager)
                    .remove::<BuildingDoorUse>()
                    .remove::<MoveTarget>();
            }
            HomePhase::OpeningToLeave { .. } | HomePhase::Leaving => {
                // A debug jump back into night while leaving turns them around
                // cleanly rather than stranding them in an impossible phase.
                *activity = CharacterActivity::Idle;
                commands
                    .entity(villager)
                    .insert((door_use, MoveTarget(inside)));
                routine.phase = HomePhase::Entering;
            }
        }
    }
}
