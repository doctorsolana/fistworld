//! Compact server controls beside the shared FistWorld village illustration.
use super::*;
use crate::ui::{
    startup::{widgets, StartupArtwork},
    typography,
};

pub(super) fn apply_launcher_window_settings(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    let resolution = capture.as_ref().map_or(LAUNCHER_RESOLUTION, |config| {
        (config.resolution[0], config.resolution[1])
    });
    for mut window in &mut windows {
        crate::app_wiring::apply_window_mode(
            &mut window,
            &mut ui_scale,
            monitors.iter().next(),
            DisplayMode::Windowed,
            DisplayResolution::new(resolution.0, resolution.1),
        );
    }
}

#[derive(Component)]
pub(super) struct ConnectionErrorText;

pub(super) fn spawn_main_menu(
    mut commands: Commands,
    art: Res<StartupArtwork>,
    server_address: Res<ServerAddress>,
    presets: Res<ServerPresets>,
    feedback: Res<crate::render::systems::ConnectionFeedback>,
    mut dropdown_state: ResMut<DropdownState>,
) {
    dropdown_state.expanded = false;
    commands
        .spawn((MainMenuRoot, widgets::screen()))
        .with_children(|root| {
            root.spawn((
                Name::new("startup-launcher-stack"),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Vw(4.5),
                    top: Val::Vh(12.0),
                    width: Val::Px(432.0),
                    max_width: Val::Vw(43.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(28.0),
                    ..default()
                },
            ))
            .with_children(|stack| {
                stack.spawn(widgets::wordmark(&art, Val::Percent(100.0)));
                stack
                    .spawn((
                        widgets::panel(),
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(378.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(Val::Px(38.0), Val::Px(40.0)),
                            row_gap: Val::Px(26.0),
                            ..default()
                        },
                        art.paper(),
                    ))
                    .with_children(|panel| {
                        panel
                            .spawn((Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: Val::Px(3.0),
                                ..default()
                            },))
                            .with_children(|heading| {
                                heading.spawn((
                                    Name::new("startup-heading"),
                                    Text::new("SERVER"),
                                    typography::heading(19.0),
                                    TextColor(widgets::INK),
                                    Pickable::IGNORE,
                                ));
                                widgets::rule(heading, 300.0, true);
                            });
                        panel
                            .spawn((Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(54.0),
                                column_gap: Val::Px(2.0),
                                ..default()
                            },))
                            .with_children(|row| {
                                row.spawn((
                                    IpInputField { focused: false },
                                    Name::new("startup-server-field"),
                                    widgets::field(&art, 54.0),
                                ))
                                .insert(Node {
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0),
                                    height: Val::Percent(100.0),
                                    padding: UiRect::horizontal(Val::Px(9.0)),
                                    overflow: Overflow::clip(),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                })
                                .with_children(|field| {
                                    field.spawn((
                                        IpTextDisplay,
                                        UiButtonLabel,
                                        Text::new(format!(
                                            "{}:{}",
                                            server_address.ip, server_address.port
                                        )),
                                        typography::reading(22.0),
                                        TextColor(widgets::INK),
                                        TextLayout::no_wrap(),
                                        Pickable::IGNORE,
                                    ));
                                });
                                if !presets.entries.is_empty() {
                                    spawn_dropdown(row, &presets, &art);
                                }
                            });
                        panel.spawn((
                            ConnectionErrorText,
                            Name::new("startup-error"),
                            Text::new(feedback.error_message.clone().unwrap_or_default()),
                            typography::reading(14.0),
                            TextColor(Color::srgb(0.53, 0.10, 0.055)),
                            Node {
                                display: if feedback.error_message.is_some() {
                                    Display::Flex
                                } else {
                                    Display::None
                                },
                                max_width: Val::Percent(100.0),
                                ..default()
                            },
                            TextLayout::justify(Justify::Center),
                            Pickable::IGNORE,
                        ));
                        widgets::action_button(
                            panel,
                            &art,
                            "CONNECT",
                            (MenuButton::Connect, crate::ui::sound::UiSoundHandled),
                            Val::Percent(100.0),
                            72.0,
                            true,
                        );
                        widgets::action_button(
                            panel,
                            &art,
                            "EXIT",
                            (MenuButton::Exit, crate::ui::sound::UiSoundHandled),
                            Val::Px(232.0),
                            56.0,
                            false,
                        );
                        widgets::frame(panel, &art);
                    });
            });
            root.spawn((
                Text::new(concat!("v", env!("CARGO_PKG_VERSION"))),
                typography::reading(16.0),
                TextColor(widgets::GOLD),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(22.0),
                    bottom: Val::Px(18.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

pub(super) fn sync_connection_error(
    feedback: Res<crate::render::systems::ConnectionFeedback>,
    mut texts: Query<(&mut Text, &mut Node), With<ConnectionErrorText>>,
) {
    for (mut text, mut node) in &mut texts {
        let message = feedback.error_message.as_deref().unwrap_or_default();
        if text.0 != message {
            text.0 = message.into();
        }
        let display = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
}

pub(super) fn despawn_main_menu(mut commands: Commands, roots: Query<Entity, With<MainMenuRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}
