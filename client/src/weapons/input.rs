//! input systems.

use super::*;

pub fn cleanup_shoot_input_suppress(time: Res<Time>, mut suppress: ResMut<ShootInputSuppress>) {
    suppress.suppress_until = time.elapsed_secs() + 0.35;
    suppress.suppress_until_release = true;
}

/// Handle shooting input and send requests to server
pub fn handle_shoot_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut shooting_state: ResMut<ShootingState>,
    mut reload_state: ResMut<ReloadState>,
    mut suppress: ResMut<ShootInputSuppress>,
    // In Lightyear 0.26, we send messages via MessageSender component - typed on message type
    mut client_query: Query<
        &mut MessageSender<ShootRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut input_state: ResMut<InputState>,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    camera: Query<&Transform, With<Camera3d>>,
    time: Res<Time>,
    game_state: Res<State<GameState>>,
    mut last_warn_time: Local<f32>,
    // Cursor state - don't shoot if cursor isn't grabbed yet (first click grabs, doesn't fire)
    windows: Query<Entity, With<PrimaryWindow>>,
    cursor_opts: Query<&CursorOptions>,
    mut cursor_guard: Local<CursorGrabShootGuard>,
) {
    // Reset flags each frame
    shooting_state.shot_fired_this_frame = false;
    shooting_state.out_of_ammo_this_frame = false;
    shooting_state.weapon_fired = None;

    // Don't shoot if paused
    if game_state.get() != &GameState::Playing {
        return;
    }

    // --- Cursor grab guard (prevents focus-click from shooting) ---
    let cursor_locked = windows
        .single()
        .ok()
        .and_then(|window_entity| cursor_opts.get(window_entity).ok())
        .map(|c| c.grab_mode == CursorGrabMode::Locked)
        .unwrap_or(false);

    let last_locked = cursor_guard.last_locked.unwrap_or(cursor_locked);
    cursor_guard.last_locked = Some(cursor_locked);

    // If we just locked this frame, suppress until LMB is released once.
    if cursor_locked && !last_locked {
        cursor_guard.suppress_until_release = true;
        return;
    }

    // While suppressing, keep eating input until the user releases LMB.
    if cursor_guard.suppress_until_release {
        if mouse.pressed(MouseButton::Left) {
            return;
        }
        cursor_guard.suppress_until_release = false;
    }

    // Still require cursor lock to shoot.
    if !cursor_locked {
        return;
    }

    let current_time = time.elapsed_secs();
    if suppress.suppress_until_release {
        if mouse.pressed(MouseButton::Left) {
            return;
        }
        suppress.suppress_until_release = false;
    }
    if current_time < suppress.suppress_until {
        return;
    }

    // Don't shoot while dead
    if input_state.is_dead {
        return;
    }

    // Don't shoot while modal UI is open.
    if input_state.ui_blocking() {
        return;
    }

    // Don't shoot while in vehicle
    if input_state.in_vehicle {
        return;
    }

    let Ok(weapon) = local_player.single() else {
        return;
    };

    let Ok(camera_transform) = camera.single() else {
        return;
    };

    // Clear local reload state if weapon changed or reload completed.
    if reload_state.weapon_type != Some(weapon.weapon_type)
        || (reload_state.expected_ammo > 0 && weapon.ammo_in_mag >= reload_state.expected_ammo)
    {
        reload_state.clear();
    }

    // Left click to fire
    let fire_pressed = mouse.pressed(MouseButton::Left);

    // Block shooting while reloading (client-side prediction).
    if reload_state.is_reloading(current_time, weapon.weapon_type) {
        shooting_state.fire_held = fire_pressed;
        return;
    }

    // Check fire rate
    let cooldown = weapon.weapon_type.fire_cooldown();
    let cooldown_passed = (current_time - shooting_state.last_fire_time) >= cooldown;
    let has_ammo = weapon.ammo_in_mag > 0;

    // Out of ammo click (only on just pressed, not held, and with rate limiting)
    if fire_pressed
        && !has_ammo
        && cooldown_passed
        && current_time - shooting_state.last_out_of_ammo_time > 0.3
    {
        shooting_state.out_of_ammo_this_frame = true;
        shooting_state.last_out_of_ammo_time = current_time;
    }

    let can_fire = has_ammo && cooldown_passed;

    if fire_pressed && can_fire {
        shooting_state.last_fire_time = current_time;

        // Get aim direction from camera
        let direction = camera_transform.forward().as_vec3();

        // Use the toggled ADS state from input (not mouse.pressed since we switched to toggle)
        let aiming = input_state.aiming;

        // Send shoot request to server via MessageSender
        if let Ok(mut sender) = client_query.single_mut() {
            sender.send::<ReliableChannel>(ShootRequest {
                direction,
                pitch: input_state.pitch,
                aiming,
            });
        } else if current_time - *last_warn_time > 1.0 {
            // If this fires, you'll hear local SFX but the server will never spawn bullets / consume ammo.
            warn!("handle_shoot_input: missing GameClient+Connected+MessageSender<ShootRequest>; shoot requests not sent");
            *last_warn_time = current_time;
        }

        // === APPLY RECOIL ===
        let stats = weapon.weapon_type.stats();

        // Accumulation multiplier based on burst length (more shots = more recoil)
        let burst_mult = RECOIL_ACCUMULATION_MULT.powi(shooting_state.shots_in_burst as i32);

        // ADS reduces recoil
        let ads_mult = if aiming { RECOIL_ADS_MULTIPLIER } else { 1.0 };

        // Calculate recoil for this shot
        let vertical_recoil = stats.recoil_vertical * burst_mult * ads_mult;
        let horizontal_recoil = stats.recoil_horizontal * burst_mult * ads_mult;

        // Random horizontal direction (left or right)
        let h_direction = if rand::random::<bool>() { 1.0 } else { -1.0 };
        // Add some randomness to horizontal (not always max)
        let h_random = 0.3 + rand::random::<f32>() * 0.7;

        // Apply recoil to camera pitch (kick up) and yaw (kick sideways)
        input_state.pitch += vertical_recoil;
        input_state.yaw += horizontal_recoil * h_direction * h_random;

        // Clamp pitch to valid range
        input_state.pitch = input_state.pitch.clamp(
            -std::f32::consts::FRAC_PI_2 + 0.01,
            std::f32::consts::FRAC_PI_2 - 0.01,
        );

        // Track accumulated recoil (for recovery system)
        shooting_state.accumulated_recoil_pitch += vertical_recoil;
        shooting_state.accumulated_recoil_yaw += horizontal_recoil * h_direction * h_random;

        // Increment burst counter
        shooting_state.shots_in_burst += 1;

        // Mark that we fired for audio system
        shooting_state.shot_fired_this_frame = true;
        shooting_state.weapon_fired = Some(weapon.weapon_type);

        // Local muzzle smoke (immediate feedback in first-person)
        if input_state.camera_mode == CameraMode::FirstPerson {
            if let Some(visuals) = weapon_visuals.as_ref() {
                let muzzle_pos = camera_transform.translation
                    + camera_transform.rotation * muzzle_offset(weapon.weapon_type);
                let direction = camera_transform.forward().as_vec3();
                spawn_muzzle_flash(
                    &mut commands,
                    visuals,
                    muzzle_pos,
                    direction,
                    weapon.weapon_type,
                );
                spawn_muzzle_smoke(
                    &mut commands,
                    visuals,
                    muzzle_pos,
                    direction,
                    weapon.weapon_type,
                );
            }
        }
    }

    shooting_state.fire_held = fire_pressed;
}

/// Recover recoil over time when not shooting
pub fn update_recoil_recovery(
    mut shooting_state: ResMut<ShootingState>,
    mut input_state: ResMut<InputState>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let current_time = time.elapsed_secs();

    // Reset burst counter if haven't shot recently
    if current_time - shooting_state.last_fire_time > RECOIL_BURST_RESET_TIME {
        shooting_state.shots_in_burst = 0;
    }

    // Recover recoil gradually (pull aim back down)
    if shooting_state.accumulated_recoil_pitch.abs() > 0.001 {
        let recovery = RECOIL_RECOVERY_SPEED * dt;

        // Recover pitch (vertical)
        if shooting_state.accumulated_recoil_pitch > 0.0 {
            let recover_amount = recovery.min(shooting_state.accumulated_recoil_pitch);
            input_state.pitch -= recover_amount;
            shooting_state.accumulated_recoil_pitch -= recover_amount;
        } else {
            let recover_amount = recovery.min(-shooting_state.accumulated_recoil_pitch);
            input_state.pitch += recover_amount;
            shooting_state.accumulated_recoil_pitch += recover_amount;
        }

        // Clamp pitch
        input_state.pitch = input_state.pitch.clamp(
            -std::f32::consts::FRAC_PI_2 + 0.01,
            std::f32::consts::FRAC_PI_2 - 0.01,
        );
    }

    // Recover yaw (horizontal) - faster recovery
    if shooting_state.accumulated_recoil_yaw.abs() > 0.001 {
        let recovery = RECOIL_RECOVERY_SPEED * 1.5 * dt;

        if shooting_state.accumulated_recoil_yaw > 0.0 {
            let recover_amount = recovery.min(shooting_state.accumulated_recoil_yaw);
            input_state.yaw -= recover_amount;
            shooting_state.accumulated_recoil_yaw -= recover_amount;
        } else {
            let recover_amount = recovery.min(-shooting_state.accumulated_recoil_yaw);
            input_state.yaw += recover_amount;
            shooting_state.accumulated_recoil_yaw += recover_amount;
        }
    }
}

/// Handle reload input - sends request to server
pub fn handle_reload_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut client_query: Query<
        &mut MessageSender<ReloadRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    local_player: Query<(&EquippedWeapon, &shared::items::Inventory), With<LocalPlayer>>,
    input_state: Res<InputState>,
    time: Res<Time>,
    mut reload_state: ResMut<ReloadState>,
) {
    // Reset flag each frame
    reload_state.reload_requested_this_frame = false;

    // Don't reload while dead
    if input_state.is_dead {
        return;
    }

    // Don't reload in vehicle
    if input_state.in_vehicle {
        return;
    }

    // Don't reload while modal UI is open.
    if input_state.ui_blocking() {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyR) {
        if let Ok((weapon, inventory)) = local_player.single() {
            let current_time = time.elapsed_secs();
            if reload_state.is_reloading(current_time, weapon.weapon_type) {
                return;
            }

            // Only send reload request if we actually need ammo and have reserve in inventory
            let stats = weapon.weapon_type.stats();
            let needed = stats.magazine_size.saturating_sub(weapon.ammo_in_mag);
            let reserve_in_inventory = inventory.count_item(weapon.weapon_type.ammo_type());
            let pending = needed.min(reserve_in_inventory);
            let duration = weapon.weapon_type.reload_duration(pending);

            if pending > 0 && duration > 0.0 {
                if let Ok(mut sender) = client_query.single_mut() {
                    sender.send::<ReliableChannel>(ReloadRequest);
                }
                reload_state.reload_requested_this_frame = true;
                reload_state.reload_started_at = current_time;
                reload_state.reload_duration = duration + CLIENT_RELOAD_PAD_SECS;
                reload_state.reload_until = current_time + reload_state.reload_duration;
                reload_state.weapon_type = Some(weapon.weapon_type);
                reload_state.expected_ammo = weapon.ammo_in_mag + pending;
                reload_state.shotgun_shells_to_load = if weapon.weapon_type == WeaponType::Shotgun {
                    pending
                } else {
                    0
                };
                reload_state.shotgun_shells_played = 0;
                reload_state.shotgun_close_played = false;
                info!("Reload requested...");
            }
        }
    }
}

/// Play weapon sound effects based on shooting/reload state
pub fn handle_weapon_sounds(
    mut commands: Commands,
    audio_assets: Option<Res<WeaponAudioAssets>>,
    shooting_state: Res<ShootingState>,
    time: Res<Time>,
    mut reload_state: ResMut<ReloadState>,
) {
    let Some(audio) = audio_assets else { return };

    // Play shot sound
    if shooting_state.shot_fired_this_frame {
        let sound = if let Some(weapon_type) = shooting_state.weapon_fired {
            match weapon_type {
                WeaponType::Shotgun => audio.shotgun_shot.clone(),
                WeaponType::Sniper => audio.sniper_shot.clone(),
                WeaponType::Pistol => audio.revolver_shot.clone(),
                WeaponType::AssaultRifle => audio.assault_shot.clone(),
                WeaponType::Unarmed => audio.assault_shot.clone(),
            }
        } else {
            audio.assault_shot.clone()
        };

        commands.spawn((
            AudioPlayer::new(sound),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
        ));
    }

    // Play out of ammo click
    if shooting_state.out_of_ammo_this_frame {
        commands.spawn((
            AudioPlayer::new(audio.out_of_ammo.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.6)),
        ));
    }

    // Play reload sound
    if reload_state.reload_requested_this_frame
        && reload_state.weapon_type != Some(WeaponType::Shotgun)
    {
        let reload_sound = match reload_state.weapon_type {
            Some(WeaponType::AssaultRifle) => audio.assault_reload.clone(),
            Some(WeaponType::Pistol) => audio.revolver_reload.clone(),
            Some(WeaponType::Sniper) => audio.sniper_reload.clone(),
            Some(WeaponType::Unarmed) | None => audio.gun_reload.clone(),
            Some(WeaponType::Shotgun) => audio.shotgun_reload.clone(),
        };
        commands.spawn((
            AudioPlayer::new(reload_sound),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
        ));
    }

    // Shotgun: play insert per shell, then close at end.
    if reload_state.weapon_type == Some(WeaponType::Shotgun) {
        let now = time.elapsed_secs();
        if reload_state.is_reloading(now, WeaponType::Shotgun) {
            let base = WeaponType::Shotgun.stats().reload_time.max(0.0);
            let shells_total = reload_state.shotgun_shells_to_load;
            if base > 0.0 && shells_total > 0 {
                let elapsed = (now - reload_state.reload_started_at).max(0.0);
                let target_shells = (elapsed / base).floor() as u32 + 1;
                let max_shells = shells_total.min(target_shells);

                while reload_state.shotgun_shells_played < max_shells {
                    commands.spawn((
                        AudioPlayer::new(audio.shotgun_reload.clone()),
                        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
                    ));
                    reload_state.shotgun_shells_played += 1;
                }
            }
        }

        if !reload_state.shotgun_close_played && reload_state.shotgun_shells_to_load > 0 {
            let finish_at = reload_state.reload_started_at
                + (reload_state.reload_duration - CLIENT_RELOAD_PAD_SECS).max(0.0);
            if now >= finish_at {
                commands.spawn((
                    AudioPlayer::new(audio.gun_reload.clone()),
                    PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
                ));
                reload_state.shotgun_close_played = true;
            }
        }
    }
}
