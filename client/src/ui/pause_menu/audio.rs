//! Retained sound levels: direct manipulation, precise steps and keyboard input.

mod layout;
#[cfg(test)]
mod tests;

pub(super) use layout::spawn_audio_panel;

use super::*;
use crate::audio::{sfx::SfxCue, AudioSettings};
use bevy::input_focus::InputFocus;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AudioControl {
    Master,
    Music,
    Effects,
}

impl AudioControl {
    const ALL: [Self; 3] = [Self::Master, Self::Music, Self::Effects];

    fn name(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Music => "music",
            Self::Effects => "effects",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Master => "MASTER",
            Self::Music => "MUSIC",
            Self::Effects => "EFFECTS",
        }
    }

    fn get(self, settings: &AudioSettings) -> f32 {
        match self {
            Self::Master => settings.master_volume,
            Self::Music => settings.music_volume,
            Self::Effects => settings.effects_volume,
        }
    }

    fn set(self, settings: &mut AudioSettings, value: f32) {
        match self {
            Self::Master => settings.master_volume = value,
            Self::Music => settings.music_volume = value,
            Self::Effects => settings.effects_volume = value,
        }
    }
}

#[derive(Component)]
pub(super) struct AudioSlider(AudioControl);
#[derive(Component)]
pub(super) struct AudioStep {
    control: AudioControl,
    delta: i32,
}
#[derive(Component)]
pub(super) struct AudioValue(AudioControl);
#[derive(Component)]
pub(super) struct AudioFill(AudioControl);
#[derive(Component)]
pub(super) struct AudioThumb(AudioControl);
/// Explicit value for one ON/OFF choice; activation never inverts the other choice.
#[derive(Component, Clone, Copy)]
pub(super) struct AudioEnabledChoice(pub bool);

#[derive(Resource, Default)]
pub(super) struct AudioDrag {
    active: Option<Entity>,
    armed: bool,
}

fn step(value: f32, direction: i32) -> f32 {
    let units = value * 20.0;
    let next = if direction > 0 {
        (units + 0.001).floor() + 1.0
    } else {
        (units - 0.001).ceil() - 1.0
    };
    (next / 20.0).clamp(0.0, 1.0)
}

pub(super) fn handle_audio_controls(
    open: Res<PauseMenuOpen>,
    state: Res<PauseMenuState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    sliders: Query<
        (
            Entity,
            Ref<Interaction>,
            &RelativeCursorPosition,
            &AudioSlider,
        ),
        Without<InteractionDisabled>,
    >,
    steps: Query<
        (
            Entity,
            Ref<Interaction>,
            &RelativeCursorPosition,
            &AudioStep,
        ),
        Without<InteractionDisabled>,
    >,
    toggles: Query<
        (
            Entity,
            Has<MusicToggle>,
            Has<EffectsToggle>,
            &AudioEnabledChoice,
        ),
        (
            Or<(With<MusicToggle>, With<EffectsToggle>)>,
            Without<InteractionDisabled>,
        ),
    >,
    mut drag: ResMut<AudioDrag>,
    mut settings: ResMut<AudioSettings>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    if !open.0 || !state.audio_open {
        *drag = AudioDrag::default();
        return;
    }
    if !drag.armed {
        drag.armed = !mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left);
        return;
    }
    let focused = focus.as_ref().and_then(|focus| focus.get());
    let activate =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    // The dedicated mouse handlers own clicks; a simultaneous activation key
    // does not need to activate the same choice a second time.
    if activate && !mouse.just_pressed(MouseButton::Left) {
        for (entity, music, effects, choice) in &toggles {
            if focused != Some(entity) {
                continue;
            }
            if music && settings.music_enabled != choice.0 {
                settings.music_enabled = choice.0;
            }
            if effects && settings.effects_enabled != choice.0 {
                settings.effects_enabled = choice.0;
            }
            sounds.emit(SfxCue::UiClick);
        }
    }
    for (entity, interaction, cursor, control) in &steps {
        let clicked = mouse.just_pressed(MouseButton::Left)
            && !interaction.is_added()
            && interaction.is_changed()
            && *interaction == Interaction::Pressed
            && cursor.cursor_over;
        if clicked || (activate && focused == Some(entity)) {
            let current = control.control.get(&settings);
            let next = step(current, control.delta);
            if current != next {
                control.control.set(&mut settings, next);
                sounds.emit(SfxCue::UiClick);
            }
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        drag.active = sliders.iter().find_map(|(entity, interaction, cursor, _)| {
            (!interaction.is_added()
                && interaction.is_changed()
                && *interaction == Interaction::Pressed
                && cursor.cursor_over)
                .then_some(entity)
        });
    }
    for (entity, _, cursor, control) in &sliders {
        let current = control.0.get(&settings);
        let mut next = current;
        if drag.active == Some(entity)
            && (mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left))
        {
            if let Some(position) = cursor.normalized {
                next = (position.x + 0.5).clamp(0.0, 1.0);
            }
        } else if focused == Some(entity) {
            if keyboard.just_pressed(KeyCode::ArrowLeft) {
                next = step(current, -1);
            }
            if keyboard.just_pressed(KeyCode::ArrowRight) {
                next = step(current, 1);
            }
            if keyboard.just_pressed(KeyCode::Home) {
                next = 0.0;
            }
            if keyboard.just_pressed(KeyCode::End) {
                next = 1.0;
            }
        }
        if current != next {
            control.0.set(&mut settings, next);
            // A drag is one gesture; its continuous updates are deliberately silent.
            if mouse.just_pressed(MouseButton::Left) || drag.active != Some(entity) {
                sounds.emit(SfxCue::UiClick);
            }
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        drag.active = None;
    }
}

pub(super) fn sync_audio_controls(
    settings: Res<AudioSettings>,
    mut labels: Query<(&AudioValue, &mut Text)>,
    mut fills: Query<(&AudioFill, &mut Node), Without<AudioThumb>>,
    mut thumbs: Query<(&AudioThumb, &mut Node), Without<AudioFill>>,
    buttons: Query<(
        Entity,
        &AudioStep,
        Has<input::SettingUnavailable>,
        Has<input::HiddenMenuControl>,
    )>,
    mut commands: Commands,
) {
    for (control, mut text) in &mut labels {
        let desired = format!("{:.0}%", control.0.get(&settings) * 100.0);
        if text.0 != desired {
            text.0 = desired;
        }
    }
    for (control, mut node) in &mut fills {
        let width = Val::Percent(control.0.get(&settings) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
    for (control, mut node) in &mut thumbs {
        let left = Val::Percent(control.0.get(&settings) * 100.0);
        if node.left != left {
            node.left = left;
        }
    }
    for (entity, control, disabled, hidden) in &buttons {
        let current = control.control.get(&settings);
        let at_limit = current == step(current, control.delta);
        input::sync_unavailable(&mut commands, entity, at_limit, disabled, hidden);
    }
}
