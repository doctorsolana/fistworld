//! Private Tavern service, daily leisure plans and embodied customer visits.
//!
//! A Tavern is an ordinary company site. It buys real ingredients through the
//! existing procurement system, quotes its own meal price, hires Innkeepers,
//! receives customer coin directly and records site/company revenue. Leisure
//! is deliberately a plan rather than a continuously ticking happiness meter.

use super::*;

const TAVERN_OPEN_MINUTE: u16 = 12 * 60;
const TAVERN_CLOSE_MINUTE: u16 = 21 * 60 + 30;
const TAVERN_DINING_SECONDS: f32 = 45.0;
const MAX_TAVERN_ROUTE_FAILURES: u8 = 3;
/// A Going-phase visitor within this range of the tavern entrance counts as
/// physically present at the line: present visitors always rank ahead of
/// distant walkers, so the door never idles waiting for someone still
/// crossing town.
const TAVERN_QUEUE_PRESENCE_RADIUS: f32 = 16.0;
/// A walking visitor whose position has not advanced for this many WORLD
/// seconds counts as one route failure. The planner can keep succeeding while
/// the body is boxed in (a state NavigationRouteFailed never reports); this
/// feeds that case into the same bounded give-up as real route failures.
const TAVERN_NO_PROGRESS_SECONDS: f64 = 45.0;
const EMPLOYED_CASH_FLOOR: u64 = 2 * PENNIES_PER_COIN;
const JOB_SEEKER_CASH_FLOOR: u64 = 5 * PENNIES_PER_COIN;
const CHILLING_CASH_FLOOR: u64 = 3 * PENNIES_PER_COIN;
const URGENT_CASH_FLOOR: u64 = PENNIES_PER_COIN;
const TAVERN_INSOLVENT_DAYS_BEFORE_LIQUIDATION: u16 = 5;
const TAVERN_QUEUE_SPACING: f32 = 1.6;
const LATE_PLAN_MIN_DELAY_MINUTES: u16 = 5;
const LATE_PLAN_DELAY_SPAN_MINUTES: u16 = 116;
const TAVERN_VISIT_TIME_ALLOWANCE_MINUTES: u16 = 20;

#[derive(Component, Debug, Clone, Copy)]
pub struct TavernVisitRoutine {
    tavern: Entity,
    /// A stable FIFO key. Assignment can happen in the same ECS update for a
    /// whole cohort, so entity identity is the deterministic tie-breaker.
    queue_order: u128,
    phase: TavernVisitPhase,
    dining_seconds: f32,
    failed_routes: u8,
    served: bool,
    /// Where the walking leg last made measurable progress, and when
    /// (absolute world seconds). Waiting in the line refreshes it too - only
    /// a walker who is genuinely pinned in place accumulates stall time.
    progress_position: Vec3,
    progress_world_seconds: f64,
}

impl TavernVisitRoutine {
    pub(crate) const fn objective(self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        match self.phase {
            TavernVisitPhase::Going => CharacterObjective::GoingToTavern,
            TavernVisitPhase::Entering => CharacterObjective::WaitingForTavernService,
            TavernVisitPhase::Dining => CharacterObjective::EatingAtTavern,
            TavernVisitPhase::Leaving => CharacterObjective::LeavingTavern,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TavernVisitPhase {
    Going,
    Entering,
    Dining,
    Leaving,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TavernWorkerRoutine {
    tavern: Entity,
    phase: TavernWorkerPhase,
}

impl TavernWorkerRoutine {
    pub(crate) const fn objective(self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        match self.phase {
            TavernWorkerPhase::Going | TavernWorkerPhase::Entering => {
                CharacterObjective::OpeningTavern
            }
            TavernWorkerPhase::Serving => CharacterObjective::ServingAtTavern,
            TavernWorkerPhase::Leaving => CharacterObjective::EndingWorkShift,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TavernWorkerPhase {
    Going,
    Entering,
    Serving,
    Leaving,
}

fn display_minute(clock: &WorldTime) -> u16 {
    ((clock.normalized_time() * 1_440.0).floor() as u16) % 1_440
}

fn day_seed(person: shared::components::PersonId, day: u32) -> u64 {
    let mut value = person.0 ^ u64::from(day).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn meal_is_affordable(wallet: Wallet, status: WorkStatus, hungry: bool, price: u64) -> bool {
    let floor = if hungry {
        URGENT_CASH_FLOOR
    } else {
        match status {
            WorkStatus::Employed => EMPLOYED_CASH_FLOOR,
            WorkStatus::LookingForWork => JOB_SEEKER_CASH_FLOOR,
            WorkStatus::Chilling => CHILLING_CASH_FLOOR,
        }
    };
    let disposable = wallet.balance().saturating_sub(floor);
    let discretionary_cap = if hungry {
        disposable
    } else {
        match status {
            WorkStatus::Employed => wallet.balance() / 3,
            WorkStatus::LookingForWork => wallet.balance() / 6,
            WorkStatus::Chilling => wallet.balance() / 2,
        }
        .min(disposable)
    };
    price > 0 && price <= discretionary_cap
}

/// Preserve a resident's natural plan when it is still in the future. If the
/// plan is first created after its natural start (most commonly immediately
/// after immigration), spread it through the remaining opening window instead
/// of making the entire arrival cohort overdue on the same simulation tick.
fn stagger_late_leisure_start(
    natural_start: u16,
    end: u16,
    current_minute: u16,
    seed: u64,
) -> Option<u16> {
    if current_minute < natural_start {
        return Some(natural_start);
    }
    let latest_start = end.saturating_sub(TAVERN_VISIT_TIME_ALLOWANCE_MINUTES);
    let delay = LATE_PLAN_MIN_DELAY_MINUTES
        + ((seed >> 24) % u64::from(LATE_PLAN_DELAY_SPAN_MINUTES)) as u16;
    let delayed = current_minute.saturating_add(delay);
    (delayed <= latest_start).then_some(delayed)
}

fn tavern_queue_target(entrance: Vec3, building_center: Vec3, rank: usize) -> Vec3 {
    if rank == 0 {
        return entrance;
    }
    let outward = Vec2::new(
        entrance.x - building_center.x,
        entrance.z - building_center.z,
    )
    .normalize_or(Vec2::NEG_Y);
    let distance = rank as f32 * TAVERN_QUEUE_SPACING;
    Vec3::new(
        entrance.x + outward.x * distance,
        entrance.y,
        entrance.z + outward.y * distance,
    )
}

fn tavern_queue_order(day: u32, minute: u16, visitor: Entity) -> u128 {
    (u128::from(day) << 80) | (u128::from(minute) << 64) | u128::from(visitor.to_bits())
}

/// Attach the replicated service board to every completed private Tavern.
pub fn ensure_tavern_services(
    mut commands: Commands,
    taverns: Query<(Entity, &SettlementBuilding), Without<TavernService>>,
) {
    for (entity, building) in taverns.iter() {
        if building.kind == SettlementBuildingKind::Tavern {
            commands.entity(entity).insert(TavernService::default());
        }
    }
}

/// Generate one compact calendar per resident per world day. A same-day job
/// change also refreshes it. The deterministic jitter avoids town-wide waves.
#[allow(clippy::type_complexity)]
pub fn refresh_character_day_plans(
    world_time: Query<&WorldTime>,
    taverns: Query<(
        &shared::components::BuildingOf,
        &SettlementBuilding,
        &shared::components::BuildingId,
        &GoodsInventory,
        &BusinessSalePolicy,
        &BusinessCondition,
    )>,
    mut commands: Commands,
    people: Query<(
        Entity,
        &shared::components::PersonId,
        &shared::components::ResidentOf,
        &WorkStatus,
        &Wallet,
        &Nutrition,
        Option<&shared::components::EmployedAt>,
        Option<&CharacterDayPlan>,
    )>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let current_minute = display_minute(clock);
    let available_by_settlement: HashMap<shared::components::SettlementId, u64> = taverns
        .iter()
        .filter(|(_, building, _, inventory, _, condition)| {
            building.kind == SettlementBuildingKind::Tavern
                && condition.state.can_operate()
                && (inventory.amount(Good::Bread) > 0 || inventory.amount(Good::Meat) > 0)
        })
        .fold(
            HashMap::new(),
            |mut prices, (building_of, _, _, _, sale, _)| {
                prices
                    .entry(building_of.0)
                    .and_modify(|price| *price = (*price).min(sale.asking_unit_price.max(1)))
                    .or_insert(sale.asking_unit_price.max(1));
                prices
            },
        );

    let tavern_jobs: HashSet<shared::components::BuildingId> = taverns
        .iter()
        .filter_map(|(_, building, building_id, _, _, _)| {
            (building.kind == SettlementBuildingKind::Tavern).then_some(*building_id)
        })
        .collect();
    for (entity, person_id, resident_of, work_status, wallet, nutrition, employed_at, existing) in
        people.iter()
    {
        let works_at_tavern =
            employed_at.is_some_and(|employment| tavern_jobs.contains(&employment.0));
        let existing_matches_workplace = existing.is_none_or(|plan| {
            let planned_tavern_shift =
                plan.work_minutes == Some((TAVERN_OPEN_MINUTE, TAVERN_CLOSE_MINUTE));
            planned_tavern_shift == works_at_tavern
        });
        if existing.is_some_and(|plan| {
            plan.day == clock.day
                && plan.planned_work_status == *work_status
                && existing_matches_workplace
        }) {
            continue;
        }
        let seed = day_seed(*person_id, clock.day);
        let jitter = (seed % 46) as u16;
        let hungry = nutrition.is_hungry();
        let price = available_by_settlement.get(&resident_of.0).copied();
        let willingness = match work_status {
            WorkStatus::Employed => 30u64,
            WorkStatus::LookingForWork => 10,
            WorkStatus::Chilling => 55,
        } + if hungry { 35 } else { 0 };
        let tavern_meal = !works_at_tavern
            && price.is_some_and(|price| {
                meal_is_affordable(*wallet, *work_status, hungry, price) && seed % 100 < willingness
            });
        let (work_minutes, natural_meal_minute, natural_leisure_minutes) = if works_at_tavern {
            (
                Some((TAVERN_OPEN_MINUTE, TAVERN_CLOSE_MINUTE)),
                11 * 60 + 30,
                (21 * 60 + 35, 22 * 60),
            )
        } else {
            match work_status {
                WorkStatus::Employed => {
                    // A village does not have one universal factory whistle.
                    // Small differences in shift end naturally stagger meals,
                    // errands and leisure even before route congestion matters.
                    let shift_end = 17 * 60 + 40 + ((seed >> 16) % 41) as u16;
                    let leisure_start = shift_end + 5 + ((seed >> 32) % 36) as u16;
                    (
                        Some((6 * 60 + jitter / 3, shift_end)),
                        leisure_start,
                        (leisure_start, TAVERN_CLOSE_MINUTE),
                    )
                }
                WorkStatus::LookingForWork => (
                    None,
                    16 * 60 + 30 + (seed % 121) as u16,
                    (16 * 60 + 30 + (seed % 121) as u16, TAVERN_CLOSE_MINUTE),
                ),
                WorkStatus::Chilling => (
                    None,
                    12 * 60 + 30 + (seed % 181) as u16,
                    (12 * 60 + 30 + (seed % 181) as u16, 20 * 60 + 30),
                ),
            }
        };
        let leisure_start = stagger_late_leisure_start(
            natural_leisure_minutes.0,
            natural_leisure_minutes.1,
            current_minute,
            seed,
        );
        let tavern_meal = tavern_meal && leisure_start.is_some();
        let leisure_minutes = (
            leisure_start.unwrap_or(natural_leisure_minutes.0),
            natural_leisure_minutes.1,
        );
        // The staggered leisure start is the meal time only when the tavern
        // visit IS the meal; workers and home-eaters keep their natural one
        // (an innkeeper does not eat lunch at closing time).
        let meal_minute = if tavern_meal {
            leisure_start.unwrap_or(natural_meal_minute)
        } else {
            natural_meal_minute
        };
        commands.entity(entity).insert(CharacterDayPlan {
            day: clock.day,
            wake_minute: 6 * 60 + (seed % 31) as u16,
            work_minutes,
            meal_minute,
            leisure_minutes,
            sleep_minute: 22 * 60 + ((seed >> 8) % 31) as u16,
            leisure: if tavern_meal {
                PlannedLeisure::TavernMeal
            } else {
                PlannedLeisure::LocalFreeTime
            },
            leisure_status: PlannedLeisureStatus::Planned,
            planned_work_status: *work_status,
        });
    }
}

/// Start Innkeeper shifts and due customer visits. Work and leisure remain
/// mutually exclusive because every other authoritative routine is excluded.
#[allow(clippy::type_complexity)]
pub fn assign_tavern_routines(
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    mut taverns: ParamSet<(
        Query<
            (
                Entity,
                &shared::components::BuildingId,
                &shared::components::BuildingOf,
                &PlayerPosition,
                &BusinessCondition,
                &TavernService,
            ),
            With<TavernService>,
        >,
        Query<&mut TavernService>,
    )>,
    active_visits: Query<&TavernVisitRoutine, With<CharacterKind>>,
    workers: Query<
        (Entity, &shared::components::EmployedAt),
        (
            With<CharacterKind>,
            Without<strategic::StrategicPerson>,
            Without<TavernWorkerRoutine>,
            Without<TavernVisitRoutine>,
            Without<HomeRoutine>,
            Without<WorkplaceDoorTransit>,
        ),
    >,
    mut visitors: Query<
        (
            Entity,
            &shared::components::ResidentOf,
            &PlayerPosition,
            &mut CharacterDayPlan,
        ),
        (
            With<CharacterKind>,
            Without<strategic::StrategicPerson>,
            Without<TavernWorkerRoutine>,
            Without<TavernVisitRoutine>,
            Without<HomeRoutine>,
            Without<WorkplaceDoorTransit>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let minute = display_minute(clock);
    let open = (TAVERN_OPEN_MINUTE..TAVERN_CLOSE_MINUTE).contains(&minute);
    let mut reserved = HashMap::<Entity, u8>::new();
    for routine in active_visits.iter() {
        let count = reserved.entry(routine.tavern).or_default();
        *count = count.saturating_add(1);
    }
    if open {
        for (worker, employed_at) in workers.iter() {
            let tavern = taverns
                .p0()
                .iter()
                .find(|(_, building_id, _, _, condition, _)| {
                    **building_id == employed_at.0 && condition.state.can_operate()
                })
                .map(|(tavern, ..)| tavern);
            if let Some(tavern) = tavern {
                commands
                    .entity(worker)
                    .remove::<ambient::AmbientRoutine>()
                    .remove::<WorkerOffDuty>()
                    .insert(TavernWorkerRoutine {
                        tavern,
                        phase: TavernWorkerPhase::Going,
                    });
            }
        }
    }

    for (visitor, resident_of, position, mut plan) in visitors.iter_mut() {
        if !open || !plan.has_due_leisure(minute) {
            continue;
        }
        let best = taverns
            .p0()
            .iter()
            .filter(|(tavern, _, building_of, _, condition, service)| {
                building_of.0 == resident_of.0
                    && condition.state.can_operate()
                    && reserved.get(tavern).copied().unwrap_or(0) < service.guest_capacity
            })
            .map(|(tavern, _, _, at, _, _)| (tavern, ground_distance(position.0, at.0)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(tavern, _)| tavern);
        let Some(tavern) = best else {
            // A full Tavern is busy, not unavailable. Leave this visit planned
            // so a later admission wave can claim the resident before closing.
            if !taverns
                .p0()
                .iter()
                .any(|(_, _, building_of, _, condition, _)| {
                    building_of.0 == resident_of.0 && condition.state.can_operate()
                })
            {
                plan.leisure_status = PlannedLeisureStatus::TavernUnavailable;
            }
            continue;
        };
        if let Ok(mut service) = taverns.p1().get_mut(tavern) {
            service.record_planned_visit(clock.day);
        }
        plan.leisure_status = PlannedLeisureStatus::InProgress;
        let count = reserved.entry(tavern).or_default();
        *count = count.saturating_add(1);
        commands
            .entity(visitor)
            .remove::<ambient::AmbientRoutine>()
            .remove::<MoveTarget>()
            .insert(TavernVisitRoutine {
                tavern,
                queue_order: tavern_queue_order(clock.day, minute, visitor),
                phase: TavernVisitPhase::Going,
                dining_seconds: 0.0,
                failed_routes: 0,
                served: false,
                progress_position: position.0,
                progress_world_seconds: f64::from(clock.day) * f64::from(clock.cycle_duration())
                    + f64::from(clock.seconds_in_cycle),
            });
    }
}

fn finish_visit(
    commands: &mut Commands,
    visitor: Entity,
    plan: &mut CharacterDayPlan,
    status: PlannedLeisureStatus,
) {
    plan.leisure_status = status;
    commands
        .entity(visitor)
        .remove::<TavernVisitRoutine>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
}

/// Advance door choreography, direct sales and indoor dining.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_tavern_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    mut companies: Query<(
        &shared::components::CompanyId,
        &mut shared::economy::CompanyAccount,
    )>,
    mut taverns: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            &mut BusinessAccount,
            &BusinessCondition,
            &shared::components::OperatedBy,
            &mut TavernService,
        ),
        Without<CharacterKind>,
    >,
    mut workers: Query<
        (
            Entity,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut TavernWorkerRoutine,
            Option<&MoveTarget>,
            Option<&WorkplaceDoorTransit>,
            Option<&NavigationRouteFailed>,
        ),
        Without<TavernVisitRoutine>,
    >,
    mut visitors: Query<
        (
            Entity,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut Wallet,
            &mut Nutrition,
            &WorkStatus,
            &mut CharacterDayPlan,
            &mut TavernVisitRoutine,
            Option<&MoveTarget>,
            Option<&WorkplaceDoorTransit>,
            Option<&NavigationRouteFailed>,
        ),
        Without<TavernWorkerRoutine>,
    >,
    employment: Query<&shared::components::EmployedAt, With<CharacterKind>>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let minute = display_minute(clock);
    let open = (TAVERN_OPEN_MINUTE..TAVERN_CLOSE_MINUTE).contains(&minute);
    let dt = simulation_time.world_seconds();
    let now = f64::from(clock.day) * f64::from(clock.cycle_duration())
        + f64::from(clock.seconds_in_cycle);

    // Only visitors still outside take queue slots. As the front visitor
    // enters, every remaining person advances by exactly one place on the next
    // simulation update, producing a real FIFO wave instead of a doorway pile.
    // Visitors physically AT the line always rank ahead of distant walkers
    // (FIFO within each group): the door must never sit idle - with a queue
    // beside it - waiting for an earlier admittee still crossing town.
    let mut exterior_by_tavern = HashMap::<Entity, Vec<(Entity, u128, Vec3)>>::new();
    for (visitor, position, _, _, _, _, _, routine, ..) in visitors.iter_mut() {
        if routine.phase == TavernVisitPhase::Going {
            exterior_by_tavern.entry(routine.tavern).or_default().push((
                visitor,
                routine.queue_order,
                position.0,
            ));
        }
    }
    let mut exterior_rank = HashMap::<Entity, usize>::new();
    for (tavern, queue) in exterior_by_tavern.iter_mut() {
        let entrance = taverns
            .get(*tavern)
            .ok()
            .map(|(building, at, rotation, ..)| building.kind.entrance_position(at.0, rotation.0));
        queue.sort_unstable_by_key(|(_, order, position)| {
            let present = entrance.is_some_and(|entrance| {
                ground_distance(*position, entrance) <= TAVERN_QUEUE_PRESENCE_RADIUS
            });
            (u8::from(!present), *order)
        });
        for (rank, (visitor, ..)) in queue.iter().enumerate() {
            exterior_rank.insert(*visitor, rank);
        }
    }

    for (worker, position, mut activity, mut routine, move_target, transit, failed) in
        workers.iter_mut()
    {
        let Ok((building, at, rotation, ..)) = taverns.get_mut(routine.tavern) else {
            commands.entity(worker).remove::<TavernWorkerRoutine>();
            continue;
        };
        let entrance = building.kind.entrance_position(at.0, rotation.0);
        let inside = building.kind.interior_door_position(at.0, rotation.0);
        if failed.is_some() {
            commands.entity(worker).remove::<NavigationRouteFailed>();
            routine.phase = TavernWorkerPhase::Leaving;
        }
        if !open && !matches!(routine.phase, TavernWorkerPhase::Leaving) {
            if matches!(routine.phase, TavernWorkerPhase::Serving) {
                begin_workplace_exit(&mut commands, worker, at.0, entrance, inside, entrance);
            }
            routine.phase = TavernWorkerPhase::Leaving;
        }
        match routine.phase {
            TavernWorkerPhase::Going => {
                if ground_distance(position.0, entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                } else {
                    begin_workplace_entry(&mut commands, worker, at.0, entrance, inside);
                    routine.phase = TavernWorkerPhase::Entering;
                }
            }
            TavernWorkerPhase::Entering if transit.is_none() => {
                routine.phase = TavernWorkerPhase::Serving;
            }
            TavernWorkerPhase::Entering => {}
            TavernWorkerPhase::Serving => *activity = CharacterActivity::Indoors,
            TavernWorkerPhase::Leaving if transit.is_none() => {
                activity.set_if_neq(CharacterActivity::Idle);
                commands
                    .entity(worker)
                    .remove::<TavernWorkerRoutine>()
                    .insert(WorkerOffDuty { day: clock.day });
            }
            TavernWorkerPhase::Leaving => {}
        }
    }

    for (
        visitor,
        position,
        mut activity,
        mut wallet,
        mut nutrition,
        work_status,
        mut plan,
        mut routine,
        move_target,
        transit,
        failed,
    ) in visitors.iter_mut()
    {
        let Ok((
            building,
            at,
            rotation,
            _,
            mut inventory,
            sale,
            mut account,
            condition,
            operated_by,
            mut service,
        )) = taverns.get_mut(routine.tavern)
        else {
            finish_visit(
                &mut commands,
                visitor,
                &mut plan,
                PlannedLeisureStatus::TavernUnavailable,
            );
            continue;
        };
        service.roll_to_day(clock.day);
        let entrance = building.kind.entrance_position(at.0, rotation.0);
        let inside = building.kind.interior_door_position(at.0, rotation.0);
        if failed.is_some() {
            routine.failed_routes = routine.failed_routes.saturating_add(1);
            commands.entity(visitor).remove::<NavigationRouteFailed>();
            if routine.failed_routes >= MAX_TAVERN_ROUTE_FAILURES {
                service.current_day.route_failures =
                    service.current_day.route_failures.saturating_add(1);
                finish_visit(
                    &mut commands,
                    visitor,
                    &mut plan,
                    PlannedLeisureStatus::CouldNotReach,
                );
                continue;
            }
            commands.entity(visitor).insert(MoveTarget(entrance));
        }
        match routine.phase {
            TavernVisitPhase::Going => {
                let rank = exterior_rank.get(&visitor).copied().unwrap_or(0);
                let queue_target = tavern_queue_target(entrance, at.0, rank);
                if !open || !condition.state.can_operate() {
                    service.current_day.unavailable_visits =
                        service.current_day.unavailable_visits.saturating_add(1);
                    finish_visit(
                        &mut commands,
                        visitor,
                        &mut plan,
                        PlannedLeisureStatus::TavernUnavailable,
                    );
                } else if ground_distance(position.0, queue_target) > DOOR_REACH {
                    if ground_distance(position.0, routine.progress_position) > 1.0 {
                        routine.progress_position = position.0;
                        routine.progress_world_seconds = now;
                    } else if now - routine.progress_world_seconds > TAVERN_NO_PROGRESS_SECONDS {
                        // Pinned in place with a live walk order: count it as
                        // a route failure so the ordinary bounded give-up
                        // applies, and retry from scratch.
                        routine.failed_routes = routine.failed_routes.saturating_add(1);
                        routine.progress_position = position.0;
                        routine.progress_world_seconds = now;
                        if routine.failed_routes >= MAX_TAVERN_ROUTE_FAILURES {
                            service.current_day.route_failures =
                                service.current_day.route_failures.saturating_add(1);
                            finish_visit(
                                &mut commands,
                                visitor,
                                &mut plan,
                                PlannedLeisureStatus::CouldNotReach,
                            );
                            continue;
                        }
                        commands
                            .entity(visitor)
                            .remove::<MoveTarget>()
                            .remove::<TravelRoute>()
                            .remove::<NavigationRoutePending>()
                            .remove::<NavigationRouteFailed>();
                        continue;
                    }
                    ensure_move_target(&mut commands, visitor, move_target, queue_target);
                } else if rank > 0 {
                    // Waiting in the line is legitimate stillness, never a
                    // stall: keep the progress marker fresh.
                    routine.progress_position = position.0;
                    routine.progress_world_seconds = now;
                    activity.set_if_neq(CharacterActivity::Idle);
                    commands
                        .entity(visitor)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                } else {
                    begin_workplace_entry(&mut commands, visitor, at.0, entrance, inside);
                    routine.phase = TavernVisitPhase::Entering;
                }
            }
            TavernVisitPhase::Entering if transit.is_none() => {
                let price = sale.asking_unit_price.max(1);
                let capacity = service.daily_capacity();
                let ingredient = [Good::Bread, Good::Meat]
                    .into_iter()
                    .filter(|good| inventory.amount(*good) > 0)
                    .max_by_key(|good| inventory.amount(*good));
                if !meal_is_affordable(*wallet, *work_status, nutrition.is_hungry(), price) {
                    service.current_day.unaffordable_visits =
                        service.current_day.unaffordable_visits.saturating_add(1);
                    routine.dining_seconds = 4.0;
                } else if ingredient.is_none()
                    || service.innkeepers_on_duty == 0
                    || service.current_day.served_meals >= capacity
                    || service.current_guests >= service.guest_capacity
                {
                    service.current_day.unavailable_visits =
                        service.current_day.unavailable_visits.saturating_add(1);
                    routine.dining_seconds = 4.0;
                } else if let Some(ingredient) = ingredient {
                    if let Some((_, mut company)) = companies
                        .iter_mut()
                        .find(|(company_id, _)| **company_id == operated_by.0)
                    {
                        if !wallet.debit(price) {
                            service.current_day.unaffordable_visits =
                                service.current_day.unaffordable_visits.saturating_add(1);
                            routine.dining_seconds = 4.0;
                            routine.phase = TavernVisitPhase::Dining;
                            continue;
                        }
                        let consumed = inventory.remove(ingredient, 1);
                        debug_assert_eq!(consumed, 1);
                        company.credit(price);
                        account.record_service_sale(clock.day, price, 1);
                        service.record_meal(clock.day, ingredient, price);
                        nutrition.record_meal(clock.day.saturating_add(1));
                        routine.served = true;
                        routine.dining_seconds = TAVERN_DINING_SECONDS;
                    } else {
                        service.current_day.unavailable_visits =
                            service.current_day.unavailable_visits.saturating_add(1);
                        routine.dining_seconds = 4.0;
                    }
                }
                routine.phase = TavernVisitPhase::Dining;
                activity.set_if_neq(CharacterActivity::Indoors);
            }
            TavernVisitPhase::Entering => {}
            TavernVisitPhase::Dining => {
                activity.set_if_neq(CharacterActivity::Indoors);
                routine.dining_seconds -= dt;
                if routine.dining_seconds <= 0.0 {
                    begin_workplace_exit(&mut commands, visitor, at.0, entrance, inside, entrance);
                    routine.phase = TavernVisitPhase::Leaving;
                }
            }
            TavernVisitPhase::Leaving if transit.is_none() => {
                activity.set_if_neq(CharacterActivity::Idle);
                let status = if routine.served {
                    PlannedLeisureStatus::Completed
                } else if meal_is_affordable(
                    *wallet,
                    *work_status,
                    nutrition.is_hungry(),
                    sale.asking_unit_price.max(1),
                ) {
                    PlannedLeisureStatus::TavernUnavailable
                } else {
                    PlannedLeisureStatus::CouldNotAfford
                };
                finish_visit(&mut commands, visitor, &mut plan, status);
            }
            TavernVisitPhase::Leaving => {}
        }
    }

    // This write is bounded by the number of Taverns, not residents. It gives
    // customers and the UI an honest same-tick capacity reading.
    let mut staffed = HashMap::<shared::components::BuildingId, u8>::new();
    for employed_at in employment.iter() {
        let count = staffed.entry(employed_at.0).or_default();
        *count = count.saturating_add(1);
    }
    for (_, _, _, building_id, _, _, _, _, _, mut service) in taverns.iter_mut() {
        service.roll_to_day(clock.day);
        service.innkeepers_on_duty = if open {
            staffed.get(building_id).copied().unwrap_or(0)
        } else {
            0
        };
        service.current_guests = 0;
    }
    for (_, _, _, _, _, _, _, routine, ..) in visitors.iter_mut() {
        if matches!(
            routine.phase,
            TavernVisitPhase::Entering | TavernVisitPhase::Dining
        ) {
            if let Ok((.., mut service)) = taverns.get_mut(routine.tavern) {
                service.current_guests = service
                    .current_guests
                    .saturating_add(1)
                    .min(service.guest_capacity);
            }
        }
    }
}

/// Settle the same private service for strategically simulated residents.
/// This pass runs at most once per display minute and never creates a route or
/// body-level routine, preserving the full economic result at cheap LOD.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_strategic_tavern_visits(
    world_time: Query<&WorldTime>,
    mut last_minute: Local<Option<(u32, u16)>>,
    mut companies: Query<(
        &shared::components::CompanyId,
        &mut shared::economy::CompanyAccount,
    )>,
    mut taverns: ParamSet<(
        Query<(
            Entity,
            &shared::components::BuildingOf,
            &BusinessSalePolicy,
            &BusinessCondition,
            &GoodsInventory,
            &TavernService,
        )>,
        Query<(
            &mut GoodsInventory,
            &BusinessSalePolicy,
            &mut BusinessAccount,
            &BusinessCondition,
            &shared::components::OperatedBy,
            &mut TavernService,
        )>,
    )>,
    mut people: Query<
        (
            &shared::components::ResidentOf,
            &mut Wallet,
            &mut Nutrition,
            &WorkStatus,
            &mut CharacterDayPlan,
        ),
        (With<CharacterKind>, With<strategic::StrategicPerson>),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let minute = display_minute(clock);
    if *last_minute == Some((clock.day, minute)) {
        return;
    }
    *last_minute = Some((clock.day, minute));
    if !(TAVERN_OPEN_MINUTE..TAVERN_CLOSE_MINUTE).contains(&minute) {
        return;
    }
    let offers: Vec<_> = taverns
        .p0()
        .iter()
        .filter(|(_, _, _, condition, inventory, service)| {
            condition.state.can_operate()
                && service.innkeepers_on_duty > 0
                && service.current_day.served_meals < service.daily_capacity()
                && (inventory.amount(Good::Bread) > 0 || inventory.amount(Good::Meat) > 0)
        })
        .map(|(entity, building_of, sale, _, _, _)| {
            (entity, building_of.0, sale.asking_unit_price.max(1))
        })
        .collect();

    let mut live_taverns = taverns.p1();
    for (resident_of, mut wallet, mut nutrition, work_status, mut plan) in people.iter_mut() {
        if !plan.has_due_leisure(minute) {
            continue;
        }
        let offer = offers
            .iter()
            .filter(|(_, settlement, _)| *settlement == resident_of.0)
            .min_by_key(|(_, _, price)| *price)
            .copied();
        let Some((tavern, _, _)) = offer else {
            plan.leisure_status = PlannedLeisureStatus::TavernUnavailable;
            continue;
        };
        let Ok((mut inventory, sale, mut account, condition, operated_by, mut service)) =
            live_taverns.get_mut(tavern)
        else {
            plan.leisure_status = PlannedLeisureStatus::TavernUnavailable;
            continue;
        };
        service.record_planned_visit(clock.day);
        let price = sale.asking_unit_price.max(1);
        if !condition.state.can_operate()
            || service.innkeepers_on_duty == 0
            || service.current_day.served_meals >= service.daily_capacity()
        {
            service.current_day.unavailable_visits =
                service.current_day.unavailable_visits.saturating_add(1);
            plan.leisure_status = PlannedLeisureStatus::TavernUnavailable;
            continue;
        }
        if !meal_is_affordable(*wallet, *work_status, nutrition.is_hungry(), price) {
            service.current_day.unaffordable_visits =
                service.current_day.unaffordable_visits.saturating_add(1);
            plan.leisure_status = PlannedLeisureStatus::CouldNotAfford;
            continue;
        }
        let ingredient = [Good::Bread, Good::Meat]
            .into_iter()
            .filter(|good| inventory.amount(*good) > 0)
            .max_by_key(|good| inventory.amount(*good));
        let Some(ingredient) = ingredient else {
            service.current_day.unavailable_visits =
                service.current_day.unavailable_visits.saturating_add(1);
            plan.leisure_status = PlannedLeisureStatus::TavernUnavailable;
            continue;
        };
        let Some((_, mut company)) = companies
            .iter_mut()
            .find(|(company_id, _)| **company_id == operated_by.0)
        else {
            service.current_day.unavailable_visits =
                service.current_day.unavailable_visits.saturating_add(1);
            plan.leisure_status = PlannedLeisureStatus::TavernUnavailable;
            continue;
        };
        if !wallet.debit(price) {
            service.current_day.unaffordable_visits =
                service.current_day.unaffordable_visits.saturating_add(1);
            plan.leisure_status = PlannedLeisureStatus::CouldNotAfford;
            continue;
        }
        let consumed = inventory.remove(ingredient, 1);
        debug_assert_eq!(consumed, 1);
        company.credit(price);
        account.record_service_sale(clock.day, price, 1);
        service.record_meal(clock.day, ingredient, price);
        nutrition.record_meal(clock.day.saturating_add(1));
        plan.leisure_status = PlannedLeisureStatus::Completed;
    }
}

/// Daily owner autopilot for a service business. It reacts to actual visits,
/// turnaways, ingredient costs and payroll; no law caps manual prices.
#[allow(clippy::type_complexity)]
pub fn review_tavern_businesses(
    world_time: Query<&WorldTime>,
    halls: Query<(&shared::components::SettlementId, &MootMarket), With<Settlement>>,
    companies: Query<(
        &shared::components::CompanyId,
        &shared::economy::CompanyAccount,
    )>,
    employment: Query<&shared::components::EmployedAt, With<CharacterKind>>,
    mut taverns: Query<
        (
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
            &shared::components::OperatedBy,
            &GoodsInventory,
            &mut BusinessAccount,
            &mut BusinessSalePolicy,
            &BusinessWagePolicy,
            &BusinessManagementPolicy,
            &mut BusinessCondition,
            &mut TavernService,
        ),
        With<SettlementBuilding>,
    >,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    let company_cash: HashMap<_, _> = companies
        .iter()
        .map(|(id, account)| {
            (
                *id,
                account
                    .cash
                    .saturating_sub(account.wage_arrears)
                    .saturating_sub(account.tax_arrears),
            )
        })
        .collect();
    let mut worker_counts = HashMap::<shared::components::BuildingId, u64>::new();
    for employed_at in employment.iter() {
        *worker_counts.entry(employed_at.0).or_default() += 1;
    }
    for (
        building_id,
        building_of,
        operated_by,
        inventory,
        mut account,
        mut sale,
        wage,
        management,
        mut condition,
        mut service,
    ) in taverns.iter_mut()
    {
        service.roll_to_day(day);
        if condition.opened_day == u32::MAX {
            condition.opened_day = day;
        }
        if condition.last_review_day == day {
            continue;
        }
        if matches!(
            condition.state,
            BusinessState::ForSale | BusinessState::Closed | BusinessState::Liquidating
        ) {
            // These states belong to the generic company-management and
            // liquidation path. Leaving their review marker untouched lets
            // that system auction stock, settle claims and release the site;
            // Tavern-specific pricing must never strand food in a failed firm.
            continue;
        }
        let elapsed = if condition.last_review_day == u32::MAX {
            1
        } else {
            day.saturating_sub(condition.last_review_day).max(1)
        }
        .min(u32::from(u16::MAX)) as u16;
        let evidence = service.previous_day;
        account.roll_to_day(day);
        if management.autopilot && sale.automatic_pricing {
            let ingredient_cost = halls.iter().find(|(id, _)| **id == building_of.0).map_or(
                Good::Meat.base_price(),
                |(_, market)| {
                    market
                        .suggested_price(Good::Bread)
                        .min(market.suggested_price(Good::Meat))
                },
            );
            let labour = wage
                .daily_wage
                .div_ceil(shared::economy::TAVERN_MEALS_PER_INNKEEPER_DAY as u64);
            account.estimated_unit_cost = ingredient_cost.saturating_add(labour);
            sale.target_margin_bps = management.strategy.target_margin_bps();
            sale.max_daily_price_change_bps = management.strategy.daily_price_step_bps();
            let sustainable = shared::economy::sustainable_unit_price(
                account.estimated_unit_cost,
                0,
                sale.target_margin_bps,
            );
            let step = u64::from(sale.max_daily_price_change_bps.max(100));
            let raise = evidence.unavailable_visits > 0
                && evidence.served_meals >= service.daily_capacity().max(1);
            let lower = evidence.unaffordable_visits > 0
                || (evidence.planned_visits > 0 && evidence.served_meals == 0)
                || (evidence.planned_visits == 0
                    && (inventory.amount(Good::Bread) > 0 || inventory.amount(Good::Meat) > 0));
            if raise {
                sale.asking_unit_price = sale
                    .asking_unit_price
                    .saturating_mul(BASIS_POINTS.saturating_add(step))
                    .div_ceil(BASIS_POINTS);
            } else if lower {
                sale.asking_unit_price = sale
                    .asking_unit_price
                    .saturating_mul(BASIS_POINTS.saturating_sub(step).max(1))
                    / BASIS_POINTS;
            } else {
                sale.asking_unit_price = (sale
                    .asking_unit_price
                    .saturating_mul(3)
                    .saturating_add(sustainable))
                    / 4;
            }
            sale.asking_unit_price = sale.asking_unit_price.max(sale.minimum_unit_price).max(1);
        }
        let free_cash = company_cash.get(&operated_by.0).copied().unwrap_or(0);
        let has_claims = account.wage_arrears > 0 || account.tax_arrears > 0;
        condition.state = if has_claims && free_cash == 0 {
            condition.insolvent_days = condition.insolvent_days.saturating_add(elapsed);
            condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
            if condition.insolvent_days >= TAVERN_INSOLVENT_DAYS_BEFORE_LIQUIDATION {
                BusinessState::Liquidating
            } else {
                BusinessState::Insolvent
            }
        } else if has_claims {
            condition.insolvent_days = 0;
            condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
            BusinessState::Distressed
        } else {
            condition.insolvent_days = 0;
            let daily_payroll = wage
                .daily_wage
                .saturating_mul(worker_counts.get(building_id).copied().unwrap_or(0));
            if daily_payroll > 0 && free_cash < daily_payroll.saturating_mul(2) {
                condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
                BusinessState::CashTight
            } else {
                condition.cash_tight_days = 0;
                if day.saturating_sub(condition.opened_day) < 3 {
                    BusinessState::New
                } else {
                    BusinessState::Operating
                }
            }
        };
        if matches!(
            condition.state,
            BusinessState::Insolvent | BusinessState::Liquidating
        ) {
            // The generic failure path owns shareholder rescue, staff claims,
            // food liquidation and the eventual property sale. Do not mark
            // this site reviewed: it must run that path in the same schedule.
            continue;
        }
        condition.last_review_day = day;
        sale.last_review_day = day;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_same_day_plans_are_spread_instead_of_immediately_overdue() {
        assert_eq!(
            stagger_late_leisure_start(18 * 60, 21 * 60, 17 * 60, 0),
            Some(18 * 60)
        );
        assert_eq!(
            stagger_late_leisure_start(18 * 60, 21 * 60, 18 * 60 + 30, 0),
            Some(18 * 60 + 35)
        );
        assert_eq!(
            stagger_late_leisure_start(18 * 60, 21 * 60, 18 * 60 + 30, 1 << 24),
            Some(18 * 60 + 36)
        );
        assert_eq!(
            stagger_late_leisure_start(18 * 60, 21 * 60, 20 * 60 + 50, 0),
            None
        );
    }

    #[test]
    fn tavern_queue_is_a_distinct_fifo_line_outside_the_door() {
        let center = Vec3::new(20.0, 3.0, 30.0);
        let entrance = Vec3::new(20.0, 3.0, 26.0);
        let points: Vec<_> = (0..8)
            .map(|rank| tavern_queue_target(entrance, center, rank))
            .collect();
        assert_eq!(points[0], entrance);
        for pair in points.windows(2) {
            assert!((ground_distance(pair[0], pair[1]) - TAVERN_QUEUE_SPACING).abs() < 0.001);
            assert!(pair[1].z < pair[0].z);
        }
    }

    #[test]
    fn a_visitor_at_the_line_enters_before_an_earlier_admittee_still_crossing_town() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, run_tavern_routines);

        let mut clock = WorldTime::new_default();
        clock.day = 2;
        clock.set_normalized_time(18.5 / 24.0);
        app.world_mut().spawn(clock);

        let mut inventory = GoodsInventory::new(shared::economy::capacity::VILLAGER);
        inventory.add(Good::Bread, 10);
        let mut service = TavernService::default();
        service.guest_capacity = 8;
        service.innkeepers_on_duty = 1;
        let tavern = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Tavern,
                    settlement: "Brackwater".to_string(),
                    owner: Some("Aldric".to_string()),
                    quality: 1.0,
                    workers: vec!["Inga".to_string()],
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                shared::components::BuildingId(7),
                inventory,
                BusinessSalePolicy::default(),
                BusinessAccount::default(),
                BusinessCondition {
                    state: BusinessState::Operating,
                    ..Default::default()
                },
                shared::components::OperatedBy(shared::components::CompanyId(1)),
                service,
            ))
            .id();
        let entrance = SettlementBuildingKind::Tavern.entrance_position(Vec3::ZERO, 0.0);
        let spawn_visitor = |app: &mut App, position: Vec3, queue_order: u128| {
            app.world_mut()
                .spawn((
                    PlayerPosition(position),
                    CharacterActivity::Idle,
                    Wallet::new(100 * PENNIES_PER_COIN),
                    Nutrition::default(),
                    WorkStatus::Employed,
                    CharacterDayPlan {
                        day: 2,
                        wake_minute: 6 * 60,
                        work_minutes: None,
                        meal_minute: 18 * 60,
                        leisure_minutes: (18 * 60, TAVERN_CLOSE_MINUTE),
                        sleep_minute: 22 * 60,
                        leisure: PlannedLeisure::TavernMeal,
                        leisure_status: PlannedLeisureStatus::InProgress,
                        planned_work_status: WorkStatus::Employed,
                    },
                    TavernVisitRoutine {
                        tavern,
                        queue_order,
                        phase: TavernVisitPhase::Going,
                        dining_seconds: 0.0,
                        failed_routes: 0,
                        served: false,
                        progress_position: position,
                        progress_world_seconds: 0.0,
                    },
                ))
                .id()
        };
        // Admitted FIRST (smaller queue order) but still 200 m out.
        let distant = spawn_visitor(&mut app, Vec3::new(200.0, 0.0, 200.0), 1);
        // Admitted second, but physically standing at the tavern door.
        let present = spawn_visitor(&mut app, entrance, 2);

        app.update();

        assert!(
            matches!(
                app.world()
                    .get::<TavernVisitRoutine>(present)
                    .unwrap()
                    .phase,
                TavernVisitPhase::Entering
            ),
            "the visitor at the door must be served, not idled behind a distant walker"
        );
        assert!(matches!(
            app.world()
                .get::<TavernVisitRoutine>(distant)
                .unwrap()
                .phase,
            TavernVisitPhase::Going
        ));
    }

    #[test]
    fn tactical_tavern_admission_reserves_capacity_before_anyone_walks() {
        let mut app = App::new();
        app.add_systems(Update, assign_tavern_routines);

        let mut clock = WorldTime::new_default();
        clock.day = 2;
        clock.set_normalized_time(18.5 / 24.0);
        app.world_mut().spawn(clock);

        let settlement_id = shared::components::SettlementId(91);
        let mut service = TavernService::default();
        service.guest_capacity = 3;
        app.world_mut().spawn((
            shared::components::BuildingId(92),
            shared::components::BuildingOf(settlement_id),
            PlayerPosition(Vec3::ZERO),
            BusinessCondition {
                state: BusinessState::Operating,
                ..Default::default()
            },
            service,
        ));

        for index in 0..10 {
            app.world_mut().spawn((
                CharacterKind::Villager,
                shared::components::ResidentOf(settlement_id),
                PlayerPosition(Vec3::new(index as f32, 0.0, 20.0)),
                CharacterDayPlan {
                    day: 2,
                    wake_minute: 6 * 60,
                    work_minutes: None,
                    meal_minute: 18 * 60,
                    leisure_minutes: (18 * 60, TAVERN_CLOSE_MINUTE),
                    sleep_minute: 22 * 60,
                    leisure: PlannedLeisure::TavernMeal,
                    leisure_status: PlannedLeisureStatus::Planned,
                    planned_work_status: WorkStatus::LookingForWork,
                },
            ));
        }

        app.update();

        let admitted = app
            .world_mut()
            .query::<&TavernVisitRoutine>()
            .iter(app.world())
            .count();
        let still_planned = app
            .world_mut()
            .query::<&CharacterDayPlan>()
            .iter(app.world())
            .filter(|plan| plan.leisure_status == PlannedLeisureStatus::Planned)
            .count();
        assert_eq!(admitted, 3);
        assert_eq!(still_planned, 7);
    }

    #[test]
    fn affordability_protects_a_job_seekers_last_coins() {
        let wallet = Wallet::new(8 * PENNIES_PER_COIN);
        assert!(!meal_is_affordable(
            wallet,
            WorkStatus::LookingForWork,
            false,
            4 * PENNIES_PER_COIN
        ));
        assert!(meal_is_affordable(
            wallet,
            WorkStatus::Employed,
            false,
            2 * PENNIES_PER_COIN
        ));
        assert!(meal_is_affordable(
            wallet,
            WorkStatus::LookingForWork,
            true,
            4 * PENNIES_PER_COIN
        ));
    }

    #[test]
    fn strategic_tavern_visit_moves_real_food_and_money_exactly_once() {
        let mut app = App::new();
        app.add_systems(Update, run_strategic_tavern_visits);

        let mut clock = WorldTime::new_default();
        clock.day = 3;
        clock.set_normalized_time(18.5 / 24.0);
        app.world_mut().spawn(clock);

        let settlement_id = shared::components::SettlementId(10);
        let company_id = shared::components::CompanyId(20);
        let company = app
            .world_mut()
            .spawn((company_id, shared::economy::CompanyAccount::default()))
            .id();

        let price = 2 * PENNIES_PER_COIN;
        let mut pantry = GoodsInventory::new(100);
        assert_eq!(pantry.add(Good::Bread, 3), 3);
        let mut service = TavernService::default();
        service.roll_to_day(3);
        service.innkeepers_on_duty = 1;
        let tavern = app
            .world_mut()
            .spawn((
                shared::components::BuildingOf(settlement_id),
                shared::components::OperatedBy(company_id),
                BusinessSalePolicy {
                    asking_unit_price: price,
                    ..Default::default()
                },
                BusinessCondition {
                    state: BusinessState::Operating,
                    ..Default::default()
                },
                BusinessAccount::default(),
                pantry,
                service,
            ))
            .id();

        let visitor = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                strategic::StrategicPerson,
                shared::components::ResidentOf(settlement_id),
                Wallet::new(10 * PENNIES_PER_COIN),
                Nutrition::default(),
                WorkStatus::Employed,
                CharacterDayPlan {
                    day: 3,
                    wake_minute: 6 * 60,
                    work_minutes: Some((6 * 60, 18 * 60)),
                    meal_minute: 18 * 60,
                    leisure_minutes: (18 * 60, 21 * 60),
                    sleep_minute: 22 * 60,
                    leisure: PlannedLeisure::TavernMeal,
                    leisure_status: PlannedLeisureStatus::Planned,
                    planned_work_status: WorkStatus::Employed,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(visitor)
                .get::<Wallet>()
                .unwrap()
                .balance(),
            8 * PENNIES_PER_COIN
        );
        assert_eq!(
            app.world()
                .entity(visitor)
                .get::<Nutrition>()
                .unwrap()
                .last_meal_day,
            Some(4)
        );
        assert_eq!(
            app.world()
                .entity(visitor)
                .get::<CharacterDayPlan>()
                .unwrap()
                .leisure_status,
            PlannedLeisureStatus::Completed
        );
        assert_eq!(
            app.world()
                .entity(tavern)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Bread),
            2
        );
        let business = app.world().entity(tavern).get::<BusinessAccount>().unwrap();
        assert_eq!(business.current_day.sold_units, 1);
        assert_eq!(business.current_day.gross_revenue, price);
        let service = app.world().entity(tavern).get::<TavernService>().unwrap();
        assert_eq!(service.current_day.served_meals, 1);
        assert_eq!(service.current_day.bread_used, 1);
        assert_eq!(service.current_day.revenue, price);
        assert_eq!(
            app.world()
                .entity(company)
                .get::<shared::economy::CompanyAccount>()
                .unwrap()
                .cash,
            price
        );

        // The display minute and plan are unchanged; the minute gate and
        // completed plan must both prevent a duplicate charge or meal.
        app.update();
        assert_eq!(
            app.world()
                .entity(visitor)
                .get::<Wallet>()
                .unwrap()
                .balance(),
            8 * PENNIES_PER_COIN
        );
        assert_eq!(
            app.world()
                .entity(tavern)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Bread),
            2
        );
    }
}
