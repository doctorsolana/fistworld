//! First-person and third-person camera system
//!
//! Updated for Lightyear 0.26 / Bevy 0.18

use crate::input::CameraMode;
use crate::render::systems::VehicleHoverBob;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::components::{LocalPlayer, PlayerWaterState};
use shared::player::PLAYER_HEIGHT;
use shared::vehicle::{Vehicle, VehicleDriver};

/// Camera offset from player position (eye level) for first person
const CAMERA_HEIGHT_OFFSET: f32 = PLAYER_HEIGHT * 0.4;
pub(crate) const CAMERA_NEAR_CLIP: f32 = 0.001;
const VEHICLE_FP_SEAT_HEIGHT: f32 = 0.75;
const HOVERBIKE_FP_SEAT_HEIGHT: f32 = 0.95;
const VEHICLE_FP_SEAT_FORWARD: f32 = 0.15;

/// Third person camera settings (GTA-style orbit)
const THIRD_PERSON_DISTANCE: f32 = 5.5; // Orbit radius from pivot
const THIRD_PERSON_BASE_HEIGHT: f32 = 1.0; // Base height offset of pivot point
const THIRD_PERSON_DEFAULT_PITCH: f32 = 0.25; // Default orbit angle (slightly above)

/// FOV settings for aiming
const FOV_DEFAULT: f32 = 70.0_f32.to_radians(); // Normal FOV in radians
const FOV_SPRINT: f32 = 76.0_f32.to_radians(); // Slight speed kick when sprinting
const FOV_ADS: f32 = 45.0_f32.to_radians(); // Zoomed FOV when ADS
const FOV_SNIPER_ADS: f32 = 20.0_f32.to_radians(); // Extra zoom for sniper
                                                   // Tuned to make distortion visible within the centered sniper scope viewport.
const SNIPER_FISHEYE_STRENGTH: f32 = 0.12;
const SNIPER_FISHEYE_EDGE_START: f32 = 0.09;
const SNIPER_FISHEYE_BLEND_SPEED: f32 = 14.0;

/// Helper to convert PeerId to u64 for driver tracking
pub(crate) fn peer_id_to_u64(peer_id: PeerId) -> u64 {
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        _ => 0, // Server or other types
    }
}

/// Update camera to follow local player
pub fn update_camera(
    // In Lightyear 0.26, use LocalId to identify the local client peer id
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    player_query: Query<&Transform, (With<LocalPlayer>, Without<Camera3d>)>,
    vehicles_query: Query<(Entity, &VehicleDriver, &Transform), (With<Vehicle>, Without<Camera3d>)>,
    hover_bobs: Query<(&VehicleHoverBob, &ChildOf)>,
    mut camera_query: Query<
        &mut Transform,
        (With<Camera3d>, Without<LocalPlayer>, Without<Vehicle>),
    >,
    input_state: Res<crate::input::InputState>,
    time: Res<Time>,
    local_water: Query<(), (With<LocalPlayer>, With<PlayerWaterState>)>,
) {
    let Some(player_transform) = player_query.iter().next() else {
        return;
    };

    let Ok(mut camera_transform) = camera_query.single_mut() else {
        return;
    };

    // Get our peer ID from the connected client entity
    let our_driver_id = client_query
        .iter()
        .next()
        .map(|r| peer_id_to_u64(r.0))
        .unwrap_or(0);

    // If we're driving a vehicle, drive the camera off the vehicle Transform.
    // This ensures camera and bike use the *same* smoothed pose, eliminating relative jitter.
    let vehicle_pose = vehicles_query
        .iter()
        .find_map(|(entity, driver, transform)| {
            (driver.driver_id == Some(our_driver_id)).then_some((
                entity,
                transform.translation,
                transform.rotation,
            ))
        });

    // Mild camera smoothing (helps remove any remaining micro-jitter from terrain/rotation)
    let cam_rate: f32 = 35.0;
    let cam_t = 1.0_f32 - (-cam_rate * time.delta_secs()).exp();

    let vehicle_bob = vehicle_pose
        .as_ref()
        .and_then(|(entity, _, _)| vehicle_bob_offset(&hover_bobs, *entity, time.elapsed_secs()));
    let in_water = !local_water.is_empty();

    let (target_pos, target_rot) = match input_state.camera_mode {
        CameraMode::FirstPerson => first_person_target(
            player_transform,
            vehicle_pose,
            vehicle_bob,
            &input_state,
            time.elapsed_secs(),
            in_water,
        ),
        CameraMode::ThirdPerson => {
            third_person_target(player_transform, vehicle_pose, &input_state)
        }
    };

    // When driving a vehicle, SNAP directly to avoid jitter at high speeds.
    // The vehicle transform is already smoothed, so no additional interpolation needed.
    // This applies to BOTH first-person and third-person vehicle modes.
    let in_vehicle = vehicle_pose.is_some();

    if in_vehicle {
        camera_transform.translation = target_pos;
        camera_transform.rotation = target_rot;
    } else {
        camera_transform.translation = camera_transform.translation.lerp(target_pos, cam_t);
        camera_transform.rotation = camera_transform.rotation.slerp(target_rot, cam_t);
    }
}

fn first_person_target(
    player_transform: &Transform,
    vehicle_pose: Option<(Entity, Vec3, Quat)>,
    vehicle_bob: Option<f32>,
    input_state: &crate::input::InputState,
    time_secs: f32,
    in_water: bool,
) -> (Vec3, Quat) {
    if let Some((_, veh_pos, veh_rot)) = vehicle_pose {
        let (seat_height, bob) = match vehicle_bob {
            Some(bob) => (HOVERBIKE_FP_SEAT_HEIGHT, bob),
            None => (VEHICLE_FP_SEAT_HEIGHT, 0.0),
        };
        // Seat/cockpit position on the speeder (local space)
        let seat_offset = Vec3::new(0.0, seat_height, VEHICLE_FP_SEAT_FORWARD) + Vec3::Y * bob;
        let seat_offset = veh_rot * seat_offset;
        let pos = veh_pos + seat_offset;

        let look_rotation = Quat::from_euler(
            EulerRot::YXZ,
            input_state.vehicle_look_yaw,
            input_state.vehicle_look_pitch,
            0.0,
        );
        let rot = veh_rot * look_rotation;
        (pos, rot)
    } else {
        let mut pos = player_transform.translation + Vec3::new(0.0, CAMERA_HEIGHT_OFFSET, 0.0);
        let mut pitch = input_state.pitch;
        let mut roll = 0.0;

        if is_moving_on_foot(input_state) && !in_water {
            let sprinting = is_sprinting_on_foot(input_state);
            let phase = time_secs * if sprinting { 13.5 } else { 8.25 };
            let vertical_amp = if sprinting { 0.038 } else { 0.012 };
            let lateral_amp = if sprinting { 0.016 } else { 0.004 };
            let pitch_amp = if sprinting { 0.010 } else { 0.003 };
            let roll_amp = if sprinting { 0.013 } else { 0.004 };

            let yaw_rot = Quat::from_rotation_y(input_state.yaw);
            let bob_local = Vec3::new(
                (phase * 0.5).sin() * lateral_amp,
                phase.sin().abs() * vertical_amp,
                0.0,
            );
            pos += yaw_rot * bob_local;
            pitch += phase.sin().abs() * pitch_amp;
            roll = (phase * 0.5).sin() * roll_amp;
        }

        let rot = Quat::from_euler(EulerRot::YXZ, input_state.yaw, pitch, roll);
        (pos, rot)
    }
}

#[inline]
fn is_moving_on_foot(input_state: &crate::input::InputState) -> bool {
    !input_state.in_vehicle
        && !input_state.fly_mode
        && (input_state.forward || input_state.backward || input_state.left || input_state.right)
}

#[inline]
fn is_sprinting_on_foot(input_state: &crate::input::InputState) -> bool {
    is_moving_on_foot(input_state)
        && input_state.shift
        && input_state.forward
        && !input_state.backward
        && !input_state.aiming
}

fn third_person_target(
    player_transform: &Transform,
    vehicle_pose: Option<(Entity, Vec3, Quat)>,
    input_state: &crate::input::InputState,
) -> (Vec3, Quat) {
    // GTA-style orbit camera: pitch input orbits camera up/down around the pivot
    // Looking down (mouse down) -> camera orbits UP and over
    // Looking up (mouse up) -> camera orbits DOWN

    if let Some((_, veh_pos, veh_rot)) = vehicle_pose {
        // Extract vehicle yaw only (ignore pitch/roll for stable camera)
        let (veh_yaw, _, _) = veh_rot.to_euler(EulerRot::YXZ);

        // Orbit angles
        let orbit_yaw = veh_yaw + input_state.vehicle_look_yaw;
        // Invert pitch: looking down (negative input) = orbit up (positive angle)
        let orbit_pitch =
            (THIRD_PERSON_DEFAULT_PITCH - input_state.vehicle_look_pitch * 0.6).clamp(-0.2, 1.3); // Clamp to prevent going under or too far over

        // Pivot point (center of vehicle)
        let pivot = veh_pos + Vec3::new(0.0, THIRD_PERSON_BASE_HEIGHT, 0.0);

        // Calculate camera position on orbit sphere
        let cam_pos = orbit_position(pivot, orbit_yaw, orbit_pitch, THIRD_PERSON_DISTANCE);

        // Camera looks at the pivot point - use look_at style rotation (keeps camera level)
        let cam_rot = look_at_level(cam_pos, pivot);

        (cam_pos, cam_rot)
    } else {
        // On foot: same orbit logic
        let orbit_yaw = input_state.yaw;
        let orbit_pitch = (THIRD_PERSON_DEFAULT_PITCH - input_state.pitch * 0.6).clamp(-0.2, 1.3);

        let pivot = player_transform.translation + Vec3::new(0.0, PLAYER_HEIGHT * 0.5, 0.0);
        let cam_pos = orbit_position(pivot, orbit_yaw, orbit_pitch, THIRD_PERSON_DISTANCE);

        let cam_rot = look_at_level(cam_pos, pivot);

        (cam_pos, cam_rot)
    }
}

fn vehicle_bob_offset(
    hover_bobs: &Query<(&VehicleHoverBob, &ChildOf)>,
    vehicle_entity: Entity,
    time_secs: f32,
) -> Option<f32> {
    hover_bobs.iter().find_map(|(hover, parent)| {
        (parent.parent() == vehicle_entity)
            .then(|| (time_secs * hover.frequency + hover.phase).sin() * hover.amplitude)
    })
}

/// Calculate camera position orbiting around a pivot point
fn orbit_position(pivot: Vec3, yaw: f32, pitch: f32, distance: f32) -> Vec3 {
    // Spherical coordinates:
    // - yaw rotates around Y axis (horizontal)
    // - pitch rotates up/down (0 = behind, positive = above)
    let cos_pitch = pitch.cos();
    let sin_pitch = pitch.sin();

    // Horizontal offset (behind based on yaw)
    let horizontal_dist = distance * cos_pitch;
    let behind_dir = Vec3::new(yaw.sin(), 0.0, yaw.cos());

    // Vertical offset (above based on pitch)
    let vertical_offset = distance * sin_pitch;

    pivot + behind_dir * horizontal_dist + Vec3::new(0.0, vertical_offset, 0.0)
}

/// Create a rotation that looks at target while keeping camera level (no roll)
fn look_at_level(eye: Vec3, target: Vec3) -> Quat {
    // Use Bevy's built-in looking_at with world up
    Transform::from_translation(eye)
        .looking_at(target, Vec3::Y)
        .rotation
}

/// Update camera FOV for ADS zoom effect
pub fn update_camera_fov(
    mut camera_query: Query<&mut Projection, With<Camera3d>>,
    input_state: Res<crate::input::InputState>,
    local_player: Query<&shared::components::EquippedWeapon, With<LocalPlayer>>,
    local_water: Query<(), (With<LocalPlayer>, With<PlayerWaterState>)>,
    time: Res<Time>,
) {
    let Ok(mut projection) = camera_query.single_mut() else {
        return;
    };

    let Projection::Perspective(ref mut persp) = *projection else {
        return;
    };

    // Keep a small near plane so point-blank NPCs/props don't clip out aggressively.
    persp.near = CAMERA_NEAR_CLIP;

    // Determine target FOV
    let target_fov = if input_state.aiming && input_state.camera_mode == CameraMode::FirstPerson {
        // Check if using sniper for extra zoom
        if let Some(weapon) = local_player.iter().next() {
            if weapon.weapon_type == shared::weapons::WeaponType::Sniper {
                FOV_SNIPER_ADS
            } else {
                FOV_ADS
            }
        } else {
            FOV_ADS
        }
    } else if input_state.camera_mode == CameraMode::FirstPerson
        && is_sprinting_on_foot(&input_state)
        && local_water.is_empty()
    {
        FOV_SPRINT
    } else {
        FOV_DEFAULT
    };

    // Smooth transition
    let zoom_speed = 12.0;
    let t = 1.0 - (-zoom_speed * time.delta_secs()).exp();
    persp.fov = persp.fov + (target_fov - persp.fov) * t;
}

/// Update sniper fisheye settings on the main gameplay camera.
pub fn update_sniper_fisheye(
    mut camera_query: Query<&mut crate::render::sniper_fisheye::SniperFisheye, With<Camera3d>>,
    input_state: Res<crate::input::InputState>,
    local_player: Query<&shared::components::EquippedWeapon, With<LocalPlayer>>,
    time: Res<Time>,
) {
    let sniper_ads = input_state.aiming
        && input_state.camera_mode == CameraMode::FirstPerson
        && local_player
            .iter()
            .next()
            .map(|weapon| weapon.weapon_type == shared::weapons::WeaponType::Sniper)
            .unwrap_or(false);

    let target_strength = if sniper_ads {
        SNIPER_FISHEYE_STRENGTH
    } else {
        0.0
    };
    let t = 1.0 - (-SNIPER_FISHEYE_BLEND_SPEED * time.delta_secs()).exp();

    for mut effect in camera_query.iter_mut() {
        effect.edge_start = SNIPER_FISHEYE_EDGE_START;
        effect.strength = effect.strength + (target_strength - effect.strength) * t;
    }
}
