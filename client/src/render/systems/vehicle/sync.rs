//! sync systems.

use super::*;

/// Sync vehicle transforms from replicated state with improved smoothing.
///
/// Improvements over basic dead-reckoning:
/// 1. Velocity smoothing - reduces jitter from velocity discontinuities
/// 2. Heading-aware extrapolation - predicts curved paths when turning
/// 3. Continuous background correction - spreads corrections over time
/// 4. Error clamping - prevents large desync
pub fn sync_vehicle_transforms(
    time: Res<Time>,
    mut vehicles: Query<
        (&VehicleState, &mut VehicleRenderSmoothing, &mut Transform),
        With<Vehicle>,
    >,
) {
    let dt = time.delta_secs();

    // === CORRECTION RATES ===
    // On-snapshot correction (stronger, only when server sends new data)
    let pos_correction_rate: f32 = 25.0;
    let rot_correction_rate: f32 = 30.0;
    let t_pos = 1.0_f32 - (-pos_correction_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_correction_rate * dt).exp();

    // Background correction (always-on, very gentle drift toward server)
    let background_pos_rate: f32 = 3.0;
    let background_rot_rate: f32 = 4.0;
    let t_bg_pos = 1.0_f32 - (-background_pos_rate * dt).exp();
    let t_bg_rot = 1.0_f32 - (-background_rot_rate * dt).exp();

    // Velocity smoothing rate
    let vel_smooth_rate: f32 = 15.0;
    let t_vel = 1.0_f32 - (-vel_smooth_rate * dt).exp();

    // Maximum allowed prediction error before emergency correction
    let max_error: f32 = 1.5; // meters

    for (state, mut smooth, mut transform) in vehicles.iter_mut() {
        if !smooth.initialized {
            smooth.initialized = true;
            smooth.position = state.position;
            smooth.heading = state.heading;
            smooth.pitch = state.pitch;
            smooth.roll = state.roll;
            smooth.last_server_position = state.position;
            smooth.last_server_heading = state.heading;
            smooth.last_server_pitch = state.pitch;
            smooth.last_server_roll = state.roll;
            smooth.smoothed_velocity = state.velocity;
            smooth.smoothed_angular_yaw = state.angular_velocity_yaw;
        } else {
            // === 1. VELOCITY SMOOTHING ===
            // Smooth velocity/angular velocity to reduce jitter from discrete server updates
            smooth.smoothed_velocity = smooth.smoothed_velocity.lerp(state.velocity, t_vel);
            smooth.smoothed_angular_yaw +=
                (state.angular_velocity_yaw - smooth.smoothed_angular_yaw) * t_vel;

            // === 2. HEADING-AWARE EXTRAPOLATION ===
            // When turning, rotate velocity by half the yaw change (midpoint
            // approximation for curves). Heading advances by +yaw_delta, so
            // the velocity must curve by +yaw_delta/2 as well — the old
            // negative sign bowed the predicted path to the OUTSIDE of turns
            // (one of this repo's recurring mirror bugs).
            let yaw_delta = smooth.smoothed_angular_yaw * dt;
            let half_yaw_rot = Quat::from_rotation_y(yaw_delta * 0.5);
            let curved_velocity = half_yaw_rot * smooth.smoothed_velocity;
            smooth.position += curved_velocity * dt;

            // Extrapolate angles
            smooth.heading = normalize_angle(smooth.heading + smooth.smoothed_angular_yaw * dt);
            smooth.pitch = normalize_angle(smooth.pitch + state.angular_velocity_pitch * dt);
            smooth.roll = normalize_angle(smooth.roll + state.angular_velocity_roll * dt);

            // === 3. CONTINUOUS BACKGROUND CORRECTION ===
            // Always apply a very gentle drift toward server (catches accumulated error)
            smooth.position = smooth.position.lerp(state.position, t_bg_pos);
            smooth.heading = lerp_angle(smooth.heading, state.heading, t_bg_rot);
            smooth.pitch = lerp_angle(smooth.pitch, state.pitch, t_bg_rot);
            smooth.roll = lerp_angle(smooth.roll, state.roll, t_bg_rot);

            // === 4. ON-SNAPSHOT STRONGER CORRECTION ===
            // When new server data arrives, apply stronger correction
            let server_updated = state.position != smooth.last_server_position
                || state.heading != smooth.last_server_heading
                || state.pitch != smooth.last_server_pitch
                || state.roll != smooth.last_server_roll;

            if server_updated {
                smooth.position = smooth.position.lerp(state.position, t_pos);
                smooth.heading = lerp_angle(smooth.heading, state.heading, t_rot);
                smooth.pitch = lerp_angle(smooth.pitch, state.pitch, t_rot);
                smooth.roll = lerp_angle(smooth.roll, state.roll, t_rot);

                smooth.last_server_position = state.position;
                smooth.last_server_heading = state.heading;
                smooth.last_server_pitch = state.pitch;
                smooth.last_server_roll = state.roll;
            }

            // === 5. ERROR CLAMPING (SAFETY NET) ===
            // If prediction drifted too far, snap harder to prevent obvious desync
            let error = (smooth.position - state.position).length();
            if error > max_error {
                let emergency_t = ((error - max_error) / max_error).clamp(0.0, 1.0) * 0.5;
                smooth.position = smooth.position.lerp(state.position, emergency_t);
            }
        }

        transform.translation = smooth.position;
        transform.rotation =
            Quat::from_euler(EulerRot::YXZ, smooth.heading, smooth.pitch, -smooth.roll);
    }
}
