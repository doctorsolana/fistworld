//! animation systems.

use super::*;

/// Smoothly animate the menu transition when opening/closing settings panels
pub(super) fn animate_menu_transition(
    time: Res<Time>,
    mut state: ResMut<PauseMenuState>,
    mut container_query: Query<
        &mut Node,
        (
            With<MenuContentContainer>,
            Without<GraphicsSettingsPanel>,
            Without<ControlsSettingsPanel>,
        ),
    >,
    mut graphics_panel_query: Query<
        &mut Node,
        (
            With<GraphicsSettingsPanel>,
            Without<MenuContentContainer>,
            Without<ControlsSettingsPanel>,
        ),
    >,
    mut controls_panel_query: Query<
        &mut Node,
        (
            With<ControlsSettingsPanel>,
            Without<MenuContentContainer>,
            Without<GraphicsSettingsPanel>,
        ),
    >,
) {
    // Determine target based on which panel is open (only one at a time)
    let target = if state.graphics_open || state.controls_open {
        1.0
    } else {
        0.0
    };
    let speed = 10.0; // Animation speed

    // Smoothly interpolate toward target
    let diff = target - state.transition;
    if diff.abs() > 0.001 {
        state.transition += diff * (1.0 - (-speed * time.delta_secs()).exp());
        state.transition = state.transition.clamp(0.0, 1.0);
    } else {
        state.transition = target;
    }

    // Shift the entire content container left when a panel opens
    // Negative margin moves it left, making room for the panel on the right
    let offset = -80.0 * state.transition;

    for mut node in container_query.iter_mut() {
        if node.margin.left != Val::Px(offset) {
            node.margin.left = Val::Px(offset);
        }
    }

    // Show/hide the graphics panel
    for mut node in graphics_panel_query.iter_mut() {
        sync_panel(&mut node, state.graphics_open, state.transition);
    }

    // Show/hide the controls panel
    for mut node in controls_panel_query.iter_mut() {
        sync_panel(&mut node, state.controls_open, state.transition);
    }
}

fn sync_panel(node: &mut Mut<Node>, open: bool, transition: f32) {
    let visible = open && transition > 0.01;
    let display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    let width = Val::Px(if visible { 260.0 * transition } else { 0.0 });
    if node.display != display {
        node.display = display;
    }
    if node.min_width != width {
        node.min_width = width;
    }
}
