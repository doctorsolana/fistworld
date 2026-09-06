//! layout systems.

use super::*;

pub(super) fn spawn_pause_menu(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    input_settings: Res<InputSettings>,
    open: Res<PauseMenuOpen>,
    existing: Query<Entity, With<PauseMenuRoot>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    if !open.0 || !existing.is_empty() {
        return;
    }
    // Full-screen darkened overlay
    commands
        .spawn((
            PauseMenuRoot,
            ModalRoot,
            modal_root_chrome(),
            BackgroundColor(MODAL_BACKDROP),
        ))
        .with_children(|parent| {
            // Content container - this is what we shift left/right
            // Using left margin to offset from center
            parent
                .spawn((
                    MenuContentContainer,
                    crate::ui::motion::UiReveal::panel(),
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
                                padding: UiRect::axes(Val::Px(22.0), Val::Px(28.0)),
                                border: UiRect::all(Val::Px(3.0)),
                                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                ..default()
                            },
                            BackgroundColor(crate::ui::styles::SIGN_WOOD),
                            BorderColor::all(crate::ui::styles::BRASS_DARK),
                            crate::ui::styles::plate_shadow(),
                        ))
                        .with_children(|col| {
                            crate::ui::frame::corners(col);
                            // Pause title
                            col.spawn((
                                Text::new("PAUSED"),
                                crate::ui::typography::heading(44.0),
                                TextColor(INK_INVERSE),
                                Node {
                                    margin: UiRect::bottom(Val::Px(24.0)),
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
                                crate::ui::typography::text(14.0),
                                TextColor(INK_INVERSE_MUTED),
                                Node {
                                    margin: UiRect::top(Val::Px(16.0)),
                                    ..default()
                                },
                            ));
                        });

                    // Graphics settings panel (hidden by default, appears to the right)
                    spawn_graphics_panel(container, &settings, monitors.iter().next());

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
