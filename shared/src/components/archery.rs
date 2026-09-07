//! Authoritative archer equipment and absolute animation/projectile timelines.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoldierRole {
    #[default]
    Infantry,
    Archer,
}
impl SoldierRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Infantry => "Infantry",
            Self::Archer => "Archers",
        }
    }
}
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FirePolicy {
    #[default]
    FireAtWill,
    HoldFire,
}
impl FirePolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::FireAtWill => "Fire at will",
            Self::HoldFire => "Hold fire",
        }
    }
}
pub const QUIVER_CAPACITY: u8 = 24;
pub const BOW_DRAW_SECONDS: f32 = 1.0;
pub const BOW_SHOT_SECONDS: f32 = 2.0;
pub const BOW_RANGE: f32 = 65.0;
pub const BOW_ADVANCE_RANGE: f32 = 42.0;
/// Humanoid bow_shoot at 1.0 s, attach.bow.L * Bow/ArrowRelease.
/// Nock position in the character's -Z-forward local frame (metres).
pub const BOW_RELEASE_LOCAL: Vec3 = Vec3::new(-0.417315, 1.162306, 0.060152);
pub const ARROW_SPEED: f32 = 38.0;
pub const ARROW_GRAVITY: f32 = 9.81;
pub const ARROW_LIFETIME: f32 = 8.0;
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quiver {
    pub arrows: u8,
}
impl Default for Quiver {
    fn default() -> Self {
        Self {
            arrows: QUIVER_CAPACITY,
        }
    }
}
/// Current weapon, removed while using a sidearm. Role and quiver survive.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BowEquipped;
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct BowShot {
    pub release_at: f64,
}
impl BowShot {
    pub fn sample(self, now: f64) -> Option<f32> {
        let elapsed = (now - self.release_at) as f32 + BOW_DRAW_SECONDS;
        (elapsed.is_finite() && (0.0..BOW_SHOT_SECONDS).contains(&elapsed)).then_some(elapsed)
    }
}
/// One immutable launch, one optional stop; no networked frame-by-frame movement.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ArrowProjectile {
    pub origin: Vec3,
    pub velocity: Vec3,
    pub launched_at: f64,
    pub stopped_at: Option<f64>,
}
impl ArrowProjectile {
    pub fn position(self, now: f64) -> Vec3 {
        let t = (now.min(self.stopped_at.unwrap_or(now)) - self.launched_at)
            .clamp(0.0, f64::from(ARROW_LIFETIME)) as f32;
        self.origin + self.velocity * t - Vec3::Y * (0.5 * ARROW_GRAVITY * t * t)
    }
    pub fn direction(self, now: f64) -> Vec3 {
        let t = (now.min(self.stopped_at.unwrap_or(now)) - self.launched_at).max(0.0) as f32;
        (self.velocity - Vec3::Y * ARROW_GRAVITY * t).normalize_or_zero()
    }
}
/// Low ballistic solution. Iterated interception leads a moving target but never
/// changes a projectile after release. None means physically unreachable.
pub fn arrow_velocity(origin: Vec3, target: Vec3, target_velocity: Vec3) -> Option<Vec3> {
    let mut aim = target;
    let motion = target_velocity.clamp_length_max(8.0);
    let mut result = Vec3::ZERO;
    for _ in 0..4 {
        let d = aim - origin;
        let r = d.xz().length();
        if !d.is_finite() || r < 0.1 {
            return None;
        }
        let speed2 = ARROW_SPEED * ARROW_SPEED;
        let discriminant =
            speed2 * speed2 - ARROW_GRAVITY * (ARROW_GRAVITY * r * r + 2.0 * d.y * speed2);
        if discriminant < 0.0 {
            return None;
        }
        let angle = ((speed2 - discriminant.sqrt()) / (ARROW_GRAVITY * r)).atan();
        let horizontal = ARROW_SPEED * angle.cos();
        let xz = d.xz() / r * horizontal;
        result = Vec3::new(xz.x, ARROW_SPEED * angle.sin(), xz.y);
        aim = target + motion * (r / horizontal);
    }
    result.is_finite().then_some(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ballistic_intercept_leads_a_crossing_target() {
        let origin = Vec3::Y * 1.4;
        let target = Vec3::new(0., 1.1, 45.);
        let motion = Vec3::X * 3.;
        let velocity = arrow_velocity(origin, target, motion).unwrap();
        let t = 45. / velocity.z;
        let p = ArrowProjectile {
            origin,
            velocity,
            launched_at: 0.,
            stopped_at: None,
        };
        assert!(p.position(t as f64).distance(target + motion * t) < 0.02);
        assert!((velocity.length() - ARROW_SPEED).abs() < 0.01);
    }
    #[test]
    fn flight_stops_and_invalid_targets_fail() {
        let p = ArrowProjectile {
            origin: Vec3::ZERO,
            velocity: Vec3::Z * 38.,
            launched_at: 10.,
            stopped_at: Some(11.),
        };
        assert_eq!(p.position(12.), p.position(11.));
        assert_eq!(p.position(9.), Vec3::ZERO);
        assert!(arrow_velocity(Vec3::ZERO, Vec3::Y * 500., Vec3::ZERO).is_none());
        assert!(arrow_velocity(Vec3::ZERO, Vec3::splat(f32::NAN), Vec3::ZERO).is_none());
    }
    #[test]
    fn wire_roundtrips() {
        let p = ArrowProjectile {
            origin: Vec3::ONE,
            velocity: Vec3::Z,
            launched_at: 9999.,
            stopped_at: Some(10000.),
        };
        assert_eq!(
            bincode::deserialize::<ArrowProjectile>(&bincode::serialize(&p).unwrap()).unwrap(),
            p
        );
        for role in [SoldierRole::Infantry, SoldierRole::Archer] {
            assert_eq!(
                bincode::deserialize::<SoldierRole>(&bincode::serialize(&role).unwrap()).unwrap(),
                role
            );
        }
    }
}
