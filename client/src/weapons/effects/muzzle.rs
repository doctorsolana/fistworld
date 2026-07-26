use super::*;

fn smoke_settings(weapon_type: WeaponType) -> SmokeSettings {
    match weapon_type {
        WeaponType::Pistol => SmokeSettings {
            puff_count: 1,
            base_scale: 0.15,
            scale_jitter: 0.06,
            lifetime: 0.8,
            speed: 0.8,
            spread: 0.0,
            forward_offset: 0.06,
        },
        WeaponType::AssaultRifle => SmokeSettings {
            puff_count: 1,
            base_scale: 0.165,
            scale_jitter: 0.06,
            lifetime: 0.9,
            speed: 0.9,
            spread: 0.0,
            forward_offset: 0.08,
        },
        WeaponType::Shotgun => SmokeSettings {
            puff_count: 1,
            base_scale: 0.225,
            scale_jitter: 0.09,
            lifetime: 1.0,
            speed: 1.0,
            spread: 0.0,
            forward_offset: 0.1,
        },
        WeaponType::Sniper => SmokeSettings {
            puff_count: 1,
            base_scale: 0.21,
            scale_jitter: 0.075,
            lifetime: 1.1,
            speed: 1.1,
            spread: 0.0,
            forward_offset: 0.1,
        },
        WeaponType::Unarmed | WeaponType::Sword | WeaponType::Shield => SmokeSettings {
            puff_count: 0,
            base_scale: 0.0,
            scale_jitter: 0.0,
            lifetime: 0.0,
            speed: 0.0,
            spread: 0.0,
            forward_offset: 0.0,
        },
    }
}

fn flash_settings(weapon_type: WeaponType) -> FlashSettings {
    match weapon_type {
        WeaponType::Pistol => FlashSettings {
            base_scale: 0.27,
            scale_jitter: 0.08,
            lifetime: 0.05,
            forward_offset: 0.06,
        },
        WeaponType::AssaultRifle => FlashSettings {
            base_scale: 0.335,
            scale_jitter: 0.08,
            lifetime: 0.05,
            forward_offset: 0.08,
        },
        WeaponType::Shotgun => FlashSettings {
            base_scale: 0.4,
            scale_jitter: 0.11,
            lifetime: 0.06,
            forward_offset: 0.1,
        },
        WeaponType::Sniper => FlashSettings {
            base_scale: 0.38,
            scale_jitter: 0.1,
            lifetime: 0.06,
            forward_offset: 0.1,
        },
        WeaponType::Unarmed | WeaponType::Sword | WeaponType::Shield => FlashSettings {
            base_scale: 0.0,
            scale_jitter: 0.0,
            lifetime: 0.0,
            forward_offset: 0.0,
        },
    }
}

fn rand_range(min: f32, max: f32) -> f32 {
    min + (max - min) * rand::random::<f32>()
}

pub(crate) fn spawn_muzzle_smoke(
    commands: &mut Commands,
    visuals: &WeaponVisualAssets,
    spawn_pos: Vec3,
    direction: Vec3,
    weapon_type: WeaponType,
) {
    let settings = smoke_settings(weapon_type);
    if settings.puff_count == 0 || visuals.smoke_materials.is_empty() {
        return;
    }

    let forward = direction.normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();

    let base_pos = spawn_pos + forward * settings.forward_offset;

    for _ in 0..settings.puff_count {
        let spread_x = rand_range(-settings.spread, settings.spread);
        let spread_y = rand_range(-settings.spread * 0.5, settings.spread * 0.5);
        let offset = right * spread_x + up * spread_y;

        let speed = settings.speed * rand_range(0.7, 1.3);
        let lift = rand_range(0.15, 0.4);
        let velocity = forward * speed + up * lift;

        let scale = settings.base_scale + rand_range(-settings.scale_jitter, settings.scale_jitter);
        let lifetime = settings.lifetime * rand_range(0.8, 1.25);

        let material = visuals.smoke_materials[0].clone();

        commands.spawn((
            MuzzleSmoke {
                lifetime,
                max_lifetime: lifetime,
                velocity,
                initial_scale: scale.max(0.01),
                frame_count: visuals.smoke_materials.len(),
            },
            Mesh3d(visuals.smoke_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(base_pos + offset).with_scale(Vec3::splat(scale.max(0.01))),
            NotShadowCaster,
        ));
    }
}

pub(crate) fn spawn_muzzle_flash(
    commands: &mut Commands,
    visuals: &WeaponVisualAssets,
    spawn_pos: Vec3,
    direction: Vec3,
    weapon_type: WeaponType,
) {
    let settings = flash_settings(weapon_type);
    if settings.lifetime <= 0.0 || visuals.flash_materials.is_empty() {
        return;
    }

    let forward = direction.normalize_or_zero();
    let base_pos = spawn_pos + forward * settings.forward_offset;
    let scale =
        (settings.base_scale + rand_range(-settings.scale_jitter, settings.scale_jitter)).max(0.01);

    commands.spawn((
        MuzzleFlash {
            lifetime: settings.lifetime,
            max_lifetime: settings.lifetime,
            base_scale: scale,
            frame_count: visuals.flash_materials.len(),
        },
        Mesh3d(visuals.flash_mesh.clone()),
        MeshMaterial3d(visuals.flash_materials[0].clone()),
        Transform::from_translation(base_pos).with_scale(Vec3::splat(scale)),
        NotShadowCaster,
    ));
}

/// Update muzzle smoke puffs - drift, expand, fade
pub fn update_muzzle_smoke(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    visuals: Res<WeaponVisualAssets>,
    mut puffs: Query<(
        Entity,
        &mut MuzzleSmoke,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    let lift = Vec3::new(0.0, 0.8, 0.0);
    let camera_pos = camera.iter().next().map(|c| c.translation());

    for (entity, mut puff, mut transform, mut material) in puffs.iter_mut() {
        puff.lifetime -= dt;
        if puff.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Gentle rise + drag
        puff.velocity += lift * dt;
        puff.velocity *= 0.92_f32.powf(dt * 60.0);
        transform.translation += puff.velocity * dt;

        let life_progress = 1.0 - (puff.lifetime / puff.max_lifetime).max(0.001);
        let scale_multiplier = 1.0 + life_progress * 2.2;
        let fade = (puff.lifetime / puff.max_lifetime).powf(1.3);
        transform.scale = Vec3::splat(puff.initial_scale * scale_multiplier * fade);

        if puff.frame_count > 0 {
            let frame = (life_progress * (puff.frame_count as f32 - 1.0)).floor() as usize;
            if let Some(handle) = visuals.smoke_materials.get(frame) {
                material.0 = handle.clone();
            }
        }

        if let Some(cam) = camera_pos {
            let to_cam = (cam - transform.translation).normalize_or_zero();
            if to_cam.length_squared() > 0.0001 {
                transform.rotation = Quat::from_rotation_arc(Vec3::Y, to_cam);
            }
        }
    }
}

/// Update muzzle flash (billboard + flipbook)
pub fn update_muzzle_flash(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    visuals: Res<WeaponVisualAssets>,
    mut flashes: Query<(
        Entity,
        &mut MuzzleFlash,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    let camera_pos = camera.iter().next().map(|c| c.translation());

    for (entity, mut flash, mut transform, mut material) in flashes.iter_mut() {
        flash.lifetime -= dt;
        if flash.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        let progress = 1.0 - (flash.lifetime / flash.max_lifetime).max(0.001);
        let frame = (progress * (flash.frame_count as f32 - 1.0)).floor() as usize;
        if let Some(handle) = visuals.flash_materials.get(frame) {
            material.0 = handle.clone();
        }

        let scale = flash.base_scale * (1.0 - progress).max(0.1);
        transform.scale = Vec3::splat(scale);

        if let Some(cam) = camera_pos {
            let to_cam = (cam - transform.translation).normalize_or_zero();
            if to_cam.length_squared() > 0.0001 {
                transform.rotation = Quat::from_rotation_arc(Vec3::Y, to_cam);
            }
        }
    }
}
