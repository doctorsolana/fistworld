use super::*;
use crate::ui::{
    pause_menu::skin::{self, ControlFace},
    sound::UiSoundHandled,
    startup::{widgets::INK, StartupArtwork},
    typography,
};

pub(in crate::ui::pause_menu) fn spawn_audio_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    settings: &AudioSettings,
    art: &StartupArtwork,
) {
    parent
        .spawn((
            AudioSettingsPanel,
            Name::new("pause-audio-panel"),
            crate::ui::motion::UiReveal::page(),
            Node {
                row_gap: Val::Px(10.0),
                ..crate::ui::pause_menu::widgets::page()
            },
        ))
        .with_children(|panel| {
            crate::ui::pause_menu::widgets::title(panel, "AUDIO");
            skin::label(panel, "Music, world and interface sounds.", 17.0);
            skin::rule(panel);
            for control in AudioControl::ALL {
                spawn_slider(panel, control, settings, art);
                skin::rule(panel);
            }
            panel.spawn((
                Text::new("Drag to adjust  ·  - / + for small steps\nTab select  ·  Left / Right adjust  ·  Home / End min / max"),
                typography::reading(14.0),
                TextColor(INK),
                TextLayout::justify(Justify::Center),
                Node {
                    width: Val::Percent(100.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

fn spawn_slider(
    parent: &mut ChildSpawnerCommands<'_>,
    control: AudioControl,
    settings: &AudioSettings,
    art: &StartupArtwork,
) {
    let value = control.get(settings);
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(11.0),
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|group| {
            group
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(40.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(if control == AudioControl::Master {
                            "MASTER VOLUME"
                        } else {
                            control.label()
                        }),
                        typography::reading_strong(20.0),
                        TextColor(INK),
                        Pickable::IGNORE,
                    ));
                    match control {
                        AudioControl::Master => {}
                        AudioControl::Music => spawn_toggle(row, control, settings.music_enabled),
                        AudioControl::Effects => {
                            spawn_toggle(row, control, settings.effects_enabled)
                        }
                    }
                });
            group
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    height: Val::Px(42.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(20.0),
                    ..default()
                })
                .with_children(|row| {
                    step_button(row, control, -1);
                    row.spawn((
                        Button,
                        AudioSlider(control),
                        UiSoundHandled,
                        Name::new(format!("audio-{}-slider", control.name())),
                        RelativeCursorPosition::default(),
                        button_chrome(UiButtonVariant::Ghost),
                        Node {
                            height: Val::Px(40.0),
                            flex_grow: 1.0,
                            min_width: Val::Px(100.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ))
                    .with_children(|track| {
                        track
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(16.0),
                                    border: UiRect::all(Val::Px(3.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(FRONT_PANEL),
                                BorderColor::all(BRASS_DARK),
                                Pickable::IGNORE,
                            ))
                            .with_children(|rail| {
                                rail.spawn((
                                    AudioFill(control),
                                    Node {
                                        width: Val::Percent(value * 100.0),
                                        height: Val::Percent(100.0),
                                        border: UiRect::vertical(Val::Px(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(crate::ui::startup::widgets::GOLD),
                                    BorderColor::all(INK_INVERSE_HEADING),
                                    Pickable::IGNORE,
                                ));
                            });
                        for tick in 0..=10 {
                            track.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(tick as f32 * 10.0),
                                    bottom: Val::Px(0.0),
                                    width: Val::Px(1.0),
                                    height: Val::Px(if tick % 5 == 0 { 6.0 } else { 4.0 }),
                                    ..default()
                                },
                                BackgroundColor(BRASS_DARK),
                                Pickable::IGNORE,
                            ));
                        }
                        track.spawn((
                            AudioThumb(control),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(value * 100.0),
                                top: Val::Px(7.0),
                                margin: UiRect::left(Val::Px(-13.0)),
                                width: Val::Px(26.0),
                                height: Val::Px(26.0),
                                ..default()
                            },
                            art.brass(),
                            UiTransform::from_rotation(Rot2::degrees(45.0)),
                            ZIndex(1),
                            Pickable::IGNORE,
                        ));
                    });
                    step_button(row, control, 1);
                    row.spawn((
                        AudioValue(control),
                        Name::new(format!("audio-{}-value", control.name())),
                        Text::new(format!("{:.0}%", value * 100.0)),
                        typography::reading_strong(18.0),
                        TextColor(INK),
                        Node {
                            width: Val::Px(57.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        TextLayout::justify(Justify::Right),
                        Pickable::IGNORE,
                    ));
                });
        });
}

fn step_button(parent: &mut ChildSpawnerCommands<'_>, control: AudioControl, delta: i32) {
    parent
        .spawn((
            Button,
            AudioStep { control, delta },
            UiSoundHandled,
            ControlFace::Brass,
            Name::new(format!(
                "audio-{}-{}",
                control.name(),
                if delta < 0 { "decrease" } else { "increase" }
            )),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(42.0),
                height: Val::Px(42.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            button_chrome(UiButtonVariant::Inverse),
        ))
        .with_child((
            UiButtonLabel,
            Text::new(if delta < 0 { "-" } else { "+" }),
            typography::reading_strong(25.0),
            TextColor(INK_INVERSE),
            Pickable::IGNORE,
        ));
}

fn spawn_toggle(parent: &mut ChildSpawnerCommands<'_>, control: AudioControl, enabled: bool) {
    parent
        .spawn(Node {
            width: Val::Px(196.0),
            height: Val::Px(40.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|choices| {
            for choice in [true, false] {
                let mut button = choices.spawn((
                    Button,
                    AudioEnabledChoice(choice),
                    Name::new(format!(
                        "pause-{}-{}",
                        control.name(),
                        if choice { "on" } else { "off" }
                    )),
                    ControlFace::Choice,
                    RelativeCursorPosition::default(),
                    Node {
                        width: Val::Percent(50.0),
                        height: Val::Percent(100.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    selected_button_chrome(UiButtonVariant::Inverse, enabled == choice),
                ));
                if control == AudioControl::Music {
                    button.insert(MusicToggle);
                } else {
                    button.insert(EffectsToggle);
                }
                button.with_child((
                    UiButtonLabel,
                    Text::new(if choice { "ON" } else { "OFF" }),
                    typography::reading_strong(16.0),
                    TextColor(crate::ui::startup::widgets::IVORY),
                    Pickable::IGNORE,
                ));
            }
        });
}
