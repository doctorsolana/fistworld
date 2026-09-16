//! Exclusively assigned construction labor and physical travel.
use super::project::*;
use crate::player::hero::MoveTarget;
use crate::world::village::{self, VillagerIntent};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{GoodsInventory, Wallet};
const WORK_REACH: f32 = 1.35;

pub(super) fn choose_worker(world: &mut World, project: &UpgradeProject) -> Option<Entity> {
    let mut blocked = world.query_filtered::<(), village::worker_activity::JobChangeBlocked>();
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
                && blocked.get(world, *entity).is_err()
                && world.get::<MoveTarget>(*entity).is_none()
                && world.get::<TravelRoute>(*entity).is_none()
                && world.get::<NavigationRoutePending>(*entity).is_none()
                && world.get::<village::MootSteward>(*entity).is_none()
                && world.get::<village::CompanyPorter>(*entity).is_none()
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
    world
        .entity_mut(worker)
        .insert((MoveTarget(target), CharacterActivity::Idle));
}

pub(super) fn arrive(world: &mut World, project: &UpgradeProject, target: Vec3) -> bool {
    let worker = project.worker.expect("assigned worker");
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
            let needs_own_movement = world
                .query_filtered::<(), village::worker_activity::PersonalNeedsOwnMovement>()
                .get(world, worker)
                .is_ok();
            world
                .entity_mut(worker)
                .remove::<HouseUpgradeBuilderRoutine>();
            if needs_own_movement {
                // Cancelling the construction contract releases only its own
                // marker; the meal/shopping owner still has a trip to finish.
                return;
            }
            world
                .entity_mut(worker)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert(CharacterActivity::Idle);
        }
    }
}
