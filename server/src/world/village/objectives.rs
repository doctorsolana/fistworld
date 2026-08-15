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
        ),
        (
            Option<&FarmerRoutine>,
            Option<&FishingRoutine>,
            Option<&LumberjackRoutine>,
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
        ),
    )>,
) {
    for (
        entity,
        kind,
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
        ),
        (
            farmer,
            fisher,
            lumberjack,
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
        ),
    ) in people.iter()
    {
        if *kind != CharacterKind::Villager {
            continue;
        }

        let objective = objective_for(
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
            farmer,
            fisher,
            lumberjack,
            processing,
            ambient,
            off_duty,
            work_status,
            move_target.is_some(),
        );
        let navigation = if route_failed.is_some() {
            CharacterNavigationStatus::RouteBlocked
        } else if route_pending.is_some() {
            CharacterNavigationStatus::PlanningRoute
        } else if move_target.is_some() || travel_route.is_some() {
            CharacterNavigationStatus::Walking
        } else {
            CharacterNavigationStatus::Stationary
        };

        if current_objective.copied() != Some(objective)
            || current_navigation.copied() != Some(navigation)
        {
            commands.entity(entity).insert((objective, navigation));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn objective_for(
    intent: Option<&VillagerIntent>,
    migration_cooldown: Option<&MigrationCooldown>,
    queue: Option<&moot_services::MootQueueTicket>,
    meal: Option<&moot_services::MootMealRoutine>,
    permit: Option<&moot_services::PermitPickupRoutine>,
    construction: Option<&ConstructionMaterialRoutine>,
    road: Option<&RoadBuilderRoutine>,
    home: Option<&HomeRoutine>,
    shopping: Option<&HouseholdShoppingRoutine>,
    market_collection: Option<&MarketCollectionRoutine>,
    internal_delivery: Option<&InternalDeliveryRoutine>,
    farmer: Option<&FarmerRoutine>,
    fisher: Option<&FishingRoutine>,
    lumberjack: Option<&LumberjackRoutine>,
    processing: Option<&ProcessingRoutine>,
    ambient: Option<&ambient::AmbientRoutine>,
    off_duty: Option<&WorkerOffDuty>,
    work_status: Option<&WorkStatus>,
    moving: bool,
) -> CharacterObjective {
    if let Some(queue) = queue {
        return queue.objective();
    }
    if let Some(meal) = meal.copied() {
        return meal.objective();
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
        return match construction.phase {
            ConstructionMaterialPhase::Seeking
            | ConstructionMaterialPhase::UnloadingAtHall { .. }
            | ConstructionMaterialPhase::CollectingFromStore { .. }
            | ConstructionMaterialPhase::WalkingToTree { .. }
            | ConstructionMaterialPhase::Chopping { .. } => {
                CharacterObjective::FindingConstructionWood
            }
            ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
            | ConstructionMaterialPhase::Delivering { .. }
            | ConstructionMaterialPhase::LeavingDeliveryAccess { .. } => {
                CharacterObjective::CarryingConstructionWood
            }
        };
    }
    if let Some(road) = road {
        return road.objective();
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
    if let Some(shopping) = shopping {
        return match shopping.phase {
            HouseholdShoppingPhase::GoingToMarket => CharacterObjective::GoingHouseholdShopping,
            HouseholdShoppingPhase::ReturningHome => CharacterObjective::ReturningWithHouseholdFood,
        };
    }
    if let Some(collection) = market_collection {
        return match collection.phase {
            MarketCollectionPhase::GoingToBusiness => CharacterObjective::CollectingMarketGoods,
            MarketCollectionPhase::ReturningToHall | MarketCollectionPhase::DeliveringInput => {
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
            LumberjackPhase::Chopping => CharacterObjective::ChoppingTimber,
            LumberjackPhase::ReturningToHut => CharacterObjective::ReturningTimber,
            LumberjackPhase::EndingShift => CharacterObjective::EndingWorkShift,
        };
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
