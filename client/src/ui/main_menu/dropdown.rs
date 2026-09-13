//! Saved addresses share the input row; the popup does not reflow the launcher.
use super::*;
use crate::ui::{
    ledger,
    startup::{widgets, StartupArtwork},
    typography,
};
use bevy::input_focus::InputFocus;

pub(super) fn spawn_dropdown(
    parent: &mut ChildSpawnerCommands<'_>,
    presets: &ServerPresets,
    art: &StartupArtwork,
) {
    parent
        .spawn(Node {
            width: Val::Px(50.0),
            height: Val::Percent(100.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|container| {
            container
                .spawn((
                    DropdownToggle,
                    Name::new("startup-presets"),
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Secondary),
                    ledger::LedgerButtonFace(art.brass()),
                ))
                .with_children(|toggle| {
                    toggle
                        .spawn((
                            Node {
                                width: Val::Px(20.0),
                                height: Val::Px(14.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|arrow| {
                            for (x, angle) in [(0.0, 45.0), (8.0, -45.0)] {
                                arrow.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(x),
                                        top: Val::Px(6.0),
                                        width: Val::Px(12.0),
                                        height: Val::Px(2.0),
                                        ..default()
                                    },
                                    UiTransform::from_rotation(Rot2::degrees(angle)),
                                    BackgroundColor(widgets::INK),
                                    Pickable::IGNORE,
                                ));
                            }
                        });
                });
            container
                .spawn((
                    DropdownOptions,
                    Name::new("startup-preset-options"),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(0.0),
                        top: Val::Px(61.0),
                        width: Val::Px(356.0),
                        max_width: Val::Vw(40.0),
                        max_height: Val::Px(240.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(6.0)),
                        display: Display::None,
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    art.paper(),
                    // Escape the address row's local stacking context: the
                    // following Connect button must never cover saved servers.
                    GlobalZIndex(crate::ui::foundation::layer::FLOATING_PANEL),
                    crate::ui::foundation::surface_block(),
                    crate::ui::styles::plate_shadow(),
                ))
                .with_children(|options| {
                    for (index, entry) in presets.entries.iter().enumerate() {
                        options
                            .spawn((
                                DropdownOption { index },
                                Button,
                                Name::new(format!("startup-preset-{index}")),
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(44.0),
                                    padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                selected_button_chrome(
                                    UiButtonVariant::Row,
                                    presets.selected_index == Some(index),
                                ),
                            ))
                            .with_children(|option| {
                                option.spawn((
                                    UiButtonLabel,
                                    Text::new(&entry.name),
                                    typography::reading(17.0),
                                    TextColor(widgets::INK),
                                    Pickable::IGNORE,
                                ));
                            });
                    }
                });
        });
}

pub(super) fn handle_dropdown_toggle(
    toggle_query: Query<(Entity, Ref<Interaction>), With<DropdownToggle>>,
    mut state: ResMut<DropdownState>,
    mut options: Query<&mut Node, With<DropdownOptions>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    option_interactions: Query<&Interaction, With<DropdownOption>>,
    toggle_interactions: Query<&Interaction, With<DropdownToggle>>,
) {
    let activate =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    for (entity, interaction) in &toggle_query {
        if (interaction.is_changed() && *interaction == Interaction::Pressed)
            || (activate && focus.get() == Some(entity))
        {
            state.expanded = !state.expanded;
        }
    }
    if keyboard.just_pressed(KeyCode::Escape) {
        state.expanded = false;
    }
    if mouse.just_pressed(MouseButton::Left)
        && state.expanded
        && !option_interactions
            .iter()
            .any(|i| *i == Interaction::Pressed)
        && !toggle_interactions
            .iter()
            .any(|i| *i == Interaction::Pressed)
    {
        state.expanded = false;
    }
    for mut node in &mut options {
        let display = if state.expanded {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

pub(super) fn handle_dropdown_selection(
    options: Query<
        (Entity, Ref<Interaction>, &DropdownOption),
        Without<bevy::ui::InteractionDisabled>,
    >,
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut address: ResMut<ServerAddress>,
    mut presets: ResMut<ServerPresets>,
    mut state: ResMut<DropdownState>,
    mut feedback: ResMut<crate::render::systems::ConnectionFeedback>,
) {
    if !state.expanded {
        return;
    }
    let activate =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    for (entity, interaction, option) in &options {
        if (interaction.is_changed() && *interaction == Interaction::Pressed)
            || (activate && focus.get() == Some(entity))
        {
            if let Some(entry) = presets.entries.get(option.index) {
                let Ok(preset_address) =
                    network_input::parse_server_address(&entry.ip, SERVER_PORT)
                else {
                    feedback.error_message = Some("This saved server address is invalid.".into());
                    return;
                };
                *address = preset_address;
                presets.selected_index = Some(option.index);
                state.expanded = false;
                feedback.error_message = None;
                return;
            }
        }
    }
}

pub(super) fn update_dropdown_display(
    presets: Res<ServerPresets>,
    mut options: Query<(&DropdownOption, &mut UiButtonStyle)>,
) {
    for (option, mut style) in &mut options {
        let selected = presets.selected_index == Some(option.index);
        if style.selected != selected {
            style.selected = selected;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select_preset(ip: &str, expanded: bool) -> App {
        let mut app = App::new();
        app.insert_resource(ServerPresets {
            entries: vec![ServerEntry {
                name: "Test".into(),
                ip: ip.into(),
            }],
            selected_index: None,
        })
        .insert_resource(ServerAddress {
            ip: "old.example".into(),
            port: 6000,
        })
        .insert_resource(DropdownState { expanded })
        .init_resource::<crate::render::systems::ConnectionFeedback>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .add_systems(Update, handle_dropdown_selection);
        let option = app
            .world_mut()
            .spawn((DropdownOption { index: 0 }, Interaction::None))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(option, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        app
    }

    #[test]
    fn closed_presets_cannot_activate_from_stale_keyboard_focus() {
        let app = select_preset("127.0.0.1", false);
        assert_eq!(app.world().resource::<ServerAddress>().ip, "old.example");
        assert_eq!(app.world().resource::<ServerAddress>().port, 6000);
    }

    #[test]
    fn presets_use_their_own_default_or_explicit_port() {
        for (configured, host, port) in [
            ("127.0.0.1", "127.0.0.1", SERVER_PORT),
            ("game.example:7000", "game.example", 7000),
            ("[::1]:8000", "::1", 8000),
        ] {
            let app = select_preset(configured, true);
            let address = app.world().resource::<ServerAddress>();
            assert_eq!(address.ip, host);
            assert_eq!(address.port, port);
            assert!(!app.world().resource::<DropdownState>().expanded);
        }
    }
}
