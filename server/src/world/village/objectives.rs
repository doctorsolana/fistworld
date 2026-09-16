//! Compact, replicated explanations of what each tactical villager is doing.
//!
//! The simulation's authoritative routines remain server-only. This pass folds
//! them into stable enums only when the answer changes, giving inspection UI
//! useful evidence without replicating timers, entity targets or AI internals.

use super::*;

use shared::components::{CharacterNavigationStatus, CharacterObjective};

#[allow(clippy::type_complexity)]
pub fn sync_character_objectives(
    mut commands: Commands,
    people: Query<(
        Entity,
        &CharacterKind,
        Option<&PlayerConstructionAssignment>,
        Option<&crate::world::house_upgrades::HouseUpgradeBuilderRoutine>,
        (
            Option<&crate::world::shipping::crew::ShipCrew>,
            Has<crate::world::ports::PortBuilder>,
            Has<crate::world::shipping::PortHaulRoutine>,
            Has<crate::world::regional_roads::bridge::BridgeBuilder>,
            Option<&crate::world::immigration::ImmigrantArrival>,
            Has<shared::components::AboardBoat>,
        ),
        (
            Option<&VillagerIntent>,
            Option<&MigrationCooldown>,
            Option<&moot_services::MootQueueTicket>,
            Option<&moot_services::MootMealRoutine>,
            Option<&moot_services::PermitPickupRoutine>,
            Option<&ConstructionMaterialRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&MarketCollectionRoutine>,
            Option<&InternalDeliveryRoutine>,
            Option<&TradeRouteRoutine>,
            Option<&crate::world::settlement_development::CivicHallBuilderRoutine>,
            Option<&TavernVisitRoutine>,
            Option<&TavernWorkerRoutine>,
        ),
        (
            Option<&FarmerRoutine>,
            Option<&FishingRoutine>,
            Option<&LumberjackRoutine>,
            Option<&QuarryRoutine>,
            Option<&ProcessingRoutine>,
            Option<&ambient::AmbientRoutine>,
            Option<&WorkerOffDuty>,
            Option<&WorkStatus>,
            Option<&MoveTarget>,
            Option<&TravelRoute>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
            Option<&CharacterObjective>,
            Option<&CharacterNavigationStatus>,
            Option<&population::ImmigrationDeparture>,
        ),
    )>,
) {
    for (
        entity,
        kind,
        player_construction,
        house_upgrade,
        (ship_crew, port_builder, port_hauler, bridge_builder, arrival, aboard),
        (
            intent,
            migration_cooldown,
            queue,
            meal,
            permit,
            construction,
            road,
            home,
            shopping,
            market_collection,
            internal_delivery,
            trade_route,
            civic_hall_builder,
            tavern_visit,
            tavern_worker,
        ),
        (
            farmer,
            fisher,
            lumberjack,
            quarry,
            processing,
            ambient,
            off_duty,
            work_status,
            move_target,
            travel_route,
            route_pending,
            route_failed,
            current_objective,
            current_navigation,
            immigration_departure,
        ),
    ) in people.iter()
    {
        // Heroes use the same physical supply loop without civilian intent.
        // Clear the explanation once a new player order cancels that work.
        if *kind != CharacterKind::Villager && player_construction.is_none() {
            if current_objective.is_some() || current_navigation.is_some() {
                commands
                    .entity(entity)
                    .remove::<CharacterObjective>()
                    .remove::<CharacterNavigationStatus>();
            }
            continue;
        }

        let objective = if aboard && arrival.is_some_and(|arrival| arrival.chosen_at.is_none()) {
            CharacterObjective::ChoosingSettlement
        } else if let Some(crew) = ship_crew {
            crew.objective()
        } else if port_builder {
            CharacterObjective::ConstructingBuilding
        } else if port_hauler {
            CharacterObjective::HaulingConstructionSupplies
        } else if bridge_builder {
            CharacterObjective::BuildingBridge
        } else {
            objective_for(
                player_construction.is_some(),
                house_upgrade,
                intent,
                migration_cooldown,
                queue,
                immigration_departure,
                meal,
                permit,
                construction,
                road,
                home,
                shopping,
                market_collection,
                internal_delivery,
                trade_route,
                civic_hall_builder,
                tavern_visit,
                tavern_worker,
                farmer,
                fisher,
                lumberjack,
                quarry,
                processing,
                ambient,
                off_duty,
                work_status,
                move_target.is_some(),
            )
        };
        let navigation = if route_failed.is_some() {
            CharacterNavigationStatus::RouteBlocked
        } else if route_pending.is_some() {
            CharacterNavigationStatus::PlanningRoute
        } else if move_target.is_some() || travel_route.is_some() {
            CharacterNavigationStatus::Walking
        } else {
            CharacterNavigationStatus::Stationary
        };

        // Insert each component on its own: a tuple insert marks BOTH
        // Changed, so a navigation flip used to re-replicate an unchanged
        // objective (and vice versa) for every walker.
        if current_objective.copied() != Some(objective) {
            commands.entity(entity).insert(objective);
        }
        if current_navigation.copied() != Some(navigation) {
            commands.entity(entity).insert(navigation);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn objective_for(
    player_construction: bool,
    house_upgrade: Option<&crate::world::house_upgrades::HouseUpgradeBuilderRoutine>,
    intent: Option<&VillagerIntent>,
    migration_cooldown: Option<&MigrationCooldown>,
    queue: Option<&moot_services::MootQueueTicket>,
    immigration_departure: Option<&population::ImmigrationDeparture>,
    meal: Option<&moot_services::MootMealRoutine>,
    permit: Option<&moot_services::PermitPickupRoutine>,
    construction: Option<&ConstructionMaterialRoutine>,
    road: Option<&RoadBuilderRoutine>,
    home: Option<&HomeRoutine>,
    shopping: Option<&HouseholdShoppingRoutine>,
    market_collection: Option<&MarketCollectionRoutine>,
    internal_delivery: Option<&InternalDeliveryRoutine>,
    trade_route: Option<&TradeRouteRoutine>,
    civic_hall_builder: Option<&crate::world::settlement_development::CivicHallBuilderRoutine>,
    tavern_visit: Option<&TavernVisitRoutine>,
    tavern_worker: Option<&TavernWorkerRoutine>,
    farmer: Option<&FarmerRoutine>,
    fisher: Option<&FishingRoutine>,
    lumberjack: Option<&LumberjackRoutine>,
    quarry: Option<&QuarryRoutine>,
    processing: Option<&ProcessingRoutine>,
    ambient: Option<&ambient::AmbientRoutine>,
    off_duty: Option<&WorkerOffDuty>,
    work_status: Option<&WorkStatus>,
    moving: bool,
) -> CharacterObjective {
    if immigration_departure.is_some() {
        return CharacterObjective::LeavingImmigrationCounter;
    }
    // A reserved errand waits while construction completes its current
    // physical load/corridor. Report the same owner that execution uses.
    if let Some(construction) =
        construction.filter(|routine| routine.finishes_before_personal_needs())
    {
        if meal.is_some()
            || shopping.is_some()
            || queue.is_some_and(|ticket| ticket.kind != MootServiceKind::ConstructionMaterial)
        {
            return construction_objective(construction);
        }
    }
    if let Some(queue) = queue {
        return queue.objective();
    }
    if let Some(meal) = meal.copied() {
        return meal.objective();
    }
    if let Some(shopping) = shopping {
        return match shopping.phase {
            HouseholdShoppingPhase::GoingToMarket => CharacterObjective::GoingHouseholdShopping,
            HouseholdShoppingPhase::ReturningHome => CharacterObjective::ReturningWithHouseholdFood,
            HouseholdShoppingPhase::ReturningToMarket => CharacterObjective::DeliveringMarketGoods,
        };
    }
    if let Some(upgrade) = house_upgrade {
        return if upgrade.carrying {
            CharacterObjective::CarryingConstructionWood
        } else if moving {
            CharacterObjective::CollectingConstructionWood
        } else {
            CharacterObjective::ConstructingBuilding
        };
    }
    if player_construction && construction.is_none() {
        return CharacterObjective::ConstructingBuilding;
    }
    if let Some(visit) = tavern_visit.copied() {
        return visit.objective();
    }
    if let Some(worker) = tavern_worker.copied() {
        return worker.objective();
    }
    if let Some(intent) = intent {
        match intent {
            VillagerIntent::Idle => {
                return if migration_cooldown.is_some() {
                    CharacterObjective::WaitingToRetryMigration
                } else {
                    CharacterObjective::LookingForSettlement
                };
            }
            VillagerIntent::ArrivingBySea { .. } => {
                return CharacterObjective::SailingToSettlement;
            }
            VillagerIntent::Travelling { .. } => {
                return CharacterObjective::TravellingToSettlement;
            }
            VillagerIntent::Building { .. } if construction.is_none() => {
                return CharacterObjective::ConstructingBuilding;
            }
            VillagerIntent::RoadBuilding { .. } if road.is_none() => {
                return CharacterObjective::BuildingRoad;
            }
            VillagerIntent::Resident { .. }
            | VillagerIntent::Building { .. }
            | VillagerIntent::RoadBuilding { .. } => {}
        }
    }
    if permit.is_some() {
        return CharacterObjective::CollectingPermit;
    }
    if let Some(construction) = construction {
        return construction_objective(construction);
    }
    if let Some(road) = road {
        return road.objective();
    }
    if civic_hall_builder.is_some() {
        return CharacterObjective::ConstructingBuilding;
    }
    if let Some(home) = home {
        return match home.phase {
            HomePhase::LeavingWorkplace | HomePhase::GoingToDoor => CharacterObjective::GoingHome,
            HomePhase::OpeningToEnter { .. } | HomePhase::Entering => {
                CharacterObjective::EnteringHome
            }
            HomePhase::Sleeping => CharacterObjective::Sleeping,
            HomePhase::OpeningToLeave { .. } | HomePhase::Leaving => {
                CharacterObjective::LeavingHome
            }
        };
    }
    if let Some(collection) = market_collection {
        return match collection.phase {
            MarketCollectionPhase::GoingToBusiness | MarketCollectionPhase::GoingToInputCounter => {
                CharacterObjective::CollectingMarketGoods
            }
            MarketCollectionPhase::ReturningToHall
            | MarketCollectionPhase::ReturningToBusinessAfterFailedSale
            | MarketCollectionPhase::DeliveringInput
            | MarketCollectionPhase::ReturningFailedInput => {
                CharacterObjective::DeliveringMarketGoods
            }
        };
    }
    if let Some(delivery) = internal_delivery {
        return match delivery.phase {
            InternalDeliveryPhase::GoingToSupplier => CharacterObjective::CollectingCompanyInputs,
            InternalDeliveryPhase::Delivering => CharacterObjective::DeliveringCompanyInputs,
        };
    }
    if let Some(route) = trade_route {
        return route.objective();
    }
    if let Some(farmer) = farmer {
        return match farmer.phase {
            FarmerPhase::GoingToFarmstead
            | FarmerPhase::Inside { .. }
            | FarmerPhase::WalkingToField { .. } => CharacterObjective::GoingToFarm,
            FarmerPhase::Farming => CharacterObjective::Farming,
            FarmerPhase::ReturningToFarmstead => CharacterObjective::ReturningHarvest,
            FarmerPhase::EndingShift => CharacterObjective::EndingWorkShift,
        };
    }
    if let Some(fisher) = fisher {
        return match fisher.phase {
            FishingPhase::GoingToHut
            | FishingPhase::Inside { .. }
            | FishingPhase::StagingForPier { .. }
            | FishingPhase::WalkingToPier => CharacterObjective::GoingFishing,
            FishingPhase::Fishing => CharacterObjective::Fishing,
            FishingPhase::ReturningFromPier { .. } | FishingPhase::ReturningToHut => {
                CharacterObjective::ReturningCatch
            }
            FishingPhase::EndingShift => CharacterObjective::EndingWorkShift,
        };
    }
    if let Some(lumberjack) = lumberjack {
        return match lumberjack.phase {
            LumberjackPhase::GoingToHut
            | LumberjackPhase::Inside { .. }
            | LumberjackPhase::WalkingToTree { .. } => CharacterObjective::GoingToLumberWork,
            LumberjackPhase::Chopping { .. } => CharacterObjective::ChoppingTimber,
            LumberjackPhase::ReturningToHut => CharacterObjective::ReturningTimber,
            LumberjackPhase::EndingShift => CharacterObjective::EndingWorkShift,
        };
    }
    if let Some(quarry) = quarry {
        return quarry.objective();
    }
    if let Some(processing) = processing {
        return processing.objective();
    }
    if let Some(ambient) = ambient {
        return ambient.objective();
    }
    if off_duty.is_some() {
        return CharacterObjective::OffDuty;
    }
    if work_status.is_some_and(|status| *status == WorkStatus::LookingForWork) {
        return CharacterObjective::LookingForWork;
    }
    if moving {
        CharacterObjective::WalkingToDestination
    } else {
        CharacterObjective::Idle
    }
}

fn construction_objective(construction: &ConstructionMaterialRoutine) -> CharacterObjective {
    match construction.phase {
        ConstructionMaterialPhase::Seeking
        | ConstructionMaterialPhase::UnloadingAtHall { .. }
        | ConstructionMaterialPhase::CollectingFromStore { .. }
        | ConstructionMaterialPhase::WalkingToTree { .. }
        | ConstructionMaterialPhase::LeavingDeliveryAccess { .. } => {
            CharacterObjective::FindingConstructionWood
        }
        ConstructionMaterialPhase::Chopping { .. } => CharacterObjective::ChoppingTimber,
        ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
        | ConstructionMaterialPhase::Delivering { .. }
        | ConstructionMaterialPhase::WaitingForDeliveryAccess { .. } => {
            CharacterObjective::CarryingConstructionWood
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn household_trip_outranks_building_intent_after_the_committed_load_finishes() {
        let mut app = App::new();
        app.add_systems(Update, sync_character_objectives);
        let site = app.world_mut().spawn_empty().id();
        let shopper = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                VillagerIntent::Building {
                    settlement: site,
                    site,
                },
                HouseholdShoppingRoutine {
                    account: site,
                    household: shared::components::HouseholdId(1),
                    home: site,
                    hall: site,
                    counter: Vec3::ZERO,
                    phase: HouseholdShoppingPhase::ReturningHome,
                    cargo: [0; Good::COUNT],
                },
                MoveTarget(Vec3::X * 10.0),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(shopper),
            Some(&CharacterObjective::ReturningWithHouseholdFood)
        );
        assert_eq!(
            app.world().get::<CharacterNavigationStatus>(shopper),
            Some(&CharacterNavigationStatus::Walking)
        );

        let mut material = ConstructionMaterialRoutine::new(site);
        material.phase = ConstructionMaterialPhase::Delivering {
            destination: Vec3::ZERO,
        };
        app.world_mut().entity_mut(shopper).insert(material);
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(shopper),
            Some(&CharacterObjective::CarryingConstructionWood)
        );
        app.world_mut()
            .get_mut::<ConstructionMaterialRoutine>(shopper)
            .unwrap()
            .phase = ConstructionMaterialPhase::Seeking;
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(shopper),
            Some(&CharacterObjective::ReturningWithHouseholdFood)
        );
    }

    #[test]
    fn pending_meal_reports_the_committed_material_owner_until_it_yields() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<MootQueueClock>()
            .add_systems(Update, sync_character_objectives);
        let site = app.world_mut().spawn_empty().id();
        let mut material = ConstructionMaterialRoutine::new(site);
        material.phase = ConstructionMaterialPhase::ApproachingDeliveryAccess { entry: Vec3::ZERO };
        let builder = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                VillagerIntent::Building {
                    settlement: site,
                    site,
                },
                material,
            ))
            .id();
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands, mut clock: ResMut<MootQueueClock>| {
                    moot_services::reserve_meal(
                        &mut commands,
                        &mut clock,
                        builder,
                        site,
                        MootServiceKind::PersonalMeal,
                        Good::Bread,
                        1,
                    );
                },
            )
            .unwrap();
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(builder),
            Some(&CharacterObjective::CarryingConstructionWood)
        );
        app.world_mut()
            .get_mut::<ConstructionMaterialRoutine>(builder)
            .unwrap()
            .phase = ConstructionMaterialPhase::Seeking;
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(builder),
            Some(&CharacterObjective::QueuedForPersonalFood)
        );
    }

    #[test]
    fn house_upgrade_reports_the_actual_queue_then_carried_meal() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let at = Vec3::new(1700.0, 80.0, 0.0);
        let mut terrain = WorldTerrain::default();
        terrain.apply_flatten_rect(at, Vec2::splat(40.0), 0.0, 4.0);
        app.insert_resource(terrain)
            .init_resource::<Time>()
            .init_resource::<MootQueueClock>();
        app.world_mut().spawn(WorldTime::new_default());
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Objective test".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 20,
                },
                PlayerPosition(at + Vec3::X * 12.0),
                PlayerRotation(0.0),
            ))
            .id();
        let builder = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(at),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(at),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                Nutrition::default(),
                crate::world::house_upgrades::HouseUpgradeBuilderRoutine {
                    project: hall,
                    carrying: true,
                },
            ))
            .id();
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands, mut clock: ResMut<MootQueueClock>| {
                    moot_services::reserve_meal(
                        &mut commands,
                        &mut clock,
                        builder,
                        hall,
                        MootServiceKind::PersonalMeal,
                        Good::Bread,
                        1,
                    );
                },
            )
            .unwrap();
        app.world_mut()
            .run_system_once(sync_character_objectives)
            .unwrap();
        assert_eq!(
            app.world().get::<CharacterObjective>(builder),
            Some(&CharacterObjective::QueuedForPersonalFood)
        );
        app.add_systems(
            Update,
            (
                advance_moot_service_queues,
                run_moot_meal_collections,
                crate::player::hero::step_units,
                sync_character_objectives,
            )
                .chain(),
        );
        for _ in 0..1000 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.1));
            app.update();
            if app.world().get::<CharacterObjective>(builder)
                == Some(&CharacterObjective::CollectingFood)
            {
                break;
            }
        }
        assert!(app.world().get::<MootQueueTicket>(builder).is_none());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(builder)
                .unwrap()
                .amount(Good::Bread),
            1
        );
        assert_eq!(
            app.world().get::<CharacterObjective>(builder),
            Some(&CharacterObjective::CollectingFood)
        );
        assert!(
            app.world()
                .get::<crate::world::house_upgrades::HouseUpgradeBuilderRoutine>(builder)
                .is_some()
        );
    }

    #[test]
    fn commanded_hero_reports_supply_chop_build_and_clears_after_cancellation() {
        let mut app = App::new();
        app.add_systems(Update, sync_character_objectives);
        let site = app.world_mut().spawn_empty().id();
        let hero = app
            .world_mut()
            .spawn((
                CharacterKind::Hero,
                PlayerConstructionAssignment {
                    site,
                    settlement: site,
                },
                ConstructionMaterialRoutine::new(site),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(hero),
            Some(&CharacterObjective::FindingConstructionWood)
        );
        app.world_mut()
            .get_mut::<ConstructionMaterialRoutine>(hero)
            .unwrap()
            .phase = ConstructionMaterialPhase::Chopping {
            tree: Vec3::ZERO,
            seconds_left: 40.0,
        };
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(hero),
            Some(&CharacterObjective::ChoppingTimber)
        );
        app.world_mut()
            .entity_mut(hero)
            .remove::<ConstructionMaterialRoutine>();
        app.update();
        assert_eq!(
            app.world().get::<CharacterObjective>(hero),
            Some(&CharacterObjective::ConstructingBuilding)
        );
        app.world_mut()
            .entity_mut(hero)
            .remove::<PlayerConstructionAssignment>();
        app.update();
        assert!(app.world().get::<CharacterObjective>(hero).is_none());
        assert!(app.world().get::<CharacterNavigationStatus>(hero).is_none());
    }
}
