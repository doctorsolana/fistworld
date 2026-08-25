use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Every embodied character begins with this much health.
pub const CHARACTER_MAX_HEALTH: f32 = 100.0;

/// Once ten consecutive missed meals have elapsed, every further missed day
/// inflicts lethal starvation damage against the ten-point hunger floor.
pub const STARVATION_DAMAGE_PER_DAY: f32 = 10.0;

/// Stable reason attached to mortality history and inspection records.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCause {
    Starvation,
    /// Struck down by another character's weapon.
    Combat,
    Unknown,
}

impl DeathCause {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Starvation => "Starvation",
            Self::Combat => "Killed in battle",
            Self::Unknown => "Unknown",
        }
    }
}

/// Health component for damageable entities.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self::new(CHARACTER_MAX_HEALTH)
    }
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn take_damage(&mut self, amount: f32) -> bool {
        self.current = (self.current - amount).max(0.0);
        self.current <= 0.0
    }

    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }

    pub fn percentage(&self) -> f32 {
        if self.max <= f32::EPSILON {
            0.0
        } else {
            (self.current / self.max).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_starvation_damage_exhausts_the_hunger_floor() {
        let mut health = Health {
            current: 10.0,
            max: CHARACTER_MAX_HEALTH,
        };
        assert!(health.take_damage(STARVATION_DAMAGE_PER_DAY));
        assert!(health.is_dead());
    }

    #[test]
    fn percentage_is_safe_for_malformed_zero_maximum_health() {
        assert_eq!(
            Health {
                current: 0.0,
                max: 0.0
            }
            .percentage(),
            0.0
        );
    }
}
