//! projectiles systems.

use super::*;

/// Keep `PlayerOwnerIndex` in sync with replicated player entities.
pub fn sync_player_owner_index(
    mut index: ResMut<PlayerOwnerIndex>,
    players: Query<(Entity, &Player), Or<(Added<Player>, Changed<Player>)>>,
    mut removed_players: RemovedComponents<Player>,
) {
    for removed_entity in removed_players.read() {
        if let Some(owner_id) = index.by_entity.remove(&removed_entity) {
            if index
                .by_owner_id
                .get(&owner_id)
                .is_some_and(|entity| *entity == removed_entity)
            {
                index.by_owner_id.remove(&owner_id);
            }
        }
    }

    for (entity, player) in players.iter() {
        let owner_id = peer_id_to_u64(player.client_id);

        if let Some(previous_owner_id) = index.by_entity.insert(entity, owner_id) {
            if previous_owner_id != owner_id {
                index.by_owner_id.remove(&previous_owner_id);
            }
        }

        if let Some(previous_entity) = index.by_owner_id.insert(owner_id, entity) {
            if previous_entity != entity {
                index.by_entity.remove(&previous_entity);
            }
        }
    }
}

/// Keep cached remote muzzle transforms in sync with replicated weapon entities.
pub fn sync_remote_muzzle_index(
    mut index: ResMut<RemoteMuzzleIndex>,
    remote_weapons: Query<
        (Entity, &RemoteThirdPersonWeapon, &GlobalTransform),
        Or<(Added<RemoteThirdPersonWeapon>, Changed<GlobalTransform>)>,
    >,
    mut removed_weapons: RemovedComponents<RemoteThirdPersonWeapon>,
) {
    for removed_weapon_entity in removed_weapons.read() {
        if let Some(owner_entity) = index.by_weapon_entity.remove(&removed_weapon_entity) {
            if index
                .by_owner_weapon
                .get(&owner_entity)
                .is_some_and(|weapon| *weapon == removed_weapon_entity)
            {
                index.by_owner_weapon.remove(&owner_entity);
                index.by_owner.remove(&owner_entity);
            }
        }
    }

    for (weapon_entity, weapon, global) in remote_weapons.iter() {
        let transform = global.compute_transform();
        let forward = transform.rotation * Vec3::NEG_Z;

        if let Some(previous_weapon) = index.by_owner_weapon.insert(weapon.owner, weapon_entity) {
            if previous_weapon != weapon_entity {
                index.by_weapon_entity.remove(&previous_weapon);
            }
        }

        index
            .by_owner
            .insert(weapon.owner, (transform.translation, forward));
        index.by_weapon_entity.insert(weapon_entity, weapon.owner);
    }
}

/// Reset projectile helper indices when leaving gameplay.
pub fn reset_projectile_indices(
    mut owner_index: ResMut<PlayerOwnerIndex>,
    mut muzzle_index: ResMut<RemoteMuzzleIndex>,
) {
    owner_index.by_owner_id.clear();
    owner_index.by_entity.clear();
    muzzle_index.by_owner.clear();
    muzzle_index.by_weapon_entity.clear();
    muzzle_index.by_owner_weapon.clear();
}

/// Update local tracers (simulating bullet flight for prediction)
pub fn update_local_tracers(
    mut commands: Commands,
    mut tracers: Query<(
        Entity,
        &mut LocalTracer,
        &mut BulletVelocity,
        &mut Transform,
    )>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let current_time = time.elapsed_secs();

    for (entity, tracer, mut velocity, mut transform) in tracers.iter_mut() {
        // Check lifetime
        if current_time - tracer.spawn_time > tracer.lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        // Step physics
        let (new_pos, new_vel) =
            ballistics::step_bullet_physics(transform.translation, velocity.0, dt);

        transform.translation = new_pos;
        velocity.0 = new_vel;

        // Orient tracer along velocity
        if new_vel.length() > 0.1 {
            transform.look_to(new_vel.normalize(), Vec3::Y);
        }
    }
}

/// Handle replicated bullets - spawn visual representations
pub fn handle_bullet_spawned(
    mut commands: Commands,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    bullets: Query<(Entity, &Bullet, &BulletVelocity, Option<&PlayerPosition>), Added<Bullet>>,
    local_player: Query<&Player, With<LocalPlayer>>,
    owner_index: Res<PlayerOwnerIndex>,
    muzzle_index: Res<RemoteMuzzleIndex>,
    mut last_muzzle_by_owner: Local<HashMap<u64, f32>>,
) {
    let Some(weapon_visuals) = weapon_visuals else {
        return;
    };
    if bullets.is_empty() {
        return;
    }

    let local_client_id = local_player
        .iter()
        .next()
        .map(|p| peer_id_to_u64(p.client_id));

    for (entity, bullet, velocity, position) in bullets.iter() {
        let spawn_pos = position.map(|p| p.0).unwrap_or(bullet.spawn_position);

        // Calculate initial rotation from velocity
        let direction = velocity.0.normalize_or_zero();
        let rotation = if direction.length() > 0.1 {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };

        commands.entity(entity).insert((
            Mesh3d(weapon_visuals.tracer_mesh.clone()),
            MeshMaterial3d(weapon_visuals.tracer_material.clone()),
            Transform::from_translation(spawn_pos).with_rotation(rotation),
            BulletTrail {
                positions: vec![spawn_pos],
            },
            NotShadowCaster,
        ));

        // Spawn muzzle smoke for remote shooters (local shooter already handled immediately)
        let is_local = local_client_id == Some(bullet.owner_id);
        if !is_local {
            let last_time = last_muzzle_by_owner.get(&bullet.owner_id).copied();
            if last_time.is_some_and(|t| (bullet.spawn_time - t).abs() < 1e-3) {
                continue;
            }
            last_muzzle_by_owner.insert(bullet.owner_id, bullet.spawn_time);

            let (muzzle_pos, muzzle_dir) = owner_index
                .by_owner_id
                .get(&bullet.owner_id)
                .and_then(|owner| muzzle_index.by_owner.get(owner))
                .map(|(pos, dir)| (*pos, *dir))
                .unwrap_or((spawn_pos, velocity.0));

            spawn_muzzle_flash(
                &mut commands,
                &weapon_visuals,
                muzzle_pos,
                muzzle_dir,
                bullet.weapon_type,
            );
            spawn_muzzle_smoke(
                &mut commands,
                &weapon_visuals,
                muzzle_pos,
                muzzle_dir,
                bullet.weapon_type,
            );
        }
    }
}

/// Update bullet visuals to follow replicated positions
pub fn update_bullet_visuals(
    mut bullets: Query<
        (
            &PlayerPosition,
            &BulletVelocity,
            &mut Transform,
            &mut BulletTrail,
        ),
        With<Bullet>,
    >,
) {
    for (pos, velocity, mut transform, mut trail) in bullets.iter_mut() {
        // IMPORTANT: move bullet based on replicated PlayerPosition (Transform itself is not replicated)
        transform.translation = pos.0;

        // Orient bullet along velocity direction
        if velocity.0.length() > 0.1 {
            let direction = velocity.0.normalize();
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, direction);
        }

        // Record position for trail (every few frames to avoid too many points, capped at 100)
        if trail.positions.len() < 100
            && (trail.positions.is_empty()
                || trail
                    .positions
                    .last()
                    .is_none_or(|last| last.distance(transform.translation) > 5.0))
        {
            trail.positions.push(transform.translation);
        }
    }
}

/// Handle server-authoritative bullet impacts (reliable even for very fast bullets).
/// Spawns impact markers and (when debug is ON) stores a red trajectory line.
pub fn handle_bullet_impacts(
    mut commands: Commands,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    // In Lightyear 0.26, we receive messages via MessageReceiver component
    mut client_query: Query<
        &mut MessageReceiver<BulletImpact>,
        (With<crate::GameClient>, With<Connected>),
    >,
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    debug_mode: Res<WeaponDebugMode>,
    mut debug_trails: ResMut<DebugBulletTrails>,
) {
    let Some(weapon_visuals) = weapon_visuals else {
        return;
    };

    let now = time.elapsed_secs();

    // Keep trails bounded even if debug is toggled on/off
    debug_trails
        .trails
        .retain(|(_trail, spawn_time, _color)| now - *spawn_time <= 10.0);

    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };

    for impact in receiver.receive() {
        let normal = impact.impact_normal.normalize_or_zero();
        let offset = if normal.length_squared() > 0.001 {
            normal * 0.03
        } else {
            Vec3::Y * 0.03
        };

        match impact.surface {
            BulletImpactSurface::Terrain | BulletImpactSurface::PracticeWall => {
                // Choose shared marker material by surface type (no per-hit allocation)
                let (marker_material, radius) = match impact.surface {
                    BulletImpactSurface::Terrain => {
                        (weapon_visuals.impact_terrain_material.clone(), 0.35)
                    }
                    BulletImpactSurface::PracticeWall => {
                        (weapon_visuals.impact_wall_material.clone(), 0.25)
                    }
                    _ => unreachable!(),
                };

                // Rotate so the disk is flush with the surface
                let rot = if normal.length_squared() > 0.001 {
                    Quat::from_rotation_arc(Vec3::Y, normal)
                } else {
                    Quat::IDENTITY
                };

                commands.spawn((
                    ImpactMarker {
                        spawn_time: now,
                        lifetime: 6.0,
                        base_scale: radius,
                    },
                    Mesh3d(weapon_visuals.impact_disk_mesh_unit.clone()),
                    MeshMaterial3d(marker_material),
                    Transform::from_translation(impact.impact_position + offset)
                        .with_rotation(rot)
                        .with_scale(Vec3::splat(radius)),
                    NotShadowCaster,
                ));
            }
            BulletImpactSurface::Player | BulletImpactSurface::Npc => {
                // Blood feedback on character hits (NPCs + players)
                spawn_blood_splatter(
                    &mut commands,
                    &weapon_visuals,
                    terrain.as_deref(),
                    impact.impact_position,
                    impact.impact_normal,
                    now,
                );
            }
        }

        // Debug trail: simulate the ballistic path from spawn -> impact using initial velocity
        if debug_mode.0 {
            let spawn = impact.spawn_position;
            let target = impact.impact_position;
            let v0 = impact.initial_velocity;

            let dist = (target - spawn).length().max(1.0);
            let speed = v0.length().max(1.0);
            let est_time = (dist / speed).clamp(0.02, 5.0);

            let dt = 1.0 / 600.0; // higher sim rate for smooth debug lines
            let mut steps = (est_time / dt).ceil() as usize;
            steps = steps.clamp(8, 2000);

            let mut points = Vec::with_capacity(steps + 2);
            let mut pos = spawn;
            let mut vel = v0;
            points.push(pos);

            for _ in 0..steps {
                let (new_pos, new_vel) = ballistics::step_bullet_physics(pos, vel, dt);
                pos = new_pos;
                vel = new_vel;
                points.push(pos);

                // Stop early if we're very close to the impact point
                if (pos - target).length_squared() < 4.0 {
                    break;
                }
            }

            points.push(target);
            debug_trails
                .trails
                .push((points, now, Color::srgb(1.0, 0.0, 0.0)));
        }
    }
}

/// Handle hit confirmations from server
pub fn handle_hit_confirms(
    mut commands: Commands,
    mut client_query: Query<
        &mut MessageReceiver<HitConfirm>,
        (With<crate::GameClient>, With<Connected>),
    >,
    time: Res<Time>,
) {
    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };

    for confirm in receiver.receive() {
        debug!(
            "Hit confirmed! Damage: {:.1}, Zone: {:?}, Part: {:?}, Headshot: {}, Kill: {}",
            confirm.damage, confirm.hit_zone, confirm.body_part, confirm.headshot, confirm.kill
        );

        // Spawn hit marker
        crosshair::spawn_hit_marker(
            &mut commands,
            &time,
            confirm.kill,
            confirm.hit_zone,
            confirm.body_part,
            confirm.damage,
        );
    }
}
