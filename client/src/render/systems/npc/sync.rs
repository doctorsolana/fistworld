//! sync systems.

use super::*;

#[derive(Default)]
pub(crate) struct NpcNetDebugWindow {
    initialized: bool,
    enabled: bool,
    interval_secs: f32,
    window_started: f32,
    npc_samples: u64,
    sample_events: u64,
    sample_gap_sum: f32,
    sample_gap_max: f32,
    stale_sum: f32,
    stale_max: f32,
    stale_count: u64,
    stale_over_250ms: u64,
    stale_over_1000ms: u64,
    snap_events: u64,
}

impl NpcNetDebugWindow {
    fn reset_window(&mut self, now: f32) {
        self.window_started = now;
        self.npc_samples = 0;
        self.sample_events = 0;
        self.sample_gap_sum = 0.0;
        self.sample_gap_max = 0.0;
        self.stale_sum = 0.0;
        self.stale_max = 0.0;
        self.stale_count = 0;
        self.stale_over_250ms = 0;
        self.stale_over_1000ms = 0;
        self.snap_events = 0;
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

/// Sync NPC transforms from replicated components.
pub fn sync_npc_transforms(
    mut commands: Commands,
    time: Res<Time>,
    mut net_debug: Local<NpcNetDebugWindow>,
    mut npcs: Query<
        (
            Entity,
            &NpcPosition,
            &NpcRotation,
            &mut Transform,
            Option<&mut NpcNetSmoothing>,
        ),
        With<Npc>,
    >,
) {
    let now = time.elapsed_secs();
    if !net_debug.initialized {
        net_debug.initialized = true;
        net_debug.enabled = net_debug_enabled();
        net_debug.interval_secs = net_debug_interval_secs();
        net_debug.reset_window(now);
        if net_debug.enabled {
            info!(
                "NPC net debug enabled (interval={:.2}s)",
                net_debug.interval_secs
            );
        }
    }

    let dt = time.delta_secs().max(1.0 / 240.0);
    let pos_rate: f32 = 10.0;
    let rot_rate: f32 = 14.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();
    const SNAP_DISTANCE_SQ: f32 = 8.0 * 8.0;

    for (entity, pos, rot, mut transform, smoothing_opt) in npcs.iter_mut() {
        if net_debug.enabled {
            net_debug.npc_samples = net_debug.npc_samples.saturating_add(1);
        }

        let Some(mut smoothing) = smoothing_opt else {
            commands
                .entity(entity)
                .insert(NpcNetSmoothing::from_sample(pos.0, rot.0, now));
            transform.translation = pos.0;
            transform.rotation = Quat::from_rotation_y(rot.0);
            if net_debug.enabled {
                net_debug.stale_count = net_debug.stale_count.saturating_add(1);
            }
            continue;
        };

        if !smoothing.initialized {
            *smoothing = NpcNetSmoothing::from_sample(pos.0, rot.0, now);
            transform.translation = pos.0;
            transform.rotation = Quat::from_rotation_y(rot.0);
            if net_debug.enabled {
                net_debug.stale_count = net_debug.stale_count.saturating_add(1);
            }
            continue;
        }

        let pos_delta = pos.0 - smoothing.last_net_pos;
        let yaw_delta = wrap_yaw_delta(rot.0 - smoothing.last_net_yaw);
        let new_pos_sample = pos_delta.length_squared() > 1.0e-8;
        let new_yaw_sample = yaw_delta.abs() > 1.0e-6;

        if new_pos_sample || new_yaw_sample {
            let sample_dt = (now - smoothing.last_sample_time).max(1.0 / 240.0);
            if net_debug.enabled {
                net_debug.sample_events = net_debug.sample_events.saturating_add(1);
                net_debug.sample_gap_sum += sample_dt;
                net_debug.sample_gap_max = net_debug.sample_gap_max.max(sample_dt);
            }

            if new_pos_sample {
                let measured_vel = pos_delta / sample_dt;
                smoothing.velocity = smoothing.velocity.lerp(measured_vel, 0.6);
                smoothing.last_net_pos = pos.0;
                smoothing.target_pos = pos.0;
            }
            if new_yaw_sample {
                let measured_yaw_rate = yaw_delta / sample_dt;
                smoothing.yaw_rate = smoothing.yaw_rate * 0.4 + measured_yaw_rate * 0.6;
                smoothing.last_net_yaw = rot.0;
                smoothing.target_yaw = rot.0;
            }
            smoothing.last_sample_time = now;
        }

        let age = (now - smoothing.last_sample_time).max(0.0);
        let extrap_t = age.min(NPC_NET_EXTRAPOLATE_MAX_SECS);
        let predicted_pos = smoothing.target_pos + smoothing.velocity * extrap_t;
        let predicted_yaw = smoothing.target_yaw + smoothing.yaw_rate * extrap_t;

        let correction = predicted_pos - transform.translation;
        if correction.length_squared() > SNAP_DISTANCE_SQ {
            transform.translation = predicted_pos;
            if net_debug.enabled {
                net_debug.snap_events = net_debug.snap_events.saturating_add(1);
            }
        } else {
            transform.translation = transform.translation.lerp(predicted_pos, t_pos);
        }

        let target_rot = Quat::from_rotation_y(predicted_yaw);
        transform.rotation = transform.rotation.slerp(target_rot, t_rot);

        if net_debug.enabled {
            net_debug.stale_count = net_debug.stale_count.saturating_add(1);
            net_debug.stale_sum += age;
            net_debug.stale_max = net_debug.stale_max.max(age);
            if age > 0.25 {
                net_debug.stale_over_250ms = net_debug.stale_over_250ms.saturating_add(1);
            }
            if age > 1.0 {
                net_debug.stale_over_1000ms = net_debug.stale_over_1000ms.saturating_add(1);
            }
        }
    }

    if net_debug.enabled && now - net_debug.window_started >= net_debug.interval_secs {
        let window = (now - net_debug.window_started).max(0.001);
        let sample_rate = net_debug.sample_events as f32 / window;
        let snap_rate = net_debug.snap_events as f32 / window;
        let avg_gap_ms = if net_debug.sample_events > 0 {
            (net_debug.sample_gap_sum / net_debug.sample_events as f32) * 1000.0
        } else {
            0.0
        };
        let max_gap_ms = net_debug.sample_gap_max * 1000.0;
        let avg_stale_ms = if net_debug.stale_count > 0 {
            (net_debug.stale_sum / net_debug.stale_count as f32) * 1000.0
        } else {
            0.0
        };
        let max_stale_ms = net_debug.stale_max * 1000.0;
        let stale_250_pct = if net_debug.stale_count > 0 {
            net_debug.stale_over_250ms as f32 * 100.0 / net_debug.stale_count as f32
        } else {
            0.0
        };
        let stale_1000_pct = if net_debug.stale_count > 0 {
            net_debug.stale_over_1000ms as f32 * 100.0 / net_debug.stale_count as f32
        } else {
            0.0
        };

        info!(
            "NPC net debug: npcs={} sample_events/s={:.1} avg_gap={:.1}ms max_gap={:.1}ms avg_stale={:.1}ms max_stale={:.1}ms stale>250ms={:.1}% stale>1000ms={:.1}% snap_events/s={:.1}",
            net_debug.npc_samples,
            sample_rate,
            avg_gap_ms,
            max_gap_ms,
            avg_stale_ms,
            max_stale_ms,
            stale_250_pct,
            stale_1000_pct,
            snap_rate
        );

        net_debug.reset_window(now);
    }
}
