//! Replicated siege state. Timing is absolute world time; no client owns damage.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const CATAPULT_SPEED: f32 = 1.15;
pub const CATAPULT_TURN_SPEED: f32 = 0.65;
pub const CATAPULT_CLEARANCE: f32 = 3.0;
pub const CATAPULT_MIN_RANGE: f32 = 16.0;
pub const CATAPULT_MAX_RANGE: f32 = 125.0;
pub const CATAPULT_BLAST_RADIUS: f32 = 6.0;
pub const CATAPULT_DAMAGE: f32 = 115.0;
pub const CATAPULT_WINDUP: f64 = 1.6;
pub const CATAPULT_RELOAD: f64 = 8.5;
pub const CATAPULT_AMMUNITION: u16 = 20;

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catapult {
    pub ammunition: u16,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SiegePhase {
    #[default]
    Ready,
    Moving,
    Turning,
    Winding,
    Reloading,
    OutOfRange,
    Empty,
    Blocked,
    Destroyed,
}
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct CatapultStatus {
    pub phase: SiegePhase,
    pub aim: Option<Vec3>,
    pub cycle_at: f64,
    pub fire_at: f64,
    pub ready_at: f64,
}
impl SiegePhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Moving => "MOVING",
            Self::Turning => "TURNING TO TARGET",
            Self::Winding => "WINDING",
            Self::Reloading => "RELOADING",
            Self::OutOfRange => "OUT OF RANGE",
            Self::Empty => "OUT OF STONES",
            Self::Blocked => "ROUTE BLOCKED",
            Self::Destroyed => "DESTROYED",
        }
    }
}
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SiegeProjectile {
    pub origin: Vec3,
    pub aim: Vec3,
    pub impact: Vec3,
    pub launched_at: f64,
    pub flight_seconds: f32,
    pub impact_at: f64,
    pub seed: u64,
}
impl SiegeProjectile {
    pub fn position(&self, now: f64) -> Vec3 {
        let t = ((now - self.launched_at) as f32 / self.flight_seconds.max(0.01)).clamp(0.0, 1.0);
        let height = (self.origin.xz().distance(self.aim.xz()) * 0.26).clamp(8.0, 28.0);
        self.origin.lerp(self.aim, t) + Vec3::Y * (4.0 * t * (1.0 - t) * height)
    }
}
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SiegeImpact {
    pub position: Vec3,
    pub at: f64,
    pub seed: u64,
}

pub fn siege_in_range(from: Vec3, to: Vec3) -> bool {
    from.is_finite()
        && to.is_finite()
        && (CATAPULT_MIN_RANGE..=CATAPULT_MAX_RANGE).contains(&from.xz().distance(to.xz()))
}
pub fn siege_splash_damage(distance: f32) -> f32 {
    if !distance.is_finite() || distance < 0.0 || distance >= CATAPULT_BLAST_RADIUS {
        return 0.0;
    }
    let t = distance / CATAPULT_BLAST_RADIUS;
    CATAPULT_DAMAGE * (1.0 - t).powi(2)
}
pub fn turn_siege_towards(yaw: f32, direction: Vec2, dt: f32) -> (f32, bool) {
    let wanted = f32::atan2(-direction.x, -direction.y);
    let delta = (wanted - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    (
        yaw + delta.clamp(-CATAPULT_TURN_SPEED * dt, CATAPULT_TURN_SPEED * dt),
        delta.abs() < 0.08,
    )
}
/// Arm kinematics shared by the visual rig and projectile launch socket.
pub fn catapult_arm_angle(status: &CatapultStatus, now: f64) -> f32 {
    if status.fire_at <= 0.0 {
        return -0.12;
    }
    let since = (now - status.fire_at) as f32;
    if since < 0.0 {
        return -0.12
            + 0.07 * ((now - status.cycle_at) as f32 / CATAPULT_WINDUP as f32).clamp(0.0, 1.0);
    }
    if since < 0.22 {
        let t = (since / 0.22).clamp(0.0, 1.0);
        return -0.05 - 1.92 * (1.0 - (1.0 - t).powi(3));
    }
    if since < 0.9 {
        return -1.85 - 0.12 * ((since - 0.22) * 19.0).cos() * (-(since - 0.22) * 5.0).exp();
    }
    let t = ((since - 0.9) / (CATAPULT_RELOAD as f32 - 0.9)).clamp(0.0, 1.0);
    -1.85 + 1.73 * t * t * (3.0 - 2.0 * t)
}
/// Centre of the stone in the authored spoon, in carriage-local coordinates.
pub fn catapult_stone_socket(status: &CatapultStatus, now: f64) -> Vec3 {
    Vec3::new(0.0, 1.75, 0.0)
        + Quat::from_rotation_x(catapult_arm_angle(status, now)) * Vec3::new(0.0, 0.25, 2.2)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splash_is_local_and_falls_off() {
        assert_eq!(siege_splash_damage(0.0), 115.0);
        assert!(siege_splash_damage(3.0) < 30.0);
        assert_eq!(siege_splash_damage(6.0), 0.0);
        assert_eq!(siege_splash_damage(f32::NAN), 0.0);
    }
    #[test]
    fn ballistic_arc_and_wire_timing_survive_roundtrip() {
        let p = SiegeProjectile {
            origin: Vec3::new(0.0, 3.0, 0.0),
            aim: Vec3::new(0.0, 0.0, -60.0),
            impact: Vec3::new(0.0, 0.0, -60.0),
            launched_at: 100.0,
            flight_seconds: 3.0,
            impact_at: 103.0,
            seed: u64::MAX,
        };
        assert_eq!(p.position(100.0), p.origin);
        assert_eq!(p.position(103.0), p.aim);
        assert!(p.position(101.5).y > 16.0);
        let state = (
            Catapult { ammunition: 20 },
            CatapultStatus {
                phase: SiegePhase::Winding,
                aim: Some(p.aim),
                cycle_at: 98.4,
                fire_at: 100.0,
                ready_at: 108.5,
            },
            p,
            SiegeImpact {
                position: p.impact,
                at: 103.0,
                seed: 42,
            },
        );
        assert_eq!(
            bincode::deserialize::<(Catapult, CatapultStatus, SiegeProjectile, SiegeImpact)>(
                &bincode::serialize(&state).unwrap()
            )
            .unwrap(),
            state
        );
    }
}
