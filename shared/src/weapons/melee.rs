//! Melee swing and shield-block math, shared so the server authoritatively
//! computes hits/blocks and the client can predict animation timing.

use bevy::prelude::*;

use super::WeaponType;

/// Tuning for a melee weapon's swing.
#[derive(Clone, Copy, Debug)]
pub struct MeleeStats {
    /// Damage per landed swing.
    pub damage: f32,
    /// Reach from the attacker's eye, meters.
    pub range: f32,
    /// Full horizontal arc of the swing, radians.
    pub arc: f32,
    /// Seconds between swings (server-enforced, survives hotbar swaps).
    pub cooldown: f32,
    /// Seconds the swing animation plays (drives PlayerMeleeState).
    pub swing_duration: f32,
    /// Horizontal knockback impulse applied to NPC victims (m/s of dv).
    pub knockback: f32,
    /// How many victims one swing can hit (cleave).
    pub max_targets: u32,
}

impl MeleeStats {
    pub fn for_weapon(weapon: WeaponType) -> Option<Self> {
        match weapon {
            WeaponType::Sword => Some(Self {
                damage: 42.0,
                range: 2.7,
                arc: 110f32.to_radians(),
                cooldown: 0.55,
                swing_duration: 0.38,
                knockback: 5.0,
                max_targets: 3,
            }),
            // Shield bash: short, slow, weak, but shoves hard.
            WeaponType::Shield => Some(Self {
                damage: 10.0,
                range: 1.9,
                arc: 70f32.to_radians(),
                cooldown: 0.9,
                swing_duration: 0.35,
                knockback: 9.0,
                max_targets: 1,
            }),
            _ => None,
        }
    }
}

/// Full frontal arc within which a raised shield blocks incoming damage.
pub const BLOCK_ARC: f32 = 2.0943952; // 120 degrees
/// Damage multiplier for BULLETS stopped by a raised shield (chip damage).
pub const BLOCK_BULLET_DAMAGE_MULT: f32 = 0.15;
/// Damage multiplier for MELEE stopped by a raised shield (perfect block).
pub const BLOCK_MELEE_DAMAGE_MULT: f32 = 0.0;

/// Player facing for a replicated yaw — matches the server movement
/// convention (`server/src/physics/dynamic_actors.rs`): yaw 0 faces -Z.
#[inline]
pub fn facing_from_yaw(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

/// Does a raised shield stop an attack?
///
/// `victim_yaw`: the blocker's replicated facing yaw.
/// `attacker_pos` / `victim_pos`: world positions; only the horizontal
/// direction matters. Attacks from inside the frontal [`BLOCK_ARC`] are
/// blocked; flanks and backstabs go through.
pub fn is_attack_blocked(victim_yaw: f32, victim_pos: Vec3, attacker_pos: Vec3) -> bool {
    let mut to_attacker = attacker_pos - victim_pos;
    to_attacker.y = 0.0;
    let Some(to_attacker) = to_attacker.try_normalize() else {
        // Attacker directly above/below: the shield doesn't cover that.
        return false;
    };
    let facing = facing_from_yaw(victim_yaw);
    to_attacker.dot(facing) >= (BLOCK_ARC * 0.5).cos()
}

/// Test one capsule-shaped target against a melee swing.
///
/// `eye`: attacker's eye position. `dir`: aim direction (normalized).
/// The target is the vertical capsule `seg_bottom..seg_top` with `radius`.
/// Returns the distance to the target's nearest point when it is inside the
/// swing's range + arc, for nearest-first sorting.
pub fn melee_target_in_arc(
    eye: Vec3,
    dir: Vec3,
    seg_bottom: Vec3,
    seg_top: Vec3,
    radius: f32,
    stats: &MeleeStats,
) -> Option<f32> {
    // Nearest point on the target's core segment to the attacker's eye.
    let seg = seg_top - seg_bottom;
    let t = if seg.length_squared() < 1e-6 {
        0.0
    } else {
        ((eye - seg_bottom).dot(seg) / seg.length_squared()).clamp(0.0, 1.0)
    };
    let nearest = seg_bottom + seg * t;
    let to_target = nearest - eye;
    let dist = (to_target.length() - radius).max(0.0);
    if dist > stats.range {
        return None;
    }

    // Horizontal arc test against the aim direction.
    let dir_h = Vec3::new(dir.x, 0.0, dir.z);
    let to_h = Vec3::new(to_target.x, 0.0, to_target.z);
    // Target essentially on top of the attacker: always in arc.
    if to_h.length() < 0.35 {
        return Some(dist);
    }
    let (Some(dir_h), Some(to_h)) = (dir_h.try_normalize(), to_h.try_normalize()) else {
        return Some(dist);
    };
    (dir_h.dot(to_h) >= (stats.arc * 0.5).cos()).then_some(dist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_covers_front_not_back() {
        // Yaw 0 faces -Z: attacker at -Z is frontal, +Z is a backstab.
        let victim = Vec3::new(0.0, 0.0, 0.0);
        assert!(is_attack_blocked(0.0, victim, Vec3::new(0.0, 0.0, -5.0)));
        assert!(!is_attack_blocked(0.0, victim, Vec3::new(0.0, 0.0, 5.0)));
        // 50 degrees off-center: inside the 120-degree arc.
        let off = Vec3::new(-(50f32.to_radians().sin()) * 5.0, 0.0, -(50f32.to_radians().cos()) * 5.0);
        assert!(is_attack_blocked(0.0, victim, off));
        // 70 degrees off-center: outside.
        let flank = Vec3::new(-(70f32.to_radians().sin()) * 5.0, 0.0, -(70f32.to_radians().cos()) * 5.0);
        assert!(!is_attack_blocked(0.0, victim, flank));
        // Height difference must not affect the horizontal test.
        assert!(is_attack_blocked(0.0, victim, Vec3::new(0.0, 3.0, -5.0)));
    }

    #[test]
    fn block_follows_yaw() {
        // Yaw PI/2 faces -X (facing = (-sin, 0, -cos)).
        let victim = Vec3::ZERO;
        let yaw = std::f32::consts::FRAC_PI_2;
        assert!(is_attack_blocked(yaw, victim, Vec3::new(-5.0, 0.0, 0.0)));
        assert!(!is_attack_blocked(yaw, victim, Vec3::new(5.0, 0.0, 0.0)));
    }

    #[test]
    fn sword_arc_hits_front_only_within_range() {
        let stats = MeleeStats::for_weapon(WeaponType::Sword).unwrap();
        let eye = Vec3::new(0.0, 1.6, 0.0);
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let capsule = |x: f32, z: f32| {
            (
                Vec3::new(x, 0.0, z),
                Vec3::new(x, 1.8, z),
                0.35,
            )
        };

        // Directly ahead, in range.
        let (b, t, r) = capsule(0.0, -2.0);
        assert!(melee_target_in_arc(eye, dir, b, t, r, &stats).is_some());
        // Ahead but out of range.
        let (b, t, r) = capsule(0.0, -4.0);
        assert!(melee_target_in_arc(eye, dir, b, t, r, &stats).is_none());
        // 40 degrees off: inside the 110-degree arc.
        let (b, t, r) = capsule(-1.3, -1.55);
        assert!(melee_target_in_arc(eye, dir, b, t, r, &stats).is_some());
        // Behind: never.
        let (b, t, r) = capsule(0.0, 2.0);
        assert!(melee_target_in_arc(eye, dir, b, t, r, &stats).is_none());
        // Nearest-first ordering: closer target reports smaller distance.
        let (b1, t1, r1) = capsule(0.0, -1.0);
        let (b2, t2, r2) = capsule(0.0, -2.2);
        let d1 = melee_target_in_arc(eye, dir, b1, t1, r1, &stats).unwrap();
        let d2 = melee_target_in_arc(eye, dir, b2, t2, r2, &stats).unwrap();
        assert!(d1 < d2);
    }
}
