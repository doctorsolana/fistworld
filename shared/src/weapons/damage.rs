//! Damage calculation system
//!
//! Handles damage falloff, hit zones, and armor.

use super::WeaponStats;
use serde::{Deserialize, Serialize};

/// Body zones for hit detection with different damage multipliers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HitZone {
    Head,
    #[default]
    Chest,
    Stomach,
    Arms,
    Legs,
}

impl HitZone {
    /// Get the base damage multiplier for this hit zone
    pub fn base_multiplier(&self) -> f32 {
        match self {
            HitZone::Head => 1.0, // Headshot mult applied separately
            HitZone::Chest => 1.0,
            HitZone::Stomach => 0.95,
            HitZone::Arms => 0.75,
            HitZone::Legs => 0.65,
        }
    }

    /// Determine hit zone based on relative hit position
    /// `relative_height` is 0.0 at feet, 1.0 at top of head
    pub fn from_relative_height(relative_height: f32) -> Self {
        if relative_height > 0.85 {
            HitZone::Head
        } else if relative_height > 0.65 {
            HitZone::Chest
        } else if relative_height > 0.45 {
            HitZone::Stomach
        } else if relative_height > 0.25 {
            // Could also be arms based on horizontal position
            HitZone::Stomach
        } else {
            HitZone::Legs
        }
    }
}

/// Calculate distance-based damage falloff
///
/// Returns a multiplier between min_damage_mult and 1.0
fn calculate_falloff(distance: f32, stats: &WeaponStats) -> f32 {
    if distance <= stats.damage_falloff_start {
        1.0
    } else if distance >= stats.damage_falloff_end {
        stats.min_damage_mult
    } else {
        // Linear interpolation between start and end
        let t = (distance - stats.damage_falloff_start)
            / (stats.damage_falloff_end - stats.damage_falloff_start);
        1.0 - t * (1.0 - stats.min_damage_mult)
    }
}

/// Calculate final damage for a hit
///
/// Factors in:
/// - Base weapon damage
/// - Distance falloff
/// - Hit zone multiplier
/// - Headshot multiplier (if head)
pub fn calculate_damage(stats: &WeaponStats, distance: f32, hit_zone: HitZone) -> f32 {
    let base = stats.damage;
    let falloff = calculate_falloff(distance, stats);
    let zone_mult = hit_zone.base_multiplier();

    let headshot_mult = if hit_zone == HitZone::Head {
        stats.headshot_mult
    } else {
        1.0
    };

    base * falloff * zone_mult * headshot_mult
}

/// Calculate damage with armor reduction
///
/// Armor absorbs a percentage of damage and is depleted
pub fn calculate_damage_with_armor(
    stats: &WeaponStats,
    distance: f32,
    hit_zone: HitZone,
    armor: f32,
    armor_protection: f32, // 0.0 to 1.0, how much damage armor absorbs
) -> (f32, f32) {
    let raw_damage = calculate_damage(stats, distance, hit_zone);

    if armor <= 0.0 {
        return (raw_damage, 0.0);
    }

    // Calculate absorbed damage
    let absorbed = raw_damage * armor_protection;
    let armor_damage = absorbed.min(armor);
    let actual_absorbed = armor_damage; // Can't absorb more than armor has

    let final_damage = raw_damage - actual_absorbed;
    let remaining_armor = armor - armor_damage;

    (final_damage.max(0.0), remaining_armor.max(0.0))
}

/// Result of a damage calculation
#[derive(Clone, Debug)]
pub struct DamageResult {
    pub damage: f32,
    pub hit_zone: HitZone,
    pub is_headshot: bool,
    pub is_kill: bool,
    pub distance: f32,
}

impl DamageResult {
    pub fn new(damage: f32, hit_zone: HitZone, distance: f32, victim_health: f32) -> Self {
        Self {
            damage,
            hit_zone,
            is_headshot: hit_zone == HitZone::Head,
            is_kill: damage >= victim_health,
            distance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weapons::WeaponType;

    #[test]
    fn test_no_falloff_close_range() {
        let stats = WeaponType::AssaultRifle.stats();
        let damage = calculate_damage(&stats, 10.0, HitZone::Chest);
        // Close range, no falloff
        assert!((damage - stats.damage).abs() < 0.01);
    }

    #[test]
    fn test_falloff_at_max_range() {
        let stats = WeaponType::AssaultRifle.stats();
        let damage = calculate_damage(&stats, stats.damage_falloff_end, HitZone::Chest);
        let expected = stats.damage * stats.min_damage_mult;
        assert!((damage - expected).abs() < 0.01);
    }

    #[test]
    fn test_headshot_multiplier() {
        let stats = WeaponType::Sniper.stats();
        let body_damage = calculate_damage(&stats, 100.0, HitZone::Chest);
        let head_damage = calculate_damage(&stats, 100.0, HitZone::Head);

        assert!((head_damage / body_damage - stats.headshot_mult).abs() < 0.01);
    }
}
