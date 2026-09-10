//! Outdoor dining keeps reservation ownership in the existing authoritative visit.
use super::*;
use shared::building::tavern::{outdoor_seat, OUTDOOR_SEATS};

pub(super) fn reserve(mask: &mut u8) -> Option<u8> {
    let index = (0..OUTDOOR_SEATS).find(|index| *mask & (1 << index) == 0)?;
    *mask |= 1 << index;
    Some(index)
}

pub(super) fn restore_ground(
    commands: &mut Commands,
    visitor: Entity,
    position: Vec3,
    routine: &TavernVisitRoutine,
) {
    if routine.phase == TavernVisitPhase::OutdoorDining {
        commands.entity(visitor).insert(PlayerPosition(
            position - Vec3::Y * shared::building::tavern::layout().bench_height,
        ));
    }
}

/// Advance only the patio portion. Returns true when the guest has left their bench.
pub(super) fn advance(
    commands: &mut Commands,
    visitor: Entity,
    position: Vec3,
    activity: &mut CharacterActivity,
    routine: &mut TavernVisitRoutine,
    move_target: Option<&MoveTarget>,
    transit: Option<&WorkplaceDoorTransit>,
    origin: Vec3,
    yaw: f32,
    dt: f32,
    now: f64,
) -> bool {
    let Some(seat) = routine
        .outdoor_seat
        .and_then(|index| outdoor_seat(index, origin, yaw))
    else {
        return false;
    };
    let ground = seat.position - Vec3::Y * shared::building::tavern::layout().bench_height;
    match routine.phase {
        TavernVisitPhase::GoingOutside if transit.is_none() => {
            routine.phase = TavernVisitPhase::ToSeat;
            routine.progress_position = position;
            routine.progress_world_seconds = now;
            commands.entity(visitor).insert(MoveTarget(ground));
        }
        TavernVisitPhase::ToSeat | TavernVisitPhase::LeavingSeat => {
            let target = if routine.phase == TavernVisitPhase::ToSeat {
                ground
            } else {
                seat.approach
            };
            if ground_distance(position, target) <= DOOR_REACH {
                if routine.phase == TavernVisitPhase::LeavingSeat {
                    return true;
                }
                commands
                    .entity(visitor)
                    .insert((PlayerPosition(seat.position), PlayerRotation(seat.facing)))
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>();
                *activity = CharacterActivity::Sitting;
                routine.phase = TavernVisitPhase::OutdoorDining;
            } else {
                // Bounded walking even if the planner succeeds but the body is pinned.
                if ground_distance(position, routine.progress_position) > 0.5 {
                    routine.progress_position = position;
                    routine.progress_world_seconds = now;
                } else if now - routine.progress_world_seconds > TAVERN_NO_PROGRESS_SECONDS {
                    return true;
                }
                ensure_move_target(commands, visitor, move_target, target);
            }
        }
        TavernVisitPhase::OutdoorDining => {
            routine.dining_seconds -= dt;
            if routine.dining_seconds <= 0.0 {
                restore_ground(commands, visitor, position, routine);
                *activity = CharacterActivity::Idle;
                routine.phase = TavernVisitPhase::LeavingSeat;
                routine.progress_position = ground;
                routine.progress_world_seconds = now;
                commands.entity(visitor).insert(MoveTarget(seat.approach));
            }
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_are_unique_bounded_and_reusable() {
        let mut mask = 0;
        for index in 0..OUTDOOR_SEATS {
            assert_eq!(reserve(&mut mask), Some(index));
        }
        assert_eq!(reserve(&mut mask), None);
        mask &= !(1 << 3);
        assert_eq!(reserve(&mut mask), Some(3));
        assert_eq!(reserve(&mut mask), None);
    }
}
