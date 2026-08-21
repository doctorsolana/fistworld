//! Visible, FIFO services in the Moot Hall forecourt.
//!
//! The market transaction remains authoritative when a ration is reserved or
//! a permit is approved. Tactical villagers then occupy distinct queue places
//! and collect the thing in person; strategic villagers use the same economic
//! rules without paying for local paths.

use super::*;

const QUEUE_REACH: f32 = 0.55;
const QUEUE_SPACING: f32 = 1.45;
/// A follower advances only after the person ahead has physically cleared
/// most of the next place. This turns a whole-line shuffle into the small
/// forward-moving waves seen in real switchback queues without adding a
/// neighbour search or another simulation pass.
const QUEUE_FOLLOW_CLEARANCE: f32 = 0.90;
const QUEUE_ROW_LENGTH: usize = 6;
const QUEUE_FIRST_ROW_Z: f32 = -6.85;
const QUEUE_SERVICE_Z: f32 = -5.85;
const MAX_QUEUE_ROUTE_FAILURES: u8 = 6;
/// Only time without measurable forward progress counts toward this fallback.
/// A distant resident walking normally may take longer; a genuinely frozen
/// head must release the entire line promptly.
const MAX_QUEUE_HEAD_WAIT_SECONDS: f32 = 15.0;
const QUEUE_HEAD_PROGRESS_EPSILON: f32 = 0.02;
// These are handovers of transactions already decided by the market, not full
// bureaucratic appointments. A hundred-household town must clear its morning
// food line in minutes rather than carrying it into the next world day.
const PERMIT_SERVICE_SECONDS: f32 = 3.0;
const IMMIGRATION_SERVICE_SECONDS: f32 = 2.0;
const SHOPPING_SERVICE_SECONDS: f32 = 1.0;
const MEAL_SERVICE_SECONDS: f32 = 1.0;
const COMMONS_MEAL_SECONDS: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MootServiceLane {
    Immigration,
    Resident,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MootServiceKind {
    Immigration,
    Permit,
    HouseholdShopping,
    PersonalMeal,
    PoorRelief,
}

impl MootServiceKind {
    fn lane(self) -> MootServiceLane {
        match self {
            Self::Immigration => MootServiceLane::Immigration,
            Self::Permit | Self::HouseholdShopping | Self::PersonalMeal | Self::PoorRelief => {
                MootServiceLane::Resident
            }
        }
    }

    fn service_seconds(self) -> f32 {
        match self {
            Self::Immigration => IMMIGRATION_SERVICE_SECONDS,
            Self::Permit => PERMIT_SERVICE_SECONDS,
            Self::HouseholdShopping => SHOPPING_SERVICE_SECONDS,
            Self::PersonalMeal | Self::PoorRelief => MEAL_SERVICE_SECONDS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Immigration => "immigration registration",
            Self::Permit => "permit",
            Self::HouseholdShopping => "household shopping",
            Self::PersonalMeal => "food purchase",
            Self::PoorRelief => "Poor Relief",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MootQueueState {
    Queued,
    Serving { seconds_left: f32 },
    Ready,
}

/// Server-only place in one of a hall's visible service lines.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MootQueueTicket {
    pub(crate) hall: Entity,
    serial: u64,
    pub(crate) kind: MootServiceKind,
    state: MootQueueState,
    failed_routes: u8,
    head_wait_seconds: f32,
    head_best_distance: f32,
}

impl MootQueueTicket {
    pub(crate) fn is_ready(self) -> bool {
        self.state == MootQueueState::Ready
    }

    pub(crate) const fn objective(self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        let at_counter = matches!(
            self.state,
            MootQueueState::Serving { .. } | MootQueueState::Ready
        );
        match (self.kind, at_counter) {
            (MootServiceKind::Immigration, false) => CharacterObjective::QueuedForImmigration,
            (MootServiceKind::Immigration, true) => CharacterObjective::RegisteringImmigration,
            (MootServiceKind::Permit, false) => CharacterObjective::QueuedForPermit,
            (MootServiceKind::Permit, true) => CharacterObjective::CollectingPermit,
            (MootServiceKind::HouseholdShopping, _) => CharacterObjective::QueuedForHouseholdFood,
            (MootServiceKind::PersonalMeal, _) => CharacterObjective::QueuedForPersonalFood,
            (MootServiceKind::PoorRelief, _) => CharacterObjective::QueuedForPoorRelief,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct MootQueueClock {
    next_serial: u64,
    peak_depth: HashMap<(Entity, MootServiceLane), usize>,
}

impl MootQueueClock {
    fn issue(&mut self) -> u64 {
        self.next_serial = self.next_serial.wrapping_add(1).max(1);
        self.next_serial
    }
}

/// A short, collision-checked step along the authored Moot forecourt. It is
/// intentionally excluded from global A*: advancing one place in a visible
/// line must not create a fresh town-scale route request.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MootQueueTransit;

/// A purchased ration belongs to this villager while they queue. It is no
/// longer market stock, but it is not visible cargo until the counter hands it
/// over.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MootMealRoutine {
    hall: Entity,
    pub(crate) good: Good,
    pub(crate) meal_day: u32,
    phase: MootMealPhase,
}

impl MootMealRoutine {
    pub(crate) const fn objective(self) -> shared::components::CharacterObjective {
        use shared::components::CharacterObjective;
        match self.phase {
            // While queueing, MootQueueTicket provides the more precise
            // purchase-versus-relief reason.
            MootMealPhase::Queueing => CharacterObjective::QueuedForPersonalFood,
            MootMealPhase::Carrying { .. } => CharacterObjective::CollectingFood,
            MootMealPhase::Eating { .. } => CharacterObjective::Eating,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MootMealPhase {
    Queueing,
    Carrying { destination: Vec3 },
    Eating { seconds_left: f32 },
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PermitPickupRoutine {
    site: Entity,
}

pub(crate) fn enqueue_moot_service(
    commands: &mut Commands,
    clock: &mut MootQueueClock,
    person: Entity,
    hall: Entity,
    kind: MootServiceKind,
) {
    commands
        .entity(person)
        .insert((
            MootQueueTicket {
                hall,
                serial: clock.issue(),
                kind,
                state: MootQueueState::Queued,
                failed_routes: 0,
                head_wait_seconds: 0.0,
                head_best_distance: f32::INFINITY,
            },
            CharacterActivity::Idle,
        ))
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<BuildingDoorUse>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<HomeRoutine>()
        .remove::<ambient::AmbientRoutine>()
        .remove::<MootQueueTransit>();
}

fn local_queue_slot(lane: MootServiceLane, index: usize) -> Vec2 {
    let side = match lane {
        MootServiceLane::Immigration => -1.0,
        MootServiceLane::Resident => 1.0,
    };
    if index == 0 {
        return Vec2::new(side * 1.25, QUEUE_SERVICE_Z);
    }
    let offset = index - 1;
    let row = offset / QUEUE_ROW_LENGTH;
    let column = offset % QUEUE_ROW_LENGTH;
    let x_index = if row.is_multiple_of(2) {
        column + 1
    } else {
        QUEUE_ROW_LENGTH - column
    };
    Vec2::new(
        side * (1.25 + x_index as f32 * QUEUE_SPACING),
        QUEUE_FIRST_ROW_Z - row as f32 * QUEUE_SPACING,
    )
}

fn world_slot(
    hall: Vec3,
    yaw: f32,
    lane: MootServiceLane,
    index: usize,
    terrain: Option<&WorldTerrain>,
) -> Vec3 {
    let offset = shared::rotation::local_to_world_xz(local_queue_slot(lane, index), yaw);
    let x = hall.x + offset.x;
    let z = hall.z + offset.y;
    let y = terrain.map_or(hall.y, |terrain| terrain.get_height(x, z));
    Vec3::new(x, y, z)
}

fn facing_toward(from: Vec3, to: Vec3) -> f32 {
    let direction = Vec2::new(to.x - from.x, to.z - from.z);
    f32::atan2(-direction.x, -direction.y)
}

/// Keep one stable FIFO line per hall and service lane. A changing line only changes each
/// person's short forecourt destination; the ordinary cached road/nav systems
/// still own the longer approach.
#[allow(clippy::type_complexity)]
pub(crate) fn advance_moot_service_queues(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    mut clock: ResMut<MootQueueClock>,
    halls: Query<(&PlayerPosition, Option<&PlayerRotation>), With<Settlement>>,
    mut people: Query<
        (
            Entity,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            Option<&MoveTarget>,
            Option<&TravelRoute>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
            Has<MootQueueTransit>,
            &mut MootQueueTicket,
        ),
        Without<Settlement>,
    >,
    mut commands: Commands,
) {
    let mut queues: HashMap<(Entity, MootServiceLane), Vec<(u64, Entity, Vec3)>> = HashMap::new();
    for (person, position, _, _, _, _, _, _, _, ticket) in people.iter_mut() {
        queues
            .entry((ticket.hall, ticket.kind.lane()))
            .or_default()
            .push((ticket.serial, person, position.0));
    }
    for queue in queues.values_mut() {
        queue.sort_unstable_by_key(|(serial, _, _)| *serial);
    }

    for ((hall_entity, lane), queue) in queues {
        let Ok((hall_position, hall_rotation)) = halls.get(hall_entity) else {
            for (_, person, _) in queue {
                commands
                    .entity(person)
                    .remove::<MootQueueTicket>()
                    .remove::<MootQueueTransit>()
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
            }
            continue;
        };
        let previous_peak = clock.peak_depth.entry((hall_entity, lane)).or_default();
        if queue.len() > *previous_peak {
            *previous_peak = queue.len();
            info!("Moot {lane:?} queue reached {} villagers", queue.len());
        }
        let yaw = hall_rotation.map_or(0.0, |rotation| rotation.0);
        for rank in 0..queue.len() {
            let (_, person, _) = queue[rank];
            let target = world_slot(hall_position.0, yaw, lane, rank, terrain.as_deref());
            let ahead_target = if rank == 0 {
                hall_position.0
            } else {
                world_slot(hall_position.0, yaw, lane, rank - 1, terrain.as_deref())
            };
            let ahead_position = rank.checked_sub(1).map(|ahead| queue[ahead].2);
            let Ok((
                _,
                position,
                mut rotation,
                mut activity,
                move_target,
                travel_route,
                route_pending,
                route_failed,
                queue_transit,
                mut ticket,
            )) = people.get_mut(person)
            else {
                continue;
            };
            activity.set_if_neq(CharacterActivity::Idle);
            if ticket.is_ready() {
                continue;
            }
            let arrived = ground_distance(position.0, target) <= QUEUE_REACH;
            if rank == 0 && !arrived {
                let distance = ground_distance(position.0, target);
                if distance + QUEUE_HEAD_PROGRESS_EPSILON < ticket.head_best_distance {
                    ticket.head_best_distance = distance;
                    ticket.head_wait_seconds = 0.0;
                } else {
                    ticket.head_wait_seconds += simulation_time.world_seconds();
                }
                if ticket.head_wait_seconds >= MAX_QUEUE_HEAD_WAIT_SECONDS {
                    warn!(
                        "A villager made no progress at the front of the Moot {} line for {:.0} world seconds; completing at the counter fallback",
                        ticket.kind.label(),
                        ticket.head_wait_seconds,
                    );
                    ticket.state = MootQueueState::Ready;
                    commands
                        .entity(person)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<MootQueueTransit>();
                    continue;
                }
            } else {
                ticket.head_wait_seconds = 0.0;
                ticket.head_best_distance = f32::INFINITY;
            }
            // Do not command the entire line toward newly assigned slots in
            // one synchronized burst. The authoritative positions captured at
            // the start of this O(n) pass are enough to let the empty place
            // ripple backward one neighbour at a time.
            if !arrived
                && ahead_position
                    .is_some_and(|ahead| ground_distance(ahead, target) < QUEUE_FOLLOW_CLEARANCE)
            {
                if let Some(ahead) = ahead_position {
                    if ground_distance(position.0, ahead) > 0.05 {
                        rotation.set_if_neq(PlayerRotation(facing_toward(position.0, ahead)));
                    }
                }
                ticket.state = MootQueueState::Queued;
                // The follower may sit still for many ticks. Queue deferred
                // removals only on the transition into waiting, not on every
                // tick of a thousand-person line.
                if move_target.is_some()
                    || travel_route.is_some()
                    || route_pending.is_some()
                    || route_failed.is_some()
                    || queue_transit
                {
                    commands
                        .entity(person)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<MootQueueTransit>();
                }
                continue;
            }
            if route_failed.is_some() {
                ticket.failed_routes = ticket.failed_routes.saturating_add(1);
                commands
                    .entity(person)
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .remove::<MootQueueTransit>()
                    .insert(MoveTarget(target));
                if ticket.failed_routes >= MAX_QUEUE_ROUTE_FAILURES && rank == 0 {
                    warn!(
                        "A villager could not reach the Moot {} counter after {} routes; completing at the counter fallback",
                        ticket.kind.label(),
                        ticket.failed_routes
                    );
                    ticket.state = MootQueueState::Ready;
                }
                continue;
            }
            if !arrived {
                let start = Vec2::new(position.0.x, position.0.z);
                let end = Vec2::new(target.x, target.z);
                let local_step_clear = crate::player::hero::navigation_segment_clear(
                    start,
                    end,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                );
                ensure_move_target(&mut commands, person, move_target, target);
                if local_step_clear {
                    commands
                        .entity(person)
                        .insert(MootQueueTransit)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                } else {
                    commands.entity(person).remove::<MootQueueTransit>();
                    // A previous direct forecourt step may have discovered a
                    // newly streamed blocker. Preserve the pending request it
                    // created so ordinary A* can route around it.
                    if route_pending.is_none() {
                        commands
                            .entity(person)
                            .insert(NavigationRoutePending::new(target));
                    }
                }
                ticket.state = MootQueueState::Queued;
                continue;
            }
            commands
                .entity(person)
                .remove::<MoveTarget>()
                .remove::<MootQueueTransit>()
                .remove::<NavigationRoutePending>();
            rotation.set_if_neq(PlayerRotation(facing_toward(position.0, ahead_target)));
            if rank != 0 {
                ticket.state = MootQueueState::Queued;
                continue;
            }
            match ticket.state {
                MootQueueState::Queued => {
                    ticket.state = MootQueueState::Serving {
                        seconds_left: ticket.kind.service_seconds(),
                    };
                }
                MootQueueState::Serving { mut seconds_left } => {
                    seconds_left -= simulation_time.world_seconds();
                    ticket.state = if seconds_left <= 0.0 {
                        MootQueueState::Ready
                    } else {
                        MootQueueState::Serving { seconds_left }
                    };
                }
                MootQueueState::Ready => {}
            }
        }
    }
}

/// The queue is the last step of permit administration. Construction material
/// work begins only after the approved applicant has physically collected the
/// stamped permit.
pub(crate) fn complete_moot_permit_pickups(
    mut commands: Commands,
    applicants: Query<(Entity, &MootQueueTicket, &PermitPickupRoutine)>,
) {
    for (applicant, ticket, pickup) in applicants.iter() {
        if ticket.kind != MootServiceKind::Permit || !ticket.is_ready() {
            continue;
        }
        commands
            .entity(applicant)
            .insert((
                ConstructionMaterialRoutine::new(pickup.site),
                CharacterActivity::Idle,
            ))
            .remove::<MootQueueTicket>()
            .remove::<PermitPickupRoutine>()
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .remove::<MootQueueTransit>();
    }
}

fn commons_meal_spot(person: Entity, hall: Vec3, yaw: f32, terrain: Option<&WorldTerrain>) -> Vec3 {
    let slot = person.to_bits() as usize % 6;
    let row = slot / 3;
    let column = slot % 3;
    let local = Vec2::new(2.4 + column as f32 * 1.65, -7.0 - row as f32 * 1.65);
    let offset = shared::rotation::local_to_world_xz(local, yaw);
    let x = hall.x + offset.x;
    let z = hall.z + offset.y;
    let y = terrain.map_or(hall.y, |terrain| terrain.get_height(x, z));
    Vec3::new(x, y, z)
}

/// Hand over a reserved ration, show it as carried cargo, and let the villager
/// eat in the Moot commons. Failed last-metre routes resolve the already-paid
/// meal rather than destroying it or leaving hunger permanently wedged.
#[allow(clippy::type_complexity)]
pub(crate) fn run_moot_meal_collections(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    halls: Query<(&PlayerPosition, Option<&PlayerRotation>), With<Settlement>>,
    mut people: Query<
        (
            Entity,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut GoodsInventory,
            &mut Nutrition,
            &mut MootMealRoutine,
            Option<&MootQueueTicket>,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        Without<Settlement>,
    >,
    mut commands: Commands,
) {
    for (
        person,
        position,
        mut rotation,
        mut activity,
        mut carrier,
        mut nutrition,
        mut meal,
        ticket,
        move_target,
        route_failed,
    ) in people.iter_mut()
    {
        match meal.phase {
            MootMealPhase::Queueing => {
                let Some(ticket) = ticket else {
                    // The ration was paid for and removed from sale already.
                    nutrition.record_meal(meal.meal_day);
                    commands.entity(person).remove::<MootMealRoutine>();
                    continue;
                };
                if !ticket.is_ready() {
                    continue;
                }
                let Ok((hall, hall_rotation)) = halls.get(ticket.hall) else {
                    nutrition.record_meal(meal.meal_day);
                    commands
                        .entity(person)
                        .remove::<MootMealRoutine>()
                        .remove::<MootQueueTicket>();
                    continue;
                };
                if carrier.add(meal.good, 1) != 1 {
                    nutrition.record_meal(meal.meal_day);
                    commands
                        .entity(person)
                        .remove::<MootMealRoutine>()
                        .remove::<MootQueueTicket>();
                    continue;
                }
                let yaw = hall_rotation.map_or(0.0, |rotation| rotation.0);
                let destination = commons_meal_spot(person, hall.0, yaw, terrain.as_deref());
                meal.phase = MootMealPhase::Carrying { destination };
                commands
                    .entity(person)
                    .insert(MoveTarget(destination))
                    .remove::<MootQueueTicket>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
            }
            MootMealPhase::Carrying { destination } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if route_failed.is_some() {
                    carrier.remove(meal.good, 1);
                    nutrition.record_meal(meal.meal_day);
                    commands
                        .entity(person)
                        .remove::<MootMealRoutine>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    continue;
                }
                if ground_distance(position.0, destination) > QUEUE_REACH {
                    ensure_move_target(&mut commands, person, move_target, destination);
                    continue;
                }
                carrier.remove(meal.good, 1);
                nutrition.record_meal(meal.meal_day);
                let hall_position = halls.get(meal.hall).map_or(position.0, |hall| hall.0 .0);
                rotation.set_if_neq(PlayerRotation(facing_toward(position.0, hall_position)));
                activity.set_if_neq(CharacterActivity::Sitting);
                meal.phase = MootMealPhase::Eating {
                    seconds_left: COMMONS_MEAL_SECONDS,
                };
                commands.entity(person).remove::<MoveTarget>();
            }
            MootMealPhase::Eating { mut seconds_left } => {
                activity.set_if_neq(CharacterActivity::Sitting);
                seconds_left -= simulation_time.world_seconds();
                if seconds_left <= 0.0 {
                    activity.set_if_neq(CharacterActivity::Idle);
                    commands.entity(person).remove::<MootMealRoutine>();
                } else {
                    meal.phase = MootMealPhase::Eating { seconds_left };
                }
            }
        }
    }
}

pub(crate) fn reserve_meal(
    commands: &mut Commands,
    clock: &mut MootQueueClock,
    person: Entity,
    hall: Entity,
    kind: MootServiceKind,
    good: Good,
    meal_day: u32,
) {
    commands.entity(person).insert(MootMealRoutine {
        hall,
        good,
        meal_day,
        phase: MootMealPhase::Queueing,
    });
    enqueue_moot_service(commands, clock, person, hall, kind);
}

pub(crate) fn wait_for_permit(
    commands: &mut Commands,
    clock: &mut MootQueueClock,
    person: Entity,
    hall: Entity,
    site: Entity,
) {
    commands.entity(person).insert(PermitPickupRoutine { site });
    enqueue_moot_service(commands, clock, person, hall, MootServiceKind::Permit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_ticket_reports_the_service_instead_of_the_idle_animation() {
        let mut ticket = MootQueueTicket {
            hall: Entity::PLACEHOLDER,
            serial: 1,
            kind: MootServiceKind::Immigration,
            state: MootQueueState::Queued,
            failed_routes: 0,
            head_wait_seconds: 0.0,
            head_best_distance: f32::INFINITY,
        };
        assert_eq!(
            ticket.objective(),
            shared::components::CharacterObjective::QueuedForImmigration
        );
        ticket.state = MootQueueState::Serving { seconds_left: 1.0 };
        assert_eq!(
            ticket.objective(),
            shared::components::CharacterObjective::RegisteringImmigration
        );
    }

    fn hall() -> (Settlement, PlayerPosition, PlayerRotation) {
        (
            Settlement {
                name: "Queueford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
        )
    }

    #[test]
    fn queue_snakes_without_overlapping_and_stays_inside_forecourt() {
        let mut slots = Vec::new();
        for lane in [MootServiceLane::Immigration, MootServiceLane::Resident] {
            for index in 0..30 {
                let slot = local_queue_slot(lane, index);
                assert!(
                    slot.length() < 16.0,
                    "slot {lane:?}/{index} escaped: {slot:?}"
                );
                assert!(
                    slots.iter().all(|other: &Vec2| other.distance(slot) > 1.0),
                    "slot {lane:?}/{index} overlaps another place"
                );
                slots.push(slot);
            }
        }
    }

    #[test]
    fn rotated_queue_uses_the_authored_hall_frame() {
        let hall = Vec3::new(20.0, 3.0, -7.0);
        let yaw = std::f32::consts::FRAC_PI_2;
        let actual = world_slot(hall, yaw, MootServiceLane::Resident, 0, None);
        let expected = shared::rotation::local_to_world_xz(
            local_queue_slot(MootServiceLane::Resident, 0),
            yaw,
        );
        assert!((Vec2::new(actual.x - hall.x, actual.z - hall.z) - expected).length() < 1e-4);
    }

    fn hundred_shopper_queue_clear_seconds(warp: f32) -> f32 {
        use crate::player::hero::step_units;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<MootQueueClock>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, (advance_moot_service_queues, step_units).chain());
        app.world_mut()
            .spawn(shared::components::TimeWarp::clamped(warp));
        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            Vec3::new(0.0, terrain.get_height(0.0, 0.0), 0.0)
        };
        let hall_entity = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Queueford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 400,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        for rank in 0..100 {
            let position = world_slot(
                hall_position,
                0.0,
                MootServiceLane::Resident,
                rank,
                Some(app.world().resource::<WorldTerrain>()),
            );
            app.world_mut().spawn((
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(position),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(position),
                MootQueueTicket {
                    hall: hall_entity,
                    serial: rank as u64 + 1,
                    kind: MootServiceKind::HouseholdShopping,
                    state: MootQueueState::Queued,
                    failed_routes: 0,
                    head_wait_seconds: 0.0,
                    head_best_distance: f32::INFINITY,
                },
            ));
        }

        let real_step = 1.0 / 60.0;
        let max_world_seconds = 5.0 * 60.0;
        let max_steps = (max_world_seconds / (real_step * warp)) as usize;
        for step in 1..=max_steps {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(real_step));
            app.update();
            let ready: Vec<_> = {
                let world = app.world_mut();
                world
                    .query::<(Entity, &MootQueueTicket)>()
                    .iter(world)
                    .filter_map(|(person, ticket)| ticket.is_ready().then_some(person))
                    .collect()
            };
            for person in ready {
                app.world_mut()
                    .entity_mut(person)
                    .remove::<MootQueueTicket>();
            }
            let remaining = {
                let world = app.world_mut();
                world.query::<&MootQueueTicket>().iter(world).count()
            };
            if remaining == 0 {
                return step as f32 * real_step * warp;
            }
        }
        max_world_seconds
    }

    #[test]
    fn hundred_household_shoppers_clear_within_five_world_minutes_at_one_and_ten_x() {
        for warp in [1.0, 10.0] {
            let seconds = hundred_shopper_queue_clear_seconds(warp);
            assert!(
                seconds < 5.0 * 60.0,
                "100 household shoppers took {seconds:.1} world seconds at {warp}x"
            );
        }
    }

    #[test]
    fn only_the_fifo_head_is_served_and_the_next_person_advances() {
        let mut app = App::new();
        app.init_resource::<MootQueueClock>();
        app.add_systems(Update, advance_moot_service_queues);
        let hall_entity = app.world_mut().spawn(hall()).id();
        let first = app
            .world_mut()
            .spawn((
                PlayerPosition(world_slot(
                    Vec3::ZERO,
                    0.0,
                    MootServiceLane::Resident,
                    0,
                    None,
                )),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                MootQueueTicket {
                    hall: hall_entity,
                    serial: 1,
                    kind: MootServiceKind::Permit,
                    state: MootQueueState::Serving {
                        seconds_left: 0.001,
                    },
                    failed_routes: 0,
                    head_wait_seconds: 0.0,
                    head_best_distance: f32::INFINITY,
                },
            ))
            .id();
        let second = app
            .world_mut()
            .spawn((
                PlayerPosition(world_slot(
                    Vec3::ZERO,
                    0.0,
                    MootServiceLane::Resident,
                    1,
                    None,
                )),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                MootQueueTicket {
                    hall: hall_entity,
                    serial: 2,
                    kind: MootServiceKind::PoorRelief,
                    state: MootQueueState::Queued,
                    failed_routes: 0,
                    head_wait_seconds: 0.0,
                    head_best_distance: f32::INFINITY,
                },
            ))
            .id();

        app.update();
        assert!(app
            .world()
            .get::<MootQueueTicket>(first)
            .unwrap()
            .is_ready());
        assert_eq!(
            app.world().get::<MootQueueTicket>(second).unwrap().state,
            MootQueueState::Queued
        );

        app.world_mut()
            .entity_mut(first)
            .remove::<MootQueueTicket>();
        app.world_mut().get_mut::<PlayerPosition>(second).unwrap().0 =
            world_slot(Vec3::ZERO, 0.0, MootServiceLane::Resident, 0, None);
        app.update();
        assert!(matches!(
            app.world().get::<MootQueueTicket>(second).unwrap().state,
            MootQueueState::Serving { .. }
        ));
    }

    #[test]
    fn a_vacant_queue_place_ripples_backward_instead_of_moving_everyone_at_once() {
        let mut app = App::new();
        app.init_resource::<MootQueueClock>();
        app.add_systems(Update, advance_moot_service_queues);
        let hall_entity = app.world_mut().spawn(hall()).id();
        let mut people = Vec::new();

        // These are the three people left after the old head departed. They
        // still occupy ranks 1, 2 and 3 and should advance in a wave toward
        // their newly assigned ranks 0, 1 and 2.
        for old_rank in 1..=3 {
            people.push(
                app.world_mut()
                    .spawn((
                        PlayerPosition(world_slot(
                            Vec3::ZERO,
                            0.0,
                            MootServiceLane::Resident,
                            old_rank,
                            None,
                        )),
                        PlayerRotation(0.0),
                        CharacterActivity::Idle,
                        MootQueueTicket {
                            hall: hall_entity,
                            serial: old_rank as u64,
                            kind: MootServiceKind::Permit,
                            state: MootQueueState::Queued,
                            failed_routes: 0,
                            head_wait_seconds: 0.0,
                            head_best_distance: f32::INFINITY,
                        },
                    ))
                    .id(),
            );
        }

        app.update();
        assert!(app.world().get::<MoveTarget>(people[0]).is_some());
        assert!(app.world().get::<MoveTarget>(people[1]).is_none());
        assert!(app.world().get::<MoveTarget>(people[2]).is_none());

        app.world_mut()
            .get_mut::<PlayerPosition>(people[0])
            .unwrap()
            .0 = world_slot(Vec3::ZERO, 0.0, MootServiceLane::Resident, 0, None);
        app.update();

        assert!(app.world().get::<MoveTarget>(people[1]).is_some());
        assert!(
            app.world().get::<MoveTarget>(people[2]).is_none(),
            "the third person must wait until the second has cleared their place"
        );
    }

    #[test]
    fn immigration_and_resident_counters_serve_in_parallel() {
        let mut app = App::new();
        app.init_resource::<MootQueueClock>();
        app.add_systems(Update, advance_moot_service_queues);
        let hall_entity = app.world_mut().spawn(hall()).id();
        let mut heads = Vec::new();
        for (serial, lane, kind) in [
            (
                1,
                MootServiceLane::Immigration,
                MootServiceKind::Immigration,
            ),
            (2, MootServiceLane::Resident, MootServiceKind::Permit),
        ] {
            heads.push(
                app.world_mut()
                    .spawn((
                        PlayerPosition(world_slot(Vec3::ZERO, 0.0, lane, 0, None)),
                        PlayerRotation(0.0),
                        CharacterActivity::Idle,
                        MootQueueTicket {
                            hall: hall_entity,
                            serial,
                            kind,
                            state: MootQueueState::Serving {
                                seconds_left: 0.001,
                            },
                            failed_routes: 0,
                            head_wait_seconds: 0.0,
                            head_best_distance: f32::INFINITY,
                        },
                    ))
                    .id(),
            );
        }

        app.update();
        assert!(heads.iter().all(|person| app
            .world()
            .get::<MootQueueTicket>(*person)
            .is_some_and(|ticket| ticket.is_ready())));
    }

    #[test]
    fn a_pending_route_cannot_freeze_the_front_of_a_moot_queue() {
        let mut app = App::new();
        app.init_resource::<MootQueueClock>();
        app.add_systems(Update, advance_moot_service_queues);
        let hall_entity = app.world_mut().spawn(hall()).id();
        let target = world_slot(Vec3::ZERO, 0.0, MootServiceLane::Resident, 0, None);
        let person = app
            .world_mut()
            .spawn((
                PlayerPosition(target + Vec3::X * 10.0),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                MoveTarget(target),
                NavigationRoutePending::new(target),
                MootQueueTicket {
                    hall: hall_entity,
                    serial: 1,
                    kind: MootServiceKind::Permit,
                    state: MootQueueState::Queued,
                    failed_routes: 0,
                    head_wait_seconds: MAX_QUEUE_HEAD_WAIT_SECONDS,
                    head_best_distance: ground_distance(target + Vec3::X * 10.0, target),
                },
            ))
            .id();

        app.update();

        assert!(app
            .world()
            .get::<MootQueueTicket>(person)
            .is_some_and(|ticket| ticket.is_ready()));
        assert!(app.world().get::<MoveTarget>(person).is_none());
        assert!(app.world().get::<NavigationRoutePending>(person).is_none());
    }

    #[test]
    fn tactical_immigration_completes_only_after_the_visible_fifo_counter() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<MootQueueClock>();
        app.add_systems(
            Update,
            (
                arrive_at_settlement,
                advance_moot_service_queues,
                recount_residents,
            )
                .chain(),
        );
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hall_entity = app.world_mut().spawn(hall()).id();
        let immigrant = app
            .world_mut()
            .spawn((
                PlayerPosition(world_slot(
                    Vec3::ZERO,
                    0.0,
                    MootServiceLane::Immigration,
                    0,
                    None,
                )),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                VillagerIntent::Travelling {
                    settlement: hall_entity,
                },
            ))
            .id();

        app.update();
        let ticket = app.world().get::<MootQueueTicket>(immigrant).unwrap();
        assert_eq!(ticket.kind, MootServiceKind::Immigration);
        assert!(matches!(
            app.world().get::<VillagerIntent>(immigrant),
            Some(VillagerIntent::Travelling { .. })
        ));
        assert_eq!(
            app.world()
                .get::<Settlement>(hall_entity)
                .unwrap()
                .residents,
            0
        );

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(
                IMMIGRATION_SERVICE_SECONDS + 0.1,
            ));
        app.update();
        assert!(app
            .world()
            .get::<MootQueueTicket>(immigrant)
            .unwrap()
            .is_ready());
        assert_eq!(
            app.world()
                .get::<Settlement>(hall_entity)
                .unwrap()
                .residents,
            0
        );

        app.update();
        assert!(app.world().get::<MootQueueTicket>(immigrant).is_none());
        assert!(matches!(
            app.world().get::<VillagerIntent>(immigrant),
            Some(VillagerIntent::Resident { settlement }) if *settlement == hall_entity
        ));
        assert_eq!(
            app.world()
                .get::<Settlement>(hall_entity)
                .unwrap()
                .residents,
            1
        );
    }

    #[test]
    fn hundred_immigrants_keep_their_fifo_line_through_a_hall_upgrade() {
        use crate::collision::building_index::{sync_building_spatial_index, BuildingSpatialIndex};
        use crate::player::hero::step_units;
        use crate::world::navgrid::{sync_obstacle_grid, ObstacleGridState};
        use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
        use shared::components::{CivicHallLevel, TimeWarp};
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<MootQueueClock>();
        app.init_resource::<BuildingSpatialIndex>();
        app.init_resource::<ObstacleGridState>();
        app.init_resource::<SpatialObstacleGrid>();
        app.init_resource::<VillageRoadGraph>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                sync_building_spatial_index,
                sync_obstacle_grid,
                arrive_at_settlement,
                advance_moot_service_queues,
                step_units,
                recount_residents,
            )
                .chain(),
        );

        let hall_position = Vec3::new(1_700.0, 0.0, 0.0);
        let hall_position = Vec3::new(
            hall_position.x,
            app.world()
                .resource::<WorldTerrain>()
                .get_height(hall_position.x, hall_position.z),
            hall_position.z,
        );
        let hall_yaw = 0.43;
        let hall_entity = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Queueford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(hall_yaw),
                CivicHallLevel::Moot,
                PlacedBuilding {
                    building_type: BuildingType::MootHall,
                    rotation: hall_yaw,
                },
                BuildingPosition(hall_position),
            ))
            .id();
        app.world_mut().spawn(TimeWarp::clamped(10.0));

        for rank in 0..100 {
            let position = world_slot(
                hall_position,
                hall_yaw,
                MootServiceLane::Immigration,
                rank,
                Some(app.world().resource::<WorldTerrain>()),
            );
            app.world_mut().spawn((
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(position),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(position),
                VillagerIntent::Travelling {
                    settlement: hall_entity,
                },
                MootQueueTicket {
                    hall: hall_entity,
                    serial: rank as u64 + 1,
                    kind: MootServiceKind::Immigration,
                    state: MootQueueState::Queued,
                    failed_routes: 0,
                    head_wait_seconds: 0.0,
                    head_best_distance: f32::INFINITY,
                },
            ));
        }

        let tick = std::time::Duration::from_secs_f32(1.0 / 60.0);
        for step in 0..4_500 {
            if step == 1_200 {
                // The settlement entity, door and every queue ticket survive
                // an in-place civic upgrade. The larger shell grows backward;
                // rebuilding collision here catches any accidental shift into
                // the occupied forecourt.
                app.world_mut().entity_mut(hall_entity).insert((
                    CivicHallLevel::Town,
                    PlacedBuilding {
                        building_type: BuildingType::TownHall,
                        rotation: hall_yaw,
                    },
                ));
            }
            app.world_mut().resource_mut::<Time>().advance_by(tick);
            app.update();
        }

        let (residents, queue_tickets, pending, failed) = {
            let world = app.world_mut();
            (
                world.get::<Settlement>(hall_entity).unwrap().residents,
                world.query::<&MootQueueTicket>().iter(world).count(),
                world.query::<&NavigationRoutePending>().iter(world).count(),
                world.query::<&NavigationRouteFailed>().iter(world).count(),
            )
        };
        assert_eq!((residents, queue_tickets, pending, failed), (100, 0, 0, 0));
        assert_eq!(
            app.world()
                .resource::<MootQueueClock>()
                .peak_depth
                .get(&(hall_entity, MootServiceLane::Immigration)),
            Some(&100),
            "the simultaneous arrivals must form one visible 100-person FIFO line"
        );
    }

    #[test]
    fn collecting_a_permit_is_the_gate_to_material_work() {
        let mut app = App::new();
        app.add_systems(Update, complete_moot_permit_pickups);
        let hall_entity = app.world_mut().spawn(hall()).id();
        let site = app.world_mut().spawn_empty().id();
        let applicant = app
            .world_mut()
            .spawn((
                MootQueueTicket {
                    hall: hall_entity,
                    serial: 1,
                    kind: MootServiceKind::Permit,
                    state: MootQueueState::Ready,
                    failed_routes: 0,
                    head_wait_seconds: 0.0,
                    head_best_distance: f32::INFINITY,
                },
                PermitPickupRoutine { site },
            ))
            .id();
        assert!(app
            .world()
            .get::<ConstructionMaterialRoutine>(applicant)
            .is_none());
        app.update();
        let routine = app
            .world()
            .get::<ConstructionMaterialRoutine>(applicant)
            .unwrap();
        assert_eq!(routine.site, site);
        assert!(app.world().get::<MootQueueTicket>(applicant).is_none());
    }

    #[test]
    fn a_reserved_ration_becomes_visible_cargo_then_a_real_meal() {
        let mut app = App::new();
        app.add_systems(Update, run_moot_meal_collections);
        let hall_entity = app.world_mut().spawn(hall()).id();
        let person = app
            .world_mut()
            .spawn((
                PlayerPosition(world_slot(
                    Vec3::ZERO,
                    0.0,
                    MootServiceLane::Resident,
                    0,
                    None,
                )),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                GoodsInventory::new(8),
                Nutrition::default(),
                MootQueueTicket {
                    hall: hall_entity,
                    serial: 1,
                    kind: MootServiceKind::PoorRelief,
                    state: MootQueueState::Ready,
                    failed_routes: 0,
                    head_wait_seconds: 0.0,
                    head_best_distance: f32::INFINITY,
                },
                MootMealRoutine {
                    hall: hall_entity,
                    good: Good::Bread,
                    meal_day: 4,
                    phase: MootMealPhase::Queueing,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(person)
                .unwrap()
                .amount(Good::Bread),
            1
        );
        assert!(app.world().get::<MootQueueTicket>(person).is_none());
        let destination = match app.world().get::<MootMealRoutine>(person).unwrap().phase {
            MootMealPhase::Carrying { destination } => destination,
            phase => panic!("unexpected meal phase: {phase:?}"),
        };
        app.world_mut().get_mut::<PlayerPosition>(person).unwrap().0 = destination;
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(person)
                .unwrap()
                .amount(Good::Bread),
            0
        );
        assert_eq!(
            app.world().get::<Nutrition>(person).unwrap().last_meal_day,
            Some(4)
        );
        assert_eq!(
            *app.world().get::<CharacterActivity>(person).unwrap(),
            CharacterActivity::Sitting
        );
    }
}
