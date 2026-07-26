//! animation systems.

use super::*;
use shared::player::{PLAYER_SPEED, PLAYER_SPRINT_MULT};

/// Once the model scene hierarchy is spawned, attach the animation graph to the glTF player.
pub fn setup_player_rig(
    mut commands: Commands,
    character_assets: Option<Res<PlayerCharacterAssets>>,
    model_roots: Query<(Entity, &PlayerModelRoot), With<NeedsPlayerRigSetup>>,
    children_q: Query<&Children>,
    anim_players: Query<&AnimationPlayer>,
    parents_q: Query<&ChildOf>,
) {
    let Some(character_assets) = character_assets else {
        return;
    };

    for (model_root, model_info) in model_roots.iter() {
        // Cache the owning player entity (parent of the PlayerModelRoot).
        let Ok(owner) = parents_q.get(model_root).map(|p| p.parent()) else {
            continue;
        };

        let Some(assets) = character_assets.characters.get(&model_info.character) else {
            commands.entity(model_root).remove::<NeedsPlayerRigSetup>();
            continue;
        };

        // Find the first AnimationPlayer in the spawned scene (glTF loader adds this when animations exist).
        let mut stack: Vec<Entity> = vec![model_root];
        let mut rig_root: Option<Entity> = None;
        while let Some(e) = stack.pop() {
            if anim_players.get(e).is_ok() {
                rig_root = Some(e);
                break;
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter());
            }
        }

        let Some(rig_root) = rig_root else {
            // Scene not spawned yet (assets still loading).
            continue;
        };

        // Attach animation graph and state to the animation root.
        commands.entity(rig_root).insert((
            PlayerAnimationRoot,
            PlayerRigOwner(owner),
            PlayerRigCharacter(model_info.character),
            PlayerAnimState::default(),
            AnimationGraphHandle(assets.animation_graph.clone()),
        ));

        // Mark done so we don't redo the hierarchy walk every frame.
        commands.entity(model_root).remove::<NeedsPlayerRigSetup>();
    }
}

/// Helper to get the animation node for a movement animation
fn movement_anim_to_node(anim: MovementAnim, assets: &CharacterAssets) -> AnimationNodeIndex {
    match anim {
        MovementAnim::Idle => assets.idle_node,
        MovementAnim::Walk => assets.walk_node,
        MovementAnim::WalkBack => assets.walk_back_node,
        MovementAnim::StrafeLeft => assets.strafe_left_node,
        MovementAnim::StrafeRight => assets.strafe_right_node,
        MovementAnim::Run => assets.run_node,
        MovementAnim::Jump => assets.jump_node,
        MovementAnim::Fall => assets.fall_node,
    }
}

/// Determine target animation based on input state (for local player)
fn determine_local_target_anim(
    input: &crate::input::InputState,
    speed_xz: f32,
    was_moving: bool,
) -> MovementAnim {

    let forward = input.forward;
    let backward = input.backward;
    let left = input.left;
    let right = input.right;

    let has_forward = forward && !backward;
    let has_backward = backward && !forward;
    let has_left = left && !right;
    let has_right = right && !left;

    let moving_input = has_forward || has_backward || has_left || has_right;
    if !moving_input {
        return MovementAnim::Idle;
    }
    let moving = if was_moving {
        speed_xz > MOVE_STOP_SPEED
    } else {
        speed_xz > MOVE_START_SPEED
    };
    if !moving {
        return MovementAnim::Idle;
    }

    if has_forward {
        if input.shift {
            MovementAnim::Run
        } else {
            MovementAnim::Walk
        }
    } else if has_backward {
        MovementAnim::WalkBack
    } else if has_left {
        MovementAnim::StrafeLeft
    } else if has_right {
        MovementAnim::StrafeRight
    } else {
        MovementAnim::Idle
    }
}

/// Determine target animation based on movement direction + speed (for remote players)
fn determine_remote_target_anim(
    speed_xz: f32,
    move_dir_xz: Vec2,
    forward_xz: Vec2,
    right_xz: Vec2,
    was_moving: bool,
) -> MovementAnim {
    if speed_xz < MOVE_STOP_SPEED {
        return MovementAnim::Idle;
    }
    if speed_xz < MOVE_START_SPEED && !was_moving {
        return MovementAnim::Idle;
    }

    let move_dir = move_dir_xz.normalize_or_zero();
    let forward = forward_xz.normalize_or_zero();
    let right = right_xz.normalize_or_zero();

    let dot_forward = move_dir.dot(forward);
    let dot_right = move_dir.dot(right);

    if dot_forward.abs() >= dot_right.abs() {
        if dot_forward >= 0.0 {
            let run_speed_threshold = PLAYER_SPEED * (1.0 + PLAYER_SPRINT_MULT) * 0.5;
            if speed_xz > run_speed_threshold {
                MovementAnim::Run
            } else {
                MovementAnim::Walk
            }
        } else {
            MovementAnim::WalkBack
        }
    } else if dot_right >= 0.0 {
        MovementAnim::StrafeRight
    } else {
        MovementAnim::StrafeLeft
    }
}

/// Drive the player animations with movement + falling + smooth blending:
/// - Local player: animation based on input
/// - Remote players: animation based on smoothed speed
/// - Falling animation triggers on downward velocity
/// - Crossfade blending between animation states over 0.2 seconds
pub fn update_player_animation(
    character_assets: Option<Res<PlayerCharacterAssets>>,
    input_state: Res<crate::input::InputState>,
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    mut anim_roots: Query<
        (
            &PlayerRigOwner,
            &PlayerRigCharacter,
            &mut PlayerAnimState,
            &mut AnimationPlayer,
        ),
        With<PlayerAnimationRoot>,
    >,
    local_players: Query<(), With<LocalPlayer>>,
    players_with_health: Query<&Health, With<Player>>,
    player_transforms: Query<&Transform, With<Player>>,
    player_jump_states: Query<&PlayerJumpState, With<Player>>,
    player_water_states: Query<&PlayerWaterState, With<Player>>,
) {
    let Some(character_assets) = character_assets else {
        return;
    };
    let dt = time.delta_secs().max(1e-6);
    for (owner, rig_character, mut state, mut player) in anim_roots.iter_mut() {
        let Some(assets) = character_assets.characters.get(&rig_character.0) else {
            continue;
        };
        // Check if this player is dead
        let is_dead = players_with_health
            .get(owner.0)
            .map(|h| h.is_dead())
            .unwrap_or(false);

        // Handle death animation with the current fallback clip.
        if is_dead && !state.dead {
            player.stop_all();
            player.start(assets.fall_node);
            state.dead = true;
            state.current_anim = MovementAnim::Fall;
            state.target_anim = MovementAnim::Fall;
            state.blend_progress = 1.0;
            continue;
        }

        // If dead, keep death animation (don't switch back)
        if state.dead {
            // Check if player respawned (health restored)
            if !is_dead {
                state.dead = false;
                player.stop_all();
                player.start(assets.idle_node).repeat();
                state.current_anim = MovementAnim::Idle;
                state.target_anim = MovementAnim::Idle;
                state.blend_progress = 1.0;
            }
            continue;
        }

        // Get current position and compute velocities
        let mut current_pos: Option<Vec3> = None;
        let (vert_velocity, speed_xz, move_dir_xz, forward_xz, right_xz) =
            if let Ok(transform) = player_transforms.get(owner.0) {
                let pos = transform.translation;
                current_pos = Some(pos);
                let forward = transform.rotation * Vec3::NEG_Z;
                let right = transform.rotation * Vec3::X;
                if state.initialized {
                    let delta = pos - state.last_pos;
                    let vy = delta.y / dt;
                    let raw_speed_xz = Vec2::new(delta.x, delta.z).length() / dt;

                    // Smooth the horizontal speed for stable animation selection
                    let smooth_rate = 8.0;
                    state.smoothed_speed +=
                        (raw_speed_xz - state.smoothed_speed) * (1.0 - (-smooth_rate * dt).exp());

                    state.last_pos = pos;
                    state.last_y = pos.y;
                    (
                        vy,
                        state.smoothed_speed,
                        Vec2::new(delta.x, delta.z),
                        Vec2::new(forward.x, forward.z),
                        Vec2::new(right.x, right.z),
                    )
                } else {
                    state.initialized = true;
                    state.last_pos = pos;
                    state.last_y = pos.y;
                    state.smoothed_speed = 0.0;
                    (
                        0.0,
                        0.0,
                        Vec2::ZERO,
                        Vec2::new(forward.x, forward.z),
                        Vec2::new(right.x, right.z),
                    )
                }
            } else {
                (
                    0.0,
                    state.smoothed_speed,
                    Vec2::ZERO,
                    Vec2::new(0.0, -1.0),
                    Vec2::X,
                )
            };

        let is_local = local_players.contains(owner.0);
        let in_water = player_water_states
            .get(owner.0)
            .map(|s| s.in_water)
            .unwrap_or(false);

        let terrain_grounded = if is_local {
            if let (Some(pos), Some(terrain)) = (current_pos, terrain.as_ref()) {
                let ground_y = terrain.get_height(pos.x, pos.z) + ground_clearance_center();
                (pos.y - ground_y).abs() <= (GROUND_SNAP_DISTANCE + 0.05)
            } else {
                false
            }
        } else {
            false
        };

        let was_moving = matches!(
            state.target_anim,
            MovementAnim::Walk
                | MovementAnim::Run
                | MovementAnim::WalkBack
                | MovementAnim::StrafeLeft
                | MovementAnim::StrafeRight
        );

        let jump_pressed = is_local && input_state.jump && !in_water;
        let jump_just_pressed = jump_pressed && !state.last_jump_pressed;
        let fall_looks_like_jump = assets.fall_node == assets.jump_node;
        let remote_jump_timer = if !is_local {
            player_jump_states
                .get(owner.0)
                .map(|s| s.timer)
                .unwrap_or(0.0)
        } else {
            0.0
        };

        if in_water {
            state.airborne = false;
            state.jump_timer = 0.0;
            state.landing_timer = 0.0;
            state.airborne_from_jump = false;
        } else if is_local {
            // Local player: drive jump animation from actual jump input.
            if jump_just_pressed && !state.airborne {
                state.airborne = true;
                state.jump_timer = JUMP_ANIM_MIN_SECS;
                state.landing_timer = 0.0;
                state.airborne_from_jump = true;
            } else if !terrain_grounded && vert_velocity < FALL_VELOCITY_THRESHOLD {
                // Allow falling animation when actually leaving the ground.
                state.airborne = true;
                state.airborne_from_jump = false;
            } else if vert_velocity < FALL_VELOCITY_THRESHOLD {
                // Ignore shallow downhill slopes for local jump/fall.
            }

            if state.airborne {
                if state.jump_timer > 0.0 {
                    state.jump_timer = (state.jump_timer - dt).max(0.0);
                    state.landing_timer = 0.0;
                } else if vert_velocity.abs() < LANDING_VELOCITY_THRESHOLD {
                    state.landing_timer += dt;
                    if state.landing_timer >= LANDING_CONFIRM_SECS {
                        state.airborne = false;
                        state.landing_timer = 0.0;
                        state.airborne_from_jump = false;
                    }
                } else {
                    state.landing_timer = 0.0;
                }
            } else {
                state.jump_timer = 0.0;
                state.landing_timer = 0.0;
                state.airborne_from_jump = false;
            }
        } else {
            // Remote players: use server-authoritative jump state only.
            if remote_jump_timer > 0.0 {
                state.airborne = true;
                state.jump_timer = remote_jump_timer;
                state.airborne_from_jump = true;
            } else {
                state.airborne = false;
                state.jump_timer = 0.0;
                state.airborne_from_jump = false;
            }
            state.landing_timer = 0.0;
        }

        state.last_jump_pressed = if is_local { input_state.jump } else { false };

        let allow_fall_anim = !fall_looks_like_jump || state.airborne_from_jump;
        let target_anim = if state.airborne && state.jump_timer > 0.0 {
            MovementAnim::Jump
        } else if state.airborne && allow_fall_anim {
            MovementAnim::Fall
        } else if is_local {
            determine_local_target_anim(&input_state, speed_xz, was_moving)
        } else {
            determine_remote_target_anim(speed_xz, move_dir_xz, forward_xz, right_xz, was_moving)
        };

        // Check if we need to start a new transition
        if target_anim != state.target_anim {
            let target_node = movement_anim_to_node(target_anim, assets);
            let existing_target_node = movement_anim_to_node(state.target_anim, assets);
            if target_node == existing_target_node {
                state.target_anim = target_anim;
                continue;
            }
            // If we were in the middle of a transition, snap to current target first
            if state.blend_progress < 1.0 {
                let old_current_node = movement_anim_to_node(state.current_anim, assets);
                player.stop(old_current_node);
                state.current_anim = state.target_anim;
            }

            // Start new transition
            state.target_anim = target_anim;
            state.blend_progress = 0.0;

            // Start target animation at weight 0
            match target_anim {
                MovementAnim::Fall | MovementAnim::Jump => {
                    player.start(target_node).set_weight(0.0);
                }
                _ => {
                    player.start(target_node).repeat().set_weight(0.0);
                }
            }
        }

        // Update blend progress
        if state.blend_progress < 1.0 {
            state.blend_progress = (state.blend_progress + dt / ANIM_BLEND_DURATION).min(1.0);

            let current_node = movement_anim_to_node(state.current_anim, assets);
            let target_node = movement_anim_to_node(state.target_anim, assets);

            // Apply weights for crossfade
            let current_weight = 1.0 - state.blend_progress;
            let target_weight = state.blend_progress;

            if let Some(anim) = player.animation_mut(current_node) {
                anim.set_weight(current_weight);
            }
            if let Some(anim) = player.animation_mut(target_node) {
                anim.set_weight(target_weight);
            }

            // Transition complete - stop old animation
            if state.blend_progress >= 1.0 {
                player.stop(current_node);
                state.current_anim = state.target_anim;
            }
        } else {
            // Ensure current animation is playing (important on first frame after rig setup)
            let current_node = movement_anim_to_node(state.current_anim, assets);
            if !player.is_playing_animation(current_node) {
                match state.current_anim {
                    MovementAnim::Fall | MovementAnim::Jump => {
                        player.start(current_node);
                    }
                    _ => {
                        player.start(current_node).repeat();
                    }
                }
            }
        }
    }
}
