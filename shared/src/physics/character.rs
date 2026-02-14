use bevy::prelude::*;

use crate::components::{PlayerGrounded, PlayerPosition, PlayerRotation, PlayerVelocity};
use crate::player::PLAYER_SPEED;
use crate::protocol::PlayerInput;
use crate::terrain::WorldTerrain;

use crate::physics::{
    ground_clearance_center, FLY_FAST_MULT, FLY_SPEED, GRAVITY, GROUND_SNAP_DISTANCE,
    JUMP_VELOCITY, MOVE_ACCEL, MOVE_BRAKE, SWIM_UP_SPEED, WATER_ACCEL_MULT, WATER_BRAKE_MULT,
    WATER_BUOYANCY, WATER_GRAVITY_SCALE, WATER_HORIZONTAL_DRAG, WATER_SPEED_MULT, WATER_SWIM_DEPTH,
    WATER_VERTICAL_DRAG,
};

/// Step the player character one fixed tick.
pub fn step_character(
    input: &PlayerInput,
    terrain: &WorldTerrain,
    position: &mut PlayerPosition,
    rotation: &mut PlayerRotation,
    velocity: &mut PlayerVelocity,
    grounded: &mut PlayerGrounded,
    dt: f32,
) -> bool {
    rotation.0 = input.yaw;

    if input.fly_mode {
        let forward = Vec3::new(-rotation.0.sin(), 0.0, -rotation.0.cos());
        let right = Vec3::new(rotation.0.cos(), 0.0, -rotation.0.sin());

        let mut move_dir = Vec3::ZERO;
        if input.forward {
            move_dir += forward;
        }
        if input.backward {
            move_dir -= forward;
        }
        if input.right {
            move_dir += right;
        }
        if input.left {
            move_dir -= right;
        }

        if move_dir.length_squared() > 0.0 {
            move_dir = move_dir.normalize();
        }

        let speed = if input.fly_fast {
            FLY_SPEED * FLY_FAST_MULT
        } else {
            FLY_SPEED
        };

        let mut vertical = 0.0;
        if input.jump {
            vertical += 1.0;
        }
        if input.fly_down {
            vertical -= 1.0;
        }

        let vel = Vec3::new(move_dir.x * speed, vertical * speed, move_dir.z * speed);
        velocity.0 = vel;
        position.0 += velocity.0 * dt;

        grounded.on_terrain = false;
        grounded.on_static = false;
        grounded.time_since_grounded = PlayerGrounded::COYOTE_TIME;
        return false;
    }

    let water_height = terrain.get_water_height(position.0.x, position.0.z);
    let water_depth = water_height.map(|h| h - position.0.y).unwrap_or(-1.0);
    let in_water = water_depth > WATER_SWIM_DEPTH;

    let forward = Vec3::new(-rotation.0.sin(), 0.0, -rotation.0.cos());
    let right = Vec3::new(rotation.0.cos(), 0.0, -rotation.0.sin());

    let mut move_dir = Vec3::ZERO;
    if input.forward {
        move_dir += forward;
    }
    if input.backward {
        move_dir -= forward;
    }
    if input.right {
        move_dir += right;
    }
    if input.left {
        move_dir -= right;
    }

    if move_dir.length_squared() > 0.0 {
        move_dir = move_dir.normalize();
    }

    let speed = if in_water {
        PLAYER_SPEED * WATER_SPEED_MULT
    } else {
        PLAYER_SPEED
    };
    let desired_horiz = move_dir * speed;
    let mut horiz = Vec3::new(velocity.0.x, 0.0, velocity.0.z);

    let delta = desired_horiz - horiz;
    let accel = if move_dir.length_squared() > 0.0 {
        if in_water {
            MOVE_ACCEL * WATER_ACCEL_MULT
        } else {
            MOVE_ACCEL
        }
    } else if in_water {
        MOVE_BRAKE * WATER_BRAKE_MULT
    } else {
        MOVE_BRAKE
    };
    let max_change = accel * dt;

    if delta.length_squared() > 0.0 {
        let delta_len = delta.length();
        if delta_len <= max_change {
            horiz = desired_horiz;
        } else {
            horiz += delta * (max_change / delta_len);
        }
    }

    velocity.0.x = horiz.x;
    velocity.0.z = horiz.z;

    let mut did_jump = false;
    if in_water {
        if input.jump {
            velocity.0.y = SWIM_UP_SPEED;
        }
    } else if input.jump && grounded.can_jump() && velocity.0.y < 1.0 {
        velocity.0.y = JUMP_VELOCITY;
        grounded.time_since_grounded = PlayerGrounded::COYOTE_TIME;
        did_jump = true;
    }

    if in_water {
        velocity.0.y += GRAVITY * WATER_GRAVITY_SCALE * dt;
        if water_depth > 0.0 {
            velocity.0.y += water_depth * WATER_BUOYANCY * dt;
        }
        let drag = (1.0 - WATER_VERTICAL_DRAG * dt).clamp(0.0, 1.0);
        velocity.0.y *= drag;

        let h_drag = (1.0 - WATER_HORIZONTAL_DRAG * dt).clamp(0.0, 1.0);
        velocity.0.x *= h_drag;
        velocity.0.z *= h_drag;
    } else {
        velocity.0.y += GRAVITY * dt;
    }

    position.0 += velocity.0 * dt;

    let ground_y = terrain.get_height(position.0.x, position.0.z);
    let target_y = ground_y + ground_clearance_center();

    let mut on_terrain_now = false;

    if position.0.y < target_y {
        position.0.y = target_y;
        if velocity.0.y < 0.0 {
            velocity.0.y = 0.0;
        }
        on_terrain_now = true;
    } else if velocity.0.y <= 0.0 && (position.0.y - target_y) < GROUND_SNAP_DISTANCE {
        position.0.y = target_y;
        velocity.0.y = 0.0;
        on_terrain_now = true;
    }

    grounded.on_terrain = on_terrain_now;

    if grounded.is_grounded() {
        grounded.time_since_grounded = 0.0;
    } else {
        grounded.time_since_grounded += dt;
    }

    did_jump
}
