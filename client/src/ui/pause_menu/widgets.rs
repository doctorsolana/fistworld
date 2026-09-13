//! Retained parchment-page controls. Actions and values remain owned by the
//! existing settings systems; these helpers only lay out their native widgets.

mod controls;
mod graphics;
#[cfg(test)]
mod tests;

pub(super) use controls::spawn_controls_panel;
pub(super) use graphics::spawn_graphics_panel;

use super::*;
use crate::ui::{startup::widgets::INK as PAPER_INK, typography};

/// A fixed ON/OFF choice, rather than a button whose meaning flips after use.
#[derive(Component)]
pub(super) struct GraphicsToggleChoice(pub bool);

/// The stationary parchment host owns its safe edge gutter. Scrolling starts
/// inside that gutter, so clipped controls cannot cross the torn paper border.
pub(super) fn page() -> Node {
    Node {
        display: Display::None,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        min_height: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        overflow: Overflow::scroll_y(),
        scrollbar_width: 8.0,
        ..default()
    }
}

pub(super) fn title(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text),
        typography::heading(44.0),
        TextColor(PAPER_INK),
        Node {
            flex_shrink: 0.0,
            margin: UiRect::bottom(Val::Px(8.0)),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

fn row() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        min_height: Val::Px(44.0),
        flex_shrink: 0.0,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        column_gap: Val::Px(10.0),
        margin: UiRect::bottom(Val::Px(6.0)),
        ..default()
    }
}

fn field_label(parent: &mut ChildSpawnerCommands<'_>, label: &str) {
    parent.spawn((
        Text::new(label),
        typography::reading_strong(20.0),
        TextColor(PAPER_INK),
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

fn spawn_toggle(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    toggle: GraphicsToggle,
    enabled: bool,
) {
    parent.spawn(row()).with_children(|row| {
        field_label(row, label);
        row.spawn(control_column(
            180.0,
            matches!(toggle, GraphicsToggle::Vsync),
        ))
        .with_children(|column| {
            column
                .spawn(Node {
                    width: Val::Px(180.0),
                    height: Val::Px(40.0),
                    column_gap: Val::Px(2.0),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|choices| {
                    for (text, value) in [("ON", true), ("OFF", false)] {
                        choices
                            .spawn((
                                Button,
                                Name::new(format!("settings-{label}-{text}")),
                                toggle,
                                GraphicsToggleChoice(value),
                                skin::ControlFace::Choice,
                                selected_button_chrome(UiButtonVariant::Inverse, enabled == value),
                                Node {
                                    flex_grow: 1.0,
                                    flex_basis: Val::Px(0.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                            ))
                            .with_child((
                                Text::new(text),
                                UiButtonLabel,
                                typography::reading(16.0),
                                TextColor(crate::ui::startup::widgets::IVORY),
                                Pickable::IGNORE,
                            ));
                    }
                });
        });
    });
}

fn spawn_slider(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    control: SliderControl,
    value: &str,
    wide: bool,
) {
    spawn_stepper(
        parent,
        label,
        SliderValueText(control),
        SliderStep { control, delta: -1 },
        SliderStep { control, delta: 1 },
        value,
        if wide { 300.0 } else { 196.0 },
        wide,
    );
}

fn spawn_stepper(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    value_marker: impl Component,
    lower: impl Component,
    higher: impl Component,
    value: &str,
    width: f32,
    display_column: bool,
) {
    parent.spawn(row()).with_children(|row| {
        field_label(row, label);
        row.spawn(control_column(width, display_column))
            .with_children(|column| {
                column
                    .spawn(Node {
                        width: Val::Px(width),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|controls| {
                        spawn_step_button(controls, lower, label, false);
                        controls
                            .spawn((
                                Node {
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ))
                            .with_child((
                                value_marker,
                                Text::new(value),
                                typography::reading(17.0),
                                TextLayout::justify(Justify::Center).with_no_wrap(),
                                TextColor(PAPER_INK),
                                Pickable::IGNORE,
                            ));
                        spawn_step_button(controls, higher, label, true);
                    });
            });
    });
}

/// Display values share the mode selector's center; compact quality controls
/// remain flush with their own column's edge.
fn control_column(width: f32, display: bool) -> Node {
    Node {
        width: if display {
            Val::Percent(72.0)
        } else {
            Val::Px(width)
        },
        min_width: Val::Px(width),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn spawn_step_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Component,
    label: &str,
    increase: bool,
) {
    parent
        .spawn((
            Button,
            marker,
            Name::new(format!(
                "settings-{label}-{}",
                if increase { "increase" } else { "decrease" }
            )),
            skin::ControlFace::Brass,
            Node {
                width: Val::Px(40.0),
                height: Val::Px(40.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            button_chrome(UiButtonVariant::Inverse),
        ))
        .with_child((
            Text::new(if increase { ">" } else { "<" }),
            UiButtonLabel,
            typography::reading_strong(20.0),
            TextColor(crate::ui::startup::widgets::IVORY),
            Pickable::IGNORE,
        ));
}

fn columns() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        flex_shrink: 0.0,
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(24.0),
        row_gap: Val::Px(18.0),
        ..default()
    }
}

fn column() -> Node {
    Node {
        flex_basis: Val::Px(330.0),
        flex_grow: 1.0,
        min_width: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        ..default()
    }
}
