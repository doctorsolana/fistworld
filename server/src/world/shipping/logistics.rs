//! Finite, titled Hall-to-shore construction deliveries. Trade stock uses the
//! public market at the port and never enters this additional haul lifecycle.

use crate::player::hero::{MoveTarget, OfflineHero};
use crate::world::simulation_time::SimulationDelta;
use crate::world::village::worker_activity::{PersonalNeedsOwnMovement, ProductionStartBlocked};
use crate::world::village::{HomeRoutine, VillagerIntent};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{Good, GoodsInventory, Wallet};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PortHaulRequest {
    pub owner: PortCargoOwner,
    pub settlement: SettlementId,
    pub hall: Entity,
    pub source: Entity,
    pub destination: Entity,
    pub pickup: Vec3,
    pub delivery: Vec3,
    pub good: Good,
    pub units: u32,
    /// Already debited by the caller; ownership transfers to this job.
    pub fee_escrow: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortHaulStatus {
    Waiting,
    Hauling,
    Returning,
    Completed,
    Cancelled,
}

#[derive(Component)]
struct ActivePortHaul;

#[derive(Component, Clone, Debug)]
pub(crate) struct PortHaulJob {
    pub request: PortHaulRequest,
    pub delivered: u32,
    pub fee_remaining: u64,
    pub status: PortHaulStatus,
    worker: Option<Entity>,
    cancelled: bool,
    recruit_in: f32,
}

impl PortHaulJob {
    pub(crate) fn finished(&self) -> bool {
        self.worker.is_none()
            && matches!(
                self.status,
                PortHaulStatus::Completed | PortHaulStatus::Cancelled
            )
    }
}

/// Tracks only this reservation's cargo. Personal food and other goods stay
/// in the same physical inventory without being counted or returned as freight.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PortHaulRoutine {
    pub(crate) job: Entity,
    carried: u32,
    delivering: bool,
}

pub(crate) fn quote_haul_fee(
    clock: &WorldTime,
    pickup: Vec3,
    delivery: Vec3,
    good: Good,
    units: u32,
    daily_wage: u64,
) -> u64 {
    let load = (shared::economy::capacity::VILLAGER / good.bulk_per_unit().max(1)).max(1);
    let trips = units.div_ceil(load);
    let seconds = f64::from(trips)
        * (2.0 * f64::from(pickup.xz().distance(delivery.xz()))
            / f64::from(shared::player::HERO_MOVE_SPEED)
            + 4.0);
    // Price this finite contract against the same ordinary shift as the
    // production systems, including custom-length lab/world clocks.
    ((seconds / f64::from(clock.ordinary_shift_seconds()).max(1.0) * daily_wage as f64).ceil()
        as u64)
        .max(u64::from(units > 0))
}

pub(crate) fn spawn_port_haul(world: &mut World, request: PortHaulRequest) -> Entity {
    let entity = world
        .spawn(PortHaulJob {
            request,
            delivered: 0,
            fee_remaining: request.fee_escrow,
            status: if request.units == 0 {
                PortHaulStatus::Completed
            } else {
                PortHaulStatus::Waiting
            },
            worker: None,
            cancelled: false,
            recruit_in: 0.0,
        })
        .id();
    if request.units > 0 {
        world.entity_mut(entity).insert(ActivePortHaul);
    }
    entity
}

pub(crate) fn cancel_port_haul(world: &mut World, entity: Entity) {
    if let Some(mut job) = world.get_mut::<PortHaulJob>(entity) {
        if !job.finished() {
            job.cancelled = true;
        }
    }
}

fn transfer(world: &mut World, from: Entity, to: Entity, good: Good, requested: u32) -> u32 {
    if from == to {
        return 0;
    }
    let mut stores = world.query::<&mut GoodsInventory>();
    let Ok([mut source, mut destination]) = stores.get_many_mut(world, [from, to]) else {
        return 0;
    };
    source.transfer_to(&mut destination, good, requested)
}

fn approach(world: &mut World, worker: Entity, destination: Vec3) -> bool {
    if world
        .get::<PlayerPosition>(worker)
        .is_some_and(|p| p.0.xz().distance_squared(destination.xz()) <= 0.7 * 0.7)
    {
        return true;
    }
    if world
        .get::<MoveTarget>(worker)
        .is_none_or(|target| target.0.distance_squared(destination) > 0.01)
    {
        world.entity_mut(worker).insert(MoveTarget(destination));
    }
    false
}

fn release(world: &mut World, worker: Entity) {
    if let Ok(mut actor) = world.get_entity_mut(worker) {
        actor.remove::<(
            PortHaulRoutine,
            MoveTarget,
            TravelRoute,
            NavigationRoutePending,
            NavigationRouteFailed,
        )>();
        actor.insert(CharacterActivity::Idle);
    }
}

fn recruit(world: &mut World, job: &PortHaulJob) -> Option<Entity> {
    let mut query = world.query_filtered::<(
        Entity,
        &ResidentOf,
        &PersonId,
        &PlayerPosition,
        &GoodsInventory,
        &VillagerIntent,
    ), (
        Without<EmployedAt>,
        Without<CivicEmployment>,
        Without<Hero>,
        Without<OfflineHero>,
        Without<crate::player::hero::MoveTarget>,
        Without<crate::world::village_roads::TravelRoute>,
        Without<crate::world::village_roads::NavigationRoutePending>,
        Without<PortHaulRoutine>,
        Without<MemberOfBattalion>,
        Without<CommandedBy>,
        With<Wallet>,
    )>();
    let candidates: Vec<_> = query.iter(world).filter(|(_, resident, _, _, inventory, intent)| {
        resident.0 == job.request.settlement && inventory.free_bulk_for(job.request.good) >= job.request.good.bulk_per_unit()
            && matches!(intent, VillagerIntent::Resident { settlement } if *settlement == job.request.hall)
    }).map(|(entity, _, id, position, _, _)| (entity, position.0.xz().distance_squared(job.request.pickup.xz()), *id)).collect();
    let mut blocked = world.query_filtered::<Entity, ProductionStartBlocked>();
    candidates
        .into_iter()
        .filter(|(entity, _, _)| {
            blocked.get(world, *entity).is_err()
                && world.get::<Wallet>(*entity).is_some_and(|wallet| {
                    wallet
                        .balance()
                        .checked_add(job.request.fee_escrow)
                        .is_some()
                })
                && world
                    .get::<Health>(*entity)
                    .is_none_or(|health| health.current > 0.0)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)))
        .map(|item| item.0)
}

fn owed(job: &PortHaulJob) -> u64 {
    let earned = (u128::from(job.request.fee_escrow) * u128::from(job.delivered)
        / u128::from(job.request.units.max(1))) as u64;
    earned.saturating_sub(job.request.fee_escrow - job.fee_remaining)
}

fn pay_earned(world: &mut World, worker: Entity, job: &mut PortHaulJob) {
    if let Some(mut wallet) = world.get_mut::<Wallet>(worker) {
        let pay = owed(job)
            .min(job.fee_remaining)
            .min(u64::MAX - wallet.balance());
        wallet.credit(pay);
        job.fee_remaining -= pay;
    }
}

pub(crate) fn advance_port_hauls(world: &mut World) {
    let dt = world
        .get_resource::<SimulationDelta>()
        .copied()
        .unwrap_or_default()
        .world_seconds();
    let working = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .is_none_or(|time| {
            crate::world::village::worker_activity::schedule::ORDINARY.contains(time)
        });
    let jobs: Vec<_> = world
        .query_filtered::<(Entity, &PortHaulJob), With<ActivePortHaul>>()
        .iter(world)
        .filter(|(_, job)| !job.finished())
        .map(|(entity, job)| (entity, job.clone()))
        .collect();
    let mut hire_attempts = 2;
    for (entity, mut job) in jobs {
        if job.worker.is_none() {
            if job.cancelled {
                job.status = PortHaulStatus::Cancelled;
            } else {
                job.recruit_in -= dt;
                if working && job.recruit_in <= 0.0 && hire_attempts > 0 {
                    hire_attempts -= 1;
                    job.recruit_in = 5.0;
                    if let Some(worker) = recruit(world, &job) {
                        world.entity_mut(worker).insert((PortHaulRoutine {
                            job: entity,
                            carried: 0,
                            delivering: false,
                        },));
                        job.worker = Some(worker);
                        job.status = PortHaulStatus::Hauling;
                    }
                }
            }
        }
        if let Some(worker) = job.worker {
            // Retain missing/dead cargo owners for explicit recovery instead of
            // silently cloning a replacement load or deleting escrow.
            if let Some(mut routine) = world
                .get::<PortHaulRoutine>(worker)
                .copied()
                .filter(|routine| routine.job == entity)
            {
                pay_earned(world, worker, &mut job);
                let paused = world
                    .query_filtered::<Entity, PersonalNeedsOwnMovement>()
                    .get(world, worker)
                    .is_ok()
                    || world.get::<HomeRoutine>(worker).is_some();
                if !paused && (working || job.cancelled) {
                    if job.cancelled {
                        job.status = PortHaulStatus::Returning;
                        if routine.carried == 0 || approach(world, worker, job.request.pickup) {
                            routine.carried -= transfer(
                                world,
                                worker,
                                job.request.source,
                                job.request.good,
                                routine.carried,
                            );
                            if routine.carried == 0 && owed(&job) == 0 {
                                release(world, worker);
                                job.worker = None;
                                job.status = PortHaulStatus::Cancelled;
                            }
                        }
                    } else if routine.delivering {
                        if approach(world, worker, job.request.delivery) {
                            let moved = transfer(
                                world,
                                worker,
                                job.request.destination,
                                job.request.good,
                                routine.carried,
                            );
                            routine.carried -= moved;
                            job.delivered += moved;
                            pay_earned(world, worker, &mut job);
                            if routine.carried == 0 {
                                // Keep an earned but presently unpayable claim
                                // attached to this worker; owner cancellation may
                                // not refund it or saturating-credit it away.
                                routine.delivering = job.delivered == job.request.units;
                                if job.delivered == job.request.units && owed(&job) == 0 {
                                    release(world, worker);
                                    job.worker = None;
                                    job.status = PortHaulStatus::Completed;
                                }
                            }
                        }
                    } else if approach(world, worker, job.request.pickup) {
                        routine.carried = transfer(
                            world,
                            job.request.source,
                            worker,
                            job.request.good,
                            job.request.units - job.delivered,
                        );
                        routine.delivering = routine.carried > 0;
                    }
                    if job.worker.is_some() {
                        world.entity_mut(worker).insert(routine);
                    }
                }
            }
        }
        if job.finished() {
            world.entity_mut(entity).remove::<ActivePortHaul>();
        }
        world.entity_mut(entity).insert(job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (World, Entity, Entity, Entity, Entity) {
        let mut world = World::new();
        let hall = world.spawn_empty().id();
        let mut stock = GoodsInventory::new(100);
        stock.add(Good::Wood, 6);
        let source = world.spawn(stock).id();
        let destination = world.spawn(GoodsInventory::new(100)).id();
        let actor = world
            .spawn((
                ResidentOf(SettlementId(1)),
                PersonId(1),
                PlayerPosition(Vec3::ZERO),
                GoodsInventory::new(12),
                Wallet::new(0),
                VillagerIntent::Resident { settlement: hall },
            ))
            .id();
        let job = spawn_port_haul(
            &mut world,
            PortHaulRequest {
                owner: PortCargoOwner::Treasury(SettlementId(1)),
                settlement: SettlementId(1),
                hall,
                source,
                destination,
                pickup: Vec3::ZERO,
                delivery: Vec3::X * 10.0,
                good: Good::Wood,
                units: 6,
                fee_escrow: 30,
            },
        );
        (world, job, actor, source, destination)
    }
    #[test]
    fn offscreen_haul_waits_for_existing_journey_then_moves_owned_stock_for_earned_fee() {
        let (mut world, job, actor, source, destination) = fixture();
        world.entity_mut(actor).insert((
            crate::player::hero::MoveTarget(Vec3::Z * 10.),
            crate::world::village_roads::TravelRoute {
                goal: Vec3::Z * 10.,
                waypoints: vec![crate::world::village_roads::RouteWaypoint {
                    position: Vec3::Z * 10.,
                    on_road: false,
                }],
                next: 0,
                geometry_version: 0,
            },
        ));
        advance_port_hauls(&mut world);
        assert!(world.get::<PortHaulRoutine>(actor).is_none());
        assert!(
            world
                .get::<crate::world::village_roads::TravelRoute>(actor)
                .is_some()
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(source)
                .unwrap()
                .amount(Good::Wood),
            6
        );
        // Journey completion is supplied by this assignment/transaction
        // fixture; no observer or camera is added and no cargo is relocated.
        world.entity_mut(actor).remove::<(
            crate::player::hero::MoveTarget,
            crate::world::village_roads::TravelRoute,
        )>();
        for _ in 0..320 {
            advance_port_hauls(&mut world);
        }

        assert_eq!(world.get::<PortHaulRoutine>(actor).unwrap().job, job);
        assert_eq!(world.get::<MoveTarget>(actor).unwrap().0, Vec3::X * 10.);
        assert_eq!(world.get::<PlayerPosition>(actor).unwrap().0, Vec3::ZERO);
        assert_eq!(
            world
                .get::<GoodsInventory>(destination)
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 0);
        // Physical arrival is supplied separately: time spent waiting at the
        // pickup never deposits cargo or earns a delivery fee.
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::X * 10.;
        advance_port_hauls(&mut world);
        // This fixture's 12-bulk bag holds three Wood, so finishing six
        // requires a real return to the source and a second delivery.
        assert!(!world.get::<PortHaulJob>(job).unwrap().finished());
        assert_eq!(world.get::<PortHaulJob>(job).unwrap().delivered, 3);
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 15);
        advance_port_hauls(&mut world);
        assert_eq!(world.get::<MoveTarget>(actor).unwrap().0, Vec3::ZERO);
        assert_eq!(world.get::<PortHaulJob>(job).unwrap().delivered, 3);
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::ZERO;
        advance_port_hauls(&mut world);
        assert_eq!(world.get::<PortHaulRoutine>(actor).unwrap().carried, 3);
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::X * 10.;
        advance_port_hauls(&mut world);
        assert!(world.get::<PortHaulJob>(job).unwrap().finished());
        assert_eq!(
            world
                .get::<GoodsInventory>(destination)
                .unwrap()
                .amount(Good::Wood),
            6
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(source)
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 30);
        assert_eq!(world.get::<PortHaulJob>(job).unwrap().fee_remaining, 0);
    }

    #[test]
    fn cancellation_returns_only_owned_load_before_releasing_worker_and_escrow() {
        let (mut world, job, actor, source, destination) = fixture();
        world
            .get_mut::<GoodsInventory>(actor)
            .unwrap()
            .add(Good::Bread, 1);
        advance_port_hauls(&mut world);
        let carried = world.get::<PortHaulRoutine>(actor).unwrap().carried;
        assert!(carried > 0);
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::X * 5.0;
        cancel_port_haul(&mut world, job);
        advance_port_hauls(&mut world);
        assert!(!world.get::<PortHaulJob>(job).unwrap().finished());
        assert_eq!(world.get::<MoveTarget>(actor).unwrap().0, Vec3::ZERO);
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::ZERO;
        advance_port_hauls(&mut world);
        assert!(world.get::<PortHaulJob>(job).unwrap().finished());
        assert_eq!(
            world
                .get::<GoodsInventory>(source)
                .unwrap()
                .amount(Good::Wood),
            6
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(destination)
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(actor)
                .unwrap()
                .amount(Good::Bread),
            1
        );
        assert_eq!(world.get::<PortHaulJob>(job).unwrap().fee_remaining, 30);
    }
    #[test]
    fn destination_capacity_never_discards_load_or_pays_for_undelivered_units() {
        let (mut world, job, actor, source, destination) = fixture();
        advance_port_hauls(&mut world);
        world
            .get_mut::<GoodsInventory>(destination)
            .unwrap()
            .resize_bulk_capacity(Good::Wood.bulk_per_unit());
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::X * 10.0;
        advance_port_hauls(&mut world);
        assert_eq!(world.get::<PortHaulJob>(job).unwrap().delivered, 1);
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 5);
        let total = [source, actor, destination]
            .into_iter()
            .map(|e| world.get::<GoodsInventory>(e).unwrap().amount(Good::Wood))
            .sum::<u32>();
        assert_eq!(total, 6);
        assert!(!world.get::<PortHaulJob>(job).unwrap().finished());
    }
    #[test]
    fn earned_fee_survives_wallet_capacity_and_cannot_be_refunded_on_cancel() {
        let (mut world, job, actor, source, _) = fixture();
        advance_port_hauls(&mut world);
        world.entity_mut(actor).insert(Wallet::new(u64::MAX));
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = Vec3::X * 10.0;
        advance_port_hauls(&mut world);
        let delivered = world.get::<PortHaulJob>(job).unwrap().delivered;
        assert!(delivered > 0);
        assert_eq!(world.get::<PortHaulJob>(job).unwrap().fee_remaining, 30);
        cancel_port_haul(&mut world, job);
        advance_port_hauls(&mut world);
        assert!(!world.get::<PortHaulJob>(job).unwrap().finished());
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), u64::MAX);
        world.get_mut::<Wallet>(actor).unwrap().debit(30);
        advance_port_hauls(&mut world);
        let state = world.get::<PortHaulJob>(job).unwrap();
        assert!(state.finished());
        assert_eq!(owed(state), 0);
        assert_eq!(state.fee_remaining, 30 - u64::from(delivered) * 5);
        assert_eq!(
            world
                .get::<GoodsInventory>(source)
                .unwrap()
                .amount(Good::Wood),
            6 - delivered
        );
    }
    #[test]
    fn haul_quote_scales_with_the_authoritative_ordinary_shift() {
        let ordinary = WorldTime::new_default();
        let short = WorldTime::new(180., 60., 0.);
        let distance = Vec3::X * 150.;
        let normal_fee = quote_haul_fee(&ordinary, Vec3::ZERO, distance, Good::Wood, 48, 100);
        let short_fee = quote_haul_fee(&short, Vec3::ZERO, distance, Good::Wood, 48, 100);
        let ratio = ordinary.ordinary_shift_seconds() / short.ordinary_shift_seconds();
        assert!(short_fee > normal_fee);
        assert!((short_fee as f32 - normal_fee as f32 * ratio).abs() <= ratio);
        assert_eq!(
            quote_haul_fee(&short, Vec3::ZERO, distance, Good::Wood, 0, 100),
            0
        );
    }
}
