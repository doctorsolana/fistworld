//! Stable household preferences, with coherent patches instead of per-plant confetti.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PlantKind {
    Shrub,
    Perennial,
    Herbs,
}

#[derive(Clone, Copy)]
pub(super) struct GardenStyle {
    family: u64,
}

impl GardenStyle {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            family: shared::worldgen::splitmix64(seed ^ 0x71ac_395d) % 4,
        }
    }

    /// Several plants share a patch's species and dominant flower colour.
    /// An irregular open interval between patches leaves useful bare/lawn ground.
    pub(super) fn patch(&self, along: f32, length: f32, seed: u64) -> Option<u64> {
        let period = 2.35 + unit(seed) * 1.5;
        let phase = along + unit(seed.wrapping_add(61)) * period * 0.45;
        let interval = (phase / period).floor() as u64;
        let fill = [0.75, 0.64, 0.82, 0.69][self.family as usize];
        if length > 3.0 && phase % period > period * fill {
            return None;
        }
        Some(shared::worldgen::splitmix64(
            seed.wrapping_add(interval * 839),
        ))
    }

    pub(super) fn kind(&self, seed: u64, use_kind: YardUse) -> PlantKind {
        let roll = unit(seed);
        let (shrub, perennial) = match use_kind {
            YardUse::Flowers => {
                [(0.23, 0.85), (0.18, 0.78), (0.50, 0.88), (0.30, 0.73)][self.family as usize]
            }
            YardUse::Vegetables => (0.30, 0.52),
            YardUse::Laundry => (0.42, 0.72),
            YardUse::Firewood => (0.72, 0.81),
        };
        if roll < shrub {
            PlantKind::Shrub
        } else if roll < perennial {
            PlantKind::Perennial
        } else {
            PlantKind::Herbs
        }
    }

    pub(super) fn leaf_color(&self, kind: PlantKind) -> Vec3 {
        let base = match self.family {
            0 => Vec3::new(0.36, 0.51, 0.22),
            1 => Vec3::new(0.42, 0.55, 0.27),
            2 => Vec3::new(0.31, 0.47, 0.235),
            _ => Vec3::new(0.39, 0.51, 0.29),
        };
        match kind {
            PlantKind::Shrub => base,
            PlantKind::Perennial => base * 1.08,
            PlantKind::Herbs => base.lerp(Vec3::new(0.49, 0.57, 0.37), 0.40),
        }
    }

    pub(super) fn yellow(&self, patch: u64) -> bool {
        unit(patch.wrapping_add(43)) < [0.26, 0.62, 0.36, 0.18][self.family as usize]
    }
}
