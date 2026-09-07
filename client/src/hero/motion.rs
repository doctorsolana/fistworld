//! Bounded rendering interpolation of authoritative character snapshots.

use bevy::prelude::*;
use shared::character::locomotion::WALK_CYCLE_SPEED;
use shared::components::{CharacterMotion, PlayerPosition, PlayerRotation, TimeWarp};

/// Client-side smoothing/animation state on the hero root.
#[derive(Component)]
pub struct HeroVisual {
    /// Smoothed world-space speed (m/s) of the VISUAL transform — drives the
    /// walk animation, so feet track what the eye actually sees.
    pub(super) speed: f32,
}

/// Last authoritative position snapshot received by this client. Rendering
/// extrapolates it for a tightly bounded interval using replicated velocity;
/// decisions and route truth remain entirely server-authoritative.
#[derive(Component)]
pub(crate) struct HeroMotionSnapshot {
    pub(super) position: Vec3,
    pub(super) received_at: f64,
}

impl HeroVisual {
    /// Smoothed visual speed, m/s.
    ///
    /// The honest source for "is this character moving?". The replicated
    /// position is a staircase at network rate, so comparing it frame to frame
    /// answers "did a packet land this frame?" rather than "is it walking?".
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// A rig that should always animate at full walk speed (the character
    /// creator preview walks in place on its turntable).
    pub fn walking_in_place() -> Self {
        Self {
            speed: WALK_CYCLE_SPEED,
        }
    }
}

/// One packet can be late without making a walking crowd pause. Beyond this
/// horizon we hold the latest truth instead of inventing a long prediction.
pub(super) const MAX_MOTION_EXTRAPOLATION_SECONDS: f64 = 0.08;

/// Rendering follows accelerated simulation in world time. Dividing the
/// extrapolation horizon by the same factor that multiplied server velocity
/// keeps its maximum spatial guess constant: 10x may move a villager ten
/// times faster, but it must not draw them ten times farther beyond a queue
/// place while waiting for the next snapshot.
pub(super) fn visual_time_factor(warp: f32) -> f32 {
    if warp.is_finite() {
        warp.max(1.0)
    } else {
        1.0
    }
}

pub(super) fn extrapolated_motion_target(
    position: Vec3,
    velocity: Vec3,
    snapshot_age: f64,
    time_factor: f32,
) -> Vec3 {
    let horizon = MAX_MOTION_EXTRAPOLATION_SECONDS / f64::from(time_factor.max(1.0));
    position + velocity * snapshot_age.clamp(0.0, horizon) as f32
}

pub(super) fn visual_position_blend(real_seconds: f32, time_factor: f32) -> f32 {
    // ~12/world-second: at every warp, the visual body trails the
    // authoritative body by the same WORLD distance instead of the same real
    // time. This is particularly visible in tightly spaced Moot queues.
    1.0 - (-12.0 * time_factor.max(1.0) * real_seconds).exp()
}

/// Exponentially smooth the visual transform toward the replicated state.
///
/// Replication arrives at ~30Hz in steps; the visual lerp hides the steps.
/// The observed visual speed feeds the walk animation, so the feet always
/// match the motion on screen, whatever the network does.
pub(crate) fn sync_hero_transforms(
    time: Res<Time>,
    warp: Query<&TimeWarp>,
    mut heroes: Query<(
        Ref<PlayerPosition>,
        &PlayerRotation,
        Option<&CharacterMotion>,
        &mut Transform,
        &mut HeroVisual,
        &mut HeroMotionSnapshot,
    )>,
) {
    let dt = time.delta_secs().max(1e-4);
    let now = time.elapsed_secs_f64();
    let time_factor = visual_time_factor(warp.iter().next().map_or(1.0, |warp| warp.0));
    let blend = visual_position_blend(dt, time_factor);

    for (pos, rot, motion, mut transform, mut visual, mut snapshot) in heroes.iter_mut() {
        if pos.is_changed() {
            snapshot.position = pos.0;
            snapshot.received_at = now;
        }
        let velocity = motion.map_or(Vec3::ZERO, |motion| motion.velocity);
        let before = transform.translation;
        let target = extrapolated_motion_target(
            snapshot.position,
            velocity,
            now - snapshot.received_at,
            time_factor,
        );
        let next = if before.distance_squared(target) > 20.0 * 20.0 {
            target // Teleport-scale jumps snap instead of gliding.
        } else {
            before.lerp(target, blend)
        };
        if next != before {
            transform.translation = next;
        }

        let target_rot = Quat::from_rotation_y(rot.0);
        if transform.rotation != target_rot {
            transform.rotation = transform.rotation.slerp(target_rot, blend);
        }

        let frame_speed = next.distance(before) / dt;
        visual.speed = visual.speed + (frame_speed - visual.speed) * blend;
    }
}
