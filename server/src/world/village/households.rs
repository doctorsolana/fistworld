//! Household membership, shared necessities and visible home routines.
//!
//! Durable PersonId and BuildingId relationships are authoritative; readable
//! resident names remain derived inspection data only.

use super::*;

/// Choose real food offers by price per ration, retaining the authored food
/// preference only as a tie-breaker. A household may prefer Bread, but it
/// should not spend its entire purse on one luxury loaf while affordable Meat,
/// Fish or Flour is sitting on the next market table.
///
/// The first absent preference is retained after all physical offers. If the
/// available substitutes cannot fill the pantry, the caller records the
/// remaining request against exactly that one good. This is how a completely
/// empty market still tells a bakery that residents want bread without also
/// claiming that the same rations were independently demanded as meat, fish
/// and flour.
fn household_food_purchase_order(hall_store: &GoodsInventory, market: &MootMarket) -> Vec<Good> {
    let mut foods: Vec<(usize, Good)> = Good::HOUSEHOLD_FOOD_PRIORITY
        .into_iter()
        .enumerate()
        .filter(|(_, good)| hall_store.amount(*good) > 0)
        .collect();
    foods.sort_by_key(|(preference, good)| (market.pool(*good).ask.max(1), *preference));
    let mut order: Vec<_> = foods.into_iter().map(|(_, good)| good).collect();
    if let Some(absent_preference) = Good::HOUSEHOLD_FOOD_PRIORITY
        .into_iter()
        .find(|good| hall_store.amount(*good) == 0)
    {
        order.push(absent_preference);
    }
    order
}

/// Cash moved into the shared pantry purse must correspond to food that can
/// actually be bought today. An unavailable order still records demand below,
/// but pre-funding empty shelves strands household wealth precisely when a new
/// farm, mill or bakery needs that coin as investment capital.
fn stocked_food_budget(
    deficit: u32,
    hall_store: &GoodsInventory,
    market: &MootMarket,
    order: &[Good],
) -> u64 {
    let mut remaining = deficit;
    let mut pennies = 0u64;
    for good in order.iter().copied() {
        if remaining == 0 {
            break;
        }
        let units = remaining.min(hall_store.amount(good));
        pennies =
            pennies.saturating_add(u64::from(units).saturating_mul(market.pool(good).ask.max(1)));
        remaining -= units;
    }
    pennies
}

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
    mut business_events: ResMut<BusinessEventQueue>,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
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
    marketplaces: Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            &PlayerRotation,
        ),
        Without<Household>,
    >,
    positions: Query<&PlayerPosition>,
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
            With<InternalDeliveryRoutine>,
            With<MootQueueTicket>,
            With<MootMealRoutine>,
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
        let food_order = household_food_purchase_order(&hall_store, &market);
        let estimated_unit_price = food_order
            .first()
            .map_or(0, |good| market.pool(*good).ask.max(1));
        let wanted_budget = stocked_food_budget(deficit, &hall_store, &market, &food_order);
        let mut needed = wanted_budget.saturating_sub(economy.pennies);
        if needed > 0 {
            // All earners contribute toward the same concrete pantry target.
            // Two discretionary coins are protected only when the pantry can
            // already cover today's household. A family with fewer than one
            // ration per member spends those coins before accepting hunger.
            let personal_floor = if pantry.edible_amount() < members.len() as u32 {
                0
            } else {
                2 * PENNIES_PER_COIN
            };
            let mut contribution_order = members.clone();
            contribution_order.sort_by_key(|(person_id, ..)| *person_id);
            for (person_id, _, _) in contribution_order {
                let Some(member_entity) = resident_by_id.get(&person_id).copied() else {
                    continue;
                };
                let Ok((_, _, _, mut wallet, _, _, _, _)) = residents.get_mut(member_entity) else {
                    continue;
                };
                let available = wallet.balance().saturating_sub(personal_floor);
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
        let can_purchase_now = food_order.first().is_some_and(|good| {
            hall_store.amount(*good) > 0
                && estimated_unit_price > 0
                && economy.pennies >= estimated_unit_price
        });
        if tactical_shopper && can_purchase_now && clock.is_day() {
            if let Some(shopper) = shopper_entity {
                let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
                    hall_position.0,
                    hall_rotation.map_or(0.0, |rotation| rotation.0),
                );
                let counter = nearest_public_market_entrance(
                    positions
                        .get(house_entity)
                        .map_or(hall_position.0, |position| position.0),
                    hall_entrance,
                    marketplaces
                        .iter()
                        .filter(|(building, owner, ..)| {
                            building.kind == SettlementBuildingKind::Market
                                && owner.0 == building_of.0
                        })
                        .map(|(building, _, position, rotation)| {
                            building.kind.entrance_position(position.0, rotation.0)
                        }),
                );
                commands.entity(shopper).insert(HouseholdShoppingRoutine {
                    home: house_entity,
                    hall: hall_entity,
                    counter,
                    phase: HouseholdShoppingPhase::GoingToMarket,
                });
                if counter == hall_entrance {
                    if let Some(queue_clock) = queue_clock.as_deref_mut() {
                        moot_services::enqueue_moot_service(
                            &mut commands,
                            queue_clock,
                            shopper,
                            hall_entity,
                            MootServiceKind::HouseholdShopping,
                        );
                    } else {
                        commands.entity(shopper).insert(MoveTarget(counter));
                    }
                } else {
                    commands.entity(shopper).insert(MoveTarget(counter));
                }
                economy.last_budget_day = day;
                continue;
            }
        }

        let mut remaining = deficit;
        for good in food_order {
            if remaining == 0 {
                break;
            }
            let room = pantry.free_bulk() / good.bulk_per_unit();
            let requested = remaining.min(room);
            let purchase =
                market.purchase_recording_demand(good, requested, economy.pennies, None, None);
            if purchase.trade.units == 0 {
                // The order helper places at most one absent substitute after
                // every live offer. Its failed purchase has now recorded the
                // residual shortage, so stop rather than duplicating the same
                // ration demand across other food groups.
                break;
            }
            if economy.pennies < purchase.trade.pennies {
                continue;
            }
            economy.pennies -= purchase.trade.pennies;
            let moved = hall_store.transfer_to(&mut pantry, good, purchase.trade.units);
            debug_assert_eq!(moved, purchase.trade.units);
            business_events.record_market_purchase(day, building_of.0, purchase.fills);
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
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
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
            Option<&MootQueueTicket>,
            Option<&MoveTarget>,
            Option<&HomeRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (
        shopper,
        position,
        mut activity,
        mut carrier,
        mut routine,
        queue_ticket,
        move_target,
        home,
        route_failed,
    ) in shoppers.iter_mut()
    {
        if home.is_some() {
            continue;
        }
        let Ok((settlement_id, _hall_position, _hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(routine.hall)
        else {
            commands
                .entity(shopper)
                .remove::<HouseholdShoppingRoutine>()
                .remove::<MootQueueTicket>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };
        let Ok((building, home_position, home_rotation, household, mut economy, mut pantry)) =
            houses.get_mut(routine.home)
        else {
            commands
                .entity(shopper)
                .remove::<HouseholdShoppingRoutine>()
                .remove::<MootQueueTicket>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };
        let home_entrance = building
            .kind
            .entrance_position(home_position.0, home_rotation.0);
        if route_failed.is_some() && queue_ticket.is_none() {
            let mut shopper_commands = commands.entity(shopper);
            shopper_commands
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            match routine.phase {
                HouseholdShoppingPhase::GoingToMarket => {
                    // Nothing has been purchased yet. Release this attempt so
                    // the household can select a shopper again on its next
                    // budget pass instead of leaving one resident asleep on a
                    // terminal navigation result forever.
                    shopper_commands.remove::<HouseholdShoppingRoutine>();
                }
                HouseholdShoppingPhase::ReturningHome => {
                    // Purchased food is physical cargo. Preserve both it and
                    // the routine, then request the cabin entrance again; a
                    // road/prop revision may make the retry viable next tick.
                    shopper_commands.insert(MoveTarget(home_entrance));
                }
            }
            continue;
        }
        activity.set_if_neq(CharacterActivity::Idle);
        match routine.phase {
            HouseholdShoppingPhase::GoingToMarket => {
                if let Some(ticket) = queue_ticket {
                    if !ticket.is_ready() {
                        continue;
                    }
                } else if ground_distance(position.0, routine.counter) > WORK_REACH {
                    // Compatibility for a pre-queue save or a focused test
                    // that creates only the shopping routine.
                    ensure_move_target(&mut commands, shopper, move_target, routine.counter);
                    continue;
                }
                let target = (household.resident_ids.len() as u32)
                    .saturating_mul(u32::from(economy.pantry_target_days));
                let mut remaining = target.saturating_sub(pantry.edible_amount());
                let food_order = household_food_purchase_order(&hall_store, &market);
                for good in food_order {
                    if remaining == 0 {
                        break;
                    }
                    let requested = remaining.min(carrier.free_bulk() / good.bulk_per_unit());
                    let purchase = market.purchase_recording_demand(
                        good,
                        requested,
                        economy.pennies,
                        None,
                        None,
                    );
                    if purchase.trade.units == 0 || economy.pennies < purchase.trade.pennies {
                        break;
                    }
                    economy.pennies -= purchase.trade.pennies;
                    let moved = hall_store.transfer_to(&mut carrier, good, purchase.trade.units);
                    debug_assert_eq!(moved, purchase.trade.units);
                    business_events.record_market_purchase(day, *settlement_id, purchase.fills);
                    remaining = remaining.saturating_sub(moved);
                }
                if carrier.edible_amount() == 0 {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MootQueueTicket>()
                        .remove::<MoveTarget>();
                    continue;
                }
                commands
                    .entity(shopper)
                    .insert(MoveTarget(home_entrance))
                    .remove::<MootQueueTicket>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
                routine.phase = HouseholdShoppingPhase::ReturningHome;
            }
            HouseholdShoppingPhase::ReturningHome => {
                if ground_distance(position.0, home_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, shopper, move_target, home_entrance);
                    continue;
                }
                for good in Good::HOUSEHOLD_FOOD_PRIORITY {
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
    moot_service_busy: Query<
        (),
        Or<(
            With<MootQueueTicket>,
            With<MootMealRoutine>,
            With<HouseholdShoppingRoutine>,
            With<TradeRouteRoutine>,
            With<TavernVisitRoutine>,
            With<TavernWorkerRoutine>,
            With<crate::world::settlement_development::CivicHallBuilderRoutine>,
        )>,
    >,
    carried_inventories: Query<&GoodsInventory, With<CharacterKind>>,
    mut villagers: Query<
        (
            Entity,
            &shared::components::PersonId,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &HomeAssignment,
            Option<&MoveTarget>,
            Option<&mut HomeRoutine>,
            (
                Option<&mut FarmerRoutine>,
                Option<&mut LumberjackRoutine>,
                Option<&mut FishingRoutine>,
                Option<&mut QuarryRoutine>,
                Option<&mut ProcessingRoutine>,
                Option<&mut RoadBuilderRoutine>,
                Option<&WorkplaceDoorTransit>,
                Option<&NavigationRouteFailed>,
            ),
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
        mut position,
        mut facing,
        mut activity,
        assignment,
        move_target,
        routine,
        (
            mut farmer,
            mut lumberjack,
            mut fisher,
            mut quarry,
            mut processor,
            mut road_builder,
            workplace_transit,
            route_failed,
        ),
    ) in villagers.iter_mut()
    {
        if moot_service_busy.get(villager).is_ok() {
            continue;
        }
        let assigned_home = homes.get(assignment.home).ok();

        let Some(mut routine) = routine else {
            if is_day {
                continue;
            }
            // Production owns the worker until its last physical load reaches
            // the workplace store. Sending a loaded farmer, fisher or
            // woodcutter home here used to reset the trade phase first; their
            // goods then remained in personal inventory overnight and were
            // often not unloaded until part-way through the next shift.
            let returning_workplace_goods =
                carried_inventories.get(villager).is_ok_and(|inventory| {
                    (farmer.is_some() && inventory.amount(Good::Wheat) > 0)
                        || (fisher.is_some() && inventory.amount(Good::Food) > 0)
                        || (lumberjack.is_some() && inventory.amount(Good::Wood) > 0)
                        || (quarry.is_some() && inventory.amount(Good::Stone) > 0)
                });
            if returning_workplace_goods {
                continue;
            }
            let Some((home, _, home_position, home_rotation, _)) = assigned_home else {
                continue;
            };
            let door =
                SettlementBuildingKind::House.entrance_position(home_position.0, home_rotation.0);
            activity.set_if_neq(CharacterActivity::Idle);
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
                })
                .or_else(|| {
                    processor.as_deref().and_then(|processor| {
                        processor
                            .is_working_inside()
                            .then(|| workplaces.get(processor.workplace()).ok())
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
            if let Some(quarry) = quarry.as_deref_mut() {
                // Quarry work is outdoors; after an empty-handed cutoff the
                // retained routine can safely restart from its workplace on
                // the next daylight shift. A loaded quarrier was kept above
                // until the Stone reached bounded business storage.
                quarry.restart_for_morning();
            }
            if let Some(processor) = processor.as_deref_mut() {
                processor.reset_for_morning();
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
            commands.entity(villager).insert(HomeRoutine {
                home,
                phase,
                failed_routes: 0,
            });
            continue;
        };

        let Ok((_, building, home_position, home_rotation, household)) = homes.get(routine.home)
        else {
            activity.set_if_neq(CharacterActivity::Idle);
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
            activity.set_if_neq(CharacterActivity::Idle);
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

        if route_failed.is_some() {
            routine.failed_routes = routine.failed_routes.saturating_add(1);
            commands
                .entity(villager)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            if is_day {
                // The ordinary daylight branch below releases this routine.
            } else if routine.failed_routes < 3 {
                // A road or prop revision may make the same essential trip
                // viable. Retrying is bounded so bad geometry cannot create a
                // permanent sleeper or an unbounded route-warning loop.
                commands.entity(villager).insert(MoveTarget(door));
                routine.phase = HomePhase::GoingToDoor;
                continue;
            } else {
                // Shelter is an aggregate guarantee as well as visible
                // behaviour. Compress only the impossible remainder of this
                // trip after three certified route failures. Ordinary homes
                // still walk through the threshold and animate their door.
                position.0 = inside;
                activity.set_if_neq(CharacterActivity::Indoors);
                commands
                    .entity(villager)
                    .remove::<BuildingDoorUse>()
                    .remove::<MoveTarget>();
                routine.phase = HomePhase::Sleeping;
                continue;
            }
        }

        if is_day {
            match routine.phase {
                HomePhase::LeavingWorkplace => {
                    // Finish crossing the workplace threshold first. Its
                    // transit changes this to GoingToDoor, which releases the
                    // villager back to work on the following daylight tick.
                }
                HomePhase::GoingToDoor | HomePhase::OpeningToEnter { .. } => {
                    activity.set_if_neq(CharacterActivity::Idle);
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
                    activity.set_if_neq(CharacterActivity::Idle);
                    commands
                        .entity(villager)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert((door_use, MoveTarget(outside)));
                    routine.phase = HomePhase::Leaving;
                }
                HomePhase::Sleeping => {
                    activity.set_if_neq(CharacterActivity::Indoors);
                    commands
                        .entity(villager)
                        .insert(door_use)
                        .remove::<MoveTarget>();
                    routine.phase = HomePhase::OpeningToLeave {
                        seconds_left: DOOR_OPEN_SECONDS,
                    };
                }
                HomePhase::OpeningToLeave { seconds_left } => {
                    activity.set_if_neq(CharacterActivity::Indoors);
                    commands
                        .entity(villager)
                        .insert(door_use)
                        .remove::<MoveTarget>();
                    let left = seconds_left - dt;
                    if left > 0.0 {
                        routine.phase = HomePhase::OpeningToLeave { seconds_left: left };
                    } else {
                        activity.set_if_neq(CharacterActivity::Idle);
                        commands.entity(villager).insert(MoveTarget(outside));
                        routine.phase = HomePhase::Leaving;
                    }
                }
                HomePhase::Leaving => {
                    activity.set_if_neq(CharacterActivity::Idle);
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
                activity.set_if_neq(CharacterActivity::Idle);
            }
            HomePhase::GoingToDoor => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, door) <= DOOR_REACH {
                    routine.failed_routes = 0;
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
                activity.set_if_neq(CharacterActivity::Idle);
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
                activity.set_if_neq(CharacterActivity::Idle);
                commands.entity(villager).insert(door_use);
                if ground_distance(position.0, inside) <= DOOR_REACH {
                    routine.failed_routes = 0;
                    activity.set_if_neq(CharacterActivity::Indoors);
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
                activity.set_if_neq(CharacterActivity::Indoors);
                commands
                    .entity(villager)
                    .remove::<BuildingDoorUse>()
                    .remove::<MoveTarget>();
            }
            HomePhase::OpeningToLeave { .. } | HomePhase::Leaving => {
                // A debug jump back into night while leaving turns them around
                // cleanly rather than stranding them in an impossible phase.
                activity.set_if_neq(CharacterActivity::Idle);
                commands
                    .entity(villager)
                    .insert((door_use, MoveTarget(inside)));
                routine.phase = HomePhase::Entering;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn households_choose_an_affordable_ration_before_luxury_bread() {
        let mut hall = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(hall.add(Good::Bread, 4), 4);
        assert_eq!(hall.add(Good::Flour, 4), 4);
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(shared::components::BuildingId(1)),
            Good::Bread,
            4,
            1_000,
        );
        market.consign(
            MarketSeller::Business(shared::components::BuildingId(2)),
            Good::Flour,
            4,
            120,
        );

        assert_eq!(
            household_food_purchase_order(&hall, &market),
            vec![Good::Flour, Good::Bread, Good::Meat]
        );
    }

    #[test]
    fn empty_food_shelves_leave_one_preferred_restart_order() {
        let hall = GoodsInventory::new(shared::economy::capacity::HALL);
        let market = MootMarket::founding();

        assert_eq!(
            household_food_purchase_order(&hall, &market),
            vec![Good::Bread]
        );
        assert_eq!(stocked_food_budget(8, &hall, &market, &[Good::Bread]), 0);
    }

    #[test]
    fn pantry_purse_funds_only_rations_that_are_physically_for_sale() {
        let mut hall = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(hall.add(Good::Flour, 3), 3);
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(shared::components::BuildingId(7)),
            Good::Flour,
            3,
            120,
        );
        let order = household_food_purchase_order(&hall, &market);

        assert_eq!(stocked_food_budget(8, &hall, &market, &order), 360);
    }
}
