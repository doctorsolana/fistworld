//! sync systems.

use super::*;

#[derive(Default)]
pub(crate) struct LocalPlayerNetDebugWindow {
    initialized: bool,
    enabled: bool,
    interval_secs: f32,
    window_started: f32,
    has_sample: bool,
    last_net_pos: Vec3,
    last_net_yaw: f32,
    last_sample_time: f32,
    sample_events: u64,
    sample_gap_sum: f32,
    sample_gap_max: f32,
    stale_sum: f32,
    stale_max: f32,
    stale_count: u64,
    render_error_sum: f32,
    render_error_max: f32,
}

impl LocalPlayerNetDebugWindow {
    fn reset_window(&mut self, now: f32) {
        self.window_started = now;
        self.sample_events = 0;
        self.sample_gap_sum = 0.0;
        self.sample_gap_max = 0.0;
        self.stale_sum = 0.0;
        self.stale_max = 0.0;
        self.stale_count = 0;
        self.render_error_sum = 0.0;
        self.render_error_max = 0.0;
    }
}

#[inline]
fn net_debug_enabled() -> bool {
    std::env::var("CITYSIM_NET_DEBUG")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "off")
        })
        .unwrap_or(false)
}

#[inline]
fn net_debug_interval_secs() -> f32 {
    std::env::var("CITYSIM_NET_DEBUG_INTERVAL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .unwrap_or(2.0)
        .clamp(0.5, 30.0)
}

#[inline]
fn wrap_yaw_delta(delta: f32) -> f32 {
    (delta + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Sync player transforms (visibility is handled by update_local_player_visibility)
pub fn sync_player_transforms(
    time: Res<Time>,
    vehicles: Query<
        (Entity, &Vehicle, &VehicleDriver, &Transform),
        (With<Vehicle>, Without<Player>),
    >,
    hover_bobs: Query<(&VehicleHoverBob, &ChildOf)>,
    mut players: Query<
        (
            &Player,
            &PlayerPosition,
            &PlayerRotation,
            Option<&LocalPlayer>,
            &mut Transform,
        ),
        Without<Vehicle>,
    >,
    mut driver_to_vehicle: Local<HashMap<u64, (Entity, VehicleType, Vec3, Quat)>>,
    mut vehicle_bobs: Local<HashMap<Entity, f32>>,
    mut local_net_debug: Local<LocalPlayerNetDebugWindow>,
) {
    let now = time.elapsed_secs();
    if !local_net_debug.initialized {
        local_net_debug.initialized = true;
        local_net_debug.enabled = net_debug_enabled();
        local_net_debug.interval_secs = net_debug_interval_secs();
        local_net_debug.reset_window(now);
        if local_net_debug.enabled {
            info!(
                "Local player net debug enabled (interval={:.2}s)",
                local_net_debug.interval_secs
            );
        }
    }

    let dt = time.delta_secs();
    let pos_rate: f32 = 22.0;
    let rot_rate: f32 = 26.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();

    // Map: driver_id -> vehicle transform (already smoothed in `sync_vehicle_transforms`)
    driver_to_vehicle.clear();
    for (entity, vehicle, driver, veh_transform) in vehicles.iter() {
        if let Some(driver_id) = driver.driver_id {
            driver_to_vehicle.insert(
                driver_id,
                (
                    entity,
                    vehicle.vehicle_type,
                    veh_transform.translation,
                    veh_transform.rotation,
                ),
            );
        }
    }

    let t = now;
    vehicle_bobs.clear();
    for (hover, parent) in hover_bobs.iter() {
        let vehicle_entity = parent.parent();
        let bob = (t * hover.frequency + hover.phase).sin() * hover.amplitude;
        vehicle_bobs.insert(vehicle_entity, bob);
    }

    let mut saw_local_player = false;

    for (player, position, rotation, is_local, mut transform) in players.iter_mut() {
        // If this player is driving a vehicle, attach their visual to the vehicle to eliminate
        // relative jitter between player and bike at high speed.
        if let Some((veh_entity, vehicle_type, veh_pos, veh_rot)) =
            driver_to_vehicle.get(&peer_id_to_u64(player.client_id))
        {
            let (bob, is_hover_bike) = match vehicle_bobs.get(veh_entity) {
                Some(bob) => (*bob, true),
                None => (0.0, false),
            };
            let (seat_height, seat_forward) = match vehicle_type {
                VehicleType::Motorbike if is_hover_bike => (0.90, 0.45),
                VehicleType::Motorbike => (0.65, 0.20),
                VehicleType::Car => (0.92, 0.08),
            };

            // Seat offset: slightly above and forward/back on the vehicle.
            let seat_local = Vec3::new(0.0, seat_height, seat_forward) + Vec3::Y * bob;
            let seat_offset = *veh_rot * seat_local;
            let target_pos = *veh_pos + seat_offset;
            // SNAP directly to vehicle - no lerp needed, vehicle is already smoothed
            transform.translation = target_pos;
            transform.rotation = *veh_rot;
        } else {
            transform.translation = transform.translation.lerp(position.0, t_pos);
            let target_rot = Quat::from_rotation_y(rotation.0);
            transform.rotation = transform.rotation.slerp(target_rot, t_rot);
        }

        if local_net_debug.enabled && is_local.is_some() {
            saw_local_player = true;
            if !local_net_debug.has_sample {
                local_net_debug.has_sample = true;
                local_net_debug.last_net_pos = position.0;
                local_net_debug.last_net_yaw = rotation.0;
                local_net_debug.last_sample_time = now;
            }

            let pos_delta_sq = (position.0 - local_net_debug.last_net_pos).length_squared();
            let yaw_delta = wrap_yaw_delta(rotation.0 - local_net_debug.last_net_yaw).abs();
            if pos_delta_sq > 1.0e-8 || yaw_delta > 1.0e-6 {
                let gap = (now - local_net_debug.last_sample_time).max(1.0 / 240.0);
                local_net_debug.sample_events = local_net_debug.sample_events.saturating_add(1);
                local_net_debug.sample_gap_sum += gap;
                local_net_debug.sample_gap_max = local_net_debug.sample_gap_max.max(gap);
                local_net_debug.last_net_pos = position.0;
                local_net_debug.last_net_yaw = rotation.0;
                local_net_debug.last_sample_time = now;
            }

            let stale_age = (now - local_net_debug.last_sample_time).max(0.0);
            local_net_debug.stale_count = local_net_debug.stale_count.saturating_add(1);
            local_net_debug.stale_sum += stale_age;
            local_net_debug.stale_max = local_net_debug.stale_max.max(stale_age);

            let render_error = (position.0 - transform.translation).length();
            local_net_debug.render_error_sum += render_error;
            local_net_debug.render_error_max = local_net_debug.render_error_max.max(render_error);
        }
    }

    if local_net_debug.enabled
        && now - local_net_debug.window_started >= local_net_debug.interval_secs
    {
        let window = (now - local_net_debug.window_started).max(0.001);
        if saw_local_player && local_net_debug.has_sample {
            let sample_rate = local_net_debug.sample_events as f32 / window;
            let avg_gap_ms = if local_net_debug.sample_events > 0 {
                (local_net_debug.sample_gap_sum / local_net_debug.sample_events as f32) * 1000.0
            } else {
                0.0
            };
            let max_gap_ms = local_net_debug.sample_gap_max * 1000.0;
            let avg_stale_ms = if local_net_debug.stale_count > 0 {
                (local_net_debug.stale_sum / local_net_debug.stale_count as f32) * 1000.0
            } else {
                0.0
            };
            let max_stale_ms = local_net_debug.stale_max * 1000.0;
            let avg_render_error = if local_net_debug.stale_count > 0 {
                local_net_debug.render_error_sum / local_net_debug.stale_count as f32
            } else {
                0.0
            };

            info!(
                "Local net debug: sample_events/s={:.1} avg_gap={:.1}ms max_gap={:.1}ms avg_stale={:.1}ms max_stale={:.1}ms avg_render_err={:.3}m max_render_err={:.3}m",
                sample_rate,
                avg_gap_ms,
                max_gap_ms,
                avg_stale_ms,
                max_stale_ms,
                avg_render_error,
                local_net_debug.render_error_max,
            );
        } else {
            info!("Local net debug: no LocalPlayer sampled in this window");
        }

        local_net_debug.reset_window(now);
    }
}
