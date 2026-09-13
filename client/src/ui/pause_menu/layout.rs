//! Retained standalone menu and connected parchment settings folio.
use super::*;
use crate::ui::{
    ledger,
    startup::{widgets as startup, StartupArtwork},
    typography,
};

#[derive(Component)]
pub(super) struct StandaloneMenu;
#[derive(Component)]
pub(super) struct SettingsMenu;

// Header and body share the same column boundary at every window size.
const NAVIGATION_WIDTH: f32 = 344.0;

pub(super) fn spawn_pause_menu(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    input_settings: Res<InputSettings>,
    audio_settings: Res<crate::audio::AudioSettings>,
    art: Res<StartupArtwork>,
    open: Res<PauseMenuOpen>,
    existing: Query<Entity, With<PauseMenuRoot>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    if !open.0 || !existing.is_empty() {
        return;
    }
    commands
        .spawn((
            PauseMenuRoot,
            ModalRoot,
            Name::new("pause-root"),
            modal_root_chrome(),
            ledger::LedgerButtonScope,
        ))
        .with_children(|root| {
            root.spawn((
                backdrop::OutsideDismiss::default(),
                Name::new("pause-outside-dismiss"),
                super::super::modal::modal_backdrop_chrome(Color::NONE),
                crate::ui::foundation::surface_block(),
                bevy::ui::RelativeCursorPosition::default(),
                ZIndex(-1),
            ));
            standalone(root, &art);
            root.spawn((
                SettingsMenu,
                Name::new("pause-settings-frame"),
                crate::ui::motion::UiReveal::panel(),
                Node {
                    display: Display::None,
                    width: Val::Px(1360.0),
                    max_width: Val::Vw(90.0),
                    height: Val::Px(840.0),
                    max_height: Val::Vh(94.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                ledger::wood(),
                crate::ui::foundation::surface_block(),
                plate_shadow(),
            ))
            .with_children(|frame| {
                settings_header(frame, &art);
                frame
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|body| {
                        body.spawn(Node {
                            width: Val::Px(NAVIGATION_WIDTH),
                            flex_shrink: 0.0,
                            padding: UiRect::axes(Val::Px(36.0), Val::Px(44.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(16.0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        })
                        .with_children(|rail| navigation(rail, false));
                        body.spawn((
                            Name::new("pause-parchment"),
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                height: Val::Percent(100.0),
                                margin: UiRect::top(Val::Px(-4.0)),
                                // Keep the paper edge stationary and outside the
                                // inner page's scroll/clipping rectangle.
                                padding: UiRect::axes(Val::Px(48.0), Val::Px(32.0)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            art.paper(),
                        ))
                        .with_children(|page| {
                            spawn_graphics_panel(page, &settings, monitors.iter().next(), &art);
                            spawn_controls_panel(page, &input_settings, &art);
                            audio::spawn_audio_panel(page, &audio_settings, &art);
                        });
                    });
                frame
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(56.0),
                        flex_shrink: 0.0,
                        padding: UiRect::horizontal(Val::Px(26.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|footer| {
                        footer.spawn((
                            Node {
                                width: Val::Px(115.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                        startup::rule(footer, 210.0, false);
                        footer
                            .spawn((
                                Button,
                                PauseButton::Back,
                                Name::new("pause-back"),
                                Node {
                                    width: Val::Px(115.0),
                                    height: Val::Px(34.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                button_chrome(UiButtonVariant::Ghost),
                            ))
                            .with_child((
                                UiButtonLabel,
                                Text::new("ESC  Back"),
                                typography::reading(17.0),
                                TextColor(INK_INVERSE),
                                crate::ui::foundation::UiButtonLabelTint(INK_INVERSE),
                                Pickable::IGNORE,
                            ));
                    });
                startup::frame(frame, &art);
            });
        });
}

fn standalone(parent: &mut ChildSpawnerCommands<'_>, art: &StartupArtwork) {
    parent
        .spawn((
            StandaloneMenu,
            Node {
                width: Val::Px(460.0),
                max_width: Val::Vw(80.0),
                max_height: Val::Vh(93.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(22.0),
                ..default()
            },
            crate::ui::motion::UiReveal::panel(),
        ))
        .with_children(|stack| {
            stack.spawn((
                Node {
                    width: Val::Px(290.0),
                    height: Val::Px(82.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode::new(art.compact_wordmark.clone()),
                Pickable::IGNORE,
            ));
            stack
                .spawn((
                    Name::new("pause-menu-frame"),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    ledger::wood(),
                    crate::ui::foundation::surface_block(),
                    plate_shadow(),
                ))
                .with_children(|frame| {
                    frame
                        .spawn(Node {
                            height: Val::Px(88.0),
                            width: Val::Percent(100.0),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|header| {
                            header.spawn((
                                Text::new("GAME MENU"),
                                typography::heading(35.0),
                                TextColor(startup::IVORY),
                                Pickable::IGNORE,
                            ));
                            startup::rule(header, 220.0, false);
                        });
                    frame
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                min_height: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(32.0)),
                                row_gap: Val::Px(14.0),
                                overflow: Overflow::scroll_y(),
                                margin: UiRect::top(Val::Px(-4.0)),
                                ..default()
                            },
                            art.paper(),
                        ))
                        .with_children(|paper| {
                            navigation(paper, true);
                            paper.spawn((
                                Text::new("ESC   Return to game"),
                                typography::reading(15.0),
                                TextColor(INK),
                                Node {
                                    align_self: AlignSelf::Center,
                                    margin: UiRect::top(Val::Px(8.0)),
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ));
                        });
                    startup::frame(frame, art);
                });
        });
}

fn navigation(parent: &mut ChildSpawnerCommands<'_>, standalone: bool) {
    for (label, action) in [
        ("RESUME", PauseButton::Resume),
        ("GRAPHICS", PauseButton::Graphics),
        ("AUDIO", PauseButton::Audio),
        ("CONTROLS", PauseButton::Controls),
    ] {
        skin::action_button(
            parent,
            label,
            action,
            if standalone { 54.0 } else { 64.0 },
            standalone,
        );
    }
    if standalone {
        skin::rule(parent);
    } else {
        parent
            .spawn((
                Node {
                    height: Val::Px(24.0),
                    width: Val::Percent(100.0),
                    margin: UiRect::vertical(Val::Px(10.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|rule| startup::rule(rule, 260.0, false));
    }
    for (label, action) in [
        ("DISCONNECT", PauseButton::Disconnect),
        ("EXIT GAME", PauseButton::Exit),
    ] {
        skin::action_button(
            parent,
            label,
            action,
            if standalone { 50.0 } else { 64.0 },
            standalone,
        );
    }
}

fn settings_header(parent: &mut ChildSpawnerCommands<'_>, art: &StartupArtwork) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Px(100.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    width: Val::Px(NAVIGATION_WIDTH),
                    height: Val::Percent(100.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_child((
                    Node {
                        width: Val::Px(240.0),
                        height: Val::Px(240.0 * 170.0 / 600.0),
                        ..default()
                    },
                    ImageNode::new(art.compact_wordmark.clone()),
                    Pickable::IGNORE,
                ));
            header
                .spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn((
                        Text::new("SETTINGS"),
                        typography::heading(42.0),
                        TextColor(startup::IVORY),
                        Pickable::IGNORE,
                    ));
                    startup::rule(title, 310.0, false);
                });
            header
                .spawn((
                    Button,
                    PauseButton::Resume,
                    skin::ControlFace::Brass,
                    Name::new("pause-close"),
                    Node {
                        width: Val::Px(50.0),
                        height: Val::Px(50.0),
                        // The close control must not shift the parchment heading.
                        position_type: PositionType::Absolute,
                        right: Val::Px(30.0),
                        top: Val::Px(25.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Inverse),
                ))
                .with_child((
                    UiButtonLabel,
                    Text::new("X"),
                    typography::heading(26.0),
                    TextColor(startup::IVORY),
                    Pickable::IGNORE,
                ));
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
    for entity in &query {
        commands.entity(entity).despawn();
    }
}
