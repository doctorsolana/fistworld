//! First-person weapon offsets.

use bevy::prelude::Vec3;

use super::types::WeaponType;

/// First-person muzzle offset in camera space.
/// Negative Z is forward (in front of the camera).
pub fn muzzle_offset(weapon_type: WeaponType) -> Vec3 {
    match weapon_type {
        WeaponType::Pistol => Vec3::new(0.23, -0.16, -0.65),
        WeaponType::AssaultRifle => Vec3::new(0.28, -0.18, -0.85),
        WeaponType::Shotgun => Vec3::new(0.27, -0.17, -0.92),
        WeaponType::Sniper => Vec3::new(0.25, -0.16, -1.0),
        WeaponType::Unarmed => Vec3::ZERO,
        // Right-hand grip anchor for the sword; the shield offset is the
        // LEFT-hand holder's base (mirrored X).
        WeaponType::Sword => Vec3::new(0.30, -0.22, -0.55),
        WeaponType::Shield => Vec3::new(-0.32, -0.20, -0.60),
    }
}
