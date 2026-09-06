//! Shared character animation graphs, activity clips and visibility culling.

use super::appearance::{HeroAssets, HeroManifest, HeroPreviewRig};
use super::motion::HeroVisual;
use bevy::animation::AnimationTargetId;
use bevy::camera::primitives::{Frustum, Sphere};
use bevy::camera::visibility::ViewVisibility;
use bevy::gltf::Gltf;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::{CharacterActivity, CharacterKind, CharacterMotion};
use shared::economy::{CarriedLoad, PorterCartState};
use shared::player::HERO_MOVE_SPEED;

/// Animation graph handles, shared by every hero.
#[derive(Clone)]
pub struct HeroGraph {
    pub(super) handle: Handle<AnimationGraph>,
    /// Body clip nodes by manifest clip name.
    pub(super) body: HashMap<String, AnimationNodeIndex>,
    /// Face clip nodes by manifest clip name.
    pub(super) face: HashMap<String, AnimationNodeIndex>,
}

/// Link from the hero root to the AnimationPlayer entity inside its scene.
#[derive(Component)]
pub(super) struct HeroAnim {
    pub(super) player: Entity,
    pub(super) idle: Option<AnimationNodeIndex>,
    pub(super) walk: Option<AnimationNodeIndex>,
    pub(super) build: Option<AnimationNodeIndex>,
    pub(super) chop: Option<AnimationNodeIndex>,
    pub(super) combat: super::combat_animation::CombatClips,
    pub(super) harvest: Option<AnimationNodeIndex>,
    pub(super) carry: Option<AnimationNodeIndex>,
    pub(super) pull: Option<AnimationNodeIndex>,
    pub(super) sit_idle: Option<AnimationNodeIndex>,
    pub(super) current_body: Option<AnimationNodeIndex>,
    pub(super) fading_body: Option<AnimationNodeIndex>,
    pub(super) body_fade_seconds: f32,
    pub(super) paused: bool,
    /// Clip weights at the moment the rig was hidden, restored on unhide.
    /// Bevy evaluates every PAUSED clip in full each frame; only a weight of
    /// exactly 0 skips the graph walk, curve sampling and bone writes.
    pub(super) saved_weights: Vec<(AnimationNodeIndex, f32)>,
}

pub(super) const BODY_ANIMATION_FADE_SECONDS: f32 = 0.14;

/// Rigs whose root lies within this distance of the camera frustum keep
/// animating even when no view sees a single mesh part yet, so a character
/// stepping into frame arrives mid-stride instead of catching up by a frame.
pub(super) const RIG_ANIMATION_MARGIN: f32 = 6.0;

/// The mesh primitives of one character rig, gathered as they instantiate.
///
/// Lets the locomotion driver ask "does ANY view (camera or shadow cascade)
/// see this rig?" through the primitives' `ViewVisibility` without walking the
/// hierarchy every frame. Dead entries (a respawned scene) are pruned on use.
#[derive(Component, Default)]
pub(super) struct RigMeshParts(pub(super) Vec<Entity>);

/// Per-frame rig animation census, logged every 10 s under
/// `FISTFORCE_CLIENT_PERF=1` so a perf run states how many rigs Bevy actually
/// evaluated versus how many the visibility cull and the indoors pause skipped.
#[derive(Default)]
pub(super) struct RigAnimationTally {
    pub(super) enabled: Option<bool>,
    pub(super) since: f32,
    pub(super) frames: u32,
    pub(super) rigs: u64,
    pub(super) hidden: u64,
    pub(super) unseen: u64,
    pub(super) with_parts: u64,
    pub(super) any_seen: u64,
    pub(super) in_margin: u64,
}

/// Mask group ids. In Bevy a SET bit means "this node may not animate that
/// group", so the body layer masks FACE out and the face layer masks BODY out.
pub(super) const MASK_GROUP_FACE: u32 = 0;

pub(super) const MASK_GROUP_BODY: u32 = 1;

/// Bones whose name starts with this belong to the face layer. Everything else
/// is body — so a new bone can never silently escape a mask group.
pub(super) const FACE_BONE_PREFIX: &str = "eye.";

/// Clip names the locomotion blend expects to find in `body_clips`.
pub(super) const CLIP_IDLE: &str = "idle";

pub(super) const CLIP_WALK: &str = "walk";

/// Played while a villager works on a construction site.
pub(super) const CLIP_BUILD: &str = "build";

/// Played while working a tree and while walking with a physical load.
pub(super) const CLIP_CHOP: &str = "chop";

/// Played while cutting and gathering a wheat field.
pub(super) const CLIP_HARVEST: &str = "harvest";

pub(super) const CLIP_CARRY: &str = "carry";

/// Played while an active porter trip owns the world-space handcart.
pub(super) const CLIP_PULL: &str = "pull";

pub(super) const CLIP_SIT_IDLE: &str = "sit_idle";

/// Resting expression; face clips play on their own masked layer.
pub(super) const CLIP_FACE_IDLE: &str = "face_idle";

/// Wire the scene's [`AnimationPlayer`] to the shared masked graph.
///
/// Polls instead of `Added<AnimationPlayer>`: the graph can only be built once
/// the glTF asset AND an instantiated rig exist, and a one-shot trigger would
/// miss players that appear before that.
///
/// Masks are MANDATORY here. The exporter bakes every action over all 16
/// bones, so a face clip played unmasked drives the whole body to rest pose,
/// and a body clip played unmasked stamps `face_surprised`'s wide eyes on
/// permanently (the rig was left in that pose when the body actions baked).
#[allow(clippy::too_many_arguments)]
pub(super) fn setup_hero_animation(
    mut commands: Commands,
    manifest: Res<HeroManifest>,
    mut assets: ResMut<HeroAssets>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<(Entity, &mut AnimationPlayer), Without<AnimationGraphHandle>>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    children_q: Query<&Children>,
    rig_roots: Query<
        Entity,
        (
            Or<(With<CharacterKind>, With<HeroPreviewRig>)>,
            Without<HeroAnim>,
        ),
    >,
) {
    if players.is_empty() {
        return;
    }

    if assets.graph.is_none() {
        let Some(gltf) = assets.gltf.as_ref().and_then(|handle| gltfs.get(handle)) else {
            return;
        };
        // Mask groups are derived from the LIVE rig rather than a hardcoded
        // bone table: a renamed or added bone lands in the body group
        // automatically instead of silently escaping every mask.
        let Some((player_entity, _)) = players.iter().next() else {
            return;
        };
        let mut graph = AnimationGraph::new();
        let mut targets = 0usize;
        let mut face_targets = 0usize;
        let mut stack = vec![(player_entity, Vec::<String>::new())];
        while let Some((node, prefix)) = stack.pop() {
            let Ok(name) = names.get(node) else {
                continue;
            };
            // AnimationTargetId hashes the name path from the animation root
            // INCLUSIVE of the root's own name.
            let mut path = prefix;
            path.push(name.as_str().to_string());
            let leaf = name.as_str();
            let group = if leaf.starts_with(FACE_BONE_PREFIX) {
                face_targets += 1;
                MASK_GROUP_FACE
            } else {
                MASK_GROUP_BODY
            };
            graph.add_target_to_mask_group(AnimationTargetId::from_iter(path.iter()), group);
            targets += 1;
            if let Ok(children) = children_q.get(node) {
                for child in children.iter() {
                    stack.push((child, path.clone()));
                }
            }
        }
        if face_targets == 0 {
            // No eyes yet means the scene is still instantiating; a graph
            // built now would let face clips drive the body.
            return;
        }

        // Two layers off the root, each masking the other's group out.
        let body_layer = graph.add_blend_with_mask(1 << MASK_GROUP_FACE, 1.0, graph.root);
        let face_layer = graph.add_blend_with_mask(1 << MASK_GROUP_BODY, 1.0, graph.root);

        let mut body = HashMap::new();
        for clip_name in &manifest.body_clips {
            if let Some(clip) = gltf.named_animations.get(clip_name.as_str()) {
                body.insert(
                    clip_name.clone(),
                    graph.add_clip(clip.clone(), 1.0, body_layer),
                );
            } else {
                warn!("manifest body clip '{clip_name}' missing from the glb");
            }
        }
        let mut face = HashMap::new();
        for clip_name in &manifest.face_clips {
            if let Some(clip) = gltf.named_animations.get(clip_name.as_str()) {
                face.insert(
                    clip_name.clone(),
                    graph.add_clip(clip.clone(), 1.0, face_layer),
                );
            } else {
                warn!("manifest face clip '{clip_name}' missing from the glb");
            }
        }

        info!(
            "Hero animation graph: {targets} targets ({face_targets} face), \
             {} body + {} face clips",
            body.len(),
            face.len()
        );
        assets.graph = Some(HeroGraph {
            handle: graphs.add(graph),
            body,
            face,
        });
    }
    let hero_graph = assets.graph.clone().expect("graph built above");

    for (player_entity, mut player) in players.iter_mut() {
        // Only adopt players that live under a hero/preview root.
        let mut ancestor = player_entity;
        let mut rig_root = None;
        while let Ok(child_of) = parents.get(ancestor) {
            ancestor = child_of.parent();
            if let Ok(root) = rig_roots.get(ancestor) {
                rig_root = Some(root);
                break;
            }
        }
        let Some(rig_root) = rig_root else {
            continue;
        };

        commands
            .entity(player_entity)
            .try_insert(AnimationGraphHandle(hero_graph.handle.clone()));

        // Keep one steady body clip active. `drive_hero_locomotion` starts a
        // second only for its short crossfade and then removes the old clip.
        // Zero-weight clips are still traversed by Bevy's animation graph, so
        // pre-starting every possible job animation multiplied the cost of a
        // 160-person neighbourhood by seven.
        let idle = hero_graph.body.get(CLIP_IDLE).copied();
        let walk = hero_graph.body.get(CLIP_WALK).copied();
        let build = hero_graph.body.get(CLIP_BUILD).copied();
        let chop = hero_graph.body.get(CLIP_CHOP).copied();
        let harvest = hero_graph.body.get(CLIP_HARVEST).copied();
        let carry = hero_graph.body.get(CLIP_CARRY).copied();
        let pull = hero_graph.body.get(CLIP_PULL).copied();
        let sit_idle = hero_graph.body.get(CLIP_SIT_IDLE).copied();
        if let Some(idle) = idle {
            player.play(idle).repeat().set_weight(1.0);
        }
        // Resting expression on the face layer; body clips cannot touch it.
        if let Some(face_idle) = hero_graph.face.get(CLIP_FACE_IDLE).copied() {
            player.play(face_idle).repeat().set_weight(1.0);
        }

        // Both the glTF child and its replicated root may disappear between
        // query collection and deferred command application (disconnect or
        // world teardown). Cosmetic animation setup must never crash the
        // client on either stale entity.
        commands.entity(rig_root).try_insert(HeroAnim {
            player: player_entity,
            idle,
            walk,
            build,
            chop,
            combat: super::combat_animation::CombatClips {
                guard: hero_graph.body.get("combat_guard").copied(),
                strike: hero_graph.body.get("combat_strike").copied(),
                recoil: hero_graph.body.get("combat_recoil").copied(),
                fall: hero_graph.body.get("combat_fall").copied(),
            },
            harvest,
            carry,
            pull,
            sit_idle,
            current_body: idle,
            fading_body: None,
            body_fade_seconds: BODY_ANIMATION_FADE_SECONDS,
            paused: false,
            saved_weights: Vec::new(),
        });
        debug!("Hero animation configured for {rig_root:?}");
    }
}

pub(super) fn desired_body_animation(
    visual: &HeroVisual,
    anim: &HeroAnim,
    activity: Option<CharacterActivity>,
    carrying: bool,
    carting: bool,
    motion: Option<CharacterMotion>,
) -> (Option<AnimationNodeIndex>, f32, bool) {
    // A seated character can be translated by a moving parent such as a
    // vessel. That world-space speed is not locomotion: the feet must remain
    // in the authored seated pose while the boat moves beneath them.
    if activity == Some(CharacterActivity::Sitting) {
        return (anim.sit_idle.or(anim.idle), 1.0, false);
    }

    // Different start/stop thresholds keep small replicated speed noise from
    // continually restarting idle and walk.
    let current = anim.current_body;
    let was_locomoting = current.is_some()
        && (current == anim.walk || current == anim.carry || current == anim.pull);
    let observed_speed = visual
        .speed
        .max(motion.map_or(0.0, |motion| motion.velocity.length()));
    // Replicated motion is authoritative while a route is active. Visual
    // speed remains an important fallback for one-tick warp arrivals and
    // interpolation catch-up, where the server may already be stationary even
    // though the body is still visibly covering ground.
    let moving = motion.is_some_and(CharacterMotion::is_moving)
        || visual.speed > if was_locomoting { 0.10 } else { 0.24 };
    let stride_speed = (observed_speed / HERO_MOVE_SPEED).clamp(0.4, 1.6);

    if carting {
        return (
            anim.pull.or(anim.carry).or(anim.idle),
            if moving { stride_speed } else { 0.0 },
            !moving,
        );
    }
    if carrying {
        return (
            anim.carry.or(anim.idle),
            if moving { stride_speed } else { 0.0 },
            !moving,
        );
    }
    if moving {
        return (anim.walk.or(anim.idle), stride_speed, false);
    }

    let clip = match activity {
        Some(CharacterActivity::Chopping) => anim.chop,
        Some(CharacterActivity::Fighting) => anim.combat.guard,
        Some(CharacterActivity::Farming) => anim.harvest,
        Some(
            CharacterActivity::Fishing | CharacterActivity::Building | CharacterActivity::Mining,
        ) => anim.build,
        _ => anim.idle,
    }
    .or(anim.idle);
    (clip, 1.0, false)
}

/// Keep exactly one body animation active in steady state and at most two for
/// a short crossfade. The face loop remains a separate masked layer.
pub(super) fn drive_hero_locomotion(
    time: Res<Time>,
    mut heroes: Query<(
        Entity,
        &HeroVisual,
        &mut HeroAnim,
        Option<&InheritedVisibility>,
        Option<&CharacterActivity>,
        Option<&CarriedLoad>,
        Option<&PorterCartState>,
        Option<&CharacterMotion>,
        Option<&mut RigMeshParts>,
        Option<&GlobalTransform>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    frusta: Query<&Frustum, With<Camera3d>>,
    view_visibilities: Query<&ViewVisibility>,
    mut tally: Local<RigAnimationTally>,
    clocks: Query<&shared::components::WorldTime>,
    combat: Query<(
        Has<shared::components::CombatReady>,
        Option<&shared::components::CombatSwing>,
        Option<&shared::components::CombatReaction>,
    )>,
) {
    let census = *tally
        .enabled
        .get_or_insert_with(|| std::env::var("FISTFORCE_CLIENT_PERF").is_ok_and(|v| !v.is_empty()));
    if census {
        tally.frames += 1;
        tally.since += time.delta_secs();
        if tally.since >= 10.0 {
            let frames = tally.frames.max(1) as u64;
            info!(
                "ClientPerfRigs rigs={} animating={} unseen_culled={} hidden_or_indoors={} with_parts={} any_part_seen={} in_frustum_margin={} (per-frame averages)",
                tally.rigs / frames,
                (tally.rigs - tally.hidden - tally.unseen) / frames,
                tally.unseen / frames,
                tally.hidden / frames,
                tally.with_parts / frames,
                tally.any_seen / frames,
                tally.in_margin / frames
            );
            *tally = RigAnimationTally {
                enabled: Some(true),
                ..default()
            };
        }
    }
    let now = clocks.iter().next().map_or(0.0, |c| {
        f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
    });
    for (entity, visual, mut anim, inherited, activity, carried, cart, motion, parts, transform) in
        heroes.iter_mut()
    {
        // A rig no view can see (main camera AND every shadow cascade, per
        // last frame's ViewVisibility on its mesh parts) contributes nothing
        // to the image, so its animation is skipped entirely. The frustum
        // margin keeps rigs about to enter frame animating.
        let (has_parts, any_seen) = match parts {
            Some(mut parts) => {
                parts.0.retain(|part| view_visibilities.contains(*part));
                (
                    !parts.0.is_empty(),
                    parts
                        .0
                        .iter()
                        .any(|part| view_visibilities.get(*part).is_ok_and(|seen| seen.get())),
                )
            }
            None => (false, false),
        };
        let in_margin = transform.is_some_and(|transform| {
            let sphere = Sphere {
                center: transform.translation().into(),
                radius: RIG_ANIMATION_MARGIN,
            };
            frusta
                .iter()
                .any(|frustum| frustum.intersects_sphere(&sphere, false))
        });
        let unseen = has_parts && !any_seen && !in_margin;
        if census {
            tally.with_parts += u64::from(has_parts);
            tally.any_seen += u64::from(any_seen);
            tally.in_margin += u64::from(in_margin);
        }
        let indoors_or_hidden = inherited.is_some_and(|visibility| !visibility.get())
            || activity.is_some_and(|activity| *activity == CharacterActivity::Indoors);
        let hidden = indoors_or_hidden || unseen;
        if census {
            tally.rigs += 1;
            if indoors_or_hidden {
                tally.hidden += 1;
            } else if unseen {
                tally.unseen += 1;
            }
        }
        let Ok(mut player) = players.get_mut(anim.player) else {
            continue;
        };

        if hidden {
            if !anim.paused {
                // Pause freezes the seek times; zero weights are what make
                // Bevy's animate_targets skip the rig (weight == 0 is its
                // only per-clip early-out - a paused clip is still evaluated
                // and committed to every bone each frame).
                player.pause_all();
                anim.saved_weights.clear();
                for (node, active) in player.playing_animations_mut() {
                    anim.saved_weights.push((*node, active.weight()));
                    active.set_weight(0.0);
                }
                anim.paused = true;
            }
            continue;
        }
        if anim.paused {
            for (node, weight) in anim.saved_weights.drain(..) {
                if let Some(active) = player.animation_mut(node) {
                    active.set_weight(weight);
                }
            }
            player.resume_all();
            anim.paused = false;
        }

        let carting = cart.is_some();
        let carrying = !carting && carried.is_some_and(|load| !load.is_empty());
        let (mut desired, mut speed, freeze_at_contact) = desired_body_animation(
            visual,
            &anim,
            activity.copied(),
            carrying,
            carting,
            motion.copied(),
        );
        let combat_pose = combat
            .get(entity)
            .ok()
            .and_then(|(ready, swing, reaction)| {
                anim.combat.sample(
                    now,
                    ready || activity == Some(&CharacterActivity::Fighting),
                    swing,
                    reaction,
                    motion.is_some_and(|m| m.is_moving()) || visual.speed > 0.24,
                )
            });
        if let Some((clip, seek)) = combat_pose {
            desired = Some(clip);
            speed = if seek.is_some() { 0.0 } else { 1.0 };
        }
        let Some(desired) = desired else {
            continue;
        };

        if anim.current_body != Some(desired) || !player.is_playing_animation(desired) {
            if let Some(previous_fade) = anim.fading_body.take() {
                player.stop(previous_fade);
            }
            let previous = anim
                .current_body
                .filter(|previous| *previous != desired && player.is_playing_animation(*previous));
            // `start` deliberately restarts job clips at a readable contact
            // pose. Only the old and new body clips coexist during this fade.
            player
                .start(desired)
                .repeat()
                .set_weight(if previous.is_some() { 0.0 } else { 1.0 });
            anim.current_body = Some(desired);
            anim.fading_body = previous;
            anim.body_fade_seconds = 0.0;
        }

        if let Some(active) = player.animation_mut(desired) {
            if let Some((_, Some(seek))) = combat_pose {
                active.set_seek_time(seek);
            }
            if combat_pose.is_none() && freeze_at_contact && active.seek_time() != 0.0 {
                active.set_seek_time(0.0);
            }
            if (active.speed() - speed).abs() > 0.01 {
                active.set_speed(speed);
            }
        }

        if let Some(fading) = anim.fading_body {
            anim.body_fade_seconds += time.delta_secs();
            let weight = (anim.body_fade_seconds / BODY_ANIMATION_FADE_SECONDS).clamp(0.0, 1.0);
            if let Some(active) = player.animation_mut(desired) {
                active.set_weight(weight);
            }
            if let Some(active) = player.animation_mut(fading) {
                active.set_weight(1.0 - weight);
            }
            if weight >= 1.0 {
                player.stop(fading);
                anim.fading_body = None;
            }
        } else if let Some(active) = player.animation_mut(desired) {
            if (active.weight() - 1.0).abs() > 0.01 {
                active.set_weight(1.0);
            }
        }
    }
}
