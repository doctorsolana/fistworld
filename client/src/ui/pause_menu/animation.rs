//! Page visibility and selection; shared UiReveal owns spring motion.
use super::*;

pub(super) fn animate_menu_transition(
    state: Res<PauseMenuState>,
    mut nodes: Query<
        (
            &mut Node,
            Has<layout::StandaloneMenu>,
            Has<layout::SettingsMenu>,
            Has<GraphicsSettingsPanel>,
            Has<ControlsSettingsPanel>,
            Has<AudioSettingsPanel>,
        ),
        Or<(
            With<layout::StandaloneMenu>,
            With<layout::SettingsMenu>,
            With<GraphicsSettingsPanel>,
            With<ControlsSettingsPanel>,
            With<AudioSettingsPanel>,
        )>,
    >,
    mut buttons: Query<(&PauseButton, &mut UiButtonStyle)>,
) {
    let expanded = state.graphics_open || state.controls_open || state.audio_open;
    for (mut node, standalone, settings, graphics, controls, audio) in &mut nodes {
        let visible = (standalone && !expanded)
            || (settings && expanded)
            || (graphics && state.graphics_open)
            || (controls && state.controls_open)
            || (audio && state.audio_open);
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (action, mut style) in &mut buttons {
        let selected = match action {
            PauseButton::Graphics => state.graphics_open,
            PauseButton::Audio => state.audio_open,
            PauseButton::Controls => state.controls_open,
            _ => false,
        };
        if style.selected != selected {
            style.selected = selected;
        }
    }
}
