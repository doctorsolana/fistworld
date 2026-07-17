//! Blood effects: billboarded mist puffs on hit, physical droplets, and
//! textured ground decals that dry over time.
//!
//! Realism rules baked in here:
//! - Blood never glows: no emissive anywhere, colors are dark red.
//! - The hit "burst" is a soft textured mist (tinted smoke flipbook), not
//!   geometry — it sprays along the impact direction and dissipates.
//! - Ground blood is an irregular procedural splatter decal, laid flat on the
//!   terrain slope with a velocity smear, that dries from glossy red to dark
//!   matte brown and persists for ~30s.

use super::*;

fn rand_range(min: f32, max: f32) -> f32 {
    min + (max - min) * rand::random::<f32>()
}

/// Spawn blood effects for a character hit: mist puffs + flying droplets.
pub(crate) fn spawn_blood_splatter(
    commands: &mut Commands,
    visuals: &WeaponVisualAssets,
    _terrain: Option<&WorldTerrain>,
    impact_pos: Vec3,
    impact_normal: Vec3,
    now: f32,
) {
    let normal = if impact_normal.length_squared() > 1e-6 {
        impact_normal.normalize()
    } else {
        Vec3::Y
    };
    let tangent = if normal.y.abs() > 0.9 {
        Vec3::X
    } else {
        normal.cross(Vec3::Y).normalize_or_zero()
    };
    let bitangent = normal.cross(tangent).normalize_or_zero();

    // =========================================================================
    // MIST PUFFS — the instant feedback. 2-3 overlapping dark-red puffs that
    // spray out along the impact direction and dissipate in ~0.35s.
    // =========================================================================
    if !visuals.blood_mist_materials.is_empty() {
        let puffs = 2 + (rand::random::<f32>() * 2.0) as usize; // 2-3
        for i in 0..puffs {
            let jitter = tangent * rand_range(-0.06, 0.06) + bitangent * rand_range(-0.06, 0.06);
            let pos = impact_pos + normal * rand_range(0.08, 0.18) + jitter;
            let velocity =
                normal * rand_range(0.7, 1.6) + jitter * 4.0 + Vec3::Y * rand_range(0.1, 0.45);
            let scale = rand_range(0.13, 0.22) * if i == 0 { 1.25 } else { 1.0 };

            commands.spawn((
                BloodMist {
                    spawn_time: now,
                    lifetime: rand_range(0.28, 0.42),
                    velocity,
                    initial_scale: scale,
                    roll: rand_range(0.0, std::f32::consts::TAU),
                },
                Mesh3d(visuals.blood_mist_mesh.clone()),
                MeshMaterial3d(visuals.blood_mist_materials[0].clone()),
                Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
                Visibility::Visible,
                InheritedVisibility::default(),
                NotShadowCaster,
            ));
        }
    }

    // =========================================================================
    // FLYING DROPLETS — small dark specks with gravity; they become ground
    // decals where they land.
    // =========================================================================
    let num_droplets = 5 + (rand::random::<f32>() * 4.0) as usize; // 5-8
    for _ in 0..num_droplets {
        let angle = rand_range(0.0, std::f32::consts::TAU);
        let spray_dir = tangent * angle.cos() + bitangent * angle.sin();
        // Cone biased along the impact direction, with lateral spread + lift.
        let velocity = normal * rand_range(2.0, 4.5)
            + spray_dir * rand_range(0.3, 2.0)
            + Vec3::Y * rand_range(0.6, 1.8);

        let scale = rand_range(0.02, 0.05);
        commands.spawn((
            BloodDroplet {
                velocity,
                spawn_time: now,
            },
            Mesh3d(visuals.blood_droplet_mesh.clone()),
            MeshMaterial3d(visuals.blood_droplet_material.clone()),
            Transform::from_translation(impact_pos + normal * 0.08 + spray_dir * 0.04)
                .with_scale(Vec3::splat(scale)),
            Visibility::Visible,
            InheritedVisibility::default(),
            NotShadowCaster,
        ));
    }
}

/// Spawn a ground splat decal, aligned to the terrain slope and smeared along
/// the landing velocity.
fn spawn_ground_splat(
    commands: &mut Commands,
    visuals: &WeaponVisualAssets,
    terrain: Option<&WorldTerrain>,
    pos: Vec3,
    landing_velocity: Vec3,
    scale: f32,
    now: f32,
) {
    if visuals.blood_splat_variants.is_empty() {
        return;
    }
    let variant = (rand::random::<f32>() * visuals.blood_splat_variants.len() as f32) as usize
        % visuals.blood_splat_variants.len();

    // Lay the quad on the terrain slope (not always flat-horizontal).
    let ground_normal = terrain
        .map(|t| t.get_normal(pos.x, pos.z))
        .unwrap_or(Vec3::Y)
        .normalize_or_zero();
    let slope_rot = Quat::from_rotation_arc(Vec3::Y, ground_normal);

    // Smear along the landing direction; random yaw otherwise.
    let dir_xz = Vec2::new(landing_velocity.x, landing_velocity.z);
    let speed = landing_velocity.length();
    let yaw = if dir_xz.length_squared() > 0.05 {
        -dir_xz.y.atan2(dir_xz.x)
    } else {
        rand_range(0.0, std::f32::consts::TAU)
    };
    let smear = (1.0 + speed * 0.05).clamp(1.0, 1.9);

    // Small random lift avoids z-fighting between overlapping splats.
    let lift = ground_normal * (0.012 + rand::random::<f32>() * 0.012);

    commands.spawn((
        BloodGroundSplat {
            spawn_time: now,
            lifetime: BLOOD_SPLAT_LIFETIME * rand_range(0.85, 1.15),
            initial_scale: scale,
            variant,
            stage: 0,
        },
        Mesh3d(visuals.blood_splat_mesh.clone()),
        MeshMaterial3d(visuals.blood_splat_variants[variant].fresh.clone()),
        Transform::from_translation(pos + lift)
            .with_rotation(slope_rot * Quat::from_rotation_y(yaw))
            .with_scale(Vec3::new(scale * smear, 1.0, scale)),
        Visibility::Visible,
        InheritedVisibility::default(),
        NotShadowCaster,
    ));
}

/// Update flying blood droplets - gravity, stretch along velocity, land into decals.
pub fn update_blood_droplets(
    mut commands: Commands,
    mut droplets: Query<(Entity, &mut BloodDroplet, &mut Transform)>,
    terrain: Option<Res<WorldTerrain>>,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();

    let Some(visuals) = weapon_visuals else {
        return;
    };

    // Enforce entity cap
    let count = droplets.iter().len();
    if count > MAX_BLOOD_DROPLETS {
        let mut by_age: Vec<(Entity, f32)> =
            droplets.iter().map(|(e, d, _)| (e, d.spawn_time)).collect();
        by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (entity, _) in by_age.iter().take(count - MAX_BLOOD_DROPLETS) {
            commands.entity(*entity).despawn();
        }
    }

    for (entity, mut droplet, mut transform) in droplets.iter_mut() {
        let age = now - droplet.spawn_time;
        if age > BLOOD_DROPLET_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }

        // Gravity + drag.
        droplet.velocity.y -= BLOOD_GRAVITY * dt;
        let v = droplet.velocity;
        droplet.velocity -= v * (BLOOD_AIR_DRAG * dt);

        let new_pos = transform.translation + droplet.velocity * dt;

        // Stretch along velocity (motion-blurred teardrop look).
        let speed = droplet.velocity.length();
        if speed > 0.2 {
            let dir = droplet.velocity / speed;
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
            let base = transform.scale.x.max(0.008);
            let stretch = (1.0 + speed * 0.18).clamp(1.0, 3.0);
            transform.scale = Vec3::new(base * 0.6, base * stretch, base * 0.6);
        }

        let ground_y = terrain
            .as_ref()
            .map(|t| t.get_height(new_pos.x, new_pos.z))
            .unwrap_or(0.0);

        if new_pos.y <= ground_y + 0.02 {
            let splat_pos = Vec3::new(new_pos.x, ground_y, new_pos.z);
            let impact_speed = droplet.velocity.length();
            let base_scale = transform.scale.x.max(0.008);
            let splat_scale = (base_scale * 6.0 + impact_speed * 0.02).clamp(0.10, 0.45);

            spawn_ground_splat(
                &mut commands,
                &visuals,
                terrain.as_deref(),
                splat_pos,
                droplet.velocity,
                splat_scale,
                now,
            );
            commands.entity(entity).despawn();
        } else {
            transform.translation = new_pos;
        }
    }
}

/// Update blood mist puffs: drift, flipbook, billboard toward the camera.
pub fn update_blood_bursts(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    mut mists: Query<(
        Entity,
        &mut BloodMist,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(visuals) = weapon_visuals else {
        return;
    };
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let camera_pos = camera.iter().next().map(|c| c.translation());

    // Enforce entity cap
    let count = mists.iter().len();
    if count > MAX_BLOOD_MISTS {
        let mut by_age: Vec<(Entity, f32)> =
            mists.iter().map(|(e, m, _, _)| (e, m.spawn_time)).collect();
        by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (entity, _) in by_age.iter().take(count - MAX_BLOOD_MISTS) {
            commands.entity(*entity).despawn();
        }
    }

    for (entity, mut mist, mut transform, mut material) in mists.iter_mut() {
        let age = now - mist.spawn_time;
        if age > mist.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        let t = (age / mist.lifetime).clamp(0.0, 1.0);

        // Drift outward, sag slightly, slow down.
        mist.velocity.y -= 1.2 * dt;
        mist.velocity *= 0.90_f32.powf(dt * 60.0);
        transform.translation += mist.velocity * dt;

        // Expand as it dissipates; the flipbook's own alpha handles the fade.
        transform.scale = Vec3::splat(mist.initial_scale * (1.0 + t * 2.4));

        let frame_count = visuals.blood_mist_materials.len();
        if frame_count > 0 {
            let frame = (t * (frame_count as f32 - 1.0)).floor() as usize;
            if let Some(handle) = visuals.blood_mist_materials.get(frame) {
                material.0 = handle.clone();
            }
        }

        if let Some(cam) = camera_pos {
            let to_cam = (cam - transform.translation).normalize_or_zero();
            if to_cam.length_squared() > 0.0001 {
                transform.rotation =
                    Quat::from_rotation_arc(Vec3::Y, to_cam) * Quat::from_rotation_y(mist.roll);
            }
        }
    }
}

/// Update ground blood decals: dry over time (material stage swap), fade out
/// only at the very end of life.
pub fn update_blood_ground_splats(
    mut commands: Commands,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    mut splats: Query<(
        Entity,
        &mut BloodGroundSplat,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    time: Res<Time>,
) {
    let Some(visuals) = weapon_visuals else {
        return;
    };
    let now = time.elapsed_secs();

    // Enforce entity cap: despawn oldest if over budget
    let count = splats.iter().len();
    if count > MAX_BLOOD_GROUND_SPLATS {
        let mut by_age: Vec<(Entity, f32)> = splats
            .iter()
            .map(|(e, s, _, _)| (e, s.spawn_time))
            .collect();
        by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (entity, _) in by_age.iter().take(count - MAX_BLOOD_GROUND_SPLATS) {
            commands.entity(*entity).despawn();
        }
    }

    for (entity, mut splat, mut transform, mut material) in splats.iter_mut() {
        let age = now - splat.spawn_time;
        if age > splat.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        let t = age / splat.lifetime;

        // Fresh blood spreads slightly in the first moments.
        if t < 0.06 {
            let spread = 1.0 + (t / 0.06) * 0.25;
            let base = splat.initial_scale;
            transform.scale.x = transform.scale.x.max(base * spread);
            transform.scale.z = base * spread;
        }

        // Drying: swap to the darker/matte stage materials.
        let desired_stage: u8 = if t > 0.55 {
            2
        } else if t > 0.18 {
            1
        } else {
            0
        };
        if desired_stage != splat.stage {
            if let Some(set) = visuals.blood_splat_variants.get(splat.variant) {
                material.0 = match desired_stage {
                    1 => set.drying.clone(),
                    _ => set.dried.clone(),
                };
                splat.stage = desired_stage;
            }
        }

        // Only shrink away in the final 8% of life.
        if t > 0.92 {
            let fade = 1.0 - (t - 0.92) / 0.08;
            let base = splat.initial_scale * fade.max(0.0);
            transform.scale = Vec3::new(transform.scale.x.min(base * 1.9), 1.0, base);
        }
    }
}
