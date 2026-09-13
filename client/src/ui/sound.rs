//! UI presentation cues, collected after the screen has accepted its input.
//!
//! Snapshot ordinary presses before Update can replace their entities. Specific
//! actions and semantic book navigation win over that one generic candidate.
//! This adapter owns no UI or server decisions.

mod book;
#[cfg(test)]
mod tests;

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};

use super::foundation::{UiButtonStyle, UiButtonVariant};
use super::modal::ModalRoot;
use crate::audio::sfx::{SfxCue, SfxRequest, SfxSet};

/// An action with its own acceptance rules emits through `UiActionSounds`.
/// This also excludes release-driven creator controls from the press adapter.
#[derive(Component)]
pub(crate) struct UiSoundHandled;

#[derive(Resource, Default)]
pub(crate) struct UiSoundFrame {
    generic: bool,
    specific: Option<SfxCue>,
    pressed: Vec<Entity>,
}

/// Optional only to preserve small, audio-free domain test applications.
/// The full UI plugin always installs the frame resource.
#[derive(SystemParam)]
pub(crate) struct UiActionSounds<'w> {
    frame: Option<ResMut<'w, UiSoundFrame>>,
}

impl UiActionSounds<'_> {
    pub(crate) fn result(&mut self, success: bool) {
        self.emit(if success {
            SfxCue::UiConfirm
        } else {
            SfxCue::UiReject
        });
    }

    pub(crate) fn pressed(&self, entity: Entity) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|frame| frame.pressed.contains(&entity))
    }

    pub(crate) fn emit(&mut self, cue: SfxCue) {
        if let Some(frame) = self.frame.as_mut() {
            if frame.specific.is_none() || cue != SfxCue::UiClick {
                frame.specific = Some(cue);
            }
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<UiSoundFrame>();
    app.add_systems(
        PreUpdate,
        collect_button_press.after(bevy::ui::UiSystems::Focus),
    );
    app.add_systems(
        PostUpdate,
        (emit_ui_sound, add_live_pointer_tracking)
            .chain()
            .in_set(SfxSet::Collect),
    );
}

fn add_live_pointer_tracking(
    mut commands: Commands,
    buttons: Query<
        Entity,
        (
            With<Button>,
            Added<UiButtonStyle>,
            Without<RelativeCursorPosition>,
        ),
    >,
) {
    for entity in &buttons {
        commands
            .entity(entity)
            .insert(RelativeCursorPosition::default());
    }
}

pub(crate) fn collect_button_press(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (
            Entity,
            Ref<Interaction>,
            &RelativeCursorPosition,
            &UiButtonStyle,
            Has<UiSoundHandled>,
        ),
        (With<Button>, Without<InteractionDisabled>),
    >,
    parents: Query<&ChildOf>,
    modals: Query<Entity, With<ModalRoot>>,
    encyclopedia_roots: Query<(), With<super::encyclopedia::EncyclopediaRoot>>,
    encyclopedia_guard: Option<Res<super::encyclopedia::ClickGuard>>,
    mut armed_modals: Local<HashSet<Entity>>,
    mut frame: ResMut<UiSoundFrame>,
) {
    frame.generic = false;
    frame.specific = None;
    frame.pressed.clear();
    armed_modals.retain(|entity| modals.contains(*entity));
    if !mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left) {
        armed_modals.extend(modals.iter());
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (entity, interaction, cursor, style, handled) in &buttons {
        if !interaction.is_changed()
            || interaction.is_added()
            || *interaction != Interaction::Pressed
            || !cursor.cursor_over
            || (style.selected
                && matches!(style.variant, UiButtonVariant::Tab | UiButtonVariant::Row))
        {
            continue;
        }
        let modal = parents
            .iter_ancestors(entity)
            .find(|ancestor| modals.contains(*ancestor));
        let allowed = if let Some(modal) = modal {
            armed_modals.contains(&modal)
                && (!encyclopedia_roots.contains(modal)
                    || encyclopedia_guard.as_ref().is_some_and(|guard| guard.0))
        } else {
            modals.is_empty()
        };
        if allowed {
            frame.pressed.push(entity);
            frame.generic |= !handled;
        }
    }
}

fn emit_ui_sound(
    book: book::BookState,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    state: Option<Res<State<crate::states::GameState>>>,
    mut previous: Local<Option<book::BookSnapshot>>,
    mut frame: ResMut<UiSoundFrame>,
    mut requests: MessageWriter<SfxRequest>,
) {
    let current = book.snapshot(state.as_ref().map(|state| *state.get()));
    let navigation_input = mouse.just_pressed(MouseButton::Left)
        || keyboard.any_just_pressed([
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::NumpadEnter,
            KeyCode::Space,
        ]);
    let book_cue = previous
        .as_ref()
        .and_then(|previous| previous.transition(&current, navigation_input));
    *previous = Some(current);
    let specific = frame.specific.take();
    let cue = specific
        .filter(|cue| *cue != SfxCue::UiClick)
        .or(book_cue)
        .or(specific)
        .or(frame.generic.then_some(SfxCue::UiClick));
    if let Some(cue) = cue {
        requests.write(SfxRequest::new(cue));
    }
    frame.generic = false;
}
