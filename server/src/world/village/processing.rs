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

    pub(crate) fn reset_for_morning(&mut self) {
        self.phase = ProcessingPhase::GoingToWorkplace;
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
}

/// Start a visible shift for every tactically simulated miller and baker whose
/// completed workplace is connected to the settlement road network.
#[allow(clippy::type_complexity)]
pub fn assign_processing_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    workplaces: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&BusinessCondition>,
        Option<&BusinessOperatingPlan>,
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
            Without<strategic::StrategicPerson>,
            Without<ProcessingRoutine>,
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if !super::trades::ordinary_workday(clock) {
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

    for (workplace, building, at, rotation, building_id, building_of, condition, plan) in
        workplaces.iter()
    {
        let Some(recipe) = processing_recipe(building.kind) else {
            continue;
        };
        if condition.is_some_and(|condition| !condition.state.can_operate()) {
            continue;
        }
        if plan.is_some_and(|plan| plan.remaining(clock.day) < recipe.output_units) {
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
            for worker in roster {
                commands
                    .entity(*worker)
                    .insert(WorkerOffDuty { day: clock.day });
            }
            continue;
        }

        for worker in roster.iter().take(building.kind.positions() as usize) {
            let Ok((_, intent, off_duty, progress, employment)) = villagers.get(*worker) else {
                continue;
            };
            if !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let work_seconds = progress
                .filter(|progress| progress.workplace == workplace)
                .map_or(0.0, |progress| progress.seconds);
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
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: ProcessingPhase::GoingToWorkplace,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(building.kind.entrance_position(at.0, rotation.0)),
                ));
        }
    }
}

fn finish_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &ProcessingRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<ProcessingRoutine>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            ProcessorWorkProgress {
                workplace: routine.workplace,
                seconds: routine.work_seconds,
            },
        ));
}

/// Millers and bakers work continuously until shift end. Labour progress does
/// not accumulate while inputs are absent or the output store is full, so a
/// late porter delivery can never release days of fictional instant output.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_processing_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
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
            Option<&mut BusinessOperatingPlan>,
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
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let workday = super::trades::ordinary_workday(clock);

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
        if home.is_some() || shopping.is_some() || road_work.is_some() || door_transit.is_some() {
            continue;
        }
        let Ok((building, at, rotation, building_id, mut inventory, condition, mut plan)) =
            workplaces.get_mut(routine.workplace)
        else {
            commands.entity(worker).remove::<ProcessingRoutine>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if processing_recipe(building.kind) != Some(routine.recipe)
            || employment.0 != *building_id
            || condition.is_some_and(|condition| !condition.state.can_operate())
            || !intent.is_settled()
        {
            commands.entity(worker).remove::<ProcessingRoutine>();
            *activity = CharacterActivity::Idle;
            continue;
        }
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
                finish_shift(&mut commands, worker, clock.day, &routine, &mut activity);
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
                    finish_shift(&mut commands, worker, clock.day, &routine, &mut activity);
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
                *activity = CharacterActivity::Idle;
            }
            ProcessingPhase::Working => {
                *activity = CharacterActivity::Indoors;
                let recipe = routine.recipe;
                let remaining = plan
                    .as_deref()
                    .map_or(u32::MAX, |plan| plan.remaining(clock.day));
                if remaining < recipe.output_units {
                    routine.work_seconds = 0.0;
                    continue;
                }
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
                let requested = ((routine.work_seconds / recipe.work_seconds).floor() as u32)
                    .min(remaining / recipe.output_units);
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
                if cycles < requested {
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
                finish_shift(&mut commands, worker, clock.day, &routine, &mut activity);
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
    world_time: Query<&WorldTime>,
    workers: Query<(&ProcessingRoutine, &CharacterActivity)>,
    workplaces: Query<(
        Entity,
        &SettlementBuilding,
        &GoodsInventory,
        Option<&WorkplaceOperation>,
        Option<&BusinessOperatingPlan>,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let mut active_workers: HashMap<Entity, u8> = HashMap::new();
    for (routine, activity) in workers.iter() {
        if !routine.is_working_inside() || *activity != CharacterActivity::Indoors {
            continue;
        }
        let Ok((_, building, inventory, _, plan)) = workplaces.get(routine.workplace) else {
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
        if plan.is_some_and(|plan| plan.remaining(day) < recipe.output_units)
            || inventory.amount(recipe.input) < recipe.input_units
            || inventory.free_bulk().saturating_add(reclaimed_bulk) < output_bulk
        {
            continue;
        }
        active_workers
            .entry(routine.workplace)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }

    for (entity, building, _, current, _) in workplaces.iter() {
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
                PlayerPosition(Vec3::new(20.0, 0.0, 0.0)),
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
    }
}
