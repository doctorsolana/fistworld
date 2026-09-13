use super::*;
use crate::ui::sound::UiSoundHandled;

pub(in crate::ui::pause_menu) fn spawn_audio_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    settings: &AudioSettings,
) {
    parent.spawn((
        AudioSettingsPanel,
        Name::new("pause-audio-panel"),
        crate::ui::motion::UiReveal::page(),
        Node {
            display: Display::None,
            width: Val::Px(420.0),
            max_height: Val::Vh(88.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(24.0)),
            border: UiRect::all(Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            row_gap: Val::Px(18.0),
            overflow: Overflow::scroll_y(),
            scrollbar_width: 8.0,
            ..default()
        },
        BackgroundColor(FRONT_PANEL),
        BorderColor::all(BRASS_DARK),
        crate::ui::styles::plate_shadow(),
    )).with_children(|panel| {
        crate::ui::frame::corners(panel);
        panel.spawn((Text::new("AUDIO"), crate::ui::typography::heading(26.0), TextColor(INK_INVERSE_HEADING)));
        panel.spawn((Text::new("Set the balance of music, world and interface sounds."), crate::ui::typography::text(14.0), TextColor(INK_INVERSE_MUTED)));
        for control in AudioControl::ALL {
            spawn_slider(panel, control, settings);
        }
        panel.spawn((
            Text::new("Drag a slider or use the - / + buttons.\nTab to a slider: Left / Right adjusts; Home / End sets 0 / 100%."),
            crate::ui::typography::text(12.0), TextColor(INK_INVERSE_MUTED),
        ));
    });
}

fn spawn_slider(
    parent: &mut ChildSpawnerCommands<'_>,
    control: AudioControl,
    settings: &AudioSettings,
) {
    let value = control.get(settings);
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(9.0),
            width: Val::Percent(100.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|group| {
            group
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(control.label()),
                        crate::ui::typography::text(16.0),
                        TextColor(INK_INVERSE_HEADING),
                    ));
                    match control {
                        AudioControl::Master => {}
                        AudioControl::Music => spawn_toggle(row, true, settings.music_enabled),
                        AudioControl::Effects => spawn_toggle(row, false, settings.effects_enabled),
                    }
                });
            group
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(14.0),
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
                        button_chrome(UiButtonVariant::Inverse),
                        Node {
                            height: Val::Px(34.0),
                            flex_grow: 1.0,
                            min_width: Val::Px(150.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ))
                    .with_children(|track| {
                        track
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(8.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(BRASS_DARK),
                                BorderColor::all(BRASS.with_alpha(0.55)),
                                Pickable::IGNORE,
                            ))
                            .with_children(|rail| {
                                rail.spawn((
                                    AudioFill(control),
                                    Node {
                                        width: Val::Percent(value * 100.0),
                                        height: Val::Percent(100.0),
                                        border_radius: BorderRadius::all(Val::Px(4.0)),
                                        ..default()
                                    },
                                    BackgroundColor(EMBER),
                                    Pickable::IGNORE,
                                ));
                            });
                        track.spawn((
                            AudioThumb(control),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(value * 100.0),
                                margin: UiRect::left(Val::Px(-7.0)),
                                width: Val::Px(14.0),
                                height: Val::Px(26.0),
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(INK_INVERSE_HEADING),
                            BorderColor::all(BRASS_DARK),
                            Pickable::IGNORE,
                        ));
                    });
                    step_button(row, control, 1);
                    row.spawn((
                        AudioValue(control),
                        Name::new(format!("audio-{}-value", control.name())),
                        Text::new(format!("{:.0}%", value * 100.0)),
                        crate::ui::typography::text(16.0),
                        TextColor(INK_INVERSE),
                        Node {
                            width: Val::Px(48.0),
                            ..default()
                        },
                        TextLayout::justify(Justify::Right),
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
            Name::new(format!(
                "audio-{}-{}",
                control.name(),
                if delta < 0 { "decrease" } else { "increase" }
            )),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(32.0),
                height: Val::Px(34.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Inverse),
        ))
        .with_child((
            UiButtonLabel,
            Text::new(if delta < 0 { "-" } else { "+" }),
            crate::ui::typography::text(20.0),
            TextColor(INK_INVERSE),
        ));
}

fn spawn_toggle(parent: &mut ChildSpawnerCommands<'_>, music: bool, enabled: bool) {
    let mut button = parent.spawn((
        Button,
        Name::new(if music {
            "pause-music"
        } else {
            "pause-effects"
        }),
        RelativeCursorPosition::default(),
        Node {
            width: Val::Px(132.0),
            height: Val::Px(32.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        selected_button_chrome(UiButtonVariant::Inverse, enabled),
    ));
    if music {
        button.insert(MusicToggle);
    } else {
        button.insert(EffectsToggle);
    }
    button.with_children(|button| {
        let mut label = button.spawn((
            UiButtonLabel,
            Text::new(if music {
                super::super::music::label(enabled)
            } else {
                effects::label(enabled)
            }),
            crate::ui::typography::text(14.0),
            TextColor(INK_INVERSE),
        ));
        if music {
            label.insert(MusicToggleLabel);
        } else {
            label.insert(EffectsToggleLabel);
        }
    });
}
