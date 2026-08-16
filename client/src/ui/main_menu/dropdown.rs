//! dropdown systems.

use super::*;

pub(super) fn spawn_dropdown(parent: &mut ChildSpawnerCommands<'_>, presets: &ServerPresets) {
    let selected_name = presets
        .selected_index
        .and_then(|i| presets.entries.get(i))
        .map(|e| e.name.as_str())
        .unwrap_or("Custom");

    // Dropdown container (relative positioning for absolute children)
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        })
        .with_children(|dropdown_container| {
            // Toggle button (always visible)
            dropdown_container
                .spawn((
                    DropdownToggle,
                    Button,
                    Node {
                        width: Val::Px(280.0),
                        height: Val::Px(38.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Inverse),
                ))
                .with_children(|toggle| {
                    // Selected name
                    toggle.spawn((
                        DropdownText,
                        UiButtonLabel,
                        Text::new(selected_name),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                        TextColor(INK_INVERSE),
                    ));

                    // Arrow indicator
                    toggle.spawn((
                        Text::new("▼"),
                        UiButtonLabel,
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(INK_INVERSE_MUTED),
                    ));
                });

            // Options container (hidden by default, shown when expanded)
            dropdown_container
                .spawn((
                    DropdownOptions,
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Px(280.0),
                        border: UiRect::all(Val::Px(1.0)),
                        display: Display::None, // Hidden by default
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.09, 0.07)),
                    BorderColor::from(PLATE_RULE),
                    ZIndex(10), // On top of other elements
                ))
                .with_children(|options| {
                    for (i, entry) in presets.entries.iter().enumerate() {
                        let is_selected = presets.selected_index == Some(i);

                        options
                            .spawn((
                                DropdownOption { index: i },
                                Button,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(36.0),
                                    justify_content: JustifyContent::FlexStart,
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(Val::Px(12.0)),
                                    ..default()
                                },
                                selected_button_chrome(UiButtonVariant::Inverse, is_selected),
                            ))
                            .with_children(|option| {
                                option.spawn((
                                    Text::new(&entry.name),
                                    UiButtonLabel,
                                    TextFont {
                                        font_size: FontSize::Px(15.0),
                                        ..default()
                                    },
                                    TextColor(if is_selected { EMBER } else { INK_INVERSE }),
                                ));
                            });
                    }
                });
        });
}

/// Toggle dropdown open/closed when clicking the toggle button
pub(super) fn handle_dropdown_toggle(
    toggle_query: Query<&Interaction, (Changed<Interaction>, With<DropdownToggle>)>,
    mut dropdown_state: ResMut<DropdownState>,
    mut options_query: Query<&mut Node, With<DropdownOptions>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    option_interactions: Query<&Interaction, With<DropdownOption>>,
    toggle_interactions: Query<&Interaction, With<DropdownToggle>>,
) {
    // Toggle on click
    for interaction in toggle_query.iter() {
        if *interaction == Interaction::Pressed {
            dropdown_state.expanded = !dropdown_state.expanded;
        }
    }

    // Close dropdown when clicking outside (but not on options or toggle)
    if mouse_button.just_pressed(MouseButton::Left) {
        let clicking_option = option_interactions
            .iter()
            .any(|i| *i == Interaction::Pressed);
        let clicking_toggle = toggle_interactions
            .iter()
            .any(|i| *i == Interaction::Pressed);

        if dropdown_state.expanded && !clicking_option && !clicking_toggle {
            dropdown_state.expanded = false;
        }
    }

    // Update visibility
    for mut node in options_query.iter_mut() {
        node.display = if dropdown_state.expanded {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Handle clicking on a dropdown option
pub(super) fn handle_dropdown_selection(
    options_query: Query<(&Interaction, &DropdownOption), Changed<Interaction>>,
    mut server_address: ResMut<ServerAddress>,
    mut presets: ResMut<ServerPresets>,
    mut dropdown_state: ResMut<DropdownState>,
) {
    for (interaction, option) in options_query.iter() {
        if *interaction == Interaction::Pressed {
            if let Some(entry) = presets.entries.get(option.index).cloned() {
                // Update server address
                server_address.ip = entry.ip.clone();
                info!("Selected server preset: {} ({})", entry.name, entry.ip);

                // Update selected index
                presets.selected_index = Some(option.index);

                // Close dropdown
                dropdown_state.expanded = false;
            }
        }
    }
}

/// Update dropdown display text based on selection
pub(super) fn update_dropdown_display(
    presets: Res<ServerPresets>,
    mut text_query: Query<&mut Text, With<DropdownText>>,
    mut options_query: Query<(&DropdownOption, &mut UiButtonStyle)>,
) {
    // Update toggle text
    let selected_name = presets
        .selected_index
        .and_then(|i| presets.entries.get(i))
        .map(|e| e.name.as_str())
        .unwrap_or("Custom");

    for mut text in text_query.iter_mut() {
        if **text != selected_name {
            **text = selected_name.to_string();
        }
    }

    // Update option highlighting
    for (option, mut style) in options_query.iter_mut() {
        let is_selected = presets.selected_index == Some(option.index);
        style.selected = is_selected;
    }
}
