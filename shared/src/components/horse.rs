//! Horse identity, mount lifecycle and shared animation/navigation dimensions.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const HORSE_SCENE: &str = "game_assets/environment/animals/Horse.glb#Scene0";
pub const HORSE_CLEARANCE: f32 = 1.55;
/// Conservative circular ground body for local army separation (horse length2.85m).
pub const HORSE_BODY_RADIUS: f32 = 1.45;

/// Horses can wheel promptly, while still turning before advancing into a new lane.
pub fn turn_horse_towards(yaw: f32, direction: Vec2, dt: f32) -> (f32, bool) {
    let goal = f32::atan2(-direction.x, -direction.y);
    let delta = (goal - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let step = delta.clamp(-3.5 * dt, 3.5 * dt);
    (yaw + step, (delta - step).abs() < 0.22)
}
pub const HORSE_MOUNT_REACH: f32 = 2.5;
pub const HORSE_TRANSITION_SECONDS: f64 = 1.25;
pub const HORSE_DISMOUNT_OFFSET: Vec3 = Vec3::new(-0.95, 0.0, 0.0);

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Horse {
    pub id: u64,
    pub rider: Option<super::PersonId>,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HorseGait {
    Walk,
    Trot,
    #[default]
    Canter,
    Gallop,
}
impl HorseGait {
    pub fn speed(self) -> f32 {
        match self {
            Self::Walk => 1.2,
            Self::Trot => 3.0,
            Self::Canter => 4.5,
            Self::Gallop => 6.0,
        }
    }
    pub fn duration(self) -> f32 {
        match self {
            Self::Walk => 1.0,
            Self::Trot | Self::Canter => 0.7,
            Self::Gallop => 0.6,
        }
    }
    pub fn clip(self) -> &'static str {
        match self {
            Self::Walk => "horse_walk",
            Self::Trot => "horse_trot",
            Self::Canter => "horse_canter",
            Self::Gallop => "horse_gallop",
        }
    }
    pub fn rider_clip(self) -> &'static str {
        match self {
            Self::Walk => "ride_walk",
            Self::Trot => "ride_trot",
            Self::Canter => "ride_canter",
            Self::Gallop => "ride_gallop",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Walk => "Walk",
            Self::Trot => "Trot",
            Self::Canter => "Canter",
            Self::Gallop => "Gallop",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Walk => Self::Trot,
            Self::Trot => Self::Canter,
            Self::Canter => Self::Gallop,
            Self::Gallop => Self::Walk,
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorseActivity {
    Idle,
    Graze,
    Alert,
    Moving(HorseGait),
}
impl HorseActivity {
    pub fn clip(self) -> &'static str {
        match self {
            Self::Idle => "horse_idle",
            Self::Graze => "horse_graze",
            Self::Alert => "horse_alert",
            Self::Moving(g) => g.clip(),
        }
    }
    pub fn duration(self) -> f32 {
        match self {
            Self::Idle => 3.,
            Self::Graze => 4.,
            Self::Alert => 2.,
            Self::Moving(g) => g.duration(),
        }
    }
}
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct HorseAnimation {
    pub activity: HorseActivity,
    pub since: f64,
}
impl HorseAnimation {
    pub fn sample(self, now: f64) -> f32 {
        ((now - self.since).max(0.) as f32).rem_euclid(self.activity.duration())
    }
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RidingPhase {
    Mounting,
    Riding,
    Dismounting,
}
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Mounted {
    pub horse: u64,
    pub gait: HorseGait,
    pub phase: RidingPhase,
    pub since: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horse_snapshots_roundtrip_with_identity_and_animation_phase() {
        let horse = Horse {
            id: u64::MAX,
            rider: Some(super::super::PersonId(u64::MAX)),
        };
        assert_eq!(
            bincode::deserialize::<Horse>(&bincode::serialize(&horse).unwrap()).unwrap(),
            horse
        );
        for activity in [
            HorseActivity::Idle,
            HorseActivity::Graze,
            HorseActivity::Alert,
            HorseActivity::Moving(HorseGait::Walk),
            HorseActivity::Moving(HorseGait::Trot),
            HorseActivity::Moving(HorseGait::Canter),
            HorseActivity::Moving(HorseGait::Gallop),
        ] {
            let animation = HorseAnimation {
                activity,
                since: 98765.25,
            };
            assert_eq!(
                bincode::deserialize::<HorseAnimation>(&bincode::serialize(&animation).unwrap())
                    .unwrap(),
                animation
            );
            assert_eq!(animation.sample(0.), 0.);
            let sample = animation.sample(animation.since + 123.456);
            assert!((0.0..activity.duration()).contains(&sample));
        }
    }
}
