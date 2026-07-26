//! Weapon enums and stats.

use crate::items::ItemType;
use serde::{Deserialize, Serialize};

/// Available weapon types.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum WeaponType {
    #[default]
    Pistol,
    AssaultRifle,
    Sniper,
    Shotgun,
    /// No weapon equipped
    Unarmed,
    // NOTE: profiles persist this enum via bincode — append variants only.
    Sword,
    Shield,
}

/// Complete stats for a weapon type.
#[derive(Clone, Debug)]
pub struct WeaponStats {
    /// Base damage per bullet
    pub damage: f32,
    /// Fire rate in rounds per second
    pub fire_rate: f32,
    /// Bullet muzzle velocity in m/s
    pub bullet_speed: f32,
    /// Magazine capacity
    pub magazine_size: u32,
    /// Reload time in seconds
    pub reload_time: f32,
    /// Spread cone (radians) when hip-firing
    pub spread_hip: f32,
    /// Spread cone (radians) when aiming down sights
    pub spread_ads: f32,
    /// Vertical recoil per shot (radians)
    pub recoil_vertical: f32,
    /// Horizontal recoil per shot (radians)
    pub recoil_horizontal: f32,
    /// Distance (m) where damage falloff begins
    pub damage_falloff_start: f32,
    /// Distance (m) where damage reaches minimum
    pub damage_falloff_end: f32,
    /// Minimum damage multiplier at max range (0.5 = 50% damage)
    pub min_damage_mult: f32,
    /// Headshot damage multiplier
    pub headshot_mult: f32,
    /// Number of pellets (for shotguns, 1 for other weapons)
    pub pellet_count: u32,
}

impl WeaponType {
    /// Get the stats for this weapon type.
    pub fn stats(&self) -> WeaponStats {
        match self {
            WeaponType::Pistol => WeaponStats {
                damage: 25.0,
                fire_rate: 6.0,
                bullet_speed: 380.0,
                magazine_size: 15,
                reload_time: 1.5,
                spread_hip: 0.018,
                spread_ads: 0.001,
                recoil_vertical: 0.018,
                recoil_horizontal: 0.008,
                damage_falloff_start: 25.0,
                damage_falloff_end: 100.0,
                min_damage_mult: 0.55,
                headshot_mult: 2.0,
                pellet_count: 1,
            },
            WeaponType::AssaultRifle => WeaponStats {
                damage: 33.0,
                fire_rate: 11.0,
                bullet_speed: 880.0,
                magazine_size: 30,
                reload_time: 2.3,
                spread_hip: 0.022,
                spread_ads: 0.0005,
                recoil_vertical: 0.022,
                recoil_horizontal: 0.012,
                damage_falloff_start: 80.0,
                damage_falloff_end: 500.0,
                min_damage_mult: 0.65,
                headshot_mult: 2.3,
                pellet_count: 1,
            },
            WeaponType::Sniper => WeaponStats {
                damage: 85.0,
                fire_rate: 0.7,
                bullet_speed: 1000.0,
                magazine_size: 5,
                reload_time: 3.8,
                spread_hip: 0.03,
                spread_ads: 0.0,
                recoil_vertical: 0.07,
                recoil_horizontal: 0.015,
                damage_falloff_start: 150.0,
                damage_falloff_end: 900.0,
                min_damage_mult: 0.75,
                headshot_mult: 2.8,
                pellet_count: 1,
            },
            WeaponType::Shotgun => WeaponStats {
                damage: 18.0,
                fire_rate: 1.2,
                bullet_speed: 350.0,
                magazine_size: 5,
                reload_time: 0.5,
                spread_hip: 0.05,
                spread_ads: 0.025,
                recoil_vertical: 0.05,
                recoil_horizontal: 0.02,
                damage_falloff_start: 8.0,
                damage_falloff_end: 35.0,
                min_damage_mult: 0.25,
                headshot_mult: 1.5,
                pellet_count: 9,
            },
            // Melee weapons share the inert gun profile: magazine 0 keeps
            // can_fire() permanently false, so the bullet path ignores them.
            WeaponType::Unarmed | WeaponType::Sword | WeaponType::Shield => WeaponStats {
                damage: 0.0,
                fire_rate: 0.0,
                bullet_speed: 0.0,
                magazine_size: 0,
                reload_time: 0.0,
                spread_hip: 0.0,
                spread_ads: 0.0,
                recoil_vertical: 0.0,
                recoil_horizontal: 0.0,
                damage_falloff_start: 0.0,
                damage_falloff_end: 0.0,
                min_damage_mult: 0.0,
                headshot_mult: 0.0,
                pellet_count: 1,
            },
        }
    }

    /// Melee weapons swing instead of firing; they have no ammo or reload.
    pub fn is_melee(&self) -> bool {
        matches!(self, WeaponType::Sword | WeaponType::Shield)
    }

    /// Shields grant hold-to-block (in the main hand or the off-hand).
    pub fn is_shield(&self) -> bool {
        matches!(self, WeaponType::Shield)
    }

    /// Swing/block tuning for melee weapons.
    pub fn melee_stats(&self) -> Option<crate::weapons::melee::MeleeStats> {
        crate::weapons::melee::MeleeStats::for_weapon(*self)
    }

    /// Get the fire cooldown in seconds.
    pub fn fire_cooldown(&self) -> f32 {
        1.0 / self.stats().fire_rate
    }

    /// Get reload duration in seconds (shotgun uses per-shell time).
    pub fn reload_duration(&self, shells_to_load: u32) -> f32 {
        let base = self.stats().reload_time.max(0.0);
        if base <= 0.0 || shells_to_load == 0 {
            return 0.0;
        }
        if *self == WeaponType::Shotgun {
            let per_shell = base * shells_to_load as f32;
            per_shell.max(self.fire_cooldown())
        } else {
            base
        }
    }

    /// Get the ammo ItemType for this weapon.
    pub fn ammo_type(&self) -> ItemType {
        match self {
            WeaponType::Pistol => ItemType::RifleAmmo,
            WeaponType::AssaultRifle => ItemType::RifleAmmo,
            WeaponType::Sniper => ItemType::SniperRounds,
            WeaponType::Shotgun => ItemType::ShotgunShells,
            // Fallback only — melee/unarmed never reload; HUD gates on is_melee().
            WeaponType::Unarmed | WeaponType::Sword | WeaponType::Shield => ItemType::RifleAmmo,
        }
    }

    /// Convert this weapon into an inventory item (None for Unarmed).
    pub fn as_item_type(&self) -> Option<ItemType> {
        match self {
            WeaponType::Unarmed => None,
            other => Some(ItemType::Weapon(*other)),
        }
    }
}

impl Default for WeaponStats {
    fn default() -> Self {
        WeaponType::Pistol.stats()
    }
}
