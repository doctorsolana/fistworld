//! Dynamic actor setup and control for the authoritative Rapier world.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    AdditionalMassProperties, Ccd, Collider, CollisionGroups, Damping, GravityScale, LockedAxes,
    RigidBody, Sleeping, Velocity,
};

use shared::components::{
    DebugPhysicsBox, DebugPhysicsBoxPosition, DebugPhysicsBoxRotation, FlyMode, Health, Npc,
    NpcActivity, NpcActivityKind, NpcPosition, NpcRotation, NpcVelocity, Player, PlayerGrounded,
    PlayerJumpState, PlayerPosition, PlayerRotation, PlayerVelocity,
};
use shared::npc::{NPC_MOVE_SPEED, NPC_RADIUS};
use shared::physics::{
    FLY_FAST_MULT, FLY_SPEED, JUMP_VELOCITY, MOVE_ACCEL, MOVE_BRAKE, WALKABLE_THRESHOLD,
};
use shared::player::{JUMP_ANIM_MIN_SECS, PLAYER_HEIGHT, PLAYER_RADIUS, PLAYER_SPEED};
use shared::protocol::PlayerInput;
use shared::vehicle::{InVehicle, Vehicle, VehicleDriver, VehicleInput, VehicleState, VehicleType};

use crate::ai::ragdoll::NpcRagdoll;
use crate::net::input::ClientInputs;
use crate::net::peer::peer_id_to_u64;
use crate::physics::layers;
use crate::player::lifecycle::{is_player_alive, RespawnTimer};

const PLAYER_MASS_KG: f32 = 85.0;
const NPC_MASS_KG: f32 = 72.0;

#[derive(Component, Clone, Copy, Debug)]
pub struct PlayerPhysicsBody;

#[derive(Component, Clone, Copy, Debug)]
pub struct NpcPhysicsBody;

#[derive(Component, Clone, Copy, Debug)]
pub struct VehiclePhysicsBody;

fn bike_half_extents() -> Vec3 {
    Vec3::new(0.45, 0.45, 1.05)
}

fn car_half_extents() -> Vec3 {
    Vec3::new(0.95, 0.55, 2.15)
}

fn vehicle_half_extents(vehicle_type: VehicleType) -> Vec3 {
    match vehicle_type {
        VehicleType::Motorbike => bike_half_extents(),
        VehicleType::Car => car_half_extents(),
    }
}

fn vehicle_mass(vehicle_type: VehicleType) -> f32 {
    match vehicle_type {
        VehicleType::Motorbike => 240.0,
        VehicleType::Car => 1050.0,
    }
}

pub fn ensure_player_physics_bodies(
    mut commands: Commands,
    players: Query<
        (Entity, &PlayerPosition, &PlayerRotation),
        (With<Player>, Without<PlayerPhysicsBody>),
    >,
) {
    let capsule_half = (PLAYER_HEIGHT * 0.5 - PLAYER_RADIUS).max(0.01);

    for (entity, position, rotation) in players.iter() {
        let tf = Transform::from_translation(position.0)
            .with_rotation(Quat::from_rotation_y(rotation.0));
        commands.entity(entity).insert((
            PlayerPhysicsBody,
            tf,
            GlobalTransform::from(tf),
            RigidBody::Dynamic,
            Collider::capsule_y(capsule_half, PLAYER_RADIUS),
            layers::player_groups(),
            AdditionalMassProperties::Mass(PLAYER_MASS_KG),
            Damping {
                linear_damping: 3.5,
                angular_damping: 12.0,
            },
            LockedAxes::ROTATION_LOCKED_X | LockedAxes::ROTATION_LOCKED_Z,
            GravityScale(1.0),
            Velocity::default(),
            Sleeping::disabled(),
            Ccd::enabled(),
        ));
    }
}

pub fn ensure_npc_physics_bodies(
    mut commands: Commands,
    npcs: Query<
        (Entity, &Health, &NpcPosition, &NpcRotation),
        (With<Npc>, Without<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
) {
    let capsule_half = (PLAYER_HEIGHT * 0.5 - NPC_RADIUS).max(0.01);

    for (entity, health, position, rotation) in npcs.iter() {
        if health.is_dead() {
            continue;
        }

        let tf = Transform::from_translation(position.0)
            .with_rotation(Quat::from_rotation_y(rotation.0));
        commands.entity(entity).insert((
            NpcPhysicsBody,
            tf,
            GlobalTransform::from(tf),
            RigidBody::Dynamic,
            Collider::capsule_y(capsule_half, NPC_RADIUS),
            layers::npc_groups(),
            AdditionalMassProperties::Mass(NPC_MASS_KG),
            Damping {
                linear_damping: 5.0,
                angular_damping: 16.0,
            },
            LockedAxes::ROTATION_LOCKED_X | LockedAxes::ROTATION_LOCKED_Z,
            GravityScale(1.0),
            Velocity::default(),
            Sleeping::disabled(),
            Ccd::enabled(),
        ));

        commands.entity(entity).insert(NpcVelocity(Vec3::ZERO));
    }
}

pub fn cleanup_npc_physics_when_ragdoll_activates(
    mut commands: Commands,
    ragdolled: Query<Entity, (With<Npc>, With<NpcRagdoll>, With<NpcPhysicsBody>)>,
) {
    for entity in ragdolled.iter() {
        commands.entity(entity).remove::<(
            NpcPhysicsBody,
            RigidBody,
            Collider,
            CollisionGroups,
            AdditionalMassProperties,
            Damping,
            LockedAxes,
            GravityScale,
            Velocity,
            Sleeping,
            Ccd,
        )>();
    }
}

pub fn ensure_vehicle_physics_bodies(
    mut commands: Commands,
    vehicles: Query<
        (Entity, &Vehicle, &VehicleState),
        (With<Vehicle>, Without<VehiclePhysicsBody>),
    >,
) {
    for (entity, vehicle, state) in vehicles.iter() {
        let half_extents = vehicle_half_extents(vehicle.vehicle_type);
        let tf = Transform::from_translation(state.position).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            state.heading,
            state.pitch,
            state.roll,
        ));

        commands.entity(entity).insert((
            VehiclePhysicsBody,
            tf,
            GlobalTransform::from(tf),
            RigidBody::Dynamic,
            Collider::cuboid(half_extents.x, half_extents.y, half_extents.z),
            layers::vehicle_groups(),
            AdditionalMassProperties::Mass(vehicle_mass(vehicle.vehicle_type)),
            Damping {
                linear_damping: 0.6,
                angular_damping: 1.3,
            },
            GravityScale(1.0),
            Velocity::default(),
            Ccd::enabled(),
        ));
    }
}

pub fn sync_player_bodies_from_authoritative_state(
    mut players: Query<
        (
            &PlayerPosition,
            &PlayerRotation,
            &mut Transform,
            &mut Velocity,
            &mut GravityScale,
            Option<&InVehicle>,
        ),
        (With<Player>, With<PlayerPhysicsBody>),
    >,
    vehicles: Query<&VehicleState>,
) {
    for (position, rotation, mut transform, mut velocity, mut gravity_scale, in_vehicle) in
        players.iter_mut()
    {
        if let Some(in_vehicle) = in_vehicle {
            if let Ok(vehicle_state) = vehicles.get(in_vehicle.vehicle_entity) {
                transform.translation = vehicle_state.position + Vec3::new(0.0, 1.3, 0.0);
                transform.rotation = Quat::from_rotation_y(vehicle_state.heading);
                velocity.linvel = Vec3::ZERO;
                velocity.angvel = Vec3::ZERO;
                gravity_scale.0 = 0.0;
                continue;
            }
        }

        gravity_scale.0 = 1.0;

        if !transform.translation.is_finite() || transform.translation.distance(position.0) > 3.0 {
            transform.translation = position.0;
            velocity.linvel = Vec3::ZERO;
            velocity.angvel = Vec3::ZERO;
        }
        transform.rotation = Quat::from_rotation_y(rotation.0);
    }
}

pub fn apply_player_controls(
    mut commands: Commands,
    time: Res<Time>,
    inputs: Res<ClientInputs>,
    mut players: Query<
        (
            Entity,
            &Player,
            &Health,
            &mut PlayerRotation,
            &mut PlayerGrounded,
            &mut Velocity,
            &mut Transform,
            &mut GravityScale,
            Option<&RespawnTimer>,
            Option<&InVehicle>,
            Option<&FlyMode>,
        ),
        (With<Player>, With<PlayerPhysicsBody>),
    >,
) {
    let dt = time.delta_secs().max(1.0 / 240.0);
    let default_input = PlayerInput::default();

    for (
        entity,
        player,
        health,
        mut rotation,
        mut grounded,
        mut velocity,
        mut transform,
        mut gravity_scale,
        respawn_timer,
        in_vehicle,
        fly_mode,
    ) in players.iter_mut()
    {
        if !is_player_alive(health, respawn_timer) {
            velocity.linvel = Vec3::ZERO;
            velocity.angvel = Vec3::ZERO;
            continue;
        }

        if in_vehicle.is_some() {
            velocity.linvel = Vec3::ZERO;
            velocity.angvel = Vec3::ZERO;
            continue;
        }

        let input = inputs
            .latest
            .get(&player.client_id)
            .unwrap_or(&default_input);

        rotation.0 = input.yaw;
        transform.rotation = Quat::from_rotation_y(input.yaw);

        if input.fly_mode {
            if fly_mode.is_none() {
                commands.entity(entity).insert(FlyMode);
            }
            gravity_scale.0 = 0.0;

            let forward = Vec3::new(-rotation.0.sin(), 0.0, -rotation.0.cos());
            let right = Vec3::new(rotation.0.cos(), 0.0, -rotation.0.sin());
            let mut planar = Vec3::ZERO;
            if input.forward {
                planar += forward;
            }
            if input.backward {
                planar -= forward;
            }
            if input.right {
                planar += right;
            }
            if input.left {
                planar -= right;
            }
            if planar.length_squared() > 0.0 {
                planar = planar.normalize();
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
            velocity.linvel = Vec3::new(planar.x * speed, vertical * speed, planar.z * speed);
            grounded.on_terrain = false;
            grounded.on_static = false;
            grounded.time_since_grounded = PlayerGrounded::COYOTE_TIME;
            continue;
        }

        if fly_mode.is_some() {
            commands.entity(entity).remove::<FlyMode>();
        }
        gravity_scale.0 = 1.0;

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

        let desired_horiz = move_dir * PLAYER_SPEED;
        let mut horiz = Vec3::new(velocity.linvel.x, 0.0, velocity.linvel.z);
        let delta = desired_horiz - horiz;
        let mut accel = if move_dir.length_squared() > 0.0 {
            MOVE_ACCEL
        } else {
            MOVE_BRAKE
        };
        if !grounded.is_grounded() {
            accel *= 0.35;
        }
        let max_change = accel * dt;
        if delta.length_squared() > 0.0 {
            let d = delta.length();
            if d <= max_change {
                horiz = desired_horiz;
            } else {
                horiz += delta * (max_change / d);
            }
        }
        velocity.linvel.x = horiz.x;
        velocity.linvel.z = horiz.z;

        if input.jump && grounded.can_jump() && velocity.linvel.y <= 1.0 {
            velocity.linvel.y = JUMP_VELOCITY;
            commands.entity(entity).insert(PlayerJumpState {
                timer: JUMP_ANIM_MIN_SECS,
            });
        }

        if velocity.linvel.y.abs() < 0.01 && grounded.is_grounded() {
            velocity.linvel.y = velocity.linvel.y.min(0.0);
        }

        // Keep players from gradually tipping due collision impulses.
        if velocity.angvel.x.abs() > WALKABLE_THRESHOLD
            || velocity.angvel.z.abs() > WALKABLE_THRESHOLD
        {
            velocity.angvel.x = 0.0;
            velocity.angvel.z = 0.0;
        }
    }
}

pub fn tick_player_jump_timers(
    mut commands: Commands,
    time: Res<Time>,
    mut players: Query<(Entity, &mut PlayerJumpState), With<Player>>,
) {
    let dt = time.delta_secs();
    for (entity, mut jump_state) in players.iter_mut() {
        jump_state.timer = (jump_state.timer - dt).max(0.0);
        if jump_state.timer <= 0.0 {
            commands.entity(entity).remove::<PlayerJumpState>();
        }
    }
}

pub fn sync_players_from_physics(
    mut players: Query<
        (
            &Transform,
            &Velocity,
            &mut PlayerPosition,
            &mut PlayerVelocity,
            &mut PlayerRotation,
        ),
        (With<Player>, With<PlayerPhysicsBody>),
    >,
) {
    for (transform, velocity, mut position, mut player_velocity, mut rotation) in players.iter_mut()
    {
        position.0 = transform.translation;
        player_velocity.0 = velocity.linvel;
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rotation.0 = yaw;
    }
}

pub fn sync_npcs_from_physics_before_ai(
    mut npcs: Query<
        (
            &Transform,
            &mut NpcPosition,
            &mut NpcRotation,
            &mut NpcVelocity,
        ),
        (With<Npc>, With<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
) {
    for (transform, mut position, mut rotation, mut velocity) in npcs.iter_mut() {
        position.0 = transform.translation;
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rotation.0 = yaw;
        velocity.0 = Vec3::ZERO;
    }
}

pub fn apply_npc_controls_from_ai(
    time: Res<Time>,
    mut npcs: Query<
        (
            &Health,
            &NpcActivity,
            &NpcPosition,
            &NpcRotation,
            &mut NpcVelocity,
            &mut Transform,
            &mut Velocity,
        ),
        (With<Npc>, With<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
) {
    let dt = time.delta_secs().max(1.0 / 240.0);

    for (
        health,
        activity,
        target_position,
        target_rotation,
        mut npc_velocity,
        mut transform,
        mut velocity,
    ) in npcs.iter_mut()
    {
        if health.is_dead() || activity.0 == NpcActivityKind::Dead {
            velocity.linvel = Vec3::ZERO;
            velocity.angvel = Vec3::ZERO;
            npc_velocity.0 = Vec3::ZERO;
            continue;
        }

        transform.rotation = Quat::from_rotation_y(target_rotation.0);

        let delta = target_position.0 - transform.translation;
        let mut desired = delta / dt;

        let speed_cap = match activity.0 {
            NpcActivityKind::Idle
            | NpcActivityKind::Sit
            | NpcActivityKind::Talk
            | NpcActivityKind::Work
            | NpcActivityKind::Sleep => 0.0,
            NpcActivityKind::Walk => NPC_MOVE_SPEED,
            NpcActivityKind::Run | NpcActivityKind::Flee => NPC_MOVE_SPEED * 1.8,
            NpcActivityKind::Dead => 0.0,
        };

        desired.y = velocity.linvel.y;

        let horiz = Vec3::new(desired.x, 0.0, desired.z);
        let horiz_len = horiz.length();
        let horiz_limited = if horiz_len > speed_cap && speed_cap > 0.0 {
            horiz / horiz_len * speed_cap
        } else {
            horiz
        };

        let blend = if speed_cap > 0.0 { 0.55 } else { 0.30 };
        velocity.linvel.x = velocity.linvel.x + (horiz_limited.x - velocity.linvel.x) * blend;
        velocity.linvel.z = velocity.linvel.z + (horiz_limited.z - velocity.linvel.z) * blend;

        npc_velocity.0 = Vec3::new(velocity.linvel.x, 0.0, velocity.linvel.z);
    }
}

pub fn sync_npcs_from_physics_after_writeback(
    mut npcs: Query<
        (
            &Transform,
            &Velocity,
            &mut NpcPosition,
            &mut NpcRotation,
            &mut NpcVelocity,
        ),
        (With<Npc>, With<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
) {
    for (transform, velocity, mut position, mut rotation, mut npc_velocity) in npcs.iter_mut() {
        position.0 = transform.translation;
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rotation.0 = yaw;
        npc_velocity.0 = Vec3::new(velocity.linvel.x, 0.0, velocity.linvel.z);
    }
}

pub fn apply_vehicle_controls(
    time: Res<Time>,
    inputs: Res<ClientInputs>,
    players: Query<&Player>,
    mut vehicles: Query<
        (&Vehicle, &VehicleDriver, &mut Transform, &mut Velocity),
        (With<Vehicle>, With<VehiclePhysicsBody>),
    >,
) {
    let dt = time.delta_secs().max(1.0 / 240.0);

    let mut peer_by_driver_id = std::collections::HashMap::new();
    for player in players.iter() {
        peer_by_driver_id.insert(peer_id_to_u64(player.client_id), player.client_id);
    }

    for (vehicle, driver, transform, mut velocity) in vehicles.iter_mut() {
        let input = driver
            .driver_id
            .and_then(|driver_id| peer_by_driver_id.get(&driver_id).copied())
            .and_then(|peer_id| inputs.latest.get(&peer_id))
            .and_then(|input| input.vehicle_input.clone())
            .unwrap_or_else(VehicleInput::default);

        let forward = transform.rotation * Vec3::NEG_Z;
        let right = transform.rotation * Vec3::X;

        let forward_speed = velocity.linvel.dot(forward);
        let side_speed = velocity.linvel.dot(right);

        let (max_speed, accel, brake, steer_rate) = match vehicle.vehicle_type {
            VehicleType::Motorbike => (26.0, 22.0, 28.0, 2.1),
            VehicleType::Car => (32.0, 18.0, 24.0, 1.5),
        };

        let mut target_forward_speed = forward_speed + input.throttle * accel * dt;
        let brake_sign = if forward_speed.abs() > 0.2 {
            forward_speed.signum()
        } else {
            0.0
        };
        target_forward_speed -= input.brake * brake * brake_sign * dt;

        if driver.driver_id.is_none() {
            target_forward_speed *= 0.98;
        }

        target_forward_speed = target_forward_speed.clamp(-max_speed * 0.35, max_speed);

        let side_damp = (0.80f32).powf(dt * 60.0);
        let target_side_speed = side_speed * side_damp;

        let mut new_linvel = velocity.linvel;
        new_linvel += forward * (target_forward_speed - forward_speed);
        new_linvel += right * (target_side_speed - side_speed);
        velocity.linvel = new_linvel;

        let speed_ratio = (target_forward_speed.abs() / max_speed).clamp(0.0, 1.0);
        let target_yaw_rate = -input.steer * steer_rate * (0.25 + speed_ratio * 0.75);
        velocity.angvel.y += (target_yaw_rate - velocity.angvel.y) * 0.18;
        velocity.angvel.x *= 0.85;
        velocity.angvel.z *= 0.85;
    }
}

pub fn sync_vehicles_from_physics(
    mut vehicles: Query<
        (&Transform, &Velocity, &mut VehicleState),
        (With<Vehicle>, With<VehiclePhysicsBody>),
    >,
) {
    for (transform, velocity, mut state) in vehicles.iter_mut() {
        state.position = transform.translation;
        state.velocity = velocity.linvel;

        let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
        state.heading = yaw;
        state.pitch = pitch;
        state.roll = roll;
        state.angular_velocity_yaw = velocity.angvel.y;
        state.angular_velocity_pitch = velocity.angvel.x;
        state.angular_velocity_roll = velocity.angvel.z;
    }
}

pub fn sync_debug_boxes_from_physics(
    mut boxes: Query<
        (
            &Transform,
            &mut DebugPhysicsBoxPosition,
            &mut DebugPhysicsBoxRotation,
        ),
        With<DebugPhysicsBox>,
    >,
) {
    for (transform, mut pos, mut rot) in boxes.iter_mut() {
        pos.0 = transform.translation;
        rot.0 = transform.rotation;
    }
}
