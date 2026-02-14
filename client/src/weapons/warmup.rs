//! warmup systems.

use super::*;

/// Queue weapon FX materials/meshes for pipeline warm-up.
pub fn update_weapon_warmup_queue(
    mut queue: ResMut<WeaponWarmupQueue>,
    assets: Option<Res<WeaponVisualAssets>>,
) {
    if queue.done || queue.initialized {
        return;
    }
    let Some(assets) = assets else {
        return;
    };

    let mut push = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>| {
        if queue.seen.insert(material.id()) {
            queue.queue.push_back((mesh.clone(), material.clone()));
        }
    };

    // Core weapon FX materials
    push(&assets.tracer_mesh, &assets.tracer_material);
    push(
        &assets.impact_disk_mesh_unit,
        &assets.impact_terrain_material,
    );
    push(&assets.impact_disk_mesh_unit, &assets.impact_wall_material);
    push(
        &assets.blood_splatter_mesh,
        &assets.blood_splat_shared_material,
    );
    push(
        &assets.blood_splatter_mesh,
        &assets.blood_ring_shared_material,
    );
    push(&assets.blood_droplet_mesh, &assets.blood_droplet_material);
    push(&assets.blood_burst_mesh, &assets.blood_burst_material);

    for mat in &assets.smoke_materials {
        push(&assets.smoke_mesh, mat);
    }
    for mat in &assets.flash_materials {
        push(&assets.flash_mesh, mat);
    }

    queue.initialized = true;
    if !queue.queue.is_empty() {
        info!(
            "Warmup: queued {} weapon FX material(s) for pipeline precompile",
            queue.queue.len()
        );
    }
}

/// Spawn a few warmup meshes per frame to precompile weapon FX pipelines.
pub fn spawn_weapon_warmups(
    mut commands: Commands,
    mut queue: ResMut<WeaponWarmupQueue>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
) {
    if queue.done || queue.queue.is_empty() {
        return;
    }
    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    for _ in 0..WEAPON_WARMUP_PER_FRAME {
        let Some((mesh, material)) = queue.queue.pop_front() else {
            break;
        };

        let entity = commands
            .spawn((
                WarmupWeaponFx {
                    timer: Timer::from_seconds(WEAPON_WARMUP_LIFETIME, TimerMode::Once),
                },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(WEAPON_WARMUP_POS)
                    .with_scale(Vec3::splat(WEAPON_WARMUP_SCALE)),
                Visibility::Visible,
                NoFrustumCulling,
                NotShadowCaster,
            ))
            .id();
        commands.entity(world_root).add_child(entity);
    }
}

/// Clean up warmup meshes after a short lifetime.
pub fn cleanup_weapon_warmups(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<WeaponWarmupQueue>,
    mut warmups: Query<(Entity, &mut WarmupWeaponFx)>,
) {
    let mut active = 0usize;
    for (entity, mut warmup) in warmups.iter_mut() {
        warmup.timer.tick(time.delta());
        if warmup.timer.is_finished() {
            commands.entity(entity).despawn();
        } else {
            active += 1;
        }
    }

    if !queue.done && queue.initialized && queue.queue.is_empty() && active == 0 {
        queue.done = true;
        info!("Warmup: weapon FX pipeline precompile complete");
    }
}
