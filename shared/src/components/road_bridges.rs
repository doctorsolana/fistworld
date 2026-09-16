//! A paid, authoritative timber crossing. Terrain recipes remain immutable;
//! this small regional component supplies the same walking surface to both binaries.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The lowest side truss sits at most this far below the walking surface.
pub const ROAD_BRIDGE_UNDERDECK_DEPTH: f32 = 1.05;
/// Accommodate the shipped dinghy's 3.65 m mast above water, hull swell,
/// structural depth and a visible margin across the full water channel.
pub const ROAD_BRIDGE_WATER_CLEARANCE: f32 = 6.0;

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RoadBridge {
    /// Dry, ground-level ends of the two approach ramps.
    pub start: Vec3,
    pub end: Vec3,
    /// Top of the level deck, above the local river (not global sea level).
    pub deck_height: f32,
    pub ramp_length: f32,
    pub width: f32,
    /// Survey reservations and supplied work sites never grant walking access.
    pub built: bool,
}

impl RoadBridge {
    pub fn length(&self) -> f32 {
        self.start.xz().distance(self.end.xz())
    }

    pub fn valid(&self) -> bool {
        let length = self.length();
        self.start.is_finite()
            && self.end.is_finite()
            && self.deck_height.is_finite()
            && self.ramp_length.is_finite()
            && self.width.is_finite()
            && (4.0..=120.0).contains(&length)
            && (2.0..=6.0).contains(&self.width)
            && self.ramp_length >= 2.0
            && self.ramp_length * 2.0 <= length
            && self.deck_height >= self.start.y.max(self.end.y)
            && (self.deck_height - self.start.y.min(self.end.y)) / self.ramp_length <= 0.45
    }

    pub fn midpoint(&self) -> Vec3 {
        (self.start + self.end) * 0.5
    }

    /// Height along the profile, also usable by worksite and mesh generation.
    pub fn surface_height(&self, distance: f32) -> f32 {
        let length = self.length();
        let distance = distance.clamp(0.0, length);
        if distance < self.ramp_length {
            self.start.y + (self.deck_height - self.start.y) * distance / self.ramp_length
        } else if distance > length - self.ramp_length {
            self.end.y + (self.deck_height - self.end.y) * (length - distance) / self.ramp_length
        } else {
            self.deck_height
        }
    }

    /// Conservative centre clearance. Never expands the deck into nearby water.
    pub fn height_at(&self, point: Vec2, clearance: f32) -> Option<f32> {
        if !self.built || !self.valid() || !point.is_finite() || !clearance.is_finite() {
            return None;
        }
        let delta = point - self.start.xz();
        let direction = (self.end.xz() - self.start.xz()) / self.length();
        let along = delta.dot(direction);
        let side = delta.perp_dot(direction).abs();
        let half_width = self.width * 0.5 - clearance.max(0.0);
        (half_width >= 0.0 && along >= 0.0 && along <= self.length() && side <= half_width)
            .then(|| self.surface_height(along))
    }

    pub fn wood_required(&self) -> u32 {
        (self.length() * self.width * 0.3).ceil().max(12.0) as u32
    }

    pub fn stone_required(&self) -> u32 {
        (self.width * 2.0).ceil().max(4.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crossing() -> RoadBridge {
        RoadBridge {
            start: Vec3::ZERO,
            end: Vec3::new(40.0, 1.0, 0.0),
            deck_height: 3.0,
            ramp_length: 8.0,
            width: 3.6,
            built: true,
        }
    }

    #[test]
    fn only_finished_deck_with_body_clearance_supports_walkers() {
        let mut bridge = crossing();
        assert_eq!(bridge.height_at(Vec2::new(20.0, 0.0), 0.4), Some(3.0));
        assert_eq!(bridge.height_at(Vec2::new(20.0, 1.5), 0.4), None);
        assert_eq!(bridge.height_at(Vec2::new(-0.1, 0.0), 0.0), None);
        assert_eq!(bridge.height_at(Vec2::new(40.1, 0.0), 0.0), None);
        bridge.built = false;
        assert_eq!(bridge.height_at(Vec2::new(20.0, 0.0), 0.0), None);
    }

    #[test]
    fn ramps_meet_ground_and_plateau_without_steps() {
        let bridge = crossing();
        assert!(bridge.valid());
        assert_eq!(bridge.surface_height(0.0), bridge.start.y);
        assert_eq!(bridge.surface_height(40.0), bridge.end.y);
        for d in [8.0, 32.0] {
            assert!(
                (bridge.surface_height(d - 0.001) - bridge.surface_height(d + 0.001)).abs() < 0.001
            );
        }
        let mut invalid = bridge.clone();
        invalid.ramp_length = 0.0;
        assert!(!invalid.valid());
        assert!(invalid.height_at(Vec2::ZERO, 0.0).is_none());
        let bytes = bincode::serialize(&bridge).unwrap();
        assert_eq!(bincode::deserialize::<RoadBridge>(&bytes).unwrap(), bridge);
    }
}
