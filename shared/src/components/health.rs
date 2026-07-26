use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Health component for damageable entities.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: 100.0,
            max: 100.0,
        }
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
        self.current / self.max
    }
}
