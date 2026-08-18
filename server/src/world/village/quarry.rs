//! Embodied outdoor extraction for quarries and livestock farms.
//!
//! Stone, Meat and Wool enter the economy only after an employed worker reaches
//! the outdoor work area, performs real work, carries a bounded load, and
//! deposits it in the workplace store. Off-screen settlements use the matching
//! rated production model in `strategic`.

use super::*;

const QUARRY_REACH: f32 = 1.25;
const STORE_REACH: f32 = 1.35;
const MAX_QUARRY_ROUTE_FAILURES: u8 = 3;

#[derive(Component, Debug, Clone)]
pub struct QuarryRoutine {
    workplace: Entity,
    hall: Entity,
    kind: SettlementBuildingKind,
    work_seconds: f32,
    failed_routes: u8,
    production_day: u32,
    produced_today: u32,
    phase: QuarryPhase,
}

impl QuarryRoutine {
    pub(crate) const fn workplace(&self) -> Entity {
        self.workplace
    }

    pub(crate) const fn hall(&self) -> Entity {
        self.hall
    }

    pub(crate) const fn objective(&self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        match (self.kind, self.phase) {
            (SettlementBuildingKind::LivestockFarm, QuarryPhase::GoingToFace) => {
                CharacterObjective::GoingToLivestockWork
            }
            (SettlementBuildingKind::LivestockFarm, QuarryPhase::Mining) => {
                CharacterObjective::TendingLivestock
            }
            (SettlementBuildingKind::LivestockFarm, QuarryPhase::ReturningToStore) => {
                CharacterObjective::ReturningLivestockProducts
            }
            (_, QuarryPhase::GoingToFace) => CharacterObjective::GoingToQuarryWork,
            (_, QuarryPhase::Mining) => CharacterObjective::QuarryingStone,
            (_, QuarryPhase::ReturningToStore) => CharacterObjective::ReturningStone,
            (_, QuarryPhase::EndingShift) => CharacterObjective::EndingWorkShift,
        }
    }

    pub(crate) fn restart_for_morning(&mut self) {
        self.phase = QuarryPhase::GoingToFace;
        self.failed_routes = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuarryPhase {
    GoingToFace,
    Mining,
    ReturningToStore,
    EndingShift,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct QuarryWorkProgress {
    workplace: Entity,
    seconds: f32,
}

fn outdoor_work_point(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    terrain: Option<&WorldTerrain>,
) -> Vec3 {
    // The entrance remains on local -Z beside the road. The deposit face sits
    // beside the blockout so the worker is visibly outdoors and can walk
    // around the building rather than mining inside its shell.
    let point = if kind == SettlementBuildingKind::LivestockFarm {
        kind.pasture_position(position, rotation)
            .unwrap_or(position)
    } else {
        let offset = shared::rotation::local_to_world_xz(Vec2::new(5.9, 0.8), rotation);
        Vec3::new(position.x + offset.x, position.y, position.z + offset.y)
    };
    let x = point.x;
    let z = point.z;
    Vec3::new(
        x,
        terrain.map_or(position.y, |terrain| terrain.get_height(x, z)),
        z,
    )
}

fn finish_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &QuarryRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<QuarryRoutine>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            QuarryWorkProgress {
                workplace: routine.workplace,
                seconds: routine.work_seconds,
            },
        ));
}

#[allow(clippy::type_complexity)]
pub fn assign_quarry_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    terrain: Option<Res<WorldTerrain>>,
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
            Option<&QuarryWorkProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            With<CharacterKind>,
            Without<strategic::StrategicPerson>,
            Without<QuarryRoutine>,
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<ProcessingRoutine>,
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
        if !matches!(
            building.kind,
            SettlementBuildingKind::StoneQuarry | SettlementBuildingKind::LivestockFarm
        ) || condition.is_some_and(|condition| !condition.state.can_operate())
            || plan.is_some_and(|plan| plan.remaining(clock.day) == 0)
        {
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

        let face = outdoor_work_point(building.kind, at.0, rotation.0, terrain.as_deref());
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
                .remove::<QuarryWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    QuarryRoutine {
                        workplace,
                        hall,
                        kind: building.kind,
                        work_seconds,
                        failed_routes: 0,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: QuarryPhase::GoingToFace,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(face),
                ));
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_quarry_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    mut business_events: ResMut<BusinessEventQueue>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    workplaces: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
            Option<&BusinessCondition>,
        ),
        Without<CharacterKind>,
    >,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut operating_plans: Query<&mut BusinessOperatingPlan, Without<CharacterKind>>,
    mut workers: Query<
        (
            Entity,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            Option<&mut CharacterAttributes>,
            &mut QuarryRoutine,
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
        mut facing,
        mut activity,
        mut attributes,
        mut routine,
        move_target,
        failed_route,
    ) in workers.iter_mut()
    {
        if home.is_some() || shopping.is_some() || road_work.is_some() {
            continue;
        }
        let Ok((building, at, rotation, building_id, building_of, condition)) =
            workplaces.get(routine.workplace)
        else {
            commands.entity(worker).remove::<QuarryRoutine>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if building.kind != routine.kind
            || !matches!(
                building.kind,
                SettlementBuildingKind::StoneQuarry | SettlementBuildingKind::LivestockFarm
            )
            || employment.0 != *building_id
            || condition.is_some_and(|condition| !condition.state.can_operate())
            || !intent.is_settled()
            || halls
                .get(routine.hall)
                .is_ok_and(|settlement_id| *settlement_id != building_of.0)
        {
            commands.entity(worker).remove::<QuarryRoutine>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        if routine.production_day != clock.day {
            routine.production_day = clock.day;
            routine.produced_today = 0;
        }
        let face = outdoor_work_point(building.kind, at.0, rotation.0, terrain.as_deref());
        let store = building.kind.entrance_position(at.0, rotation.0);
        let output = if building.kind == SettlementBuildingKind::LivestockFarm {
            Good::Meat
        } else {
            Good::Stone
        };
        let carrying_output = inventories.get_mut(worker).is_ok_and(|inventory| {
            inventory.amount(output) > 0
                || (building.kind == SettlementBuildingKind::LivestockFarm
                    && inventory.amount(Good::Wool) > 0)
        });

        if failed_route.is_some() {
            routine.failed_routes = routine.failed_routes.saturating_add(1);
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            if carrying_output {
                routine.phase = QuarryPhase::ReturningToStore;
                commands.entity(worker).insert(MoveTarget(store));
            } else if routine.failed_routes >= MAX_QUARRY_ROUTE_FAILURES || !workday {
                finish_shift(&mut commands, worker, clock.day, &routine, &mut activity);
            } else {
                routine.phase = QuarryPhase::GoingToFace;
                commands.entity(worker).insert(MoveTarget(face));
            }
            continue;
        }

        if !workday {
            if carrying_output {
                routine.phase = QuarryPhase::ReturningToStore;
                if ground_distance(position.0, store) > STORE_REACH {
                    ensure_move_target(&mut commands, worker, move_target, store);
                    continue;
                }
            } else {
                routine.phase = QuarryPhase::EndingShift;
            }
        }

        match routine.phase {
            QuarryPhase::GoingToFace => {
                if !workday {
                    finish_shift(&mut commands, worker, clock.day, &routine, &mut activity);
                } else if ground_distance(position.0, face) <= QUARRY_REACH {
                    routine.failed_routes = 0;
                    commands.entity(worker).remove::<MoveTarget>();
                    let to_face = at.0 - position.0;
                    if to_face.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_face.x, -to_face.z);
                    }
                    *activity = if building.kind == SettlementBuildingKind::LivestockFarm {
                        CharacterActivity::Farming
                    } else {
                        CharacterActivity::Mining
                    };
                    routine.phase = QuarryPhase::Mining;
                } else {
                    ensure_move_target(&mut commands, worker, move_target, face);
                }
            }
            QuarryPhase::Mining => {
                if !workday {
                    *activity = CharacterActivity::Idle;
                    routine.phase = QuarryPhase::EndingShift;
                    continue;
                }
                *activity = if building.kind == SettlementBuildingKind::LivestockFarm {
                    CharacterActivity::Farming
                } else {
                    CharacterActivity::Mining
                };
                let remaining = operating_plans
                    .get(routine.workplace)
                    .map_or(u32::MAX, |plan| plan.remaining(clock.day));
                let cycle_bulk = output.bulk_per_unit()
                    + if building.kind == SettlementBuildingKind::LivestockFarm {
                        Good::Wool.bulk_per_unit()
                    } else {
                        0
                    };
                let can_carry = inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.free_bulk() >= cycle_bulk);
                if remaining == 0 || !can_carry {
                    *activity = CharacterActivity::Idle;
                    routine.phase = QuarryPhase::ReturningToStore;
                    commands.entity(worker).insert(MoveTarget(store));
                    continue;
                }
                routine.work_seconds += dt;
                let seconds_per_unit = if building.kind == SettlementBuildingKind::LivestockFarm {
                    livestock_seconds_per_meat(building.quality)
                } else {
                    quarry_seconds_per_stone(building.quality)
                };
                let requested =
                    ((routine.work_seconds / seconds_per_unit).floor() as u32).min(remaining);
                if requested == 0 {
                    continue;
                }
                let produced = inventories.get_mut(worker).map_or(0, |mut inventory| {
                    if building.kind == SettlementBuildingKind::LivestockFarm {
                        produce_livestock_cycles(&mut inventory, requested)
                    } else {
                        inventory.add(output, requested)
                    }
                });
                if produced == 0 {
                    continue;
                }
                routine.work_seconds =
                    (routine.work_seconds - seconds_per_unit * produced as f32).max(0.0);
                let first_output_today = routine.produced_today == 0;
                routine.produced_today = routine.produced_today.saturating_add(produced);
                if let Ok(mut plan) = operating_plans.get_mut(routine.workplace) {
                    plan.record(clock.day, produced);
                }
                business_events.record_production(clock.day, *building_id, produced);
                if building.kind == SettlementBuildingKind::LivestockFarm {
                    economy_runtime.record_food_production(routine.hall, produced);
                }
                if first_output_today {
                    if let Some(attributes) = attributes.as_deref_mut() {
                        attributes.train_physique(1);
                    }
                }
                if building.kind == SettlementBuildingKind::LivestockFarm
                    && inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Meat) >= 2)
                {
                    *activity = CharacterActivity::Idle;
                    routine.phase = QuarryPhase::ReturningToStore;
                    commands.entity(worker).insert(MoveTarget(store));
                }
            }
            QuarryPhase::ReturningToStore => {
                if ground_distance(position.0, store) > STORE_REACH {
                    ensure_move_target(&mut commands, worker, move_target, store);
                    continue;
                }
                commands.entity(worker).remove::<MoveTarget>();
                let Ok([mut carrier, mut workplace]) =
                    inventories.get_many_mut([worker, routine.workplace])
                else {
                    continue;
                };
                carrier.transfer_to(&mut workplace, output, u32::MAX);
                if building.kind == SettlementBuildingKind::LivestockFarm {
                    carrier.transfer_to(&mut workplace, Good::Wool, u32::MAX);
                }
                if carrier.amount(output) > 0
                    || (building.kind == SettlementBuildingKind::LivestockFarm
                        && carrier.amount(Good::Wool) > 0)
                {
                    continue;
                }
                drop(carrier);
                drop(workplace);
                if workday
                    && operating_plans
                        .get(routine.workplace)
                        .is_ok_and(|plan| plan.remaining(clock.day) > 0)
                {
                    routine.phase = QuarryPhase::GoingToFace;
                    commands.entity(worker).insert(MoveTarget(face));
                } else {
                    routine.phase = QuarryPhase::EndingShift;
                }
            }
            QuarryPhase::EndingShift => {
                finish_shift(&mut commands, worker, clock.day, &routine, &mut activity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_stone_ground_extracts_faster_without_creating_free_units() {
        assert!(quarry_seconds_per_stone(1.0) < quarry_seconds_per_stone(0.0));
        assert_eq!(Good::Stone.bulk_per_unit(), 6);
        let inventory = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        assert_eq!(inventory.free_bulk() / Good::Stone.bulk_per_unit(), 2);
    }
}
