use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use shared::components::{EquippedWeapon, LocalPlayer, PlayerWaterState};
use shared::weapons::WeaponType;

use crate::input::{CameraMode, InputState};
use crate::weapons::{ReloadState, ShootingState};

use super::WeaponModelAssets;

/// Marker for the first-person weapon model.
#[derive(Component)]
pub struct FirstPersonWeapon;

/// Marker for first-person weapon meshes that have had world shadows disabled.
#[derive(Component)]
pub(crate) struct FirstPersonWeaponShadowDisabled;

/// Resource tracking which weapon model is currently shown.
#[derive(Resource, Default)]
pub struct CurrentWeaponView {
    pub weapon_type: Option<WeaponType>,
}

/// Spawn or update the first-person weapon model.
pub fn update_first_person_weapon(
    mut commands: Commands,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    camera: Query<Entity, With<Camera3d>>,
    existing_weapon: Query<Entity, With<FirstPersonWeapon>>,
    weapon_models: Option<Res<WeaponModelAssets>>,
    mut current_view: ResMut<CurrentWeaponView>,
    input_state: Res<InputState>,
) {
    let Some(weapon) = local_player.iter().next() else {
        return;
    };

    let Some(camera_entity) = camera.iter().next() else {
        return;
    };

    // Hide weapon in third-person or vehicle.
    let should_show = input_state.camera_mode == CameraMode::FirstPerson
        && !input_state.in_vehicle
        && weapon.weapon_type != WeaponType::Unarmed;

    // Check if we need to change the model.
    let needs_update = current_view.weapon_type != Some(weapon.weapon_type);

    // Despawn old weapon if changing or hiding.
    if needs_update || !should_show {
        for entity in existing_weapon.iter() {
            commands.entity(entity).despawn();
        }
        current_view.weapon_type = None;
    }

    if !should_show {
        return;
    }

    // Spawn new weapon model if needed.
    if needs_update {
        spawn_weapon_model(
            &mut commands,
            weapon_models.as_deref(),
            weapon.weapon_type,
            camera_entity,
        );
        current_view.weapon_type = Some(weapon.weapon_type);
    }
}

/// Spawn the 3D weapon model attached to the camera.
fn spawn_weapon_model(
    commands: &mut Commands,
    weapon_models: Option<&WeaponModelAssets>,
    weapon_type: WeaponType,
    camera_entity: Entity,
) {
    let Some(assets) = weapon_models else { return };
    let Some(scene) = assets.scenes.get(&weapon_type) else {
        return;
    };

    // Position in bottom-right of view (camera-relative).
    let base_offset = Vec3::new(0.25, -0.2, -0.4);

    let weapon_entity = commands
        .spawn((
            FirstPersonWeapon,
            Transform::from_translation(base_offset),
            // Explicit GlobalTransform avoids B0004 warnings when child meshes are spawned immediately.
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ))
        .id();

    let model_scale = 0.35;
    commands.entity(weapon_entity).with_children(|parent| {
        parent.spawn((
            SceneRoot(scene.clone()),
            GlobalTransform::default(),
            Transform::from_scale(Vec3::splat(model_scale)),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));
    });

    // Make weapon a child of camera so it follows view.
    commands.entity(camera_entity).add_child(weapon_entity);
}

/// First-person weapon models are presentation geometry, not world geometry.
/// Letting them cast or receive sun shadows causes crawling artifacts over both the gun and nearby ground.
pub fn disable_first_person_weapon_shadows(
    mut commands: Commands,
    weapons: Query<Entity, With<FirstPersonWeapon>>,
    children: Query<&Children>,
    pending_meshes: Query<Entity, (With<Mesh3d>, Without<FirstPersonWeaponShadowDisabled>)>,
) {
    let mut stack = Vec::new();
    for weapon_entity in weapons.iter() {
        stack.clear();
        stack.push(weapon_entity);

        while let Some(entity) = stack.pop() {
            if let Ok(children) = children.get(entity) {
                stack.extend(children.iter());
            }

            if pending_meshes.get(entity).is_ok() {
                commands.entity(entity).insert((
                    NotShadowCaster,
                    NotShadowReceiver,
                    FirstPersonWeaponShadowDisabled,
                ));
            }
        }
    }
}

/// Add slight weapon sway/bob for visual polish.
#[derive(Default)]
pub(crate) struct WeaponViewRecoil {
    offset: Vec3,
    rotation: Vec3,
}

pub fn update_weapon_animation(
    mut weapons: Query<&mut Transform, With<FirstPersonWeapon>>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    local_water: Query<(), (With<LocalPlayer>, With<PlayerWaterState>)>,
    input_state: Res<InputState>,
    shooting_state: Res<ShootingState>,
    reload_state: Res<ReloadState>,
    time: Res<Time>,
    mut recoil: Local<WeaponViewRecoil>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let now = t;
    let in_water = !local_water.is_empty();

    let mut reload_amount = 0.0;
    if let Ok(weapon) = local_player.single() {
        let is_reloading = reload_state.weapon_type == Some(weapon.weapon_type)
            && now < reload_state.reload_until
            && reload_state.reload_duration > 0.0;
        if is_reloading {
            let elapsed = (now - reload_state.reload_started_at).max(0.0);
            let total = reload_state.reload_duration.max(0.01);
            let mut drop_time = 0.44;
            let mut rise_time = 0.44;
            let mut hold_time = total - drop_time - rise_time;
            if hold_time < 0.0 {
                let scale = total / (drop_time + rise_time);
                drop_time *= scale;
                rise_time *= scale;
                hold_time = 0.0;
            }

            reload_amount = if elapsed <= drop_time {
                let t = (elapsed / drop_time).clamp(0.0, 1.0);
                let omega = 13.3;
                let decay = (-t * 7.1).exp();
                let spring = decay * ((omega * t).cos() + 0.35 * (omega * t).sin());
                (1.0 - spring).clamp(0.0, 1.2)
            } else if elapsed < drop_time + hold_time {
                1.0
            } else if elapsed < drop_time + hold_time + rise_time {
                let t = ((elapsed - drop_time - hold_time) / rise_time).clamp(0.0, 1.0);
                let omega = 13.3;
                let decay = (-t * 7.1).exp();
                let spring = decay * ((omega * t).cos() + 0.55 * (omega * t).sin());
                spring.clamp(-0.35, 1.0)
            } else {
                0.0
            };
        }
    }

    let mut saw_weapon = false;
    for mut transform in weapons.iter_mut() {
        saw_weapon = true;
        // Base position.
        let mut offset = Vec3::new(0.25, -0.2, -0.4);

        // Subtle breathing/idle sway.
        offset.x += (t * 1.2).sin() * 0.003;
        offset.y += (t * 0.8).cos() * 0.002;

        let moving = !in_water
            && (input_state.forward
                || input_state.backward
                || input_state.left
                || input_state.right);
        let sprinting = !in_water
            && input_state.shift
            && input_state.forward
            && !input_state.backward
            && !input_state.aiming
            && !input_state.in_vehicle
            && !input_state.fly_mode;

        let mut move_pitch = 0.0;
        let mut move_yaw = 0.0;
        let mut move_roll = 0.0;

        // Movement bob. Sprint gets a lower, looser pose with stronger footfall rhythm.
        if moving {
            let freq = if sprinting { 13.5 } else { 8.0 };
            let phase = t * freq;
            let stride = (phase * 0.5).sin();
            let footfall = phase.sin().abs();

            if sprinting {
                offset.x += stride * 0.014 + 0.018;
                offset.y += footfall * 0.024 - 0.035;
                offset.z += stride.cos() * 0.010 - 0.035;
                move_pitch = -0.050 + footfall * 0.026;
                move_yaw = stride * 0.018;
                move_roll = stride * 0.038;
            } else {
                offset.y += footfall * 0.008;
                offset.x += stride * 0.004;
                move_pitch = footfall * 0.006;
                move_roll = stride * 0.010;
            }
        }

        if reload_amount != 0.0 {
            let mut pos_amount = reload_amount;
            if pos_amount < 0.0 {
                pos_amount *= 1.15;
            }
            offset.y -= pos_amount * 0.22;
            offset.z -= pos_amount * 0.1;
        }

        if shooting_state.shot_fired_this_frame {
            if let Some(weapon_type) = shooting_state.weapon_fired {
                let stats = weapon_type.stats();
                let kick_up = stats.recoil_vertical * 0.45 + 0.006;
                let kick_back = stats.recoil_vertical * 0.7 + 0.008;
                let kick_pitch = stats.recoil_vertical * 1.4;

                recoil.offset.y = (recoil.offset.y + kick_up).min(0.08);
                recoil.offset.z = (recoil.offset.z + kick_back).min(0.12);
                recoil.rotation.x = (recoil.rotation.x + kick_pitch).min(0.22);
            }
        }

        let return_speed = if shooting_state.fire_held { 18.0 } else { 12.0 };
        let lerp_t = (return_speed * dt).clamp(0.0, 1.0);
        recoil.offset = recoil.offset.lerp(Vec3::ZERO, lerp_t);
        recoil.rotation = recoil.rotation.lerp(Vec3::ZERO, lerp_t);

        let reload_pitch = -reload_amount * 1.05;
        transform.translation = offset + recoil.offset;
        transform.rotation = Quat::from_euler(
            EulerRot::YXZ,
            move_yaw,
            -recoil.rotation.x + reload_pitch + move_pitch,
            move_roll,
        );
    }

    if !saw_weapon {
        recoil.offset = Vec3::ZERO;
        recoil.rotation = Vec3::ZERO;
    }
}
