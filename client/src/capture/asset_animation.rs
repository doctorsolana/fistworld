//! Named-clip asset inspection, including a bareback rider on the authored socket.
//! This is an offline animation review, not a connected mounting simulation.
use super::{asset_fixtures::AssetReview, CaptureConfig, CaptureState};
use bevy::{gltf::Gltf, prelude::*};

struct ReviewRig {
    player: Entity,
    node: AnimationNodeIndex,
    duration: f32,
}

#[derive(Resource)]
pub(super) struct AnimationReview {
    horse_gltf: Handle<Gltf>,
    horse_clip: String,
    horse: Option<ReviewRig>,
    rider_clip: Option<String>,
    rider_root: Option<Entity>,
    rider_gltf: Option<Handle<Gltf>>,
    rider: Option<ReviewRig>,
    pub(super) ready: bool,
    frames: u32,
    seconds: f32,
}

pub(super) fn install(app: &mut App, path: &str) {
    let Ok(clip) = std::env::var("FISTFORCE_CAPTURE_ASSET_CLIP") else {
        return;
    };
    let horse_gltf = app
        .world()
        .resource::<AssetServer>()
        .load(path.split('#').next().unwrap().to_string());
    app.insert_resource(AnimationReview {
        horse_gltf,
        horse_clip: clip,
        horse: None,
        rider_clip: std::env::var("FISTFORCE_CAPTURE_RIDER_CLIP").ok(),
        rider_root: None,
        rider_gltf: None,
        rider: None,
        ready: false,
        frames: 0,
        seconds: 0.,
    });
    app.add_systems(Update, (setup, drive).chain());
}

fn configure(
    root: Entity,
    gltf: &Gltf,
    clip: &str,
    children: &Query<&Children>,
    players: &mut Query<&mut AnimationPlayer>,
    clips: &Assets<AnimationClip>,
    graphs: &mut Assets<AnimationGraph>,
    commands: &mut Commands,
) -> Option<ReviewRig> {
    let handle = gltf
        .named_animations
        .get(clip)
        .unwrap_or_else(|| panic!("capture asset has no clip {clip}"));
    let duration = clips.get(handle)?.duration();
    let player = std::iter::once(root)
        .chain(children.iter_descendants(root))
        .find(|e| players.contains(*e))?;
    let mut graph = AnimationGraph::new();
    let node = graph.add_clip(handle.clone(), 1., graph.root);
    commands
        .entity(player)
        .insert(AnimationGraphHandle(graphs.add(graph)));
    players
        .get_mut(player)
        .ok()?
        .start(node)
        .pause()
        .set_seek_time(0.);
    Some(ReviewRig {
        player,
        node,
        duration,
    })
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    asset: Res<AssetReview>,
    mut review: ResMut<AnimationReview>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    parents: Query<&ChildOf>,
    mut players: Query<&mut AnimationPlayer>,
    mut visible: Query<&mut Visibility>,
    manifest: Res<crate::hero::HeroManifest>,
    mut exit: MessageWriter<AppExit>,
) {
    if review.ready {
        return;
    }
    review.frames += 1;
    let budget = config.shots.first().map_or(1200, |s| {
        s.readiness.maximum_frames.max(config.warmup_frames)
    });
    if review.frames >= budget {
        error!("capture: horse/rider animation setup timed out");
        exit.write(AppExit::error());
        return;
    }
    if !asset.ready {
        return;
    }
    let Some(root) = asset.root else {
        return;
    };
    if review.horse.is_none() {
        let Some(gltf) = gltfs.get(&review.horse_gltf) else {
            return;
        };
        review.horse = configure(
            root,
            gltf,
            &review.horse_clip,
            &children,
            &mut players,
            &clips,
            &mut graphs,
            &mut commands,
        );
        if review.horse.is_none() {
            return;
        }
    }
    if review.rider_clip.is_none() {
        review.ready = true;
        return;
    }
    if review.rider_root.is_none() {
        let Some(anchor) = children
            .iter_descendants(root)
            .find(|e| names.get(*e).is_ok_and(|n| n.as_str() == "Anchor_Rider"))
        else {
            return;
        };
        // Cancel the socket's bind orientation; keep its animated translation
        // and body rotation. An upright bone is not an upright character root.
        let mut rotation = Quat::IDENTITY;
        let mut entity = anchor;
        while entity != root {
            let Ok(transform) = transforms.get(entity) else {
                return;
            };
            rotation = transform.rotation * rotation;
            let Ok(parent) = parents.get(entity) else {
                return;
            };
            entity = parent.parent();
        }
        review.rider_gltf =
            Some(assets.load(manifest.scene.split('#').next().unwrap().to_string()));
        review.rider_root = Some(
            commands
                .spawn((
                    Name::new("Capture bareback rider"),
                    WorldAssetRoot(assets.load(manifest.scene.clone())),
                    Transform::from_rotation(rotation.inverse()),
                    Visibility::Hidden,
                    ChildOf(anchor),
                ))
                .id(),
        );
        return;
    }
    let rider_root = review.rider_root.unwrap();
    let Some(gltf) = review.rider_gltf.as_ref().and_then(|h| gltfs.get(h)) else {
        return;
    };
    let outfit = shared::components::HeroOutfit::from_manifest(&manifest);
    let wardrobe_count: usize = manifest.slots.iter().map(|s| s.items.len()).sum();
    let nodes: Vec<_> = children.iter_descendants(rider_root).collect();
    if nodes
        .iter()
        .filter(|e| {
            names
                .get(**e)
                .is_ok_and(|n| manifest.is_wardrobe_node(n.as_str()))
        })
        .count()
        < wardrobe_count
    {
        return;
    }
    for entity in nodes {
        if let Ok(name) = names.get(entity) {
            if manifest.is_wardrobe_node(name.as_str()) {
                if let Ok(mut visibility) = visible.get_mut(entity) {
                    *visibility = if outfit.hides_node(&manifest, name.as_str()) {
                        Visibility::Hidden
                    } else {
                        Visibility::Inherited
                    };
                }
            }
        }
    }
    let clip = review.rider_clip.as_ref().unwrap();
    review.rider = configure(
        rider_root,
        gltf,
        clip,
        &children,
        &mut players,
        &clips,
        &mut graphs,
        &mut commands,
    );
    if review.rider.is_some() {
        if let Ok(mut visibility) = visible.get_mut(rider_root) {
            *visibility = Visibility::Visible;
        }
        review.ready = true;
    }
}

fn drive(
    time: Res<Time>,
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut review: ResMut<AnimationReview>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if !review.ready {
        return;
    }
    if matches!(*state, CaptureState::Warmup { .. })
        || matches!(*state, CaptureState::Settling { .. }) && !config.continuous
    {
        review.seconds = 0.;
    } else {
        review.seconds += time.delta_secs();
    }
    let Some(horse) = &review.horse else {
        return;
    };
    let phase = (review.seconds / horse.duration).fract();
    if let Ok(mut player) = players.get_mut(horse.player) {
        if let Some(active) = player.animation_mut(horse.node) {
            active.pause().set_seek_time(phase * horse.duration);
        }
    }
    if let Some(rider) = &review.rider {
        let seek = if matches!(review.rider_clip.as_deref(), Some("mount" | "dismount")) {
            review.seconds.min(rider.duration)
        } else {
            phase * rider.duration
        };
        if let Ok(mut player) = players.get_mut(rider.player) {
            if let Some(active) = player.animation_mut(rider.node) {
                active.pause().set_seek_time(seek);
            }
        }
    }
}
