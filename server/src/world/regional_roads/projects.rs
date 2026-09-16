//! Finite wage escrow and physical section progression.

use super::*;
use crate::player::hero::MoveTarget;
use crate::world::village::{
    HomeRoutine, HouseholdShoppingRoutine, MootMealRoutine, VillagerIntent,
};
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, TravelRoute,
};
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::*;
use shared::economy::{CivicAccount, Good, GoodsInventory, MarketSeller, MootMarket, Wallet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectStatus {
    Building,
    Cancelling,
    Completed,
    Cancelled,
}

#[derive(Component, Clone)]
pub(crate) struct RegionalProject {
    pub(crate) pair: SettlementPair,
    pub(crate) hall: Entity,
    pub(crate) settlement: SettlementId,
    pub(crate) worker: Entity,
    pub(crate) worker_id: PersonId,
    pub(crate) status: ProjectStatus,
    pub(crate) escrow_cash: u64,
    pub(crate) wages_paid: u64,
    pub(crate) wages_earned: u64,
    wage_budget: u64,
    total_work: u64,
    completed_work: u64,
    steps: Vec<RegionalStep>,
    next_step: usize,
    pub(crate) current: Option<Entity>,
    name: String,
    builder_name: String,
    pickup: Vec3,
    last_progress_day: u32,
    observed_work: u64,
    last_worker_position: Option<Vec2>,
    last_material_bulk: u32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn start_project(
    world: &mut World,
    pair: SettlementPair,
    hall: Entity,
    settlement: SettlementId,
    name: String,
    pickup: Vec3,
    worker: Entity,
    worker_id: PersonId,
    builder_name: String,
    mut steps: Vec<RegionalStep>,
    supplies: GoodsInventory,
    wages: u64,
    day: u32,
) -> Entity {
    // Work outward from the paying town, so the first assignment does not
    // send an unemployed resident all the way to the opposite end first.
    if settlement != pair.0 {
        steps.reverse();
        for step in &mut steps {
            match step {
                RegionalStep::Dirt(points) => points.reverse(),
                RegionalStep::Bridge(deck) => std::mem::swap(&mut deck.start, &mut deck.end),
            }
        }
    }
    let mut project = RegionalProject {
        pair,
        hall,
        settlement,
        worker,
        worker_id,
        status: ProjectStatus::Building,
        escrow_cash: wages,
        wages_paid: 0,
        wages_earned: 0,
        wage_budget: wages,
        total_work: steps
            .iter()
            .map(RegionalStep::work_units)
            .sum::<u64>()
            .max(1),
        completed_work: 0,
        steps,
        next_step: 0,
        current: None,
        name,
        builder_name,
        pickup,
        last_progress_day: day,
        observed_work: 0,
        last_worker_position: None,
        last_material_bulk: 0,
    };
    let entity = world.spawn((PlayerPosition(pickup), supplies)).id();
    world
        .entity_mut(worker)
        .insert((RegionalRoadWorker { project: entity },));
    let _ = start_next(world, entity, &mut project);
    world.entity_mut(entity).insert(project);
    entity
}

fn start_next(world: &mut World, entity: Entity, project: &mut RegionalProject) -> Option<bool> {
    let Some(step) = project.steps.get(project.next_step) else {
        return Some(false);
    };
    let valid = match step {
        RegionalStep::Dirt(points) => {
            crate::world::village_roads::regional_section_clear(world, points, 2.6)
        }
        RegionalStep::Bridge(deck) => bridge::revalidate(world, deck),
    };
    match valid {
        Some(true) => {}
        other => return other,
    }

    let section = match step {
        RegionalStep::Dirt(points) => {
            let trees = crate::world::village_roads::regional_tree_clearance(world, points, 2.6);
            world
                .spawn((
                    VillageRoad {
                        settlement: project.name.clone(),
                        builder: project.builder_name.clone(),
                        points: points.clone(),
                        built_through: 1,
                        width: 2.6,
                        reserved_width: 2.6,
                        surface: RoadSurface::Dirt,
                        class: RoadClass::Lane,
                        stone_committed: 0,
                    },
                    RoadOf(project.settlement),
                    shared::region::RegionCoord::from_world_pos(Vec3::new(
                        points[0].x,
                        0.0,
                        points[0].y,
                    )),
                    RegionalRoadSection { project: entity },
                    trees,
                    Replicate::to_clients(NetworkTarget::All),
                ))
                .id()
        }
        RegionalStep::Bridge(deck) => {
            let capacity = deck
                .wood_required()
                .saturating_mul(Good::Wood.bulk_per_unit())
                .saturating_add(
                    deck.stone_required()
                        .saturating_mul(Good::Stone.bulk_per_unit()),
                );
            world
                .spawn((
                    deck.clone(),
                    crate::world::village_roads::PlannedRoadAccess {
                        settlement_id: project.settlement,
                        points: vec![deck.start.xz(), deck.end.xz()],
                        half_width: deck.width * 0.5,
                    },
                    RoadOf(project.settlement),
                    RegionalRoadSection { project: entity },
                    GoodsInventory::new(capacity),
                    PlayerPosition(deck.midpoint()),
                    shared::region::RegionCoord::from_world_pos(deck.midpoint()),
                    Replicate::to_clients(NetworkTarget::All),
                ))
                .id()
        }
    };
    clear_route(world, project.worker);
    world.entity_mut(project.worker).insert((
        RoadBuilderRoutine::regional(section, project.hall),
        VillagerIntent::RoadBuilding {
            settlement: project.hall,
            road: section,
        },
        CharacterActivity::Idle,
    ));
    if matches!(step, RegionalStep::Bridge(_)) {
        bridge::start_bridge_work(
            world,
            project.worker,
            project.hall,
            entity,
            section,
            project.pickup,
        );
    }
    project.current = Some(section);
    project.last_worker_position = world
        .get::<PlayerPosition>(project.worker)
        .map(|p| p.0.xz());
    project.last_material_bulk = 0;
    Some(true)
}

fn clear_route(world: &mut World, worker: Entity) {
    world
        .entity_mut(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
}

/// There are at most two active projects. Read their actor/section by entity;
/// never scan all roads or people in this progress pass.
pub(crate) fn advance_regional_projects(
    world: &mut World,
    mut clocks: Local<Option<bevy::ecs::query::QueryState<&'static WorldTime>>>,
) {
    let clocks = clocks.get_or_insert_with(|| world.query::<&WorldTime>());
    let Some(day) = clocks.iter(world).next().map(|clock| clock.day) else {
        return;
    };
    let Some(mut state) = world.remove_resource::<RegionalInfrastructure>() else {
        return;
    };
    state.active.retain(|entity| {
        let Ok(mut entity_mut) = world.get_entity_mut(*entity) else {
            return false;
        };
        let Some(mut project) = entity_mut.take::<RegionalProject>() else {
            return false;
        };
        drop(entity_mut);
        let owns_worker = world
            .get::<RegionalRoadWorker>(project.worker)
            .is_some_and(|owner| owner.project == *entity)
            && world.get::<PersonId>(project.worker) == Some(&project.worker_id);
        if project.status == ProjectStatus::Cancelling
            || !owns_worker
            || world.get::<Settlement>(project.hall).is_none()
        {
            cancel(world, *entity, &mut project, day);
        } else if let Some(current) = project.current {
            let step = &project.steps[project.next_step];
            let step_work = step.work_units();
            let (work, finished, valid) = match step {
                RegionalStep::Dirt(expected) => {
                    world
                        .get::<VillageRoad>(current)
                        .map_or((0, false, false), |road| {
                            (
                                u64::from(road.built_through.saturating_sub(1))
                                    .min(step.work_units()),
                                road.is_complete(),
                                road.points.len() == expected.len(),
                            )
                        })
                }
                RegionalStep::Bridge(_) => {
                    world
                        .get::<RoadBridge>(current)
                        .map_or((0, false, false), |deck| {
                            (
                                if deck.built {
                                    step.work_units()
                                } else {
                                    bridge::work_progress(world, project.worker, current)
                                        .min(step.work_units().saturating_sub(1))
                                },
                                deck.built,
                                deck.valid(),
                            )
                        })
                }
            };
            if matches!(step, RegionalStep::Bridge(_)) {
                let personal = world.get::<HomeRoutine>(project.worker).is_some()
                    || world
                        .get::<HouseholdShoppingRoutine>(project.worker)
                        .is_some()
                    || world.get::<MootMealRoutine>(project.worker).is_some();
                let bulk = world
                    .get::<GoodsInventory>(current)
                    .map_or(0, GoodsInventory::used_bulk);
                let position = world
                    .get::<PlayerPosition>(project.worker)
                    .map(|p| p.0.xz());
                let travelled = !personal
                    && position.is_some_and(|p| {
                        project
                            .last_worker_position
                            .is_none_or(|last| p.distance_squared(last) >= 8.0_f32.powi(2))
                    });
                if bulk > project.last_material_bulk || travelled {
                    project.last_progress_day = day;
                    project.last_material_bulk = bulk;
                    if travelled {
                        project.last_worker_position = position;
                    }
                }
            }
            let valid = valid
                && world
                    .get::<RegionalRoadSection>(current)
                    .is_some_and(|section| section.project == *entity);
            let progress = project.completed_work.saturating_add(work);
            if progress > project.observed_work {
                project.observed_work = progress;
                project.last_progress_day = day;
            }
            project.wages_earned = earned_wages(project.wage_budget, progress, project.total_work);
            pay_earned(world, &mut project, day);
            let paused = world.get::<HomeRoutine>(project.worker).is_some()
                || world
                    .get::<HouseholdShoppingRoutine>(project.worker)
                    .is_some()
                || world.get::<MootMealRoutine>(project.worker).is_some();
            if !valid
                || day.saturating_sub(project.last_progress_day) >= 7
                || (!finished
                    && !paused
                    && world.get::<RoadBuilderRoutine>(project.worker).is_none())
            {
                cancel(world, *entity, &mut project, day);
            } else if finished {
                project.completed_work = project.completed_work.saturating_add(step_work);
                project.next_step += 1;
                project.current = None;
                if project.next_step == project.steps.len() {
                    project.wages_earned = project.wage_budget;
                    pay_earned(world, &mut project, day);
                    release_worker(world, *entity, &project);
                    project.status = ProjectStatus::Completed;
                    state.completed.insert(project.pair);
                    return_material_stock(world, *entity, &project);
                } else {
                    if start_next(world, *entity, &mut project) == Some(false) {
                        cancel(world, *entity, &mut project, day);
                    }
                }
            }
        } else if day.saturating_sub(project.last_progress_day) >= 7
            || start_next(world, *entity, &mut project) == Some(false)
        {
            cancel(world, *entity, &mut project, day);
        }
        let active = matches!(
            project.status,
            ProjectStatus::Building | ProjectStatus::Cancelling
        );
        world.entity_mut(*entity).insert(project);
        active
    });
    world.insert_resource(state);
}

fn earned_wages(budget: u64, completed: u64, total: u64) -> u64 {
    ((u128::from(budget) * u128::from(completed.min(total))) / u128::from(total.max(1))) as u64
}

fn pay_earned(world: &mut World, project: &mut RegionalProject, day: u32) {
    let due = project
        .wages_earned
        .saturating_sub(project.wages_paid)
        .min(project.escrow_cash);
    if due == 0 || world.get::<PersonId>(project.worker) != Some(&project.worker_id) {
        return;
    }
    let Some(mut wallet) = world.get_mut::<Wallet>(project.worker) else {
        return;
    };
    let payable = due.min(u64::MAX - wallet.balance());
    wallet.credit(payable);
    project.escrow_cash -= payable;
    project.wages_paid += payable;
    if let Some(mut account) = world.get_mut::<CivicAccount>(project.hall) {
        account.record_wage_expense(day.saturating_add(1), payable);
    }
}

fn cancel(world: &mut World, entity: Entity, project: &mut RegionalProject, day: u32) {
    // Cancellation may arrive before this tick's ordinary observation pass.
    // Snapshot actual on-site labour before the bridge runner is removed.
    if let (Some(current), Some(RegionalStep::Bridge(deck))) =
        (project.current, project.steps.get(project.next_step))
    {
        let total = bridge::required_work(deck);
        let work = if world
            .get::<RoadBridge>(current)
            .is_some_and(|deck| deck.built)
        {
            total
        } else {
            bridge::work_progress(world, project.worker, current).min(total.saturating_sub(1))
        };
        project.wages_earned = project.wages_earned.max(earned_wages(
            project.wage_budget,
            project.completed_work.saturating_add(work),
            project.total_work,
        ));
    }
    pay_earned(world, project, day);
    project.status = ProjectStatus::Cancelling;
    if !bridge::cancel_bridge_work(world, project.worker) {
        return;
    }
    // Earned but unpayable/deceased-worker wages remain a named claim on this
    // durable project. Refund only unearned money; never saturate it away.
    let due = project.wages_earned.saturating_sub(project.wages_paid);
    if let Some(mut hall) = world.get_mut::<Settlement>(project.hall) {
        let refund = project
            .escrow_cash
            .saturating_sub(due)
            .min(u64::MAX - hall.treasury);
        hall.treasury += refund;
        project.escrow_cash -= refund;
    }
    if let Some(current) = project.current {
        if let Some(mut road) = world.get_mut::<VillageRoad>(current) {
            let built = usize::from(road.built_through).min(road.points.len());
            road.points.truncate(built);
        }
        // Delivered bridge stock stays at the physical bank, on the cancelled
        // publicly owned section. It cannot be refunded as money or silently
        // teleported back into the Hall's sale book.
    }
    release_worker(world, entity, project);
    return_material_stock(world, entity, project);
    project.status = ProjectStatus::Cancelled;
}

fn release_worker(world: &mut World, entity: Entity, project: &RegionalProject) {
    if world
        .get::<RegionalRoadWorker>(project.worker)
        .is_none_or(|owner| owner.project != entity)
    {
        return;
    }
    // Preserve actual carried goods and any personal errand's route. Contract
    // cancellation does not confiscate unrelated property or interrupt a meal.
    let personal = world.get::<HomeRoutine>(project.worker).is_some()
        || world
            .get::<HouseholdShoppingRoutine>(project.worker)
            .is_some()
        || world.get::<MootMealRoutine>(project.worker).is_some();
    if !personal {
        clear_route(world, project.worker);
    }
    world
        .entity_mut(project.worker)
        .remove::<RegionalRoadWorker>()
        .remove::<RoadBuilderRoutine>()
        .remove::<bridge::BridgeBuilder>();
    if matches!(
        world.get::<VillagerIntent>(project.worker),
        Some(VillagerIntent::RoadBuilding { .. })
    ) {
        world
            .entity_mut(project.worker)
            .insert(VillagerIntent::Resident {
                settlement: project.hall,
            });
    }
    if !personal {
        world
            .entity_mut(project.worker)
            .insert(CharacterActivity::Idle);
    }
}

fn return_material_stock(world: &mut World, entity: Entity, project: &RegionalProject) {
    let Some(mut stock) = world.get::<GoodsInventory>(entity).cloned() else {
        return;
    };
    let Some(mut hall) = world.get::<GoodsInventory>(project.hall).cloned() else {
        return;
    };
    let Some(mut market) = world.get::<MootMarket>(project.hall).cloned() else {
        return;
    };
    for good in [Good::Wood, Good::Stone] {
        let returned = hall.add(good, stock.amount(good));
        stock.remove(good, returned);
        market.consign(
            MarketSeller::Treasury(project.settlement),
            good,
            returned,
            good.base_price(),
        );
    }
    // This source stock never left the Hall. Any full-bay remainder remains
    // reserved in the project inventory at that same physical pickup point.
    world.entity_mut(entity).insert(stock);
    world.entity_mut(project.hall).insert((hall, market));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn piecework_payment_is_cumulative_exact_and_cannot_pay_beyond_escrow() {
        assert_eq!(earned_wages(101, 1, 3), 33);
        assert_eq!(earned_wages(101, 2, 3), 67);
        assert_eq!(earned_wages(101, 3, 3), 101);
        assert_eq!(earned_wages(u64::MAX, u64::MAX, u64::MAX), u64::MAX);
    }

    fn payment_fixture() -> (App, Entity, Entity, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<RegionalInfrastructure>()
            .add_systems(Update, advance_regional_projects);
        app.world_mut().spawn(WorldTime::new_default());
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Roadstead".into(),
                    tier: SettlementTier::Village,
                    residents: 3,
                    treasury: 899,
                },
                GoodsInventory::new(100),
                MootMarket::founding(),
            ))
            .id();
        let person = PersonId(44);
        let worker = app
            .world_mut()
            .spawn((
                person,
                Wallet::new(0),
                GoodsInventory::new(24),
                CharacterActivity::Idle,
                VillagerIntent::Resident { settlement: hall },
            ))
            .id();
        let project_entity = app.world_mut().spawn(GoodsInventory::new(1)).id();
        let points = vec![Vec2::ZERO, Vec2::X * 2.0, Vec2::X * 4.0, Vec2::X * 6.0];
        let road = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Roadstead".into(),
                builder: "Rowan".into(),
                points: points.clone(),
                built_through: 1,
                width: 2.6,
                reserved_width: 2.6,
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            })
            .id();
        app.world_mut()
            .entity_mut(road)
            .insert(RegionalRoadSection {
                project: project_entity,
            });
        app.world_mut().entity_mut(worker).insert((
            RegionalRoadWorker {
                project: project_entity,
            },
            RoadBuilderRoutine::regional(road, hall),
        ));
        let project = RegionalProject {
            pair: SettlementPair(SettlementId(1), SettlementId(2)),
            hall,
            settlement: SettlementId(1),
            worker,
            worker_id: person,
            status: ProjectStatus::Building,
            escrow_cash: 101,
            wages_paid: 0,
            wages_earned: 0,
            wage_budget: 101,
            total_work: 3,
            completed_work: 0,
            steps: vec![RegionalStep::Dirt(points)],
            next_step: 0,
            current: Some(road),
            name: "Roadstead".into(),
            builder_name: "Rowan".into(),
            pickup: Vec3::ZERO,
            last_progress_day: 0,
            observed_work: 0,
            last_worker_position: None,
            last_material_bulk: 0,
        };
        app.world_mut().entity_mut(project_entity).insert(project);
        app.world_mut()
            .resource_mut::<RegionalInfrastructure>()
            .active
            .push(project_entity);
        (app, hall, worker, road, project_entity)
    }

    #[test]
    fn only_verified_section_progress_releases_the_reserved_wage_once() {
        let (mut app, hall, worker, road, project) = payment_fixture();
        app.update();
        assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 0);
        app.world_mut()
            .get_mut::<VillageRoad>(road)
            .unwrap()
            .built_through = 2;
        app.update();
        app.update();
        assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 33);
        assert_eq!(
            app.world()
                .get::<RegionalProject>(project)
                .unwrap()
                .escrow_cash,
            68
        );
        app.world_mut()
            .get_mut::<VillageRoad>(road)
            .unwrap()
            .built_through = 4;
        app.update();
        app.update();
        assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 101);
        assert_eq!(
            app.world()
                .get::<RegionalProject>(project)
                .unwrap()
                .escrow_cash,
            0
        );
        assert_eq!(
            app.world().get::<RegionalProject>(project).unwrap().status,
            ProjectStatus::Completed
        );
        assert!(app.world().get::<RegionalRoadWorker>(worker).is_none());
        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().treasury
                + app.world().get::<Wallet>(worker).unwrap().balance(),
            1000
        );
    }

    #[test]
    fn interrupted_regional_work_keeps_paid_prefix_and_refunds_only_unearned_cash() {
        let (mut app, hall, worker, road, project) = payment_fixture();
        app.world_mut()
            .get_mut::<VillageRoad>(road)
            .unwrap()
            .built_through = 2;
        app.update();
        app.world_mut()
            .entity_mut(worker)
            .remove::<RoadBuilderRoutine>();
        app.update();
        assert_eq!(
            app.world().get::<RegionalProject>(project).unwrap().status,
            ProjectStatus::Cancelled
        );
        assert_eq!(
            app.world().get::<VillageRoad>(road).unwrap().points.len(),
            2
        );
        assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 33);
        assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, 967);
        assert_eq!(
            app.world()
                .get::<RegionalProject>(project)
                .unwrap()
                .escrow_cash,
            0
        );
    }
}
