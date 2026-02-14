use super::*;

/// Spawn blood effects: instant burst for feedback + droplets for realism
pub(crate) fn spawn_blood_splatter(
    commands: &mut Commands,
    visuals: &WeaponVisualAssets,
    _terrain: Option<&WorldTerrain>,
    impact_pos: Vec3,
    impact_normal: Vec3,
    now: f32,
) {
    let impact_normal = impact_normal.normalize_or_zero();
    let normal = if impact_normal.length_squared() > 1e-6 {
        impact_normal
    } else {
        Vec3::Y
    };

    // Use a simple pseudo-random based on position for variation
    let seed = (impact_pos.x * 1000.0 + impact_pos.z * 100.0 + impact_pos.y * 10.0) as i32;

    // =========================================================================
    // INSTANT BLOOD BURST (the key visual feedback!)
    // Multiple burst particles for a cloud effect
    // =========================================================================
    let num_bursts = 3 + (seed.abs() % 3) as usize; // 3-5 burst particles
    for i in 0..num_bursts {
        // Slight offset for each burst to create cloud effect
        let offset_angle = (i as f32 / num_bursts as f32) * std::f32::consts::TAU;
        let offset_dist = 0.05 + (((seed + i as i32) as f32 * 0.3).sin().abs()) * 0.1;
        let offset = Vec3::new(
            offset_angle.cos() * offset_dist,
            (((seed + i as i32 * 2) as f32 * 0.5).sin()) * 0.08,
            offset_angle.sin() * offset_dist,
        );

        let burst_pos = impact_pos + normal * 0.15 + offset;
        let scale_variation = 0.8 + (((seed + i as i32) as f32 * 0.7).sin().abs()) * 0.4;

        // Use shared burst material (fade via scale instead of alpha mutation)
        commands.spawn((
            BloodBurst {
                spawn_time: now,
                lifetime: BLOOD_BURST_LIFETIME
                    + (((seed + i as i32) as f32 * 0.4).sin().abs()) * 0.1,
                initial_scale: BLOOD_BURST_INITIAL_SCALE * scale_variation,
                max_scale: BLOOD_BURST_MAX_SCALE * scale_variation,
                direction: normal,
            },
            Mesh3d(visuals.blood_burst_mesh.clone()),
            MeshMaterial3d(visuals.blood_burst_material.clone()),
            Transform::from_translation(burst_pos)
                .with_scale(Vec3::splat(BLOOD_BURST_INITIAL_SCALE * scale_variation)),
            Visibility::Visible,
            InheritedVisibility::default(),
            NotShadowCaster,
        ));
    }

    // =========================================================================
    // FLYING DROPLETS (secondary - for realism, less important than burst)
    // =========================================================================
    let num_droplets = 4 + (seed.abs() % 3) as usize; // 4-6 droplets (reduced from before)
    for i in 0..num_droplets {
        let angle = (i as f32 / num_droplets as f32) * std::f32::consts::TAU
            + ((seed + i as i32) as f32 * 0.3).sin() * 0.8;

        // Create tangent/bitangent for spray direction
        let tangent = if normal.y.abs() > 0.9 {
            Vec3::X
        } else {
            normal.cross(Vec3::Y).normalize_or_zero()
        };
        let bitangent = normal.cross(tangent).normalize_or_zero();

        // Spray direction: mostly outward from surface, with some upward component
        let horizontal_speed = 2.0 + (((seed + i as i32 * 7) as f32 * 0.5).sin().abs()) * 3.0;
        let vertical_speed = 1.5 + (((seed + i as i32 * 3) as f32 * 0.7).sin().abs()) * 2.5;

        let spray_dir = tangent * angle.cos() + bitangent * angle.sin();
        // Bias spray outward from the surface and a bit upward.
        let velocity = spray_dir * horizontal_speed + Vec3::Y * vertical_speed + normal * 1.2;

        // Small offset from impact point
        let droplet_pos = impact_pos + normal * 0.1 + spray_dir * 0.05;

        // Scale varies per droplet (small spheres)
        let scale = 0.04 + (((seed + i as i32 * 3) as f32 * 0.5).sin().abs()) * 0.06;

        commands.spawn((
            BloodDroplet {
                velocity,
                spawn_time: now,
            },
            Mesh3d(visuals.blood_droplet_mesh.clone()),
            MeshMaterial3d(visuals.blood_droplet_material.clone()),
            Transform::from_translation(droplet_pos).with_scale(Vec3::splat(scale)),
            Visibility::Visible,
            InheritedVisibility::default(),
            NotShadowCaster,
        ));
    }
}

/// Update flying blood droplets - apply gravity, check for ground collision
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
        // Check lifetime
        let age = now - droplet.spawn_time;
        if age > BLOOD_DROPLET_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }

        // Apply gravity + simple drag
        droplet.velocity.y -= BLOOD_GRAVITY * dt;
        let v = droplet.velocity;
        droplet.velocity -= v * (BLOOD_AIR_DRAG * dt);

        // Move droplet
        let new_pos = transform.translation + droplet.velocity * dt;

        // Make droplets look like moving blobs (stretch along velocity)
        let speed = droplet.velocity.length();
        if speed > 0.2 {
            let dir = droplet.velocity / speed;
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
            // Stretch more at higher speeds
            let base = transform.scale.x.max(0.01);
            let stretch = (1.0 + speed * 0.12).clamp(1.0, 2.2);
            transform.scale = Vec3::new(base * 0.7, base * stretch, base * 0.7);
        }

        // Check ground collision
        let ground_y = terrain
            .as_ref()
            .map(|t| t.get_height(new_pos.x, new_pos.z))
            .unwrap_or(0.0);

        if new_pos.y <= ground_y + 0.02 {
            // Hit the ground! Spawn a ground splat and despawn the droplet
            let splat_pos = Vec3::new(new_pos.x, ground_y + 0.02, new_pos.z);

            // Splat size based on droplet size and speed
            let impact_speed = droplet.velocity.length();
            let base_scale = transform.scale.x.max(0.01);
            let splat_scale = (base_scale * 4.0 + impact_speed * 0.03).clamp(0.08, 0.55);

            // Direction smear based on impact velocity projected onto ground
            let dir_xz = Vec2::new(droplet.velocity.x, droplet.velocity.z);
            let dir_angle = dir_xz.y.atan2(dir_xz.x);
            let smear_rot = Quat::from_rotation_y(-dir_angle);
            let smear = (1.0 + impact_speed * 0.06).clamp(1.0, 2.8);

            // Use shared materials (fade via scale, no per-hit allocation)
            // Main splat (slightly smeared)
            commands.spawn((
                BloodGroundSplat {
                    spawn_time: now,
                    lifetime: BLOOD_SPLAT_LIFETIME,
                    initial_scale: splat_scale,
                },
                Mesh3d(visuals.blood_splatter_mesh.clone()),
                MeshMaterial3d(visuals.blood_splat_shared_material.clone()),
                Transform::from_translation(splat_pos)
                    .with_rotation(smear_rot)
                    .with_scale(Vec3::new(splat_scale * smear, splat_scale, splat_scale)),
                Visibility::Visible,
                InheritedVisibility::default(),
                NotShadowCaster,
            ));

            // Satellite droplets around the main splat (adds "splatter" texture without a texture)
            let seed = (new_pos.x * 120.0 + new_pos.z * 70.0) as i32;
            let satellites = 3 + (seed.abs() % 4) as usize; // 3-6
            for j in 0..satellites {
                let a = (j as f32 / satellites as f32) * std::f32::consts::TAU
                    + ((seed + j as i32) as f32 * 0.3).sin() * 0.8;
                let r = 0.08 + (((seed + j as i32 * 11) as f32 * 0.7).sin().abs()) * 0.25;
                let off = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                let s = (splat_scale
                    * (0.25 + (((seed + j as i32 * 5) as f32 * 0.9).sin().abs()) * 0.35))
                    .clamp(0.03, 0.22);
                let rot = Quat::from_rotation_y(
                    ((seed + j as i32 * 13) as f32 * 0.17).sin() * std::f32::consts::TAU,
                );
                commands.spawn((
                    BloodGroundSplat {
                        spawn_time: now,
                        lifetime: BLOOD_SPLAT_LIFETIME,
                        initial_scale: s,
                    },
                    Mesh3d(visuals.blood_splatter_mesh.clone()),
                    MeshMaterial3d(visuals.blood_splat_shared_material.clone()),
                    Transform::from_translation(splat_pos + off)
                        .with_rotation(rot)
                        .with_scale(Vec3::splat(s)),
                    Visibility::Visible,
                    InheritedVisibility::default(),
                    NotShadowCaster,
                ));
            }

            // Quick splash ring (expands and fades fast)
            commands.spawn((
                BloodSplashRing {
                    spawn_time: now,
                    lifetime: 0.35,
                    initial_scale: splat_scale * 0.9,
                },
                Mesh3d(visuals.blood_splatter_mesh.clone()),
                MeshMaterial3d(visuals.blood_ring_shared_material.clone()),
                Transform::from_translation(splat_pos).with_scale(Vec3::splat(splat_scale * 0.9)),
                Visibility::Visible,
                InheritedVisibility::default(),
                NotShadowCaster,
            ));

            commands.entity(entity).despawn();
        } else {
            transform.translation = new_pos;

            // Shrink slightly as it flies (evaporation effect)
            transform.scale *= 1.0 - dt * 0.3;
        }
    }
}

/// Update splash rings - expand quickly then shrink to zero (shared material)
pub fn update_blood_splash_rings(
    mut commands: Commands,
    mut rings: Query<(Entity, &BloodSplashRing, &mut Transform)>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();

    for (entity, ring, mut transform) in rings.iter_mut() {
        let age = now - ring.spawn_time;
        if age > ring.lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        let t = (age / ring.lifetime).clamp(0.0, 1.0);
        let expand = 1.0 + t * 1.8;
        let fade = (1.0 - t).powf(0.5); // Shrink towards end of life
        transform.scale = Vec3::splat(ring.initial_scale * expand * fade);
    }
}

/// Update instant blood bursts - expand fast and fade via scale (shared material)
pub fn update_blood_bursts(
    mut commands: Commands,
    mut bursts: Query<(Entity, &BloodBurst, &mut Transform)>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();

    // Enforce entity cap
    let count = bursts.iter().len();
    if count > MAX_BLOOD_BURSTS {
        let mut by_age: Vec<(Entity, f32)> =
            bursts.iter().map(|(e, b, _)| (e, b.spawn_time)).collect();
        by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (entity, _) in by_age.iter().take(count - MAX_BLOOD_BURSTS) {
            commands.entity(*entity).despawn();
        }
    }

    for (entity, burst, mut transform) in bursts.iter_mut() {
        let age = now - burst.spawn_time;

        if age > burst.lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        // t goes 0->1 over the burst lifetime
        let t = (age / burst.lifetime).clamp(0.0, 1.0);

        // Expand quickly (ease-out curve for punchy feel)
        let ease_t = 1.0 - (1.0 - t).powi(2); // Quadratic ease-out
        let scale = burst.initial_scale + (burst.max_scale - burst.initial_scale) * ease_t;

        // Slight movement in the burst direction (blood "puffs" outward)
        let move_dist = ease_t * 0.15;
        let base_pos = transform.translation;
        transform.translation = base_pos + burst.direction * move_dist * time.delta_secs() * 10.0;

        // Scale: expand then shrink to zero for fade (shared material, can't mutate alpha)
        let fade = (1.0 - t).powf(0.7);
        transform.scale = Vec3::splat(scale * fade);
    }
}

/// Update ground blood splats - expand slightly then shrink to zero (shared material)
pub fn update_blood_ground_splats(
    mut commands: Commands,
    mut splats: Query<(Entity, &BloodGroundSplat, &mut Transform)>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();

    // Enforce entity cap: despawn oldest if over budget
    let count = splats.iter().len();
    if count > MAX_BLOOD_GROUND_SPLATS {
        let mut by_age: Vec<(Entity, f32)> =
            splats.iter().map(|(e, s, _)| (e, s.spawn_time)).collect();
        by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (entity, _) in by_age.iter().take(count - MAX_BLOOD_GROUND_SPLATS) {
            commands.entity(*entity).despawn();
        }
    }

    for (entity, splat, mut transform) in splats.iter_mut() {
        let age = now - splat.spawn_time;

        if age > splat.lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        let t = age / splat.lifetime;

        // Expand slightly in first 10% of lifetime (blood spreading)
        let expand = if t < 0.1 { 1.0 + (t / 0.1) * 0.3 } else { 1.3 };

        // Shrink to zero in the last 50% of lifetime (shared material, can't mutate alpha)
        let fade = if t > 0.5 {
            let fade_t = (t - 0.5) / 0.5;
            1.0 - fade_t
        } else {
            1.0
        };

        transform.scale = Vec3::splat(splat.initial_scale * expand * fade);
    }
}
