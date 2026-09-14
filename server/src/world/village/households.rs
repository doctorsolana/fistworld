//! Household membership, shared necessities and visible home routines.
//!
//! Durable PersonId and BuildingId relationships are authoritative; readable
//! resident names remain derived inspection data only.

use super::*;

mod membership;
mod needs;
mod provisioning;
mod shopping;
pub use membership::{assign_households, ensure_house_appearances, ensure_households};
pub(crate) use needs::HearthState;
pub use provisioning::update_household_budgets_and_pantries;
pub use shopping::run_household_shopping;

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
            With<crate::world::house_upgrades::HouseUpgradeBuilderRoutine>,
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
            Option<&HomeAssignment>,
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
        (
            With<CharacterKind>,
            Without<strategic::StrategicPerson>,
            Or<(With<HomeAssignment>, With<HomeRoutine>)>,
        ),
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
        let Some(assignment) = assignment else {
            // Losing the dwelling removes HomeAssignment before this routine
            // runs. Retain orphan sleepers in the query long enough to release
            // their hidden body and obsolete door/path ownership.
            if routine.is_some() {
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
            continue;
        };
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
        assert_eq!(
            provisioning::plan_basket(&market, &hall, 8, 0, u64::MAX, 80).pennies,
            0
        );
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
        assert_eq!(
            provisioning::plan_basket(&market, &hall, 8, 0, u64::MAX, 80).pennies,
            360
        );
    }
}
