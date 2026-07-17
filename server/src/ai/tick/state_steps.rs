use super::*;
use crate::ai::pathfinding::{
    find_path_a_star_with_scratch, pick_random_target, PathfindingScratch,
};

#[inline]
fn consume_pathfinding_request(remaining: &mut usize) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

pub(super) fn tick_idle_state(
    wander: &mut NpcWander,
    rot: &mut NpcRotation,
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    current_pos: Vec3,
    dt: f32,
    npc_id: u64,
    pathfinding_scratch: &mut PathfindingScratch,
    pathfinding_time_ms: &mut f32,
    pathfinding_requests_remaining: &mut usize,
) {
    wander.idle_timer -= dt;

    let angle_diff = wander.idle_rotation_target - rot.0;
    let normalized_diff =
        ((angle_diff + std::f32::consts::PI) % (2.0 * std::f32::consts::PI)) - std::f32::consts::PI;

    if normalized_diff.abs() > 0.01 {
        let turn_amount = wander.idle_rotation_speed * dt;
        rot.0 += normalized_diff.signum() * turn_amount.min(normalized_diff.abs());
    }

    if wander.idle_timer <= 0.0 {
        let has_remaining_path = !wander.path.is_empty() && wander.waypoint < wander.path.len();

        if has_remaining_path {
            wander.state = NpcState::Walking;
            trace!(
                "NPC {} resuming walk after pause ({} waypoints remaining)",
                npc_id,
                wander.path.len() - wander.waypoint
            );
        } else {
            if !consume_pathfinding_request(pathfinding_requests_remaining) {
                wander.idle_timer = 0.1;
                return;
            }
            wander.target = pick_random_target(
                terrain,
                obstacles,
                wander.home,
                current_pos,
                NPC_WANDER_RADIUS,
                NPC_MIN_TARGET_DIST,
                &mut wander.rng,
            );
            let path_start = std::time::Instant::now();
            wander.path = find_path_a_star_with_scratch(
                terrain,
                obstacles,
                current_pos,
                wander.target,
                pathfinding_scratch,
            );
            *pathfinding_time_ms += path_start.elapsed().as_secs_f32() * 1000.0;
            wander.waypoint = 0;

            if wander.path.is_empty() {
                wander.path.push(wander.target);
            }

            while wander.waypoint < wander.path.len() {
                let wp = wander.path[wander.waypoint];
                let dist = Vec2::new(wp.x - current_pos.x, wp.z - current_pos.z).length();
                if dist < 1.0 {
                    wander.waypoint += 1;
                } else {
                    break;
                }
            }

            wander.current_speed_multiplier = 0.9 + wander.rng.next_f32() * 0.2;
            wander.state = NpcState::Walking;

            trace!(
                "NPC {} walking to {:?} (distance: {:.1}m, path {} waypoints, speed {:.2}x)",
                npc_id,
                wander.target,
                (wander.target - current_pos).length(),
                wander.path.len(),
                wander.current_speed_multiplier
            );
        }
    }
}

/// Smoothly rotate current angle toward target angle at a given speed.
fn smooth_rotate_toward(current: f32, target: f32, turn_speed: f32, dt: f32) -> f32 {
    use std::f32::consts::PI;

    let mut diff = target - current;
    while diff > PI {
        diff -= 2.0 * PI;
    }
    while diff < -PI {
        diff += 2.0 * PI;
    }

    let max_turn = turn_speed * dt;
    if diff.abs() <= max_turn {
        target
    } else {
        current + diff.signum() * max_turn
    }
}

/// Tick walking state: follow path, handle waypoints, add variety.
pub(super) fn tick_walking_state(
    wander: &mut NpcWander,
    pos: &mut NpcPosition,
    rot: &mut NpcRotation,
    terrain: &WorldTerrain,
    dt: f32,
    npc_id: u64,
    base_walk_speed: f32,
) {
    let path_complete = wander.path.is_empty() || wander.waypoint >= wander.path.len();

    if path_complete {
        wander.idle_timer =
            NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);
        wander.idle_rotation_target = wander.rng.next_f32() * std::f32::consts::TAU;
        wander.state = NpcState::Idle;
        trace!(
            "NPC {} reached destination, idling for {:.1}s",
            npc_id,
            wander.idle_timer
        );
        return;
    }

    let Some(waypoint) = wander.path.get(wander.waypoint).copied() else {
        return;
    };

    let to = waypoint - pos.0;
    let dist_xz = Vec2::new(to.x, to.z).length();

    if dist_xz < 0.6 {
        wander.waypoint += 1;

        if wander.waypoint >= wander.path.len() {
            wander.idle_timer =
                NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);
            wander.idle_rotation_target = wander.rng.next_f32() * std::f32::consts::TAU;
            wander.state = NpcState::Idle;
            trace!(
                "NPC {} reached destination, idling for {:.1}s",
                npc_id,
                wander.idle_timer
            );
            return;
        }

        if wander.rng.next_f32() < 0.2 {
            wander.idle_timer = 0.5 + wander.rng.next_f32() * 0.5;
            wander.idle_rotation_target = rot.0;
            wander.state = NpcState::Idle;
            trace!(
                "NPC {} pausing at waypoint for {:.1}s",
                npc_id,
                wander.idle_timer
            );
            return;
        }

        return;
    }

    let dir_xz = Vec2::new(to.x, to.z).normalize_or_zero();
    let target_yaw = (-dir_xz.x).atan2(-dir_xz.y);

    rot.0 = smooth_rotate_toward(rot.0, target_yaw, NPC_TURN_SPEED, dt);

    let facing_dir = Vec2::new(-rot.0.sin(), -rot.0.cos());
    let speed = base_walk_speed * wander.current_speed_multiplier;

    let alignment = dir_xz.dot(facing_dir);
    let move_factor = alignment.max(0.0);

    let step = Vec3::new(facing_dir.x, 0.0, facing_dir.y) * (speed * move_factor * dt);

    pos.0.x += step.x;
    pos.0.z += step.z;

    let ground_y = terrain.get_height(pos.0.x, pos.0.z);
    pos.0.y = ground_y + ground_clearance_center();
}

/// Tick fleeing state: run away from threat, decrease timer.
#[allow(clippy::too_many_arguments)]
pub(super) fn tick_fleeing_state(
    wander: &mut NpcWander,
    pos: &mut NpcPosition,
    rot: &mut NpcRotation,
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    from_position: Vec3,
    mut flee_timer: f32,
    panic_speed_boost: f32,
    dt: f32,
    npc_id: u64,
    pathfinding_scratch: &mut PathfindingScratch,
    pathfinding_time_ms: &mut f32,
    pathfinding_requests_remaining: &mut usize,
) {
    flee_timer -= dt;

    if flee_timer <= 0.0 {
        wander.idle_timer =
            NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);
        wander.idle_rotation_target = wander.rng.next_f32() * std::f32::consts::TAU;
        wander.state = NpcState::Idle;
        wander.path.clear();
        wander.waypoint = 0;
        trace!("NPC {} stopped fleeing, returning to idle", npc_id);
        return;
    }

    if wander.path.is_empty() || wander.waypoint >= wander.path.len() {
        if !consume_pathfinding_request(pathfinding_requests_remaining) {
            wander.state = NpcState::Fleeing {
                from_position,
                flee_timer,
                panic_speed_boost,
            };
            return;
        }
        let away_vec = pos.0 - from_position;
        let away_dir = Vec2::new(away_vec.x, away_vec.z).normalize_or_zero();

        let flee_distance = 20.0 + wander.rng.next_f32() * 10.0;
        let flee_target_xz = Vec2::new(pos.0.x, pos.0.z) + away_dir * flee_distance;
        let flee_y = terrain.get_height(flee_target_xz.x, flee_target_xz.y);
        let flee_target = Vec3::new(
            flee_target_xz.x,
            flee_y + ground_clearance_center(),
            flee_target_xz.y,
        );

        let path_start = std::time::Instant::now();
        wander.path = find_path_a_star_with_scratch(
            terrain,
            obstacles,
            pos.0,
            flee_target,
            pathfinding_scratch,
        );
        *pathfinding_time_ms += path_start.elapsed().as_secs_f32() * 1000.0;
        wander.waypoint = 0;

        if wander.path.is_empty() {
            let random_angle = wander.rng.next_f32() * std::f32::consts::TAU;
            let random_dir = Vec2::new(random_angle.cos(), random_angle.sin());
            let fallback_target_xz = Vec2::new(pos.0.x, pos.0.z) + random_dir * 15.0;
            let fallback_y = terrain.get_height(fallback_target_xz.x, fallback_target_xz.y);
            wander.path.push(Vec3::new(
                fallback_target_xz.x,
                fallback_y + ground_clearance_center(),
                fallback_target_xz.y,
            ));
        }

        while wander.waypoint < wander.path.len() {
            let wp = wander.path[wander.waypoint];
            let dist = Vec2::new(wp.x - pos.0.x, wp.z - pos.0.z).length();
            if dist < 1.0 {
                wander.waypoint += 1;
            } else {
                break;
            }
        }

        trace!(
            "NPC {} fleeing toward {:?}, timer {:.1}s remaining",
            npc_id,
            flee_target,
            flee_timer
        );
    }

    let Some(waypoint) = wander.path.get(wander.waypoint).copied() else {
        return;
    };

    let to = waypoint - pos.0;
    let dist_xz = Vec2::new(to.x, to.z).length();

    if dist_xz < 0.6 {
        wander.waypoint += 1;
        return;
    }

    let dir_xz = Vec2::new(to.x, to.z).normalize_or_zero();
    let target_yaw = (-dir_xz.x).atan2(-dir_xz.y);

    let panic_turn_speed = NPC_TURN_SPEED * 1.5;
    rot.0 = smooth_rotate_toward(rot.0, target_yaw, panic_turn_speed, dt);

    let facing_dir = Vec2::new(-rot.0.sin(), -rot.0.cos());
    let flee_speed = NPC_MOVE_SPEED * panic_speed_boost;

    let alignment = dir_xz.dot(facing_dir);
    let move_factor = alignment.max(0.0);

    let step = Vec3::new(facing_dir.x, 0.0, facing_dir.y) * (flee_speed * move_factor * dt);

    pos.0.x += step.x;
    pos.0.z += step.z;

    let ground_y = terrain.get_height(pos.0.x, pos.0.z);
    pos.0.y = ground_y + ground_clearance_center();

    wander.state = NpcState::Fleeing {
        from_position,
        flee_timer,
        panic_speed_boost,
    };
}

#[cfg(test)]
mod tests {
    use super::consume_pathfinding_request;

    #[test]
    fn pathfinding_budget_never_underflows() {
        let mut remaining = 2;

        assert!(consume_pathfinding_request(&mut remaining));
        assert!(consume_pathfinding_request(&mut remaining));
        assert!(!consume_pathfinding_request(&mut remaining));
        assert_eq!(remaining, 0);
    }
}
