//! Off-hand shield rendering + melee swing animation poses.
//!
//! The shield rides the left hand whenever `EquippedWeapon.offhand_shield`
//! is set (server-derived from the hotbar); holding block raises it to the
//! center of the view. Swing poses are shared between the first-person
//! holder and the third-person weapon entities.

use bevy::prelude::*;
use shared::components::{EquippedWeapon, LocalPlayer, Player, PlayerMeleeState};
use shared::weapons::WeaponType;
use std::collections::HashMap;

use crate::input::{CameraMode, InputState};

use super::third_person::{RemoteThirdPersonWeapon, ThirdPersonWeapon};
use super::WeaponModelAssets;

/// First-person off-hand shield holder (child of the camera).
#[derive(Component)]
pub struct FirstPersonOffhandShield;

/// Third-person off-hand shield, attached to a player entity.
#[derive(Component)]
pub struct ThirdPersonOffhandShield {
    pub owner: Entity,
}

#[derive(Resource, Default)]
pub struct OffhandShieldIndex {
    pub by_owner: HashMap<Entity, Entity>,
}

const FP_SHIELD_REST: Vec3 = Vec3::new(-0.30, -0.24, -0.55);
const FP_SHIELD_RAISED: Vec3 = Vec3::new(-0.07, -0.13, -0.48);
const TP_SHIELD_REST: Vec3 = Vec3::new(-0.25, 0.12, -0.28);
const TP_SHIELD_RAISED: Vec3 = Vec3::new(-0.05, 0.25, -0.42);

/// Melee swing pose for normalized progress 0..1: a full diagonal cut that
/// crosses the screen. Cocked high over the shoulder, then a violent
/// edge-first slash from top-right to bottom-left (mirrored on alternate
/// swings), finishing with a damped-spring follow-through that overshoots
/// past rest before settling.
pub const SWING_WINDUP_END: f32 = 0.20;
pub const SWING_STRIKE_END: f32 = 0.46;

pub fn swing_pose(progress: f32, mirror: bool) -> (Vec3, Quat) {
    fn smooth(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    // Pose targets (yaw, pitch, roll, offset). Sign convention (camera
    // space, blade along -Z): +yaw sweeps the TIP left, +pitch sweeps the
    // TIP up. Rotation carries the arc (the tip travels ~2m); the handle
    // translation FOLLOWS the same direction — opposing signs here cancel
    // at the tip and make the handle orbit a stationary point instead.
    let windup = (-0.75f32, 0.85f32, 0.55f32, Vec3::new(0.18, 0.16, 0.10));
    let strike = (1.55f32, -0.60f32, -0.95f32, Vec3::new(-0.34, -0.24, -0.28));

    let (yaw, pitch, roll, offset) = if progress < SWING_WINDUP_END {
        // Quick cock over the shoulder, easing out at the top.
        let t = smooth(progress / SWING_WINDUP_END);
        (windup.0 * t, windup.1 * t, windup.2 * t, windup.3 * t)
    } else if progress < SWING_STRIKE_END {
        // The cut: hard ease-in so the blade explodes through the middle
        // frames — most of the arc happens in a few frames.
        let t = (progress - SWING_WINDUP_END) / (SWING_STRIKE_END - SWING_WINDUP_END);
        let t = t * t * t * (10.0 + t * (-15.0 + 6.0 * t)); // smootherstep, steep center
        (
            windup.0 + (strike.0 - windup.0) * t,
            windup.1 + (strike.1 - windup.1) * t,
            windup.2 + (strike.2 - windup.2) * t,
            windup.3.lerp(strike.3, t),
        )
    } else {
        // Follow-through: damped spring back to rest, overshooting slightly
        // to the opposite side so the arm visibly carries momentum.
        let s = (progress - SWING_STRIKE_END) / (1.0 - SWING_STRIKE_END);
        let spring = (-4.6 * s).exp() * (7.4 * s).cos();
        (
            strike.0 * spring,
            strike.1 * spring,
            strike.2 * spring,
            strike.3 * spring,
        )
    };

    let m = if mirror { -1.0 } else { 1.0 };
    (
        Vec3::new(offset.x * m, offset.y, offset.z),
        Quat::from_euler(EulerRot::YXZ, yaw * m, pitch, roll * m),
    )
}

/// Spawn/despawn the first-person off-hand shield as loadout changes.
pub fn update_first_person_offhand_shield(
    mut commands: Commands,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    existing: Query<Entity, With<FirstPersonOffhandShield>>,
    camera: Query<Entity, With<Camera3d>>,
    weapon_models: Option<Res<WeaponModelAssets>>,
) {
    let wants_shield = local_player
        .single()
        .map(|weapon| weapon.offhand_shield && weapon.weapon_type != WeaponType::Shield)
        .unwrap_or(false);

    if !wants_shield {
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }
    let (Ok(camera_entity), Some(assets)) = (camera.single(), weapon_models) else {
        return;
    };
    let Some(scene) = assets.scenes.get(&WeaponType::Shield) else {
        return;
    };

    let shield = commands
        .spawn((
            FirstPersonOffhandShield,
            Transform::from_translation(FP_SHIELD_REST),
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ))
        .id();
    commands.entity(shield).with_children(|parent| {
        parent.spawn((
            SceneRoot(scene.clone()),
            Transform::from_scale(Vec3::splat(0.35)),
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));
    });
    commands.entity(camera_entity).add_child(shield);
}

/// Raise/lower the first-person shield with block input; hide it outside
/// first-person on-foot play.
pub fn animate_first_person_offhand_shield(
    time: Res<Time>,
    input_state: Res<InputState>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    mut shields: Query<
        (&mut Transform, &mut Visibility),
        With<FirstPersonOffhandShield>,
    >,
    mut raise: Local<f32>,
) {
    let blocking = input_state.blocking_held
        || local_player
            .single()
            .map(|weapon| weapon.blocking)
            .unwrap_or(false);
    let target = if blocking { 1.0 } else { 0.0 };
    let rate = if blocking { 14.0 } else { 8.0 };
    *raise += (target - *raise) * (rate * time.delta_secs()).clamp(0.0, 1.0);

    let visible = input_state.camera_mode == CameraMode::FirstPerson && !input_state.in_vehicle;
    for (mut transform, mut visibility) in shields.iter_mut() {
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let t = time.elapsed_secs();
        let mut pos = FP_SHIELD_REST.lerp(FP_SHIELD_RAISED, *raise);
        // Idle sway, damped while raised (braced arm holds steadier).
        let sway = 1.0 - *raise * 0.7;
        pos.x += (t * 1.1).sin() * 0.004 * sway;
        pos.y += (t * 0.9).cos() * 0.003 * sway;
        transform.translation = pos;
        // Face the shield toward the camera center as it raises.
        transform.rotation = Quat::from_euler(
            EulerRot::YXZ,
            0.55 * (1.0 - *raise),
            -0.1 * *raise,
            0.15 * (1.0 - *raise),
        );
    }
}

/// Attach third-person shields to every player whose loadout carries one in
/// the off-hand (mirrors update_remote_third_person_weapons).
pub fn update_third_person_offhand_shields(
    mut commands: Commands,
    mut index: ResMut<OffhandShieldIndex>,
    players: Query<(Entity, &EquippedWeapon), (With<Player>, With<GlobalTransform>)>,
    mut removed_players: RemovedComponents<Player>,
    weapon_models: Option<Res<WeaponModelAssets>>,
) {
    for removed in removed_players.read() {
        // The shield is a child of the player and despawns recursively with
        // it; only clean the index (despawning again would warn).
        index.by_owner.remove(&removed);
    }

    let Some(assets) = weapon_models else { return };
    let Some(scene) = assets.scenes.get(&WeaponType::Shield) else {
        return;
    };

    for (player_entity, weapon) in players.iter() {
        let wants = weapon.offhand_shield && weapon.weapon_type != WeaponType::Shield;
        let existing = index.by_owner.get(&player_entity).copied();
        match (wants, existing) {
            (false, Some(shield)) => {
                index.by_owner.remove(&player_entity);
                commands.entity(shield).despawn();
            }
            (true, None) => {
                let shield = commands
                    .spawn((
                        ThirdPersonOffhandShield {
                            owner: player_entity,
                        },
                        Transform::from_translation(TP_SHIELD_REST),
                        GlobalTransform::default(),
                        Visibility::Inherited,
                        InheritedVisibility::default(),
                    ))
                    .id();
                commands.entity(shield).with_children(|parent| {
                    parent.spawn((
                        SceneRoot(scene.clone()),
                        Transform::from_scale(Vec3::splat(0.6)),
                        GlobalTransform::default(),
                        Visibility::Inherited,
                        InheritedVisibility::default(),
                    ));
                });
                commands.entity(player_entity).add_child(shield);
                index.by_owner.insert(player_entity, shield);
            }
            _ => {}
        }
    }
}

/// Third-person block pose: slide the shield to the front when its owner is
/// blocking (replicated state, so it works for remote players too). Also
/// hides the LOCAL player's shield outside third-person on-foot play.
pub fn animate_third_person_offhand_shields(
    time: Res<Time>,
    input_state: Res<InputState>,
    local_player: Query<Entity, With<LocalPlayer>>,
    owners: Query<&EquippedWeapon, With<Player>>,
    mut shields: Query<(&ThirdPersonOffhandShield, &mut Transform, &mut Visibility)>,
) {
    let local_entity = local_player.single().ok();
    let local_tp_visible =
        input_state.camera_mode == CameraMode::ThirdPerson && !input_state.in_vehicle;
    let dt = (10.0 * time.delta_secs()).clamp(0.0, 1.0);
    for (shield, mut transform, mut visibility) in shields.iter_mut() {
        let desired = if Some(shield.owner) == local_entity && !local_tp_visible {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != desired {
            *visibility = desired;
        }
        let blocking = owners
            .get(shield.owner)
            .map(|weapon| weapon.blocking)
            .unwrap_or(false);
        let target = if blocking {
            TP_SHIELD_RAISED
        } else {
            TP_SHIELD_REST
        };
        transform.translation = transform.translation.lerp(target, dt);
        let target_rot = if blocking {
            Quat::from_rotation_x(-0.15)
        } else {
            Quat::from_euler(EulerRot::YXZ, 0.6, 0.0, 0.2)
        };
        transform.rotation = transform.rotation.slerp(target_rot, dt);
    }
}

/// Swing third-person weapons from the replicated melee state (drives both
/// the local player in third person and remote players).
pub fn animate_third_person_melee(
    local_player: Query<
        (Option<&PlayerMeleeState>, &EquippedWeapon),
        With<LocalPlayer>,
    >,
    owner_states: Query<(Option<&PlayerMeleeState>, &EquippedWeapon), With<Player>>,
    mut local_weapons: Query<
        &mut Transform,
        (With<ThirdPersonWeapon>, Without<RemoteThirdPersonWeapon>),
    >,
    mut remote_weapons: Query<
        (&RemoteThirdPersonWeapon, &mut Transform),
        Without<ThirdPersonWeapon>,
    >,
) {
    // Base pose must match spawn_third_person_weapon.
    let base_pos = Vec3::new(0.2, 0.15, -0.35);
    let base_rot = Quat::from_rotation_y(-0.1);

    let apply = |state: Option<(Option<&PlayerMeleeState>, &EquippedWeapon)>,
                 transform: &mut Mut<Transform>| {
        // Only melee weapons swing: a leftover PlayerMeleeState after a
        // mid-swing hotbar swap must not wave the rifle around.
        let swing = match state {
            Some((Some(melee), equipped))
                if equipped.weapon_type.is_melee() && melee.duration > 0.0 =>
            {
                let progress = (1.0 - melee.timer / melee.duration).clamp(0.0, 1.0);
                Some(swing_pose(progress, false))
            }
            _ => None,
        };
        match swing {
            Some((offset, rot)) => {
                transform.translation = base_pos + offset;
                transform.rotation = base_rot * rot;
            }
            None => {
                // Reset once after a swing; skip the write when already at
                // rest so idle weapons don't dirty Transform every frame.
                if transform.translation.distance_squared(base_pos) > 1e-6
                    || transform.rotation.angle_between(base_rot) > 1e-3
                {
                    transform.translation = base_pos;
                    transform.rotation = base_rot;
                }
            }
        }
    };

    let local_state = local_player.single().ok();
    for mut transform in local_weapons.iter_mut() {
        apply(local_state, &mut transform);
    }
    for (remote, mut transform) in remote_weapons.iter_mut() {
        apply(owner_states.get(remote.owner).ok(), &mut transform);
    }
}
