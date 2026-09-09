//! Inspect an authored GLB before it has a gameplay spawning path.
//! This exercises Bevy's ordinary scene, skin and material loading, in rest pose.

use super::CaptureConfig;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstance;

#[derive(Resource)]
pub(super) struct AssetReview {
    path: String,
    pub(super) root: Option<Entity>,
    frames: u32,
    pub(super) ready: bool,
}

pub(super) fn install(app: &mut App) {
    let Ok(path) = std::env::var("FISTFORCE_CAPTURE_ASSET") else {
        return;
    };
    super::asset_animation::install(app, &path);
    app.insert_resource(AssetReview {
        path,
        root: None,
        frames: 0,
        ready: false,
    });
    app.add_systems(Update, stage_and_check);
}

pub(super) fn ready(
    review: Option<Res<AssetReview>>,
    animation: Option<Res<super::asset_animation::AnimationReview>>,
) -> bool {
    review.is_none_or(|review| review.ready) && animation.is_none_or(|animation| animation.ready)
}

fn stage_and_check(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    mut review: ResMut<AssetReview>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    assets: Res<AssetServer>,
    spawner: Res<WorldInstanceSpawner>,
    roots: Query<(&WorldAssetRoot, Option<&WorldInstance>)>,
    world_root: Query<Entity, With<crate::render::systems::ClientWorldRoot>>,
    mut settings: ResMut<crate::render::systems::GraphicsSettings>,
    mut exit: MessageWriter<AppExit>,
) {
    if review.ready {
        return;
    }
    review.frames += 1;
    let budget = config.shots.first().map_or(1200, |shot| {
        shot.readiness.maximum_frames.max(config.warmup_frames)
    });
    if review.frames >= budget {
        error!(
            "capture: asset {} failed to instantiate within {budget} frames",
            review.path
        );
        exit.write(AppExit::error());
        return;
    }
    settings.props_enabled = false;
    if review.root.is_none() {
        let (Some(terrain), Ok(world_root)) = (terrain, world_root.single()) else {
            return;
        };
        let focus = config.shots.first().map_or(Vec3::ZERO, |shot| shot.focus);
        review.root = Some(
            commands
                .spawn((
                    Name::new(format!("Capture asset: {}", review.path)),
                    WorldAssetRoot(assets.load(review.path.clone())),
                    Transform::from_xyz(focus.x, terrain.get_height(focus.x, focus.z), focus.z),
                    Visibility::Visible,
                    ChildOf(world_root),
                ))
                .id(),
        );
        return;
    }
    let Ok((root, instance)) = roots.get(review.root.unwrap()) else {
        return;
    };
    use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState};
    if let Some((asset, dependencies, recursive)) = assets.get_load_states(root.id()) {
        let failure = match (asset, dependencies, recursive) {
            (LoadState::Failed(error), _, _)
            | (_, DependencyLoadState::Failed(error), _)
            | (_, _, RecursiveDependencyLoadState::Failed(error)) => Some(error),
            _ => None,
        };
        if let Some(error) = failure {
            error!("capture: asset {} failed: {error}", review.path);
            exit.write(AppExit::error());
            return;
        }
    }
    if assets.is_loaded_with_dependencies(root.id())
        && instance.is_some_and(|instance| spawner.instance_is_ready(**instance))
    {
        review.ready = true;
        info!(
            "capture: asset {} instantiated with loaded dependencies",
            review.path
        );
    }
}
