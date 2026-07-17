//! Dynamic actor setup and control for the authoritative Rapier world.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    AdditionalMassProperties, Ccd, Collider, CollisionGroups, Damping, GravityScale, LockedAxes,
    RigidBody, Sleeping, Velocity,
};

use shared::components::{
    DebugPhysicsBox, DebugPhysicsBoxPosition, DebugPhysicsBoxRotation, FlyMode, Health, Npc,
    NpcActivity, NpcActivityKind, NpcPosition, NpcRotation, NpcVelocity, Player, PlayerGrounded,
    PlayerJumpState, PlayerPosition, PlayerRotation, PlayerVelocity, PlayerWaterState, WorldTime,
};
use shared::npc::{NPC_MOVE_SPEED, NPC_RADIUS};
use shared::physics::{
    FLY_FAST_MULT, FLY_SPEED, JUMP_VELOCITY, MOVE_ACCEL, MOVE_BRAKE, SWIM_DOWN_SPEED,
    SWIM_UP_SPEED, WALKABLE_THRESHOLD, WATER_ACCEL_MULT, WATER_BRAKE_MULT, WATER_BUOYANCY,
    WATER_FLOAT_DEPTH, WATER_HORIZONTAL_DRAG, WATER_MAX_DESCENT_SPEED, WATER_SPEED_MULT,
    WATER_SWIM_DEPTH, WATER_SWIM_EXIT_DEPTH, WATER_VERTICAL_DRAG,
};
use shared::player::{
    JUMP_ANIM_MIN_SECS, PLAYER_HEIGHT, PLAYER_RADIUS, PLAYER_SPEED, PLAYER_SPRINT_MULT,
};
use shared::protocol::PlayerInput;
use shared::terrain::WorldTerrain;
use shared::vehicle::{InVehicle, VehicleState};
use shared::water::{water_swell_height, OCEAN_LOOP_SECONDS, WATER_SURFACE_OFFSET};

use crate::ai::ragdoll::NpcRagdoll;
use crate::net::input::ClientInputs;
use crate::physics::layers;
use crate::player::lifecycle::{is_player_alive, RespawnTimer};

const PLAYER_MASS_KG: f32 = 85.0;
const NPC_MASS_KG: f32 = 72.0;
const PLAYER_LINEAR_DAMPING: f32 = 0.65;
const AIR_CONTROL_ACCEL_MULT: f32 = 0.55;
const JUMP_BUFFER_SECS: f32 = 0.12;
const EARLY_RELEASE_GRAVITY_MULT: f32 = 2.25;
const FALL_GRAVITY_MULT: f32 = 2.65;
const MAX_BUOYANCY_ACCEL: f32 = 45.0;
const DEFAULT_NPC_PHYSICS_ACTIVATE_RADIUS: f32 = 180.0;
const DEFAULT_NPC_PHYSICS_DEACTIVATE_RADIUS: f32 = 220.0;

#[derive(Component, Clone, Copy, Debug)]
pub struct PlayerPhysicsBody;

#[derive(Component, Clone, Copy, Debug)]
pub struct NpcPhysicsBody;

#[derive(Resource, Clone, Debug)]
pub struct NpcPhysicsLodSettings {
    pub activate_radius: f32,
    pub deactivate_radius: f32,
}

#[inline]
fn is_within_any_player_radius(position: Vec3, player_positions: &[Vec3], radius_sq: f32) -> bool {
    player_positions
        .iter()
        .any(|player_pos| player_pos.distance_squared(position) <= radius_sq)
}

impl Default for NpcPhysicsLodSettings {
    fn default() -> Self {
        let activate_radius = std::env::var("CITYSIM_NPC_PHYSICS_RADIUS")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(DEFAULT_NPC_PHYSICS_ACTIVATE_RADIUS)
            .clamp(32.0, 1000.0);
        let deactivate_radius = std::env::var("CITYSIM_NPC_PHYSICS_EXIT_RADIUS")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(DEFAULT_NPC_PHYSICS_DEACTIVATE_RADIUS)
            .clamp(activate_radius, 1200.0);
        Self {
            activate_radius,
            deactivate_radius,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PlayerJumpControllerState {
    jump_buffer_timer: f32,
    jump_was_pressed: bool,
}

impl PlayerJumpControllerState {
    fn reset(&mut self) {
        self.jump_buffer_timer = 0.0;
        self.jump_was_pressed = false;
    }
}

#[inline]
fn update_jump_buffer(state: &mut PlayerJumpControllerState, jump_pressed: bool, dt: f32) {
    if jump_pressed && !state.jump_was_pressed {
        state.jump_buffer_timer = JUMP_BUFFER_SECS;
    } else {
        state.jump_buffer_timer = (state.jump_buffer_timer - dt).max(0.0);
    }
    state.jump_was_pressed = jump_pressed;
}

#[inline]
fn desired_gravity_scale(jump_pressed: bool, vertical_speed: f32, grounded: bool) -> f32 {
    if grounded {
        1.0
    } else if vertical_speed > 0.0 && !jump_pressed {
        EARLY_RELEASE_GRAVITY_MULT
    } else if vertical_speed < -0.5 {
        FALL_GRAVITY_MULT
    } else {
        1.0
    }
}

#[inline]
fn try_consume_buffered_jump(
    state: &mut PlayerJumpControllerState,
    grounded: &mut PlayerGrounded,
    velocity: &mut Velocity,
) -> bool {
    if state.jump_buffer_timer <= 0.0 || !grounded.can_jump() || velocity.linvel.y > 1.0 {
        return false;
    }

    velocity.linvel.y = JUMP_VELOCITY;
    grounded.on_terrain = false;
    grounded.on_static = false;
    grounded.time_since_grounded = PlayerGrounded::COYOTE_TIME;
    state.jump_buffer_timer = 0.0;
    true
}

#[derive(Clone, Copy, Debug)]
struct PlayerWaterSample {
    surface_y: f32,
    depth: f32,
}

#[inline]
fn should_swim_at_depth(depth: f32, was_swimming: bool) -> bool {
    let threshold = if was_swimming {
        WATER_SWIM_EXIT_DEPTH
    } else {
        WATER_SWIM_DEPTH
    };
    depth >= threshold
}

fn sample_player_water(
    terrain: &WorldTerrain,
    position: Vec3,
    was_swimming: bool,
    ocean_seconds: f32,
) -> Option<PlayerWaterSample> {
    let base_surface_y = terrain.get_water_height(position.x, position.z)?;
    let terrain_depth = (base_surface_y - terrain.get_height(position.x, position.z)).max(0.0);
    let surface_y = base_surface_y
        + WATER_SURFACE_OFFSET
        + water_swell_height(position.x, position.z, terrain_depth, ocean_seconds);
    let depth = surface_y - position.y;
    should_swim_at_depth(depth, was_swimming).then_some(PlayerWaterSample { surface_y, depth })
}

fn apply_swimming_vertical_control(
    velocity: &mut Vec3,
    depth: f32,
    swim_up: bool,
    swim_down: bool,
    dt: f32,
) {
    let buoyancy = ((depth - WATER_FLOAT_DEPTH) * WATER_BUOYANCY)
        .clamp(-MAX_BUOYANCY_ACCEL, MAX_BUOYANCY_ACCEL);
    let vertical_drag = -velocity.y * WATER_VERTICAL_DRAG;
    velocity.y += (buoyancy + vertical_drag) * dt;

    if swim_up != swim_down {
        velocity.y = if swim_up {
            SWIM_UP_SPEED
        } else {
            -SWIM_DOWN_SPEED
        };
    }
    velocity.y = velocity.y.clamp(-WATER_MAX_DESCENT_SPEED, SWIM_UP_SPEED);
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
            PlayerJumpControllerState::default(),
            tf,
            GlobalTransform::from(tf),
            RigidBody::Dynamic,
            Collider::capsule_y(capsule_half, PLAYER_RADIUS),
            layers::player_groups(),
            AdditionalMassProperties::Mass(PLAYER_MASS_KG),
            Damping {
                linear_damping: PLAYER_LINEAR_DAMPING,
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
    settings: Res<NpcPhysicsLodSettings>,
    players: Query<(&PlayerPosition, &Health, Option<&RespawnTimer>), With<Player>>,
    inactive_npcs: Query<
        (Entity, &Health, &NpcPosition, &NpcRotation),
        (With<Npc>, Without<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
    active_npcs: Query<
        (Entity, &NpcPosition),
        (With<Npc>, With<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
) {
    let capsule_half = (PLAYER_HEIGHT * 0.5 - NPC_RADIUS).max(0.01);
    let player_positions: Vec<Vec3> = players
        .iter()
        .filter_map(|(position, health, respawn)| {
            is_player_alive(health, respawn).then_some(position.0)
        })
        .collect();
    let activate_radius_sq = settings.activate_radius * settings.activate_radius;
    let deactivate_radius_sq = settings.deactivate_radius * settings.deactivate_radius;

    for (entity, health, position, rotation) in inactive_npcs.iter() {
        if health.is_dead() {
            continue;
        }
        let should_activate =
            is_within_any_player_radius(position.0, &player_positions, activate_radius_sq);
        if !should_activate {
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
            Sleeping::default(),
            Ccd::disabled(),
        ));

        commands.entity(entity).insert(NpcVelocity(Vec3::ZERO));
    }

    for (entity, position) in active_npcs.iter() {
        let should_deactivate =
            !is_within_any_player_radius(position.0, &player_positions, deactivate_radius_sq);
        if !should_deactivate {
            continue;
        }
        commands.entity(entity).remove::<(
            NpcPhysicsBody,
            Transform,
            GlobalTransform,
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
            Transform,
            GlobalTransform,
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

pub fn sync_player_bodies_from_authoritative_state(
    mut players: Query<
        (
            &PlayerPosition,
            &PlayerRotation,
            &mut Transform,
            &mut Velocity,
            &mut GravityScale,
            &mut CollisionGroups,
            Option<&InVehicle>,
        ),
        (With<Player>, With<PlayerPhysicsBody>),
    >,
    vehicles: Query<&VehicleState>,
) {
    for (
        position,
        rotation,
        mut transform,
        mut velocity,
        mut gravity_scale,
        mut collision_groups,
        in_vehicle,
    ) in players.iter_mut()
    {
        if let Some(in_vehicle) = in_vehicle {
            if let Ok(vehicle_state) = vehicles.get(in_vehicle.vehicle_entity) {
                transform.translation = vehicle_state.position + Vec3::new(0.0, 1.3, 0.0);
                transform.rotation = Quat::from_rotation_y(vehicle_state.heading);
                velocity.linvel = Vec3::ZERO;
                velocity.angvel = Vec3::ZERO;
                gravity_scale.0 = 0.0;
                *collision_groups = layers::player_seated_groups();
                continue;
            }
        }

        gravity_scale.0 = 1.0;
        *collision_groups = layers::player_groups();

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
    terrain: Res<WorldTerrain>,
    world_time: Query<&WorldTime>,
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
            &mut PlayerJumpControllerState,
            Option<&RespawnTimer>,
            Option<&InVehicle>,
            Option<&FlyMode>,
            Option<&mut PlayerWaterState>,
        ),
        (With<Player>, With<PlayerPhysicsBody>),
    >,
) {
    let dt = time.delta_secs().max(1.0 / 240.0);
    let ocean_seconds = world_time
        .single()
        .map(|world_time| world_time.ocean_seconds)
        .unwrap_or_else(|_| time.elapsed_secs().rem_euclid(OCEAN_LOOP_SECONDS));
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
        mut jump_controller,
        respawn_timer,
        in_vehicle,
        fly_mode,
        mut water_state,
    ) in players.iter_mut()
    {
        if !is_player_alive(health, respawn_timer) {
            jump_controller.reset();
            velocity.linvel = Vec3::ZERO;
            velocity.angvel = Vec3::ZERO;
            gravity_scale.0 = 1.0;
            if water_state.is_some() {
                commands.entity(entity).remove::<PlayerWaterState>();
            }
            continue;
        }

        if in_vehicle.is_some() {
            jump_controller.reset();
            velocity.linvel = Vec3::ZERO;
            velocity.angvel = Vec3::ZERO;
            if water_state.is_some() {
                commands.entity(entity).remove::<PlayerWaterState>();
            }
            continue;
        }

        let input = inputs
            .latest
            .get(&player.client_id)
            .unwrap_or(&default_input);

        rotation.0 = input.yaw;
        transform.rotation = Quat::from_rotation_y(input.yaw);

        if input.fly_mode {
            jump_controller.reset();
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
            if water_state.is_some() {
                commands.entity(entity).remove::<PlayerWaterState>();
            }
            continue;
        }

        if fly_mode.is_some() {
            commands.entity(entity).remove::<FlyMode>();
        }

        let was_swimming = water_state
            .as_ref()
            .map(|state| state.in_water)
            .unwrap_or(false);
        let water_sample =
            sample_player_water(&terrain, transform.translation, was_swimming, ocean_seconds);
        if let Some(sample) = water_sample {
            if let Some(state) = water_state.as_deref_mut() {
                state.in_water = true;
                state.surface_y = sample.surface_y;
                state.depth = sample.depth;
            } else {
                commands.entity(entity).insert(PlayerWaterState {
                    in_water: true,
                    surface_y: sample.surface_y,
                    depth: sample.depth,
                });
            }
        } else if water_state.is_some() {
            commands.entity(entity).remove::<PlayerWaterState>();
        }
        let swimming = water_sample.is_some();

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

        let sprinting = !swimming
            && input.fly_fast
            && input.forward
            && !input.backward
            && move_dir.length_squared() > 0.0;
        let target_speed = if swimming {
            PLAYER_SPEED * WATER_SPEED_MULT
        } else if sprinting {
            PLAYER_SPEED * PLAYER_SPRINT_MULT
        } else {
            PLAYER_SPEED
        };
        let desired_horiz = move_dir * target_speed;
        let mut horiz = Vec3::new(velocity.linvel.x, 0.0, velocity.linvel.z);
        let delta = desired_horiz - horiz;
        let moving = move_dir.length_squared() > 0.0;
        let mut accel = if swimming && moving {
            MOVE_ACCEL * WATER_ACCEL_MULT
        } else if swimming {
            MOVE_BRAKE * WATER_BRAKE_MULT
        } else if moving {
            MOVE_ACCEL
        } else {
            MOVE_BRAKE
        };
        if !swimming && !grounded.is_grounded() {
            accel *= AIR_CONTROL_ACCEL_MULT;
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
        if swimming {
            horiz *= (-WATER_HORIZONTAL_DRAG * dt).exp();
        }
        velocity.linvel.x = horiz.x;
        velocity.linvel.z = horiz.z;

        if let Some(sample) = water_sample {
            jump_controller.reset();
            gravity_scale.0 = 0.0;
            apply_swimming_vertical_control(
                &mut velocity.linvel,
                sample.depth,
                input.jump,
                input.fly_down,
                dt,
            );
        } else {
            update_jump_buffer(&mut jump_controller, input.jump, dt);
            if try_consume_buffered_jump(&mut jump_controller, &mut grounded, &mut velocity) {
                commands.entity(entity).insert(PlayerJumpState {
                    timer: JUMP_ANIM_MIN_SECS,
                });
            }

            gravity_scale.0 =
                desired_gravity_scale(input.jump, velocity.linvel.y, grounded.is_grounded());
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
        (&Transform, &mut NpcPosition, &mut NpcRotation),
        (With<Npc>, With<NpcPhysicsBody>, Without<NpcRagdoll>),
    >,
) {
    for (transform, mut position, mut rotation) in npcs.iter_mut() {
        position.set_if_neq(NpcPosition(transform.translation));
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rotation.set_if_neq(NpcRotation(yaw));
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
            if velocity.linvel != Vec3::ZERO || velocity.angvel != Vec3::ZERO {
                velocity.linvel = Vec3::ZERO;
                velocity.angvel = Vec3::ZERO;
            }
            npc_velocity.set_if_neq(NpcVelocity(Vec3::ZERO));
            continue;
        }

        let desired_rotation = Quat::from_rotation_y(target_rotation.0);
        if transform.rotation.angle_between(desired_rotation) > 1.0e-5 {
            transform.rotation = desired_rotation;
        }

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
        let next_x = velocity.linvel.x + (horiz_limited.x - velocity.linvel.x) * blend;
        let next_z = velocity.linvel.z + (horiz_limited.z - velocity.linvel.z) * blend;
        if (next_x - velocity.linvel.x).abs() > 1.0e-5
            || (next_z - velocity.linvel.z).abs() > 1.0e-5
        {
            velocity.linvel.x = next_x;
            velocity.linvel.z = next_z;
        }

        npc_velocity.set_if_neq(NpcVelocity(Vec3::new(
            velocity.linvel.x,
            0.0,
            velocity.linvel.z,
        )));
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
        position.set_if_neq(NpcPosition(transform.translation));
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rotation.set_if_neq(NpcRotation(yaw));
        npc_velocity.set_if_neq(NpcVelocity(Vec3::new(
            velocity.linvel.x,
            0.0,
            velocity.linvel.z,
        )));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_buffer_only_starts_on_press_edge() {
        let mut state = PlayerJumpControllerState::default();

        update_jump_buffer(&mut state, false, 0.016);
        assert_eq!(state.jump_buffer_timer, 0.0);

        update_jump_buffer(&mut state, true, 0.016);
        assert!((state.jump_buffer_timer - JUMP_BUFFER_SECS).abs() < 1.0e-6);

        let timer_after_press = state.jump_buffer_timer;
        update_jump_buffer(&mut state, true, 0.016);
        assert!(state.jump_buffer_timer < timer_after_press);
    }

    #[test]
    fn buffered_jump_consumes_and_sets_upward_velocity() {
        let mut state = PlayerJumpControllerState {
            jump_buffer_timer: JUMP_BUFFER_SECS,
            jump_was_pressed: true,
        };
        let mut grounded = PlayerGrounded {
            on_terrain: true,
            on_static: false,
            time_since_grounded: 0.0,
        };
        let mut velocity = Velocity::default();

        assert!(try_consume_buffered_jump(
            &mut state,
            &mut grounded,
            &mut velocity
        ));
        assert_eq!(velocity.linvel.y, JUMP_VELOCITY);
        assert_eq!(state.jump_buffer_timer, 0.0);
        assert!(!grounded.is_grounded());
    }

    #[test]
    fn gravity_scale_is_heavier_on_fall_and_early_release() {
        assert_eq!(desired_gravity_scale(true, 0.0, true), 1.0);
        assert!(desired_gravity_scale(false, 3.0, false) > 1.0);
        assert!(desired_gravity_scale(true, -2.0, false) > 1.0);
    }

    #[test]
    fn swimming_depth_uses_hysteresis_at_the_surface() {
        let between_thresholds = (WATER_SWIM_DEPTH + WATER_SWIM_EXIT_DEPTH) * 0.5;

        assert!(!should_swim_at_depth(between_thresholds, false));
        assert!(should_swim_at_depth(between_thresholds, true));
        assert!(!should_swim_at_depth(WATER_SWIM_EXIT_DEPTH - 0.01, true));
    }

    #[test]
    fn buoyancy_damps_a_fast_fall_and_settles_at_float_depth() {
        let dt = 1.0 / 60.0;
        let mut falling = Vec3::new(0.0, -40.0, 0.0);
        apply_swimming_vertical_control(&mut falling, WATER_FLOAT_DEPTH + 1.0, false, false, dt);
        assert_eq!(falling.y, -WATER_MAX_DESCENT_SPEED);

        let mut floating = Vec3::ZERO;
        apply_swimming_vertical_control(&mut floating, WATER_FLOAT_DEPTH, false, false, dt);
        assert!(floating.y.abs() < 1.0e-6);
    }

    #[test]
    fn swim_controls_override_vertical_float_speed() {
        let mut velocity = Vec3::ZERO;
        apply_swimming_vertical_control(&mut velocity, WATER_FLOAT_DEPTH, true, false, 1.0 / 60.0);
        assert_eq!(velocity.y, SWIM_UP_SPEED);

        apply_swimming_vertical_control(&mut velocity, WATER_FLOAT_DEPTH, false, true, 1.0 / 60.0);
        assert_eq!(velocity.y, -SWIM_DOWN_SPEED);
    }

    #[test]
    fn npc_physics_radius_checks_all_players() {
        let players = [Vec3::new(100.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0)];

        assert!(is_within_any_player_radius(Vec3::ZERO, &players, 25.0));
        assert!(!is_within_any_player_radius(
            Vec3::ZERO,
            &players[..1],
            25.0
        ));
        assert!(!is_within_any_player_radius(Vec3::ZERO, &[], 25.0));
    }
}
