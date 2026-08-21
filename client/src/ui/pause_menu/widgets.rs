//! widgets systems.

use super::*;

pub(super) fn spawn_button(parent: &mut ChildSpawnerCommands<'_>, text: &str, action: PauseButton) {
    let variant = match action {
        PauseButton::Disconnect | PauseButton::Exit => UiButtonVariant::Danger,
        _ => UiButtonVariant::Secondary,
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
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
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
                TextColor(INK),
            ));
        });
}

pub(super) fn spawn_graphics_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    settings: &GraphicsSettings,
    monitor: Option<&Monitor>,
) {
    parent
        .spawn((
            GraphicsSettingsPanel,
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::FlexStart,
                padding: UiRect::all(Val::Px(24.0)),
                min_width: Val::Px(0.0),
                max_height: Val::Vh(88.0),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                // Start hidden so it doesn't affect layout
                display: Display::None,
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(FRONT_PANEL),
        ))
        .with_children(|panel| {
            // Panel title
            panel.spawn((
                Text::new("GRAPHICS"),
                TextFont {
                    font_size: FontSize::Px(26.0),
                    ..default()
                },
                TextColor(INK_INVERSE_HEADING),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
            ));

            // Help text
            panel.spawn((
                Text::new("Toggle/adjust to fix flickering or brightness"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK_INVERSE_MUTED),
                Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
            ));

            // Toggle buttons
            spawn_toggle(
                panel,
                "Bloom",
                GraphicsToggle::Bloom,
                settings.bloom_enabled,
            );
            spawn_toggle(
                panel,
                "Shadows",
                GraphicsToggle::Shadows,
                settings.shadows_enabled,
            );
            spawn_toggle(
                panel,
                "Atmosphere",
                GraphicsToggle::Atmosphere,
                settings.atmosphere_enabled,
            );
            spawn_toggle(
                panel,
                "Clouds",
                GraphicsToggle::Clouds,
                settings.clouds_enabled,
            );
            spawn_toggle(
                panel,
                "VSync",
                GraphicsToggle::Vsync,
                settings.vsync_enabled,
            );
            // Separator
            panel.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    margin: UiRect::vertical(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.1)),
            ));

            // Slider controls
            spawn_slider(
                panel,
                "Display Mode",
                SliderControl::DisplayMode,
                settings.display_mode().label(),
            );
            panel.spawn((
                Text::new(if cfg!(target_os = "macos") {
                    "Borderless keeps Command-Tab and Spaces available. Exclusive changes the monitor resolution."
                } else {
                    "Borderless keeps the desktop video mode. Exclusive changes the monitor resolution."
                }),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(INK_INVERSE_MUTED),
                Node {
                    max_width: Val::Px(430.0),
                    margin: UiRect::bottom(Val::Px(12.0)),
                    ..default()
                },
            ));
            spawn_slider(
                panel,
                "Resolution",
                SliderControl::Resolution,
                &settings.displayed_resolution_label(monitor),
            );
            panel
                .spawn((
                    DisplayConfirmationPanel,
                    Node {
                        display: Display::None,
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Stretch,
                        row_gap: Val::Px(8.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        margin: UiRect::bottom(Val::Px(12.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.18, 0.13, 0.06, 0.96)),
                    BorderColor::from(INK_INVERSE_HEADING),
                ))
                .with_children(|confirmation| {
                    confirmation.spawn((
                        DisplayConfirmationText,
                        Text::new("Keep this display setting?"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(INK_INVERSE),
                    ));
                    confirmation
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|buttons| {
                            for (label, action, variant) in [
                                (
                                    "KEEP",
                                    DisplayConfirmationAction::Keep,
                                    UiButtonVariant::Primary,
                                ),
                                (
                                    "REVERT",
                                    DisplayConfirmationAction::Revert,
                                    UiButtonVariant::Danger,
                                ),
                            ] {
                                buttons
                                    .spawn((
                                        Button,
                                        action,
                                        Node {
                                            height: Val::Px(30.0),
                                            flex_grow: 1.0,
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            border_radius: BorderRadius::all(Val::Px(5.0)),
                                            ..default()
                                        },
                                        button_chrome(variant),
                                    ))
                                    .with_children(|button| {
                                        button.spawn((
                                            Text::new(label),
                                            UiButtonLabel,
                                            TextFont {
                                                font_size: FontSize::Px(12.0),
                                                ..default()
                                            },
                                            TextColor(INK_INVERSE),
                                        ));
                                    });
                            }
                        });
                });
            spawn_slider(
                panel,
                "3D Render Scale",
                SliderControl::RenderScale,
                &format!("{:.0}%", settings.render_scale * 100.0),
            );
            spawn_slider(
                panel,
                "Shadow Quality",
                SliderControl::ShadowQuality,
                settings.shadow_quality.label(),
            );
            spawn_slider(
                panel,
                "Exposure",
                SliderControl::Exposure,
                &format!("{:+.1} EV", settings.grade_exposure),
            );
            spawn_slider(
                panel,
                "View Distance",
                SliderControl::ViewDistance,
                &format!("{} chunks", settings.view_distance),
            );
            spawn_slider(
                panel,
                "Prop Distance",
                SliderControl::PropDistance,
                &format!("{:.0}%", settings.prop_render_multiplier * 100.0),
            );
        });
}

pub(super) fn spawn_toggle(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    toggle: GraphicsToggle,
    enabled: bool,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(12.0)),
            ..default()
        })
        .with_children(|row| {
            // Label
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(INK_INVERSE),
                Node {
                    margin: UiRect::right(Val::Px(40.0)),
                    ..default()
                },
            ));

            // Toggle button
            let text = if enabled { "ON" } else { "OFF" };

            row.spawn((
                Button,
                toggle,
                Node {
                    width: Val::Px(60.0),
                    height: Val::Px(30.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                selected_button_chrome(UiButtonVariant::Secondary, enabled),
            ))
            .with_children(|btn| {
                btn.spawn((
                    ToggleText(toggle),
                    UiButtonLabel,
                    Text::new(text),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(INK_INVERSE),
                ));
            });
        });
}

pub(super) fn spawn_slider(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    control: SliderControl,
    value: &str,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(12.0)),
            ..default()
        })
        .with_children(|row| {
            // Label
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(INK_INVERSE),
                Node {
                    margin: UiRect::right(Val::Px(20.0)),
                    ..default()
                },
            ));

            // Control row: [-] [value] [+]
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|controls| {
                spawn_step_button(controls, SliderStep { control, delta: -1 }, "-");

                // Value display
                controls.spawn((
                    SliderValueText(control),
                    Text::new(value),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextLayout::no_wrap(),
                    TextColor(INK_INVERSE),
                    Node {
                        min_width: Val::Px(82.0),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ));

                spawn_step_button(controls, SliderStep { control, delta: 1 }, "+");
            });
        });
}

pub(super) fn spawn_controls_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    settings: &InputSettings,
) {
    parent
        .spawn((
            ControlsSettingsPanel,
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                padding: UiRect::all(Val::Px(24.0)),
                min_width: Val::Px(0.0),
                // Start hidden so it doesn't affect layout
                display: Display::None,
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(FRONT_PANEL),
        ))
        .with_children(|panel| {
            // Panel title
            panel.spawn((
                Text::new("CONTROLS"),
                TextFont {
                    font_size: FontSize::Px(26.0),
                    ..default()
                },
                TextColor(INK_INVERSE_HEADING),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
            ));

            // Help text
            panel.spawn((
                Text::new("Adjust input settings"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK_INVERSE_MUTED),
                Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
            ));

            // Mouse sensitivity slider
            spawn_input_slider(
                panel,
                "Mouse Sensitivity",
                InputSliderControl::MouseSensitivity,
                &format!("{:.0}%", settings.mouse_sensitivity * 100.0),
            );
        });
}

pub(super) fn spawn_input_slider(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    control: InputSliderControl,
    value: &str,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(12.0)),
            ..default()
        })
        .with_children(|row| {
            // Label
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(INK_INVERSE),
                Node {
                    margin: UiRect::right(Val::Px(20.0)),
                    ..default()
                },
            ));

            // Control row: [-] [value] [+]
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|controls| {
                spawn_step_button(controls, InputSliderStep { control, delta: -1 }, "-");

                // Value display
                controls.spawn((
                    InputSliderValueText(control),
                    Text::new(value),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(INK_INVERSE),
                    Node {
                        min_width: Val::Px(70.0),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ));

                spawn_step_button(controls, InputSliderStep { control, delta: 1 }, "+");
            });
        });
}

fn spawn_step_button<M: Component>(parent: &mut ChildSpawnerCommands<'_>, marker: M, glyph: &str) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                width: Val::Px(28.0),
                height: Val::Px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Inverse),
        ))
        .with_child((
            Text::new(glyph),
            UiButtonLabel,
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(INK_INVERSE),
        ));
}
