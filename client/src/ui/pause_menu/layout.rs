//! layout systems.

use super::*;

pub(super) fn spawn_pause_menu(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    input_settings: Res<InputSettings>,
    open: Res<PauseMenuOpen>,
    existing: Query<Entity, With<PauseMenuRoot>>,
) {
    if !open.0 || !existing.is_empty() {
        return;
    }
    // Full-screen darkened overlay
    commands
        .spawn((
            PauseMenuRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        ))
        .with_children(|parent| {
            // Content container - this is what we shift left/right
            // Using left margin to offset from center
            parent
                .spawn((
                    MenuContentContainer,
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(50.0),
                        // Start with 0 offset (perfectly centered)
                        margin: UiRect::left(Val::Px(0.0)),
                        ..default()
                    },
                ))
                .with_children(|container| {
                    // Main menu column
                    container
                        .spawn((
                            MainMenuColumn,
                            Node {
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ))
                        .with_children(|col| {
                            // Pause title
                            col.spawn((
                                Text::new("PAUSED"),
                                title_text_style(),
                                TextColor(TEXT_COLOR),
                                Node {
                                    margin: UiRect::bottom(Val::Px(40.0)),
                                    ..default()
                                },
                            ));

                            // Resume button
                            spawn_button(col, "RESUME", PauseButton::Resume);

                            // Graphics button
                            spawn_button(col, "GRAPHICS", PauseButton::Graphics);

                            // Controls button
                            spawn_button(col, "CONTROLS", PauseButton::Controls);

                            // Disconnect button
                            spawn_button(col, "DISCONNECT", PauseButton::Disconnect);

                            // Exit button
                            spawn_button(col, "EXIT GAME", PauseButton::Exit);

                            // Hint
                            col.spawn((
                                Text::new("Press ESC to resume"),
                                TextFont {
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(TEXT_MUTED),
                                Node {
                                    margin: UiRect::top(Val::Px(30.0)),
                                    ..default()
                                },
                            ));
                        });

                    // Graphics settings panel (hidden by default, appears to the right)
                    spawn_graphics_panel(container, &settings);

                    // Controls settings panel (hidden by default, appears to the right)
                    spawn_controls_panel(container, &input_settings);
                });
        });
}

pub(super) fn despawn_pause_menu(
    mut commands: Commands,
    open: Res<PauseMenuOpen>,
    query: Query<Entity, With<PauseMenuRoot>>,
) {
    if open.0 {
        return;
    }
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}
