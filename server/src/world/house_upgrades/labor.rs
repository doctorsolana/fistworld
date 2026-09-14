//! Exclusively assigned construction labor and finite embodied/strategic travel.
use super::project::*;
use crate::player::hero::MoveTarget;
use crate::world::village::{self, VillagerIntent};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{GoodsInventory, Wallet};
const WORK_REACH: f32 = 1.35;

pub(super) fn choose_worker(world: &mut World, project: &UpgradeProject) -> Option<Entity> {
    let mut people = world.query::<(
        Entity,
        &PersonId,
        &ResidentOf,
        &CharacterKind,
        &PlayerPosition,
        &Health,
        &Wallet,
    )>();
    people
        .iter(world)
        .filter(|(entity, _, of, kind, _, health, _)| {
            of.0 == project.settlement
                && **kind == CharacterKind::Villager
                && health.current > 0.0
                && world.get::<Hero>(*entity).is_none()
                && world.get::<AboardBoat>(*entity).is_none()
                && world.get::<CommandedBy>(*entity).is_none()
                && world.get::<MemberOfBattalion>(*entity).is_none()
                && world
                    .get::<GoodsInventory>(*entity)
                    .is_some_and(|inventory| {
                        inventory.is_empty()
                            && inventory.free_bulk() >= shared::economy::capacity::VILLAGER
                    })
                && world
                    .get::<VillagerIntent>(*entity)
                    .is_some_and(|intent| matches!(intent, VillagerIntent::Resident { .. }))
                && world.get::<EmployedAt>(*entity).is_none()
                && world.get::<CivicEmployment>(*entity).is_none()
                && world.get::<village::HomeRoutine>(*entity).is_none()
                && world
                    .get::<village::ConstructionMaterialRoutine>(*entity)
                    .is_none()
                && world
                    .get::<village::HouseholdShoppingRoutine>(*entity)
                    .is_none()
                && world.get::<village::MootQueueTicket>(*entity).is_none()
                && world.get::<village::MootMealRoutine>(*entity).is_none()
                && world.get::<village::TradeRouteRoutine>(*entity).is_none()
                && world.get::<village::LumberjackRoutine>(*entity).is_none()
                && world.get::<village::FarmerRoutine>(*entity).is_none()
                && world.get::<village::FishingRoutine>(*entity).is_none()
                && world.get::<village::ProcessingRoutine>(*entity).is_none()
                && world.get::<village::QuarryRoutine>(*entity).is_none()
                && world
                    .get::<village::MarketCollectionRoutine>(*entity)
                    .is_none()
                && world
                    .get::<village::InternalDeliveryRoutine>(*entity)
                    .is_none()
                && world.get::<village::TavernWorkerRoutine>(*entity).is_none()
                && world.get::<village::MootSteward>(*entity).is_none()
                && world.get::<village::CompanyPorter>(*entity).is_none()
                && world.get::<village::TavernVisitRoutine>(*entity).is_none()
                && world
                    .get::<village::WorkplaceDoorTransit>(*entity)
                    .is_none()
                && world
                    .get::<crate::world::village_roads::RoadBuilderRoutine>(*entity)
                    .is_none()
                && world.get::<HouseUpgradeBuilderRoutine>(*entity).is_none()
        })
        .min_by_key(|(_, id, _, _, position, _, _)| {
            (
                ((position.0.xz() - project.stand.xz()).length_squared() * 100.0) as u64,
                id.0,
            )
        })
        .map(|(entity, ..)| entity)
}

pub(super) fn start_trip(world: &mut World, project: &mut UpgradeProject, phase: Phase) {
    project.phase = phase;
    let target = if phase == Phase::ToMarket {
        project.market_stand
    } else {
        project.stand
    };
    let worker = project.worker.expect("assigned worker");
    let current = world
        .get::<PlayerPosition>(worker)
        .expect("worker position")
        .0;
    // Same base walking speed as embodied villagers, without road speedups.
    project.travel_left = current.xz().distance(target.xz()) / shared::player::HERO_MOVE_SPEED;
    if world
        .get::<village::strategic::StrategicPerson>(worker)
        .is_none()
    {
        world
            .entity_mut(worker)
            .insert((MoveTarget(target), CharacterActivity::Idle));
    } else {
        world
            .entity_mut(worker)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>();
    }
}

pub(super) fn arrive(
    world: &mut World,
    project: &mut UpgradeProject,
    target: Vec3,
    dt: f32,
) -> bool {
    let worker = project.worker.expect("assigned worker");
    if world
        .get::<village::strategic::StrategicPerson>(worker)
        .is_some()
    {
        project.travel_left = (project.travel_left - dt).max(0.0);
        if project.travel_left > 0.0 {
            return false;
        }
        if world
            .get::<PlayerPosition>(worker)
            .is_none_or(|position| position.0 != target)
        {
            world.entity_mut(worker).insert(PlayerPosition(target));
        }
    } else {
        let position = world
            .get::<PlayerPosition>(worker)
            .expect("worker position")
            .0;
        if position.xz().distance(target.xz()) > WORK_REACH {
            if world
                .get::<MoveTarget>(worker)
                .is_none_or(|goal| goal.0.distance_squared(target) > 0.01)
            {
                world.entity_mut(worker).insert(MoveTarget(target));
            }
            return false;
        }
    }
    world
        .entity_mut(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
    true
}

pub(super) fn release_worker(world: &mut World, project: &mut UpgradeProject) {
    if let Some(worker) = project.worker.take() {
        if world
            .get::<HouseUpgradeBuilderRoutine>(worker)
            .is_some_and(|routine| routine.project == project.worksite)
        {
            world
                .entity_mut(worker)
                .remove::<HouseUpgradeBuilderRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert(CharacterActivity::Idle);
        }
    }
}
