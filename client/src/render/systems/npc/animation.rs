//! animation systems.

use super::*;

fn npc_anim_set_for(assets: &NpcAssets) -> NpcAnimSet {
    NpcAnimSet {
        idle: assets.idle_node,
        walk: assets.jog_forward_node,
        run: assets.running_node,
        look_behind_run: assets.look_behind_run_node,
        death: assets.tpose_node,
    }
}

/// Helper to get the animation node for an NPC movement animation
fn npc_movement_anim_to_node(anim: NpcMovementAnim, set: &NpcAnimSet) -> AnimationNodeIndex {
    match anim {
        NpcMovementAnim::Idle => set.idle,
        NpcMovementAnim::Walk => set.walk,
        NpcMovementAnim::Run => set.run,
        NpcMovementAnim::LookBehindRun => set.look_behind_run,
    }
}

/// Determine target animation based on movement speed (Oilman thresholds).
fn determine_npc_target_anim(
    speed_xz: f32,
    current_anim: NpcMovementAnim,
    is_fleeing: bool,
) -> NpcMovementAnim {
    if is_fleeing {
        return NpcMovementAnim::LookBehindRun;
    }

    // Custom thresholds to keep walk slow and allow run on flee.
    const WALK_START: f32 = 0.08;
    const WALK_STOP: f32 = 0.03;
    const RUN_START: f32 = 3.0;
    const RUN_STOP: f32 = 2.4;

    match current_anim {
        NpcMovementAnim::Run | NpcMovementAnim::LookBehindRun => {
            if speed_xz < RUN_STOP {
                if speed_xz > WALK_START {
                    NpcMovementAnim::Walk
                } else {
                    NpcMovementAnim::Idle
                }
            } else {
                NpcMovementAnim::Run
            }
        }
        NpcMovementAnim::Walk => {
            if speed_xz > RUN_START {
                NpcMovementAnim::Run
            } else if speed_xz < WALK_STOP {
                NpcMovementAnim::Idle
            } else {
                NpcMovementAnim::Walk
            }
        }
        _ => {
            if speed_xz > RUN_START {
                NpcMovementAnim::Run
            } else if speed_xz > WALK_START {
                NpcMovementAnim::Walk
            } else {
                NpcMovementAnim::Idle
            }
        }
    }
}

/// Drive NPC animations with motion-based directional movement and smooth blending:
/// - Animation selected based on movement speed
/// - Crossfade blending between animation states over 0.2 seconds
/// - Dead NPCs play death animation
pub fn update_npc_animation(
    assets: Option<Res<NpcAssets>>,
    time: Res<Time>,
    npc_roots: Query<(&Health, &Transform, &Visibility, Option<&NpcFleeing>), With<Npc>>,
    mut anim_roots: Query<
        (&NpcRigOwner, &mut NpcAnimState, &mut AnimationPlayer),
        With<NpcAnimationRoot>,
    >,
) {
    let Some(assets) = assets else { return };

    let dt = time.delta_secs().max(1e-6);

    for (owner, mut state, mut player) in anim_roots.iter_mut() {
        let Ok((health, transform, visibility, fleeing)) = npc_roots.get(owner.0) else {
            continue;
        };
        if matches!(*visibility, Visibility::Hidden) {
            continue;
        }
        let anim_set = npc_anim_set_for(&assets);

        let npc_pos = transform.translation;
        let is_fleeing = fleeing.is_some();
        let is_dead = health.is_dead();

        // Handle death animation
        if is_dead && !state.dead {
            player.stop_all();
            player.start(anim_set.death);
            state.dead = true;
            state.current_anim = NpcMovementAnim::Idle;
            state.target_anim = NpcMovementAnim::Idle;
            state.blend_progress = 1.0;
            continue;
        }

        // If dead, keep death animation (don't switch back)
        if state.dead {
            // Check if NPC respawned (health restored)
            if !is_dead {
                state.dead = false;
                player.stop_all();
                player.start(anim_set.idle).repeat();
                state.current_anim = NpcMovementAnim::Idle;
                state.target_anim = NpcMovementAnim::Idle;
                state.blend_progress = 1.0;
            }
            continue;
        }

        // Motion-based animation detection with speed smoothing
        let instant_speed = if state.initialized {
            let d = npc_pos - state.last_pos;
            state.last_pos = npc_pos;
            Vec2::new(d.x, d.z).length() / dt
        } else {
            state.initialized = true;
            state.last_pos = npc_pos;
            0.0
        };

        // Detect instant stops: if instant speed is very low and we're moving, snap to idle immediately
        // This bypasses the smoothing delay for responsive stop detection
        let allow_instant_stop = false;
        let is_instant_stop = allow_instant_stop
            && instant_speed < INSTANT_STOP_THRESHOLD
            && matches!(
                state.target_anim,
                NpcMovementAnim::Walk | NpcMovementAnim::Run | NpcMovementAnim::LookBehindRun
            );

        // For instant stops, reset smoothed speed immediately to trigger idle transition
        if is_instant_stop {
            state.smoothed_speed = 0.0;
        } else {
            // Normal exponential moving average for smooth speed (prevents jittery animation switching)
            let smooth_factor = 1.0 - (-NPC_SPEED_SMOOTHING * dt).exp();
            state.smoothed_speed =
                state.smoothed_speed + (instant_speed - state.smoothed_speed) * smooth_factor;
        }

        // Use smoothed speed with hysteresis for stable animation selection
        let target_anim =
            determine_npc_target_anim(state.smoothed_speed, state.target_anim, is_fleeing);

        // Check if we need to start a new transition
        if target_anim != state.target_anim {
            // If we were in the middle of a transition, snap to current target first
            if state.blend_progress < 1.0 {
                let old_current_node = npc_movement_anim_to_node(state.current_anim, &anim_set);
                player.stop(old_current_node);
                state.current_anim = state.target_anim;
            }

            // Check if this is a stopping transition (moving -> idle)
            let was_moving = matches!(
                state.target_anim,
                NpcMovementAnim::Walk | NpcMovementAnim::Run | NpcMovementAnim::LookBehindRun
            );
            let going_idle = matches!(target_anim, NpcMovementAnim::Idle);
            state.is_stopping = was_moving && going_idle;

            // Start new transition
            state.target_anim = target_anim;
            state.blend_progress = 0.0;

            // Start target animation at weight 0
            let target_node = npc_movement_anim_to_node(target_anim, &anim_set);
            player.start(target_node).repeat().set_weight(0.0);
        }

        // Update blend progress (faster for stopping transitions)
        if state.blend_progress < 1.0 {
            let blend_duration = if state.is_stopping {
                NPC_STOP_BLEND_DURATION
            } else {
                NPC_ANIM_BLEND_DURATION
            };
            state.blend_progress = (state.blend_progress + dt / blend_duration).min(1.0);

            let current_node = npc_movement_anim_to_node(state.current_anim, &anim_set);
            let target_node = npc_movement_anim_to_node(state.target_anim, &anim_set);

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
            let current_node = npc_movement_anim_to_node(state.current_anim, &anim_set);
            if !player.is_playing_animation(current_node) {
                player.start(current_node).repeat();
            }
        }
    }
}
