//! overlay systems.

use super::*;

const UNDERWATER_MAX_ALPHA: f32 = 0.35;
const UNDERWATER_LERP_SPEED: f32 = 6.0;

#[derive(Component)]
pub(super) struct UnderwaterOverlay {
    pub(super) current_alpha: f32,
}

pub(super) fn spawn_underwater_overlay(
    mut commands: Commands,
    existing: Query<Entity, With<UnderwaterOverlay>>,
) {
    if !existing.is_empty() {
        return;
    }

    commands.spawn((
        UnderwaterOverlay { current_alpha: 0.0 },
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.36, 0.55, 0.0)),
        Pickable::IGNORE,
    ));
}

pub(super) fn despawn_underwater_overlay(
    mut commands: Commands,
    overlays: Query<Entity, With<UnderwaterOverlay>>,
) {
    for entity in overlays.iter() {
        commands.entity(entity).despawn();
    }
}

pub(super) fn update_underwater_overlay(
    time: Res<Time>,
    local_water: Query<&PlayerWaterState, With<LocalPlayer>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut overlays: Query<(
        &mut UnderwaterOverlay,
        &mut BackgroundColor,
        &mut Visibility,
    )>,
) {
    let target_alpha = local_water
        .single()
        .ok()
        .zip(cameras.single().ok())
        .map(|(state, camera)| {
            let camera_depth = state.surface_y - camera.translation().y;
            let depth_factor = (camera_depth / 2.0).clamp(0.0, 1.0);
            depth_factor * UNDERWATER_MAX_ALPHA
        })
        .unwrap_or(0.0);

    let dt = time.delta_secs();
    for (mut overlay, mut bg, mut vis) in overlays.iter_mut() {
        let t = 1.0 - (-UNDERWATER_LERP_SPEED * dt).exp();
        overlay.current_alpha += (target_alpha - overlay.current_alpha) * t;

        let alpha = overlay.current_alpha.clamp(0.0, UNDERWATER_MAX_ALPHA);
        if alpha <= 0.001 {
            *vis = Visibility::Hidden;
        } else {
            *vis = Visibility::Visible;
        }
        *bg = BackgroundColor(Color::srgba(0.08, 0.36, 0.55, alpha));
    }
}
