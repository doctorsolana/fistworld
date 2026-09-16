//! One finite construction contract for a public pier or a private hull.

use super::funding;
use crate::player::hero::MoveTarget;
use crate::world::shipping::{PortHaulJob, PortHaulRequest, cancel_port_haul, spawn_port_haul};
use crate::world::village::worker_activity::{self, schedule::ORDINARY};
use crate::world::village::{HomeRoutine, HouseholdShoppingRoutine, MootMealRoutine};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use bevy::prelude::*;
use shared::components::*;
use shared::economy::*;

#[derive(Component, Clone, Copy)]
pub(crate) struct PortBuilder {
    pub(crate) project: Entity,
}

#[derive(Component)]
struct ActivePortWork;

/// These are construction materials, not another town exchange or price book.
#[derive(Component, Clone, Copy)]
pub(crate) struct PortConstructionStock(pub(crate) PortCargoOwner);

struct WorkerContract {
    entity: Entity,
    person: PersonId,
    first_earned: u64,
    paid: u64,
}

#[derive(Component)]
pub(crate) struct PortWorkProject {
    pub(crate) hall: Entity,
    pub(crate) settlement: SettlementId,
    pub(crate) port: Entity,
    pub(crate) owner: PortCargoOwner,
    pub(crate) source: Entity,
    pub(crate) destination: Entity,
    pub(crate) hauls: Vec<Entity>,
    pub(crate) labour_escrow: u64,
    pub(crate) cancelling: bool,
    pub(crate) finished: bool,
    pub(crate) worked: f64,
    materials: Vec<(Good, u32)>,
    total_seconds: f64,
    wage_budget: u64,
    worker: Option<WorkerContract>,
    /// Earned claims survive worker death and cannot become an owner refund.
    unpaid_claims: Vec<(PersonId, u64)>,
    haul_quoted: u64,
    haul_accounted: u64,
    last_time: f64,
    next_hire: f64,
}

pub(super) fn time(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn start_work(
    world: &mut World,
    entity: Entity,
    hall: Entity,
    settlement: SettlementId,
    port: Entity,
    owner: PortCargoOwner,
    materials: Vec<(Good, u32)>,
    total_seconds: f32,
    wage_budget: u64,
    haul_fees: Vec<u64>,
    quote: funding::MaterialQuote,
    clock: &WorldTime,
) {
    let pickup = funding::hall_pickup(world, hall).expect("funded Hall pickup");
    let delivery = world
        .get::<SettlementPort>(port)
        .expect("funded port")
        .geometry
        .shore;
    let bulk = funding::material_bulk(&materials).expect("validated finite material basket");
    world.entity_mut(hall).insert((quote.market, quote.stock));
    world.init_resource::<crate::world::village::BusinessEventQueue>();
    world
        .resource_mut::<crate::world::village::BusinessEventQueue>()
        .record_market_purchase(clock.day, settlement, quote.fills);
    let source = world
        .spawn((
            PlayerPosition(pickup),
            PortConstructionStock(owner),
            quote.supplies,
        ))
        .id();
    let destination = world
        .spawn((
            PlayerPosition(delivery),
            PortConstructionStock(owner),
            GoodsInventory::new(bulk.max(1)),
        ))
        .id();
    let haul_quoted = haul_fees.iter().sum();
    let hauls = materials
        .iter()
        .copied()
        .zip(haul_fees)
        .map(|((good, units), fee_escrow)| {
            spawn_port_haul(
                world,
                PortHaulRequest {
                    owner,
                    settlement,
                    hall,
                    source,
                    destination,
                    pickup,
                    delivery,
                    good,
                    units,
                    fee_escrow,
                },
            )
        })
        .collect();
    world
        .entity_mut(entity)
        .insert(ActivePortWork)
        .insert(PortWorkProject {
            hall,
            settlement,
            port,
            owner,
            source,
            destination,
            hauls,
            labour_escrow: wage_budget,
            cancelling: false,
            finished: false,
            worked: 0.0,
            materials,
            total_seconds: f64::from(total_seconds),
            wage_budget,
            worker: None,
            unpaid_claims: Vec::new(),
            haul_quoted,
            haul_accounted: 0,
            last_time: time(clock),
            next_hire: 0.0,
        });
}

pub(super) fn capitalize(world: &mut World, entity: Entity, day: u32, pennies: u64) {
    if pennies == 0 {
        return;
    }
    if let Some(mut book) = world.get_mut::<BusinessAccount>(entity) {
        book.roll_to_day(day);
        book.capital_expenditures = book.capital_expenditures.saturating_add(pennies);
        book.book_value = book.book_value.saturating_add(pennies);
        book.current_day.capital_expenditures = book
            .current_day
            .capital_expenditures
            .saturating_add(pennies);
    }
}

fn charge_work(
    world: &mut World,
    entity: Entity,
    project: &PortWorkProject,
    day: u32,
    pennies: u64,
) {
    match project.owner {
        PortCargoOwner::Treasury(_) => {
            if let Some(mut account) = world.get_mut::<CivicAccount>(project.hall) {
                account.record_wage_expense(day, pennies);
            }
        }
        PortCargoOwner::Company(_) => capitalize(world, entity, day, pennies),
    }
}

fn earned(project: &PortWorkProject) -> u64 {
    let units = (project.worked / project.total_seconds.max(1.0) * 1_000_000.0).floor() as u64;
    (u128::from(project.wage_budget) * u128::from(units.min(1_000_000)) / 1_000_000) as u64
}

fn paused(world: &World, worker: Entity) -> bool {
    world.get::<HomeRoutine>(worker).is_some()
        || world.get::<HouseholdShoppingRoutine>(worker).is_some()
        || world.get::<MootMealRoutine>(worker).is_some()
}

fn release_worker(world: &mut World, entity: Entity, project: &mut PortWorkProject) {
    let Some(worker) = project.worker.take() else {
        return;
    };
    let due = earned(project)
        .saturating_sub(worker.first_earned)
        .saturating_sub(worker.paid);
    if due > 0 {
        project.unpaid_claims.push((worker.person, due));
    }
    if world
        .get::<PortBuilder>(worker.entity)
        .is_none_or(|job| job.project != entity)
    {
        return;
    }
    let personal = paused(world, worker.entity);
    let mut body = world.entity_mut(worker.entity);
    body.remove::<PortBuilder>()
        .insert((Occupation(None), WorkStatus::LookingForWork));
    if !personal {
        body.remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .insert(CharacterActivity::Idle);
    }
}

fn hire(world: &mut World, entity: Entity, project: &mut PortWorkProject, at: Vec3) {
    let blocked: std::collections::HashSet<_> = world
        .query_filtered::<Entity, worker_activity::JobChangeBlocked>()
        .iter(world)
        .collect();
    let candidate = world
        .query_filtered::<(
            Entity,
            &PersonId,
            &ResidentOf,
            &Occupation,
            &GoodsInventory,
            &PlayerPosition,
            &Wallet,
        ), (
            With<CharacterKind>,
            Without<Hero>,
            Without<crate::player::hero::MoveTarget>,
            Without<crate::world::village_roads::TravelRoute>,
            Without<crate::world::village_roads::NavigationRoutePending>,
            Without<CivicEmployment>,
            Without<EmployedAt>,
            Without<CommandedBy>,
        )>()
        .iter(world)
        .filter(|(actor, id, town, occupation, goods, _, wallet)| {
            id.is_assigned()
                && town.0 == project.settlement
                && occupation.0.is_none()
                && goods.is_empty()
                && !blocked.contains(actor)
                && wallet
                    .balance()
                    .checked_add(project.labour_escrow)
                    .is_some()
        })
        .min_by(|a, b| {
            a.5.0
                .distance_squared(at)
                .total_cmp(&b.5.0.distance_squared(at))
                .then_with(|| a.1.cmp(b.1))
        })
        .map(|(actor, id, ..)| (actor, *id));
    if let Some((worker, person)) = candidate {
        project.worker = Some(WorkerContract {
            entity: worker,
            person,
            first_earned: earned(project),
            paid: 0,
        });
        world.entity_mut(worker).insert((
            PortBuilder { project: entity },
            Occupation(Some("Harbour builder".into())),
            WorkStatus::Employed,
        ));
    }
}

fn settle_worker(world: &mut World, entity: Entity, project: &mut PortWorkProject, day: u32) {
    let cumulative = earned(project);
    let Some(worker) = project.worker.as_mut() else {
        return;
    };
    if world.get::<PersonId>(worker.entity) != Some(&worker.person) {
        return;
    }
    let Some(mut wallet) = world.get_mut::<Wallet>(worker.entity) else {
        return;
    };
    let due = cumulative
        .saturating_sub(worker.first_earned)
        .saturating_sub(worker.paid)
        .min(project.labour_escrow)
        .min(u64::MAX - wallet.balance());
    if due == 0 {
        return;
    }
    wallet.credit(due);
    worker.paid += due;
    project.labour_escrow -= due;
    charge_work(world, entity, project, day, due);
}

fn refund_cash(world: &mut World, project: &mut PortWorkProject, pennies: u64) -> u64 {
    match project.owner {
        PortCargoOwner::Treasury(_) => {
            let Some(mut town) = world.get_mut::<Settlement>(project.hall) else {
                return 0;
            };
            let paid = pennies.min(u64::MAX - town.treasury);
            town.treasury += paid;
            paid
        }
        PortCargoOwner::Company(company) => {
            let Some(entity) = funding::company_for(world, company) else {
                return 0;
            };
            let mut account = world.get_mut::<CompanyAccount>(entity).unwrap();
            let paid = pennies.min(u64::MAX - account.cash);
            account.cash += paid;
            paid
        }
    }
}

fn cancel(world: &mut World, entity: Entity, project: &mut PortWorkProject, clock: &WorldTime) {
    settle_worker(world, entity, project, clock.day);
    release_worker(world, entity, project);
    for &haul in &project.hauls {
        cancel_port_haul(world, haul);
    }
    if project.hauls.iter().any(|job| {
        world
            .get::<PortHaulJob>(*job)
            .is_some_and(|job| !job.finished())
    }) {
        return;
    }
    let fees: u64 = project
        .hauls
        .iter()
        .filter_map(|id| world.get::<PortHaulJob>(*id))
        .map(|job| job.fee_remaining)
        .sum();
    // Move refundable cash out of child requests exactly once. Their terminal
    // records and physical material piles stay available for ownership audits.
    for &id in &project.hauls {
        if let Some(mut job) = world.get_mut::<PortHaulJob>(id) {
            job.fee_remaining = 0;
        }
    }
    // Refunded child cash is no longer a quoted delivery cost. A later
    // retry (for example a full receiving treasury) must not book it as wages.
    project.haul_quoted = project.haul_quoted.saturating_sub(fees);
    project.labour_escrow = project
        .labour_escrow
        .checked_add(fees)
        .expect("original funded budget");
    let claims: u64 = project.unpaid_claims.iter().map(|(_, owed)| *owed).sum();
    let refund = project.labour_escrow.saturating_sub(claims);
    let paid = refund_cash(world, project, refund);
    project.labour_escrow -= paid;
    if paid != refund {
        return;
    }
    if world.get::<ShipConstructionOrder>(entity).is_some() {
        if let PortCargoOwner::Company(company) = project.owner {
            let now = time(clock);
            if now < project.next_hire {
                return;
            }
            project.next_hire = now + 30.0;
            if !super::recovery::reclaim(
                world,
                super::recovery::CancelledSupplies {
                    order: entity,
                    hall: project.hall,
                    port: project.port,
                    company,
                    source: project.source,
                    destination: project.destination,
                },
            ) {
                return;
            }
        }
    }
    project.finished = true;
    if let Some(mut order) = world.get_mut::<ShipConstructionOrder>(entity) {
        order.status = ShipOrderStatus::Cancelled;
    }
    // An unfinished public pier is not a market access point. Its shore stock
    // retains civic title; cancelled hull stock at a built port is reconsigned.
}

/// Address only active finite projects; no per-frame world/person scan. The
/// bounded hiring search runs once per 30 simulated seconds per waiting job.
pub(crate) fn advance_port_projects(world: &mut World) {
    let Some(clock) = funding::clock(world) else {
        return;
    };
    let now = time(&clock);
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<ActivePortWork>>()
        .iter(world)
        .collect();
    for entity in entities {
        let Some(mut project) = world.entity_mut(entity).take::<PortWorkProject>() else {
            continue;
        };
        if project.finished {
            world
                .entity_mut(entity)
                .remove::<ActivePortWork>()
                .insert(project);
            continue;
        }
        let elapsed = (now - project.last_time).max(0.0);
        project.last_time = now;
        let fee_remaining: u64 = project
            .hauls
            .iter()
            .filter_map(|id| world.get::<PortHaulJob>(*id))
            .map(|job| job.fee_remaining)
            .sum();
        let spent = project.haul_quoted.saturating_sub(fee_remaining);
        let fresh = spent.saturating_sub(project.haul_accounted);
        if fresh > 0 {
            charge_work(world, entity, &project, clock.day, fresh);
            project.haul_accounted = spent;
        }
        if world.get::<Settlement>(project.hall).is_none()
            || world.get::<SettlementPort>(project.port).is_none()
            || world
                .get::<PortConstructionStock>(project.source)
                .is_none_or(|stock| stock.0 != project.owner)
            || world
                .get::<PortConstructionStock>(project.destination)
                .is_none_or(|stock| stock.0 != project.owner)
        {
            project.cancelling = true;
        }
        if project.cancelling {
            cancel(world, entity, &mut project, &clock);
            world.entity_mut(entity).insert(project);
            continue;
        }
        let ready = project.materials.iter().all(|(good, amount)| {
            world
                .get::<GoodsInventory>(project.destination)
                .is_some_and(|pile| pile.amount(*good) >= *amount)
        });
        if ready {
            if let Some(mut order) = world.get_mut::<ShipConstructionOrder>(entity) {
                if order.status != ShipOrderStatus::Building {
                    order.status = ShipOrderStatus::Building;
                }
            }
        }
        if !ready
            || project.hauls.iter().any(|id| {
                world
                    .get::<PortHaulJob>(*id)
                    .is_none_or(|job| !job.finished())
            })
            || !ORDINARY.contains(&clock)
        {
            if let Some(worker) = project.worker.as_ref().map(|worker| worker.entity) {
                if !paused(world, worker) {
                    if let Some(mut activity) = world.get_mut::<CharacterActivity>(worker) {
                        activity.set_if_neq(CharacterActivity::Idle);
                    }
                }
            }
            world.entity_mut(entity).insert(project);
            continue;
        }
        let target = world
            .get::<SettlementPort>(project.port)
            .unwrap()
            .geometry
            .shore;
        if project.worker.as_ref().is_some_and(|worker| {
            world.get::<PersonId>(worker.entity) != Some(&worker.person)
                || world
                    .get::<Health>(worker.entity)
                    .is_some_and(Health::is_dead)
                || world
                    .get::<PortBuilder>(worker.entity)
                    .is_none_or(|job| job.project != entity)
        }) {
            release_worker(world, entity, &mut project);
        }
        if project.worked < project.total_seconds
            && project.worker.is_none()
            && now >= project.next_hire
        {
            hire(world, entity, &mut project, target);
            project.next_hire = now + 30.0;
        }
        if let Some(worker) = project.worker.as_ref().map(|worker| worker.entity) {
            if !paused(world, worker) {
                let position = world
                    .get::<PlayerPosition>(worker)
                    .map(|position| position.0);
                let at_work = position.is_some_and(|position| {
                    position.xz().distance(target.xz()) <= 0.6
                        && (position.y - target.y).abs() <= 1.0
                });
                if !at_work {
                    if let Some(mut activity) = world.get_mut::<CharacterActivity>(worker) {
                        activity.set_if_neq(CharacterActivity::Idle);
                    }
                    if world
                        .get::<MoveTarget>(worker)
                        .is_none_or(|point| point.0.distance_squared(target) > 0.01)
                    {
                        world.entity_mut(worker).insert(MoveTarget(target));
                    }
                } else {
                    world.entity_mut(worker).remove::<MoveTarget>();
                    if let Some(mut activity) = world.get_mut::<CharacterActivity>(worker) {
                        activity.set_if_neq(CharacterActivity::Building);
                    }
                    project.worked = (project.worked
                        + ORDINARY.productive_seconds_ending_at(&clock, elapsed))
                    .min(project.total_seconds);
                    settle_worker(world, entity, &mut project, clock.day);
                }
            }
        }
        if let Some(mut order) = world.get_mut::<ShipConstructionOrder>(entity) {
            let progress = ((project.worked / project.total_seconds.max(1.0)) * 1000.0)
                .floor()
                .min(1000.0) as u16;
            if order.progress != progress {
                order.progress = progress;
            }
        }
        if project.worked >= project.total_seconds {
            // A finished hull may wait for an occupied berth, but its finite
            // builder job is already earned and must not retain idle staff.
            release_worker(world, entity, &mut project);
            // Ship launch has its own berth/geometry gate; reaching the labour
            // target never grants permission to overlap another hull.
            let completed = if world.get::<ShipConstructionOrder>(entity).is_some() {
                super::orders::launch(world, entity, &project, &clock)
            } else {
                let valid = super::siting::port_still_valid(world, project.port);
                if valid {
                    world.get_mut::<SettlementPort>(project.port).unwrap().built = true;
                }
                valid
            };
            if completed {
                let mut pile = world
                    .get_mut::<GoodsInventory>(project.destination)
                    .expect("ready construction pile");
                for &(good, units) in &project.materials {
                    assert_eq!(pile.remove(good, units), units);
                }
                project.finished = true;
            }
        }
        world.entity_mut(entity).insert(project);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_harbour_builder_is_hired_and_requires_arrival_and_shift_hours_at_1x_and_25x() {
        for warp in [1.0, 25.0] {
            let mut world = World::new();
            let mut clock = WorldTime::new_default();
            clock.set_normalized_time(9.0 / 24.0);
            let clock_entity = world.spawn(clock.clone()).id();
            let town = SettlementId(6);
            let shore = Vec3::new(10., 8., 0.);
            let port = world
                .spawn((
                    Settlement {
                        name: "Testhaven".into(),
                        tier: SettlementTier::Town,
                        residents: 2,
                        treasury: 100,
                    },
                    town,
                    CivicAccount::default(),
                    SettlementPort {
                        settlement: town,
                        geometry: PortGeometry {
                            shore,
                            pier_end: Vec3::new(10., 1., 20.),
                            berth: Vec3::new(10., 0., 26.),
                            departure: Vec3::new(25., 0., 26.),
                            yaw: 0.,
                            maximum_ship: ShipKind::Coaster,
                        },
                        built: false,
                    },
                ))
                .id();
            let owner = PortCargoOwner::Treasury(town);
            let mut goods = GoodsInventory::new(10);
            goods.add(Good::Wood, 1);
            let pile = world
                .spawn((goods, PortConstructionStock(owner), PlayerPosition(shore)))
                .id();
            let actor = world
                .spawn((
                    PersonId(2),
                    ResidentOf(town),
                    CharacterKind::Villager,
                    Occupation(None),
                    GoodsInventory::new(capacity::VILLAGER),
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
                    Wallet::new(0),
                    CharacterActivity::Idle,
                    PlayerPosition(Vec3::new(0., 8., 0.)),
                ))
                .id();
            world.entity_mut(port).insert((
                ActivePortWork,
                PortWorkProject {
                    hall: port,
                    settlement: town,
                    port,
                    owner,
                    source: pile,
                    destination: pile,
                    hauls: Vec::new(),
                    labour_escrow: 180,
                    cancelling: false,
                    finished: false,
                    worked: 0.,
                    materials: vec![(Good::Wood, 1)],
                    total_seconds: 180.,
                    wage_budget: 180,
                    worker: None,
                    unpaid_claims: Vec::new(),
                    haul_quoted: 0,
                    haul_accounted: 0,
                    last_time: time(&clock),
                    next_hire: 0.,
                },
            ));
            let dt = 0.05 * warp;
            world
                .get_mut::<WorldTime>(clock_entity)
                .unwrap()
                .advance(dt, dt);
            advance_port_projects(&mut world);
            assert!(world.get::<PortBuilder>(actor).is_none());
            assert!(
                world
                    .get::<crate::world::village_roads::TravelRoute>(actor)
                    .is_some()
            );
            // The actor finishes its pre-existing journey before applying for
            // the finite contract; wait out the normal30s hiring backoff.
            world.entity_mut(actor).remove::<(
                crate::player::hero::MoveTarget,
                crate::world::village_roads::TravelRoute,
            )>();
            world
                .get_mut::<WorldTime>(clock_entity)
                .unwrap()
                .advance(30.1, 30.1);
            advance_port_projects(&mut world);
            assert_eq!(world.get::<PortBuilder>(actor).unwrap().project, port);

            assert_eq!(
                world.get::<PortWorkProject>(port).unwrap().worked,
                0.,
                "distance must not earn construction or wages"
            );
            assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 0);
            assert_eq!(world.get::<MoveTarget>(actor).unwrap().0, shore);
            // The fixture supplies arrival; this tests the real work boundary,
            // not the intervening land navigator (covered by connected labs).
            world.get_mut::<PlayerPosition>(actor).unwrap().0 = shore;
            world
                .get_mut::<WorldTime>(clock_entity)
                .unwrap()
                .advance(dt, dt);
            advance_port_projects(&mut world);
            let worked = world.get::<PortWorkProject>(port).unwrap().worked;
            assert!(worked > 0.);
            assert_eq!(
                *world.get::<CharacterActivity>(actor).unwrap(),
                CharacterActivity::Building
            );
            world
                .get_mut::<WorldTime>(clock_entity)
                .unwrap()
                .set_normalized_time(20.0 / 24.0);
            advance_port_projects(&mut world);
            assert_eq!(
                world.get::<PortWorkProject>(port).unwrap().worked,
                worked,
                "night must not accrue a waiting builder's work"
            );
            assert_eq!(
                *world.get::<CharacterActivity>(actor).unwrap(),
                CharacterActivity::Idle
            );
            assert_eq!(
                world
                    .get::<GoodsInventory>(pile)
                    .unwrap()
                    .amount(Good::Wood),
                1,
                "partial work never consumes the completed recipe"
            );
            assert_eq!(
                world.get::<Wallet>(actor).unwrap().balance()
                    + world.get::<PortWorkProject>(port).unwrap().labour_escrow,
                180
            );
        }
    }

    #[test]
    fn cancellation_refunds_only_unearned_labor_and_keeps_bought_materials_in_place() {
        let mut world = World::new();
        let clock = WorldTime::new_default();
        let town = SettlementId(1);
        let hall = world
            .spawn(Settlement {
                name: "Haven".into(),
                tier: SettlementTier::Town,
                residents: 1,
                treasury: 0,
            })
            .id();
        let actor = world
            .spawn((PersonId(2), Wallet::new(0), PortBuilder { project: hall }))
            .id();
        let mut goods = GoodsInventory::new(20);
        goods.add(Good::Wood, 2);
        let pile = world.spawn((goods, PlayerPosition(Vec3::X * 30.))).id();
        let haul = spawn_port_haul(
            &mut world,
            PortHaulRequest {
                owner: PortCargoOwner::Treasury(town),
                settlement: town,
                hall,
                source: pile,
                destination: pile,
                pickup: Vec3::ZERO,
                delivery: Vec3::X * 30.,
                good: Good::Wood,
                units: 2,
                fee_escrow: 20,
            },
        );
        let mut project = PortWorkProject {
            hall,
            settlement: town,
            port: hall,
            owner: PortCargoOwner::Treasury(town),
            source: pile,
            destination: pile,
            hauls: vec![haul],
            labour_escrow: 100,
            cancelling: true,
            finished: false,
            worked: 45.,
            materials: vec![(Good::Wood, 2)],
            total_seconds: 180.,
            wage_budget: 100,
            worker: Some(WorkerContract {
                entity: actor,
                person: PersonId(2),
                first_earned: 0,
                paid: 0,
            }),
            unpaid_claims: Vec::new(),
            haul_quoted: 20,
            haul_accounted: 0,
            last_time: 0.,
            next_hire: 0.,
        };
        cancel(&mut world, hall, &mut project, &clock);
        assert!(
            !project.finished,
            "child delivery ownership must finish before refund"
        );
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 25);
        assert_eq!(project.labour_escrow, 75);
        crate::world::shipping::advance_port_hauls(&mut world);
        cancel(&mut world, hall, &mut project, &clock);
        assert!(project.finished);
        assert_eq!(world.get::<PortHaulJob>(haul).unwrap().fee_remaining, 0);
        assert_eq!(
            project.haul_quoted, 0,
            "unused fee is a refund, never labour expense"
        );
        assert_eq!(world.get::<Wallet>(actor).unwrap().balance(), 25);
        assert_eq!(world.get::<Settlement>(hall).unwrap().treasury, 95);
        assert_eq!(project.labour_escrow, 0);
        assert_eq!(
            world
                .get::<GoodsInventory>(pile)
                .unwrap()
                .amount(Good::Wood),
            2
        );
        assert_eq!(world.get::<PlayerPosition>(pile).unwrap().0, Vec3::X * 30.);
        assert!(world.get::<PortBuilder>(actor).is_none());
        world.entity_mut(hall).insert(project);
        assert_eq!(crate::world::village_lab::total_money(&mut world), 120);
    }
}
