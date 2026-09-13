//! Readable shortcut reference with ornamental keycaps, not inactive buttons.

use super::*;
use crate::ui::startup::StartupArtwork;

pub(in crate::ui::pause_menu) fn spawn_controls_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    settings: &InputSettings,
    art: &StartupArtwork,
) {
    parent
        .spawn((
            ControlsSettingsPanel,
            Name::new("pause-controls-panel"),
            crate::ui::motion::UiReveal::page(),
            page(),
        ))
        .with_children(|panel| {
            title(panel, "CONTROLS");
            skin::rule(panel);
            spawn_stepper(
                panel,
                "Mouse Sensitivity",
                InputSliderValueText(InputSliderControl::MouseSensitivity),
                InputSliderStep {
                    control: InputSliderControl::MouseSensitivity,
                    delta: -1,
                },
                InputSliderStep {
                    control: InputSliderControl::MouseSensitivity,
                    delta: 1,
                },
                &format!("{:.0}%", settings.mouse_sensitivity * 100.0),
                250.0,
                true,
            );
            skin::rule(panel);
            panel.spawn(columns()).with_children(|groups| {
                groups.spawn(column()).with_children(|camera| {
                    skin::section(camera, "Camera & Selection");
                    camera.spawn(shortcut_row()).with_children(|row| {
                        field_label(row, "Pan view");
                        row.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(5.0),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|keys| {
                            for key in ["W", "A", "S", "D"] {
                                keycap(keys, art, key);
                            }
                        });
                    });
                    for (label, text, button) in [
                        ("Zoom", "Mouse wheel", None),
                        ("Select", "Left click", Some(false)),
                        ("Move or sail", "Right click", Some(true)),
                    ] {
                        camera.spawn(shortcut_row()).with_children(|row| {
                            field_label(row, label);
                            row.spawn(Node {
                                width: Val::Px(196.0),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(11.0),
                                flex_shrink: 0.0,
                                ..default()
                            })
                            .with_children(|binding| {
                                mouse_icon(binding, button);
                                skin::label(binding, text, 17.0);
                            });
                        });
                    }
                });
                groups.spawn(column()).with_children(|menus| {
                    skin::section(menus, "Menus & Interaction");
                    for (label, key) in [
                        ("World map", "M"),
                        ("Trade nearby", "E"),
                        ("Encyclopedia", "N"),
                    ] {
                        menus.spawn(shortcut_row()).with_children(|row| {
                            field_label(row, label);
                            keycap(row, art, key);
                        });
                    }
                });
            });
            skin::rule(panel);
            skin::label(
                panel,
                "Trade is available near a Hall or Marketplace.",
                16.0,
            );
        });
}

fn shortcut_row() -> Node {
    Node {
        min_height: Val::Px(52.0),
        margin: UiRect::bottom(Val::Px(8.0)),
        ..row()
    }
}

fn keycap(parent: &mut ChildSpawnerCommands<'_>, art: &StartupArtwork, key: &str) {
    parent
        .spawn((
            Name::new(format!("settings-keycap-{key}")),
            Node {
                width: Val::Px(40.0),
                height: Val::Px(40.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            art.brass(),
            Pickable::IGNORE,
        ))
        .with_child((
            Text::new(key),
            typography::reading(18.0),
            TextColor(crate::ui::startup::widgets::IVORY),
            Pickable::IGNORE,
        ));
}

/// A tiny native mouse silhouette avoids missing icon-font glyphs. The selected
/// upper half makes left/right click bindings distinct even at reduced UI scale.
fn mouse_icon(parent: &mut ChildSpawnerCommands<'_>, right_button: Option<bool>) {
    parent
        .spawn((
            Node {
                width: Val::Px(23.0),
                height: Val::Px(32.0),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(PAPER_INK),
            Pickable::IGNORE,
        ))
        .with_children(|mouse| {
            if let Some(right) = right_button {
                mouse.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: if right {
                            Val::Percent(50.0)
                        } else {
                            Val::Px(0.0)
                        },
                        top: Val::Px(0.0),
                        width: Val::Percent(50.0),
                        height: Val::Px(13.0),
                        ..default()
                    },
                    BackgroundColor(PAPER_INK),
                    Pickable::IGNORE,
                ));
            }
            mouse.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(46.0),
                    top: Val::Px(3.0),
                    width: Val::Px(2.0),
                    height: Val::Px(9.0),
                    border_radius: BorderRadius::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(PAPER_INK),
                Pickable::IGNORE,
            ));
            mouse.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(13.0),
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(PAPER_INK),
                Pickable::IGNORE,
            ));
        });
}
