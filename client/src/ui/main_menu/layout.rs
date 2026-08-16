//! layout systems.

use super::*;

pub(super) fn apply_launcher_window_settings(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    let monitor = monitors.iter().next();
    for mut window in windows.iter_mut() {
        crate::app_wiring::apply_window_mode(
            &mut window,
            &mut ui_scale,
            monitor,
            DisplayMode::Windowed,
            DisplayResolution::new(LAUNCHER_RESOLUTION.0, LAUNCHER_RESOLUTION.1),
        );
    }
}

pub(super) fn spawn_main_menu(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    server_address: Res<ServerAddress>,
    presets: Res<ServerPresets>,
    mut dropdown_state: ResMut<DropdownState>,
) {
    // Reset dropdown state when entering menu
    dropdown_state.expanded = false;

    // Load the logo
    let logo_handle: Handle<Image> = asset_server.load("ui/fistforce.png");

    commands
        .spawn((
            MainMenuRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(MENU_BACKGROUND),
        ))
        .with_children(|parent| {
            // Vignette overlay (dark edges for depth)
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
            ));

            // Logo image container
            parent.spawn((
                LogoImage {
                    base_scale: 1.0,
                    time: 0.0,
                },
                Node {
                    width: Val::Px(500.0),
                    height: Val::Px(281.25), // 16:9 aspect ratio (500 / 16 * 9)
                    margin: UiRect::bottom(Val::Px(40.0)),
                    ..default()
                },
                ImageNode::new(logo_handle),
            ));

            // Server IP input section
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                })
                .with_children(|ip_section| {
                    // Label
                    ip_section.spawn((
                        Text::new("SERVER IP"),
                        TextFont {
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(INK_INVERSE_MUTED),
                        Node {
                            margin: UiRect::bottom(Val::Px(8.0)),
                            ..default()
                        },
                    ));

                    // Input field container (clickable)
                    ip_section
                        .spawn((
                            IpInputField { focused: false },
                            Button,
                            Node {
                                width: Val::Px(280.0),
                                height: Val::Px(45.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(2.0)),
                                padding: UiRect::horizontal(Val::Px(12.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            button_chrome(UiButtonVariant::Inverse),
                        ))
                        .with_children(|input_box| {
                            // IP text
                            input_box.spawn((
                                IpTextDisplay,
                                UiButtonLabel,
                                Text::new(format!("{}:{}", server_address.ip, server_address.port)),
                                TextFont {
                                    font_size: FontSize::Px(20.0),
                                    ..default()
                                },
                                TextColor(INK_INVERSE),
                            ));
                        });

                    // Server preset dropdown (only if we have presets)
                    if !presets.entries.is_empty() {
                        spawn_dropdown(ip_section, &presets);
                    }

                    // Helper text
                    ip_section.spawn((
                        Text::new("Click to edit • Ctrl+V to paste • Select preset below"),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(INK_INVERSE_MUTED),
                        Node {
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        },
                    ));
                });

            // Button container
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                })
                .with_children(|btn_container| {
                    // Connect button
                    spawn_button(btn_container, "CONNECT", MenuButton::Connect);

                    // Exit button
                    spawn_button(btn_container, "EXIT", MenuButton::Exit);
                });

            // Version info at bottom
            parent.spawn((
                Text::new("v0.1.0 | Bevy + Lightyear"),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK_INVERSE_MUTED),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(20.0),
                    ..default()
                },
            ));
        });
}

pub(super) fn spawn_button(parent: &mut ChildSpawnerCommands<'_>, text: &str, action: MenuButton) {
    let variant = match action {
        MenuButton::Connect => UiButtonVariant::Primary,
        MenuButton::Exit => UiButtonVariant::Danger,
    };
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(280.0),
                height: Val::Px(55.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                margin: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            button_chrome(variant),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(text),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(22.0),
                    ..default()
                },
                TextColor(INK_INVERSE),
            ));
        });
}

pub(super) fn despawn_main_menu(mut commands: Commands, query: Query<Entity, With<MainMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}
