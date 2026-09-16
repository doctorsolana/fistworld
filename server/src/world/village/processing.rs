//! Embodied Hamlet processing industries.
//!
//! Windmills and bakeries use the same durable employment, inventories,
//! procurement, pricing, porter and history systems as extractive businesses.
//! This module owns only the physical work shift which transforms stock.

use super::*;

const MAX_PROCESSOR_ROUTE_FAILURES: u8 = 3;

#[derive(Component, Debug, Clone)]
pub struct ProcessingRoutine {
    workplace: Entity,
    hall: Entity,
    recipe: ProcessingRecipe,
    work_seconds: f32,
    failed_routes: u8,
    production_day: u32,
    produced_today: u32,
    phase: ProcessingPhase,
}

impl ProcessingRoutine {
    pub(crate) const fn workplace(&self) -> Entity {
        self.workplace
    }

    pub(crate) const fn hall(&self) -> Entity {
        self.hall
    }

    pub(crate) const fn is_working_inside(&self) -> bool {
        matches!(self.phase, ProcessingPhase::Working)
    }

    pub(crate) const fn objective(&self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        match self.phase {
            ProcessingPhase::GoingToWorkplace => CharacterObjective::GoingToProcessingWork,
            ProcessingPhase::Working => match self.recipe.output {
                Good::Flour => CharacterObjective::MillingFlour,
                Good::Bread => CharacterObjective::BakingBread,
                _ => CharacterObjective::GoingToProcessingWork,
            },
            ProcessingPhase::EndingShift => CharacterObjective::EndingWorkShift,
        }
    }
}

impl super::worker_activity::lifecycle::ProductionLifecycle for ProcessingRoutine {
    type Progress = ProcessorWorkProgress;

    fn saved_progress(&self) -> Self::Progress {
        ProcessorWorkProgress {
            workplace: self.workplace,
            seconds: self.work_seconds,
            production_day: self.production_day,
            produced_today: self.produced_today,
        }
    }

    fn resume_from_workplace(&mut self) {
        self.phase = ProcessingPhase::GoingToWorkplace;
        self.failed_routes = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessingPhase {
    GoingToWorkplace,
    Working,
    EndingShift,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ProcessorWorkProgress {
    workplace: Entity,
    seconds: f32,
    production_day: u32,
    produced_today: u32,
}

/// Start a visible shift for every tactically simulated miller and baker whose
/// completed workplace is connected to the settlement road network.
#[allow(clippy::type_complexity)]
pub fn assign_processing_routines(
    mut commands: Commands,
    off_duty_workers: Query<&WorkerOffDuty>,
    committed_workers: Query<(), super::worker_activity::ProductionStartBlocked>,
    world_time: Query<&WorldTime>,
    workplaces: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&BusinessCondition>,
    )>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    villagers: Query<
        (
            Entity,
            &VillagerIntent,
            Option<&WorkerOffDuty>,
            Option<&ProcessorWorkProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            With<CharacterKind>,
            Without<ProcessingRoutine>,
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<QuarryRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if !super::worker_activity::schedule::ORDINARY.contains(clock) {
        return;
    }

    let mut employees: HashMap<shared::components::BuildingId, Vec<Entity>> = HashMap::new();
    for (entity, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            employees.entry(employment.0).or_default().push(entity);
        }
    }
    for roster in employees.values_mut() {
        roster.sort_unstable_by_key(|entity| entity.to_bits());
    }

    for (workplace, building, at, rotation, building_id, building_of, condition) in
        workplaces.iter()
    {
        let Some(recipe) = processing_recipe(building.kind) else {
            continue;
        };
        if condition.is_some_and(|condition| !condition.state.can_operate()) {
            continue;
        }
        let Some((hall, _, hall_at, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, ..)| **settlement_id == building_of.0)
        else {
            continue;
        };
        let Some(roster) = employees.get(building_id) else {
            continue;
        };
        if !super::trades::workplace_road_is_ready(
            workplace,
            building.kind,
            at.0,
            rotation.0,
            building_of.0,
            hall_at.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            super::workplace_access::defer_shift(
                &mut commands,
                roster,
                workplace,
                clock.day,
                &off_duty_workers,
            );
            continue;
        }

        for worker in roster.iter().take(building.kind.positions() as usize) {
            let Ok((_, intent, off_duty, progress, employment)) = villagers.get(*worker) else {
                continue;
            };
            if committed_workers.get(*worker).is_ok()
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let progress = progress.filter(|progress| progress.workplace == workplace);
            let work_seconds = progress.map_or(0.0, |progress| progress.seconds);
            let production_day = progress.map_or(u32::MAX, |progress| progress.production_day);
            let produced_today = progress.map_or(0, |progress| progress.produced_today);
            commands
                .entity(*worker)
                .remove::<WorkerOffDuty>()
                .remove::<ProcessorWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    ProcessingRoutine {
                        workplace,
                        hall,
                        recipe,
                        work_seconds,
                        failed_routes: 0,
                        production_day,
                        produced_today,
                        phase: ProcessingPhase::GoingToWorkplace,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(building.kind.entrance_position(at.0, rotation.0)),
                ));
        }
    }
}

/// Millers and bakers work continuously until shift end. Labour progress does
/// not accumulate while inputs are absent or the output store is full, so a
/// late porter delivery can never release days of fictional instant output.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_processing_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    (activity_busy, interiors): (
        Query<(), super::worker_activity::ProductionPausedBy>,
        Query<&WorkplaceInterior>,
    ),
    release_requested: Query<(), With<super::worker_activity::EmploymentReleaseRequested>>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut workplaces: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &mut GoodsInventory,
            Option<&BusinessCondition>,
            Option<&mut BusinessStaffingForecast>,
        ),
        Without<CharacterKind>,
    >,
    mut workers: Query<
        (
            Entity,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut CharacterActivity,
            Option<&mut CharacterAttributes>,
            &mut ProcessingRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        (With<CharacterKind>,),
    >,
) {
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let workday = super::worker_activity::schedule::ORDINARY.contains(clock);

    for (
        worker,
        employment,
        position,
        intent,
        home,
        shopping,
        road_work,
        door_transit,
        mut activity,
        mut attributes,
        mut routine,
        move_target,
        failed_route,
    ) in workers.iter_mut()
    {
        let workday = workday && !release_requested.contains(worker);
        if home.is_some()
            || shopping.is_some()
            || road_work.is_some()
            || activity_busy.get(worker).is_ok()
        {
            if routine.phase == ProcessingPhase::Working {
                routine.phase = ProcessingPhase::GoingToWorkplace;
            }
            continue;
        }
        if door_transit.is_some() {
            continue;
        }
        let Ok((building, at, rotation, building_id, mut inventory, condition, mut plan)) =
            workplaces.get_mut(routine.workplace)
        else {
            commands.entity(worker).remove::<ProcessingRoutine>();
            super::worker_activity::lifecycle::cancel_travel(&mut commands, worker, None);
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        };
        if processing_recipe(building.kind) != Some(routine.recipe)
            || employment.0 != *building_id
            || !intent.is_settled()
        {
            commands.entity(worker).remove::<ProcessingRoutine>();
            super::worker_activity::lifecycle::cancel_travel(
                &mut commands,
                worker,
                interiors.get(worker).ok().filter(|_| intent.is_settled()),
            );
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        let workday = workday && condition.is_none_or(|condition| condition.state.can_operate());
        if routine.production_day != clock.day {
            routine.production_day = clock.day;
            routine.produced_today = 0;
        }
        let entrance = building.kind.entrance_position(at.0, rotation.0);
        let inside = building.kind.interior_door_position(at.0, rotation.0);

        if failed_route.is_some() {
            routine.failed_routes = routine.failed_routes.saturating_add(1);
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            if routine.failed_routes >= MAX_PROCESSOR_ROUTE_FAILURES || !workday {
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    clock.day,
                    &*routine,
                    &mut activity,
                );
            } else {
                commands.entity(worker).insert(MoveTarget(entrance));
            }
            continue;
        }

        if !workday {
            match routine.phase {
                ProcessingPhase::Working => {
                    begin_workplace_exit(&mut commands, worker, at.0, entrance, inside, entrance);
                    routine.phase = ProcessingPhase::EndingShift;
                }
                ProcessingPhase::GoingToWorkplace | ProcessingPhase::EndingShift => {
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        clock.day,
                        &*routine,
                        &mut activity,
                    );
                }
            }
            continue;
        }

        match routine.phase {
            ProcessingPhase::GoingToWorkplace => {
                if ground_distance(position.0, entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    continue;
                }
                routine.failed_routes = 0;
                begin_workplace_entry(&mut commands, worker, at.0, entrance, inside);
                routine.phase = ProcessingPhase::Working;
                activity.set_if_neq(CharacterActivity::Idle);
            }
            ProcessingPhase::Working => {
                if ground_distance(position.0, inside) > DOOR_REACH {
                    activity.set_if_neq(CharacterActivity::Idle);
                    routine.phase = ProcessingPhase::GoingToWorkplace;
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    continue;
                }
                activity.set_if_neq(CharacterActivity::Indoors);
                let recipe = routine.recipe;
                let has_input = inventory.amount(recipe.input) >= recipe.input_units;
                let reclaimed = recipe
                    .input_units
                    .saturating_mul(recipe.input.bulk_per_unit());
                let output_bulk = recipe
                    .output_units
                    .saturating_mul(recipe.output.bulk_per_unit());
                if !has_input || inventory.free_bulk().saturating_add(reclaimed) < output_bulk {
                    routine.work_seconds = 0.0;
                    continue;
                }
                routine.work_seconds += dt;
                let requested = (routine.work_seconds / recipe.work_seconds).floor() as u32;
                if requested == 0 {
                    continue;
                }
                let (cycles, produced) =
                    process_available_cycles(&mut inventory, recipe, requested);
                if cycles == 0 {
                    routine.work_seconds = 0.0;
                    continue;
                }
                routine.work_seconds -= recipe.work_seconds * cycles as f32;
                if cycles < requested
                    || inventory.amount(recipe.input) < recipe.input_units
                    || inventory.free_bulk().saturating_add(reclaimed) < output_bulk
                {
                    // The remaining accelerated tick occurred after the last
                    // available batch ran out. It is not bankable labour for
                    // inputs a porter may deliver later.
                    routine.work_seconds = 0.0;
                }
                let first_output_today = routine.produced_today == 0;
                routine.produced_today = routine.produced_today.saturating_add(produced);
                if let Some(plan) = plan.as_deref_mut() {
                    plan.record(clock.day, produced);
                }
                business_events.record_production(clock.day, *building_id, produced);
                economy_runtime.record_food_production(
                    routine.hall,
                    cycles.saturating_mul(recipe.net_food_units()),
                );
                if first_output_today {
                    if let Some(attributes) = attributes.as_deref_mut() {
                        attributes.train_intelligence(1);
                    }
                }
            }
            ProcessingPhase::EndingShift => {
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    clock.day,
                    &*routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Publish workplace activity only when it changes.
///
/// A processing routine being indoors is not enough: an empty mill or bakery
/// is idle. Rechecking the real recipe and inventory here keeps visual effects
/// honest while avoiding replicated writes on every simulation tick.
pub fn sync_workplace_operations(
    mut commands: Commands,
    workers: Query<(&ProcessingRoutine, &CharacterActivity)>,
    workplaces: Query<(
        Entity,
        &SettlementBuilding,
        &GoodsInventory,
        Option<&WorkplaceOperation>,
    )>,
) {
    let mut active_workers: HashMap<Entity, u8> = HashMap::new();
    for (routine, activity) in workers.iter() {
        if !routine.is_working_inside() || *activity != CharacterActivity::Indoors {
            continue;
        }
        let Ok((_, building, inventory, _)) = workplaces.get(routine.workplace) else {
            continue;
        };
        let Some(recipe) = processing_recipe(building.kind) else {
            continue;
        };
        let reclaimed_bulk = recipe
            .input_units
            .saturating_mul(recipe.input.bulk_per_unit());
        let output_bulk = recipe
            .output_units
            .saturating_mul(recipe.output.bulk_per_unit());
        if inventory.amount(recipe.input) < recipe.input_units
            || inventory.free_bulk().saturating_add(reclaimed_bulk) < output_bulk
        {
            continue;
        }
        active_workers
            .entry(routine.workplace)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }

    for (entity, building, _, current) in workplaces.iter() {
        if processing_recipe(building.kind).is_none() {
            continue;
        }
        let next = active_workers.get(&entity).copied().unwrap_or(0);
        match (current, next) {
            (Some(_), 0) => {
                commands.entity(entity).remove::<WorkplaceOperation>();
            }
            (Some(current), active_workers) if current.active_workers != active_workers => {
                commands
                    .entity(entity)
                    .insert(WorkplaceOperation { active_workers });
            }
            (None, active_workers) if active_workers > 0 => {
                commands
                    .entity(entity)
                    .insert(WorkplaceOperation { active_workers });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_processor_jobs_release_deleted_interiors_but_exit_existing_workplaces() {
        use shared::components::{BuildingId, EmployedAt};
        for deleted in [false, true] {
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<SettlementEconomyRuntime>()
                .init_resource::<BusinessEventQueue>()
                .add_systems(Update, run_processing_routines);
            app.world_mut().spawn(WorldTime::new_default());
            let hall = app.world_mut().spawn_empty().id();
            let kind = SettlementBuildingKind::Windmill;
            let at = Vec3::new(20.0, 0.0, 0.0);
            let door = kind.entrance_position(at, 0.0);
            let inside = kind.interior_door_position(at, 0.0);
            let workplace = app
                .world_mut()
                .spawn((
                    BuildingId(9),
                    PlayerPosition(at),
                    PlayerRotation(0.0),
                    SettlementBuilding {
                        kind,
                        settlement: "Cancelled job".into(),
                        owner: None,
                        quality: 1.0,
                        workers: vec![],
                    },
                    GoodsInventory::new(kind.storage_bulk_capacity()),
                ))
                .id();
            let mut cargo = GoodsInventory::new(30);
            cargo.add(Good::Bread, 2);
            let worker = app
                .world_mut()
                .spawn((
                    CharacterKind::Villager,
                    EmployedAt(BuildingId(10)),
                    PlayerPosition(inside),
                    VillagerIntent::Resident { settlement: hall },
                    CharacterActivity::Indoors,
                    cargo,
                    WorkplaceInterior {
                        building: at,
                        door,
                        inside,
                    },
                    ProcessingRoutine {
                        workplace,
                        hall,
                        recipe: processing_recipe(kind).unwrap(),
                        work_seconds: 57.0,
                        failed_routes: 0,
                        production_day: 0,
                        produced_today: 1,
                        phase: ProcessingPhase::Working,
                    },
                ))
                .id();
            if deleted {
                app.world_mut().despawn(workplace);
            }
            app.update();
            assert!(app.world().get::<ProcessingRoutine>(worker).is_none());
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(worker)
                    .unwrap()
                    .amount(Good::Bread),
                2
            );
            if deleted {
                assert!(app.world().get::<WorkplaceInterior>(worker).is_none());
                assert!(app.world().get::<WorkplaceDoorTransit>(worker).is_none());
            } else {
                assert!(app.world().get::<WorkplaceInterior>(worker).is_some());
                assert_eq!(
                    app.world()
                        .get::<WorkplaceDoorTransit>(worker)
                        .unwrap()
                        .direction,
                    WorkplaceDoorDirection::Leaving
                );
                assert!(
                    app.world().get::<MoveTarget>(worker).is_none(),
                    "changing employer must finish the doorway before ordinary navigation resumes"
                );
            }
        }
    }

    #[test]
    fn self_hauling_processor_preserves_work_through_the_entire_input_trip() {
        use shared::components::{
            BuildingId, BuildingOf, CompanyId, EmployedAt, OperatedBy, RoadClass, RoadOf,
            RoadSurface, SettlementId,
        };
        use shared::economy::{BusinessInputRule, CompanyAccount};

        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<BusinessEventQueue>()
            .init_resource::<SettlementEconomyRuntime>()
            .add_systems(
                Update,
                (
                    run_market_collections,
                    assign_processing_routines,
                    run_processing_routines,
                    run_workplace_door_transits,
                    crate::player::hero::step_units,
                )
                    .chain(),
            );
        app.insert_resource(WorldTerrain::default());
        app.world_mut().spawn(WorldTime::new_default());
        let town = SettlementId(40);
        let company_id = CompanyId(41);
        let company = app
            .world_mut()
            .spawn((
                company_id,
                CompanyAccount {
                    cash: 10_000,
                    ..default()
                },
            ))
            .id();
        let mut market = MootMarket::founding();
        market.consign(
            shared::economy::MarketSeller::Treasury(town),
            Good::Wheat,
            5,
            Good::Wheat.base_price(),
        );
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        stock.add(Good::Wheat, 5);
        let hall_at = Vec3::new(1700.0, 0.0, 0.0);
        let hall = app
            .world_mut()
            .spawn((
                town,
                PlayerPosition(hall_at),
                PlayerRotation(0.0),
                stock,
                market,
            ))
            .id();
        let counter = SettlementBuildingKind::Hall.entrance_position(hall_at, 0.0);
        let kind = SettlementBuildingKind::Windmill;
        let at = hall_at + Vec3::X * 30.0;
        let entrance = kind.entrance_position(at, 0.0);
        let inside = kind.interior_door_position(at, 0.0);
        let mut obstacles = SpatialObstacleGrid::default();
        let definition = kind.placement_definition();
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: at.xz() + definition.footprint_center,
            half_extents: definition.footprint * 0.5
                + Vec2::splat(shared::physics::CHARACTER_NAV_RADIUS),
            rotation: 0.0,
            obstacle_type: kind.art() as u32,
        });
        app.insert_resource(obstacles);
        let id = BuildingId(42);
        let mut input = GoodsInventory::new(kind.storage_bulk_capacity());
        input.add(Good::Wheat, 1);
        let workplace = app
            .world_mut()
            .spawn((
                id,
                BuildingOf(town),
                OperatedBy(company_id),
                PlayerPosition(at),
                PlayerRotation(0.0),
                SettlementBuilding {
                    kind,
                    settlement: "Input trip".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![],
                },
                input,
                BusinessSalePolicy::for_good(Good::Flour),
                BusinessAccount::default(),
                BusinessWagePolicy::default(),
                BusinessProcurementPolicy::default().with_rule(
                    Good::Wheat,
                    BusinessInputRule {
                        enabled: true,
                        coverage_days: 2,
                        reorder_below: 2,
                        target_units: 3,
                        maximum_unit_price: Good::Wheat.base_price(),
                    },
                ),
            ))
            .id();
        app.world_mut().spawn((
            VillageRoad {
                settlement: "Input trip".into(),
                builder: "Crew".into(),
                points: vec![entrance.xz(), counter.xz()],
                built_through: 2,
                width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
                reserved_width: RoadClass::Lane.initial_reserved_width(),
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            },
            RoadOf(town),
        ));
        let recipe = processing_recipe(kind).unwrap();
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                EmployedAt(id),
                VillagerIntent::Resident { settlement: hall },
                CharacterActivity::Indoors,
                CharacterAttributes::default(),
                PlayerPosition(entrance),
                PlayerRotation(0.0),
                shared::region::RegionCoord::default(),
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                ProcessingRoutine {
                    workplace,
                    hall,
                    recipe,
                    work_seconds: 57.0,
                    failed_routes: 0,
                    production_day: 0,
                    produced_today: 2,
                    phase: ProcessingPhase::Working,
                },
            ))
            .id();
        // Use the production doorway and movement systems for both crossings.
        // A collision footprint makes bypassing the door a failed physical trip.
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                begin_workplace_entry(&mut commands, worker, at, entrance, inside);
            })
            .unwrap();
        let mut saw_outbound_door = false;
        let mut saw_safe_exit = false;
        for _ in 0..1_000 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
            let Some(trip) = app.world().get::<MarketCollectionRoutine>(worker) else {
                continue;
            };
            if trip.phase == MarketCollectionPhase::DeliveringInput {
                break;
            }
            assert_eq!(trip.phase, MarketCollectionPhase::GoingToInputCounter);
            assert!(app.world().get::<ProcessingRoutine>(worker).is_none());
            assert_eq!(
                app.world()
                    .get::<ProcessorWorkProgress>(worker)
                    .unwrap()
                    .seconds,
                57.0
            );
            assert_eq!(
                app.world().get::<CompanyAccount>(company).unwrap().cash,
                10_000
            );
            assert_eq!(
                app.world()
                    .get::<GoodsInventory>(hall)
                    .unwrap()
                    .amount(Good::Wheat),
                5
            );
            assert!(app.world().get::<WorkerOffDuty>(worker).is_none());
            if app.world().get::<WorkplaceDoorTransit>(worker).is_some() {
                saw_outbound_door = true;
                assert!(
                    app.world()
                        .get::<MoveTarget>(worker)
                        .is_none_or(|target| target.0 != counter),
                    "freight must not overwrite the doorway's short crossing target"
                );
            } else if saw_outbound_door {
                let position = app.world().get::<PlayerPosition>(worker).unwrap().0;
                assert!(
                    !app.world()
                        .resource::<SpatialObstacleGrid>()
                        .point_blocked(position.xz())
                );
                saw_safe_exit = true;
            }
        }
        assert!(
            saw_outbound_door && saw_safe_exit,
            "a self-hauling miller must leave through the real doorway before approaching the counter"
        );
        assert_eq!(
            app.world()
                .get::<MarketCollectionRoutine>(worker)
                .unwrap()
                .phase,
            MarketCollectionPhase::DeliveringInput
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Wheat),
            2
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(workplace)
                .unwrap()
                .amount(Good::Wheat),
            1
        );
        assert_eq!(
            app.world().get::<CompanyAccount>(company).unwrap().cash,
            10_000 - 2 * Good::Wheat.base_price()
        );
        for _ in 0..1_000 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
            if app.world().get::<ProcessingRoutine>(worker).is_some()
                && app.world().get::<WorkplaceInterior>(worker).is_some()
                && app.world().get::<WorkplaceDoorTransit>(worker).is_none()
            {
                break;
            }
        }
        assert!(app.world().get::<MarketCollectionRoutine>(worker).is_none());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .used_bulk(),
            0
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(workplace)
                .unwrap()
                .amount(Good::Wheat),
            3
        );
        let resumed = app.world().get::<ProcessingRoutine>(worker).unwrap();
        assert!((57.0..57.1).contains(&resumed.work_seconds));
        let remaining_work = recipe.work_seconds - resumed.work_seconds;
        assert_eq!((resumed.production_day, resumed.produced_today), (0, 2));
        assert_eq!(app.world().get::<EmployedAt>(worker), Some(&EmployedAt(id)));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(remaining_work));
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(workplace)
                .unwrap()
                .amount(Good::Flour),
            recipe.output_units
        );
        assert_eq!(
            app.world()
                .get::<ProcessingRoutine>(worker)
                .unwrap()
                .produced_today,
            3
        );
        assert_eq!(
            app.world()
                .get::<CharacterAttributes>(worker)
                .unwrap()
                .intelligence(),
            10,
            "the same day's resumed production must not award first-output training twice"
        );
    }

    #[test]
    fn a_miller_creates_flour_only_after_real_work_and_input() {
        let recipe = processing_recipe(SettlementBuildingKind::Windmill).unwrap();
        let mut stock = GoodsInventory::new(20);
        assert_eq!(process_available_cycles(&mut stock, recipe, 1), (0, 0));
        stock.add(Good::Wheat, 1);
        assert_eq!(process_available_cycles(&mut stock, recipe, 1), (1, 1));
        assert_eq!(stock.amount(Good::Wheat), 0);
        assert_eq!(stock.amount(Good::Flour), 1);
    }

    #[test]
    fn workplace_operation_tracks_real_processable_work() {
        let mut app = App::new();
        app.add_systems(Update, sync_workplace_operations);
        let recipe = processing_recipe(SettlementBuildingKind::Bakery).unwrap();
        let workplace = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Bakery,
                    settlement: "Smoketest".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec!["Baker".into()],
                },
                {
                    let mut stock = GoodsInventory::new(24);
                    stock.add(Good::Flour, recipe.input_units);
                    stock
                },
            ))
            .id();
        app.world_mut().spawn((
            CharacterActivity::Indoors,
            ProcessingRoutine {
                workplace,
                hall: Entity::PLACEHOLDER,
                recipe,
                work_seconds: 0.0,
                failed_routes: 0,
                production_day: 0,
                produced_today: 0,
                phase: ProcessingPhase::Working,
            },
        ));

        app.update();
        assert_eq!(
            app.world().get::<WorkplaceOperation>(workplace),
            Some(&WorkplaceOperation { active_workers: 1 })
        );

        app.world_mut()
            .get_mut::<GoodsInventory>(workplace)
            .unwrap()
            .remove(Good::Flour, recipe.input_units);
        app.update();
        assert!(app.world().get::<WorkplaceOperation>(workplace).is_none());
    }

    #[test]
    fn an_embodied_miller_transforms_stock_only_during_a_real_shift() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<SettlementEconomyRuntime>()
            .init_resource::<BusinessEventQueue>()
            .add_systems(Update, run_processing_routines);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let hall = app.world_mut().spawn_empty().id();
        let building_id = shared::components::BuildingId(80);
        let workplace = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Windmill,
                    settlement: "Milltest".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec!["Miller".into()],
                },
                PlayerPosition(Vec3::new(20.0, 0.0, 0.0)),
                PlayerRotation(0.0),
                building_id,
                BusinessStaffingForecast {
                    day: 0,
                    expected_sales_units: 0,
                    produced_output_units: 0,
                    optimal_positions: 1,
                    marginal_daily_profit: 0,
                },
                {
                    let mut stock = GoodsInventory::new(20);
                    stock.add(Good::Wheat, 1);
                    stock
                },
            ))
            .id();
        let worker = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterActivity::Idle,
                CharacterAttributes::default(),
                PlayerPosition(
                    SettlementBuildingKind::Windmill
                        .interior_door_position(Vec3::new(20.0, 0.0, 0.0), 0.0),
                ),
                VillagerIntent::Resident { settlement: hall },
                shared::components::EmployedAt(building_id),
                ProcessingRoutine {
                    workplace,
                    hall,
                    recipe: processing_recipe(SettlementBuildingKind::Windmill).unwrap(),
                    work_seconds: 0.0,
                    failed_routes: 0,
                    production_day: u32::MAX,
                    produced_today: 0,
                    phase: ProcessingPhase::Working,
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(361.0));
        app.update();

        let stock = app.world().get::<GoodsInventory>(workplace).unwrap();
        assert_eq!(stock.amount(Good::Wheat), 0);
        assert_eq!(stock.amount(Good::Flour), 1);
        app.world_mut()
            .get_mut::<GoodsInventory>(workplace)
            .unwrap()
            .add(Good::Wheat, 1);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0));
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(workplace)
                .unwrap()
                .amount(Good::Flour),
            1,
            "starved accelerated time must not become banked future output"
        );
        assert_eq!(
            *app.world().get::<CharacterActivity>(worker).unwrap(),
            CharacterActivity::Indoors
        );
        assert!(
            app.world()
                .get::<CharacterAttributes>(worker)
                .unwrap()
                .intelligence()
                > 10
        );
        assert_eq!(
            app.world().get::<WorldTime>(clock).unwrap().day,
            0,
            "the processor must not invent its own calendar"
        );
        app.world_mut().get_mut::<PlayerPosition>(worker).unwrap().0 = Vec3::ZERO;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(361.0));
        app.update();
        let stock = app.world().get::<GoodsInventory>(workplace).unwrap();
        assert_eq!(
            stock.amount(Good::Wheat),
            1,
            "work at the Hall cannot consume workplace inputs"
        );
        assert_eq!(stock.amount(Good::Flour), 1);
        assert_eq!(
            app.world().get::<ProcessingRoutine>(worker).unwrap().phase,
            ProcessingPhase::GoingToWorkplace
        );
        assert_ne!(
            *app.world().get::<CharacterActivity>(worker).unwrap(),
            CharacterActivity::Indoors
        );
    }
}
