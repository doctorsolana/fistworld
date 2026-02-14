//! death screen systems.

use super::*;

/// Spawn the death screen UI (shows when player is dead)
pub fn spawn_death_screen(mut commands: Commands) {
    commands
        .spawn((
            DeathScreen,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.0, 0.0, 0.5)),
            Visibility::Hidden, // Hidden by default
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            // "YOU DIED" text
            parent.spawn((
                Text::new("YOU DIED"),
                TextFont {
                    font_size: 72.0,
                    ..default()
                },
                TextColor(Color::srgba(0.9, 0.2, 0.2, 1.0)),
                Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
            ));

            // Respawn timer text
            parent.spawn((
                RespawnTimerText,
                Text::new("Respawning in 4..."),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgba(0.8, 0.8, 0.8, 0.9)),
            ));
        });
}

/// Update death screen visibility based on player death state
pub fn update_death_screen(
    input_state: Res<crate::input::InputState>,
    mut death_screen: Query<&mut Visibility, With<DeathScreen>>,
    mut timer_text: Query<&mut Text, With<RespawnTimerText>>,
    _local_player: Query<&shared::components::Health, With<shared::components::LocalPlayer>>,
    time: Res<Time>,
    mut death_time: Local<Option<f32>>,
) {
    let is_dead = input_state.is_dead;

    // Track when we died
    if is_dead && death_time.is_none() {
        *death_time = Some(time.elapsed_secs());
    } else if !is_dead {
        *death_time = None;
    }

    // Update visibility
    for mut visibility in death_screen.iter_mut() {
        *visibility = if is_dead {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    // Update timer text
    if is_dead {
        if let Some(died_at) = *death_time {
            let elapsed = time.elapsed_secs() - died_at;
            let remaining = (4.0 - elapsed).max(0.0).ceil() as i32;

            for mut text in timer_text.iter_mut() {
                **text = format!("Respawning in {}...", remaining);
            }
        }
    }
}

/// Despawn death screen when leaving gameplay
pub fn despawn_death_screen(
    mut commands: Commands,
    death_screens: Query<Entity, With<DeathScreen>>,
) {
    for entity in death_screens.iter() {
        commands.entity(entity).despawn();
    }
}
