//! hit markers systems.

use super::*;

/// Spawn a hit marker when we hit someone
pub fn spawn_hit_marker(commands: &mut Commands, time: &Time, is_kill: bool) {
    let color = if is_kill {
        Color::srgba(1.0, 0.2, 0.2, 1.0) // Red for kill
    } else {
        Color::srgba(1.0, 1.0, 1.0, 1.0) // White for hit
    };

    commands
        .spawn((
            HitMarker {
                spawn_time: time.elapsed_secs(),
                is_kill,
            },
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // Children below use `Transform` for diagonal rotation, which inserts
            // `GlobalTransform`. Ensure this parent participates in transform propagation
            // so we don't trip Bevy's hierarchy B0004 warning.
            Transform::default(),
            GlobalTransform::default(),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            // X shape for hit marker
            let line_length = if is_kill { 16.0 } else { 12.0 };
            let line_width = if is_kill { 3.0 } else { 2.0 };

            // Top-left to center
            parent.spawn((
                Node {
                    width: Val::Px(line_length),
                    height: Val::Px(line_width),
                    position_type: PositionType::Absolute,
                    top: Val::Px(-line_length / 2.0),
                    left: Val::Px(-line_length / 2.0),
                    ..default()
                },
                BackgroundColor(color),
                Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ));

            // Top-right to center
            parent.spawn((
                Node {
                    width: Val::Px(line_length),
                    height: Val::Px(line_width),
                    position_type: PositionType::Absolute,
                    top: Val::Px(-line_length / 2.0),
                    right: Val::Px(-line_length / 2.0),
                    ..default()
                },
                BackgroundColor(color),
                Transform::from_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
            ));

            // Bottom-left to center
            parent.spawn((
                Node {
                    width: Val::Px(line_length),
                    height: Val::Px(line_width),
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(-line_length / 2.0),
                    left: Val::Px(-line_length / 2.0),
                    ..default()
                },
                BackgroundColor(color),
                Transform::from_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
            ));

            // Bottom-right to center
            parent.spawn((
                Node {
                    width: Val::Px(line_length),
                    height: Val::Px(line_width),
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(-line_length / 2.0),
                    right: Val::Px(-line_length / 2.0),
                    ..default()
                },
                BackgroundColor(color),
                Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ));
        });
}

/// Update and cleanup hit markers
pub fn update_hit_markers(
    mut commands: Commands,
    mut hit_markers: Query<(Entity, &HitMarker, &mut BackgroundColor)>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();
    let hit_duration = 0.15;
    let kill_duration = 0.3;

    for (entity, marker, mut _bg) in hit_markers.iter_mut() {
        let duration = if marker.is_kill {
            kill_duration
        } else {
            hit_duration
        };
        let elapsed = current_time - marker.spawn_time;

        if elapsed > duration {
            commands.entity(entity).despawn();
        }
    }
}
