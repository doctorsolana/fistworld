//! Physical bridge work shares road activity ownership; the regional contract
//! owns its reserved materials and wages. A finished deck alone grants access.

pub(crate) mod planning;
#[cfg(test)]
mod tests;
pub(crate) use planning::plan_shortcuts;

use crate::player::hero::{MoveTarget, OfflineHero};
use crate::world::village::{HomeRoutine, HouseholdShoppingRoutine, MootMealRoutine};
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, TravelRoute,
};
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{Good, GoodsInventory};

/// Paid bank-side work, shared by execution, partial claims and the civic
/// quote. Material quantities remain the surveyed deck's full requirements.
pub(super) fn required_work(bridge: &RoadBridge) -> u64 {
    (bridge.length() * bridge.width).ceil().max(1.0) as u64
}

#[derive(Component, Clone)]
pub(crate) struct BridgeBuilder {
    pub(crate) bridge: Entity,
    source: Entity,
    pickup: Vec3,
    carried: Option<(Good, u32)>,
    worked: f32,
    returning: bool,
}

pub(crate) fn revalidate(world: &mut World, bridge: &RoadBridge) -> Option<bool> {
    let terrain = world.get_resource::<shared::terrain::WorldTerrain>()?;
    let surveyed = planning::survey_bridge(bridge.start.xz(), bridge.end.xz(), |p| {
        (
            terrain.get_height(p.x, p.y),
            terrain.get_water_height(p.x, p.y),
        )
    });
    let mut expected = bridge.clone();
    expected.built = false;
    if surveyed.as_ref() != Some(&expected) {
        return Some(false);
    }
    crate::world::village_roads::regional_bridge_footprint_clear(
        world,
        &[bridge.start.xz(), bridge.end.xz()],
        bridge.width,
    )
}

pub(crate) fn start_bridge_work(
    world: &mut World,
    worker: Entity,
    hall: Entity,
    source_inventory: Entity,
    bridge_entity: Entity,
    pickup: Vec3,
) {
    // The contract lends an ordinary physical handcart. Central porter
    // capacity cleanup returns personal capacity only after its cargo is safe.
    if let Some(mut inventory) = world.get_mut::<GoodsInventory>(worker) {
        inventory.resize_bulk_capacity(shared::economy::capacity::PORTER);
    }
    let cart = shared::economy::PorterCartState::for_used_bulk(
        world
            .get::<GoodsInventory>(worker)
            .map_or(0, GoodsInventory::used_bulk),
    );
    world.entity_mut(worker).insert((
        crate::world::village::PorterCargoCapacity,
        cart,
        RoadBuilderRoutine::regional(bridge_entity, hall),
        BridgeBuilder {
            bridge: bridge_entity,
            source: source_inventory,
            pickup,
            carried: None,
            worked: 0.0,
            returning: false,
        },
    ));
}

/// Retain activity and material title until an actual return to the reserved
/// Hall stock. Cancellation cannot turn carried municipal timber into a gift.
pub(crate) fn cancel_bridge_work(world: &mut World, worker: Entity) -> bool {
    let Some(job) = world.get::<BridgeBuilder>(worker) else {
        return true;
    };
    if job.carried.is_none() {
        world
            .entity_mut(worker)
            .remove::<BridgeBuilder>()
            .remove::<RoadBuilderRoutine>();
        let personal = world.get::<HomeRoutine>(worker).is_some()
            || world.get::<HouseholdShoppingRoutine>(worker).is_some()
            || world.get::<MootMealRoutine>(worker).is_some();
        if !personal {
            settle_motion(world, worker);
        }
        return true;
    }
    world.get_mut::<BridgeBuilder>(worker).unwrap().returning = true;
    false
}

/// Actual bank-side labour remains payable if a project later cancels.
/// Full completion credit still belongs to the finished authoritative deck.
pub(crate) fn work_progress(world: &World, worker: Entity, site: Entity) -> u64 {
    let Some(job) = world
        .get::<BridgeBuilder>(worker)
        .filter(|job| job.bridge == site)
    else {
        return 0;
    };
    let Some(bridge) = world
        .get::<RoadBridge>(site)
        .filter(|bridge| bridge.valid())
    else {
        return 0;
    };
    let total = required_work(bridge);
    (job.worked.floor().max(0.0) as u64).min(total.saturating_sub(1))
}

fn destination(world: &mut World, worker: Entity, target: Vec3) {
    if world
        .get::<MoveTarget>(worker)
        .is_none_or(|old| old.0.distance_squared(target) > 0.01)
    {
        world
            .entity_mut(worker)
            .insert((MoveTarget(target), CharacterActivity::Idle));
    }
}

fn at(world: &World, worker: Entity, target: Vec3) -> bool {
    world
        .get::<PlayerPosition>(worker)
        .is_some_and(|p| p.0.xz().distance_squared(target.xz()) <= 0.65_f32.powi(2))
}

fn settle_motion(world: &mut World, worker: Entity) {
    world
        .entity_mut(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
}

/// Half-world-second cadence over active contracts only. Work requires both
/// real materials and an embodied worker at the dry bank, including after a
/// meal or overnight pause. A failed journey remains visible to project recovery.
pub(crate) fn run_bridge_work(
    world: &mut World,
    mut previous: Local<Option<f64>>,
    mut clocks: Local<Option<bevy::ecs::query::QueryState<&'static WorldTime>>>,
    mut workers: Local<Option<bevy::ecs::query::QueryState<(Entity, &'static BridgeBuilder)>>>,
) {
    let clocks = clocks.get_or_insert_with(|| world.query::<&WorldTime>());
    let Some(clock) = clocks.iter(world).next().cloned() else {
        return;
    };
    let now = f64::from(clock.day) * f64::from(clock.cycle_duration())
        + f64::from(clock.seconds_in_cycle);
    let Some(last) = *previous else {
        *previous = Some(now);
        return;
    };
    if now < last {
        *previous = Some(now);
        return;
    }
    if now - last < 0.5 {
        return;
    }
    *previous = Some(now);
    if !clock.is_ordinary_work_time() {
        return;
    }
    let dt = crate::world::village::worker_activity::schedule::ORDINARY
        .productive_seconds_ending_at(&clock, now - last) as f32;
    let workers = workers.get_or_insert_with(|| world.query::<(Entity, &BridgeBuilder)>());
    let active: Vec<_> = workers.iter(world).map(|(e, w)| (e, w.clone())).collect();
    for (worker, mut job) in active {
        if world.get::<HomeRoutine>(worker).is_some()
            || world.get::<HouseholdShoppingRoutine>(worker).is_some()
            || world.get::<MootMealRoutine>(worker).is_some()
            || world.get::<OfflineHero>(worker).is_some()
        {
            continue;
        }
        if job.returning {
            if let Some((good, carried)) = job.carried {
                if !at(world, worker, job.pickup) {
                    destination(world, worker, job.pickup);
                    continue;
                }
                settle_motion(world, worker);
                let actual = world
                    .get::<GoodsInventory>(worker)
                    .map_or(0, |stock| stock.amount(good))
                    .min(carried);
                let restored = world
                    .get_mut::<GoodsInventory>(job.source)
                    .map_or(0, |mut stock| stock.add(good, actual));
                if let Some(mut stock) = world.get_mut::<GoodsInventory>(worker) {
                    stock.remove(good, restored);
                }
                job.carried = (restored < actual).then_some((good, actual - restored));
            }
            world.entity_mut(worker).insert(job);
            continue;
        }
        let Some(bridge) = world.get::<RoadBridge>(job.bridge).cloned() else {
            continue;
        };
        if bridge.built {
            continue;
        }
        let Some(stock) = world.get::<GoodsInventory>(job.bridge) else {
            continue;
        };
        let needs = [
            (Good::Wood, bridge.wood_required()),
            (Good::Stone, bridge.stone_required()),
        ];
        let needed = needs.into_iter().find_map(|(good, required)| {
            let missing = required.saturating_sub(stock.amount(good));
            (missing > 0).then_some((good, missing))
        });
        if let Some((good, carried)) = job.carried {
            if !at(world, worker, bridge.start) {
                destination(world, worker, bridge.start);
                continue;
            }
            settle_motion(world, worker);
            let actual = world
                .get::<GoodsInventory>(worker)
                .map_or(0, |s| s.amount(good))
                .min(carried);
            let added = world
                .get_mut::<GoodsInventory>(job.bridge)
                .unwrap()
                .add(good, actual);
            if let Some(mut stock) = world.get_mut::<GoodsInventory>(worker) {
                stock.remove(good, added);
            }
            job.carried = (added < carried && actual > added).then_some((good, carried - added));
        } else if let Some((good, missing)) = needed {
            if !at(world, worker, job.pickup) {
                destination(world, worker, job.pickup);
                continue;
            }
            settle_motion(world, worker);
            let Some(source) = world.get::<GoodsInventory>(job.source) else {
                continue;
            };
            let available = source.amount(good).min(missing);
            let taken = world
                .get_mut::<GoodsInventory>(worker)
                .map_or(0, |mut stock| stock.add(good, available));
            if taken > 0 {
                world
                    .get_mut::<GoodsInventory>(job.source)
                    .unwrap()
                    .remove(good, taken);
                job.carried = Some((good, taken));
            }
        } else {
            if !at(world, worker, bridge.start) {
                destination(world, worker, bridge.start);
                continue;
            }
            settle_motion(world, worker);
            if let Some(mut activity) = world.get_mut::<CharacterActivity>(worker) {
                activity.set_if_neq(CharacterActivity::Building);
            }
            let direction = bridge.end.xz() - bridge.start.xz();
            if let Some(mut rotation) = world.get_mut::<PlayerRotation>(worker) {
                rotation.set_if_neq(PlayerRotation(f32::atan2(-direction.x, -direction.y)));
            }
            job.worked += dt;
            // Continuous paid labour grows with actual span and deck width.
            if job.worked >= required_work(&bridge) as f32 {
                match revalidate(world, &bridge) {
                    None => {
                        world.entity_mut(worker).insert(job);
                        continue;
                    }
                    Some(false) => {
                        if let Some(mut project) =
                            world.get_mut::<super::RegionalProject>(job.source)
                        {
                            project.status = super::projects::ProjectStatus::Cancelling;
                        }
                        world.entity_mut(worker).insert(job);
                        continue;
                    }
                    Some(true) => {}
                }
                let mut supplies = world.get_mut::<GoodsInventory>(job.bridge).unwrap();
                for (good, amount) in needs {
                    assert_eq!(supplies.remove(good, amount), amount);
                }
                world.get_mut::<RoadBridge>(job.bridge).unwrap().built = true;
                world
                    .entity_mut(worker)
                    .remove::<BridgeBuilder>()
                    .remove::<RoadBuilderRoutine>()
                    .insert(CharacterActivity::Idle);
                continue;
            }
        }
        world.entity_mut(worker).insert(job);
    }
}
