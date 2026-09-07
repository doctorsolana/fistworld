//! Shared arrow mesh, sampled from the immutable authoritative launch.
use bevy::prelude::*;
use shared::components::{ArrowProjectile, WorldTime};
#[derive(Component)]
pub(super) struct ArrowVisual;
pub(super) fn attach(
    mut commands: Commands,
    assets: Res<AssetServer>,
    arrows: Query<Entity, (With<ArrowProjectile>, Without<ArrowVisual>)>,
) {
    if arrows.is_empty() {
        return;
    }
    let scene = assets.load("game_assets/tools/Arrow.glb#Scene0");
    for e in &arrows {
        commands.entity(e).insert((
            ArrowVisual,
            WorldAssetRoot(scene.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}
pub(super) fn animate(
    clock: Query<&WorldTime>,
    mut arrows: Query<(&ArrowProjectile, &mut Transform, &mut Visibility), With<ArrowVisual>>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = f64::from(clock.day) * f64::from(clock.cycle_duration())
        + f64::from(clock.seconds_in_cycle);
    for (arrow, mut transform, mut visibility) in &mut arrows {
        transform.translation = arrow.position(now);
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, arrow.direction(now));
        visibility.set_if_neq(if now >= arrow.launched_at {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
