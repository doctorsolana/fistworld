//! Chat claims a frame without mutating Bevy's physical key state.

use arboard::Clipboard;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;

use crate::input::InputState;

use super::state::{ChatState, rejection_text};
use super::view::ChatDraftField;

#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct ChatInput;

pub(super) fn handle_keyboard(
    mut state: ResMut<ChatState>,
    mut input: ResMut<InputState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
    fields: Query<Entity, With<ChatDraftField>>,
    windows: Query<&Window>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    let was_open = state.open;
    let allowed = !input.ui_blocking()
        && !opening.as_ref().is_some_and(|opening| opening.is_active())
        && !windows.iter().any(|window| !window.focused);
    if !allowed {
        state.open = false;
    }
    let shortcut = keyboard.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut captured = was_open;
    let mut closed_this_frame = false;
    let mut saw_open_key = false;
    let mut saw_enter = false;
    let mut saw_escape = false;
    // Read releases too. They remain intact in ButtonInput for ordinary movement.
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        saw_open_key |= event.key_code == KeyCode::KeyT;
        saw_enter |= event.logical_key == Key::Enter;
        saw_escape |= event.logical_key == Key::Escape;
        if !allowed || closed_this_frame {
            continue;
        }
        if !state.open {
            if event.key_code == KeyCode::KeyT && !shortcut && !event.repeat {
                state.open = true;
                captured = true;
                // The opener is a command, not the first letter of the draft.
            }
            continue;
        }
        captured = true;
        match &event.logical_key {
            Key::Escape => {
                state.open = false;
                closed_this_frame = true;
            }
            Key::Enter if !event.repeat => {
                closed_this_frame = state.queue_send();
            }
            Key::Character(key) if shortcut => match key.to_ascii_lowercase().as_str() {
                "a" => {
                    state.draft.anchor = 0;
                    state.draft.cursor = state.draft.text.len();
                }
                "v" => {
                    if let Ok(mut clipboard) = Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            insert(&mut state, &text);
                        }
                    }
                }
                "c" | "x" => {
                    let range = state.draft.selection();
                    if !range.is_empty() {
                        if let Ok(mut clipboard) = Clipboard::new() {
                            if clipboard
                                .set_text(state.draft.text[range].to_owned())
                                .is_ok()
                                && key.eq_ignore_ascii_case("x")
                            {
                                state.draft.erase(true);
                            }
                        }
                    }
                }
                _ => (),
            },
            Key::Character(key) => {
                insert(&mut state, event.text.as_deref().unwrap_or(key.as_str()))
            }
            Key::Space => insert(&mut state, " "),
            Key::Backspace => {
                state.draft.erase(true);
                state.feedback = None;
            }
            Key::Delete => {
                state.draft.erase(false);
                state.feedback = None;
            }
            Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End => {
                state.draft.navigate(&event.logical_key, shift);
            }
            _ => (),
        }
    }
    // Controller/capture key injection may update ButtonInput without a text event.
    if allowed && !closed_this_frame {
        if !saw_open_key && !was_open && keyboard.just_pressed(KeyCode::KeyT) && !shortcut {
            state.open = true;
            captured = true;
        } else if state.open && !saw_escape && keyboard.just_pressed(KeyCode::Escape) {
            state.open = false;
            captured = true;
        } else if state.open && !saw_enter && keyboard.just_pressed(KeyCode::Enter) {
            state.queue_send();
            captured = true;
        }
    }
    input.text_input_active = state.open;
    input.text_input_captured = captured;
    if let Some(focus) = focus.as_mut() {
        if state.open {
            if let Some(field) = fields.iter().next() {
                focus.set(field, FocusCause::Navigated);
            }
        } else if focus.get().is_some_and(|entity| fields.contains(entity)) {
            focus.clear();
        }
    }
    state.focused = state.open;
}

fn insert(state: &mut ChatState, text: &str) {
    state.feedback = state
        .draft
        .insert(text)
        .err()
        .map(|reason| rejection_text(reason).into());
}

pub(super) fn reset(
    mut state: ResMut<ChatState>,
    mut input: ResMut<InputState>,
    fields: Query<Entity, With<ChatDraftField>>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    state.reset_session();
    input.text_input_active = false;
    input.text_input_captured = false;
    if let Some(focus) = focus.as_mut() {
        if focus.get().is_some_and(|entity| fields.contains(entity)) {
            focus.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::ButtonState;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<InputState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, handle_keyboard);
        app.world_mut()
            .resource_mut::<ChatState>()
            .set_connection(Some(Entity::from_bits(1)));
        app
    }
    fn key(app: &mut App, code: KeyCode, logical: Key, text: Option<&str>, pressed: bool) {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            text: text.map(Into::into),
            state: if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }

    #[test]
    fn opener_is_not_inserted_and_escape_preserves_draft_and_claims_closing_frame() {
        let mut app = app();
        key(
            &mut app,
            KeyCode::KeyT,
            Key::Character("t".into()),
            Some("t"),
            true,
        );
        key(
            &mut app,
            KeyCode::KeyC,
            Key::Character("c".into()),
            Some("c"),
            true,
        );
        app.update();
        assert_eq!(app.world().resource::<ChatState>().draft.text, "c");
        assert!(app.world().resource::<InputState>().text_input_active);
        key(&mut app, KeyCode::Escape, Key::Escape, None, true);
        app.update();
        assert_eq!(app.world().resource::<ChatState>().draft.text, "c");
        assert!(!app.world().resource::<InputState>().text_input_active);
        assert!(app.world().resource::<InputState>().text_input_captured);
        app.update();
        assert!(!app.world().resource::<InputState>().text_input_captured);
    }

    #[test]
    fn enter_queues_once_and_never_resurrects_released_movement_keys() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        key(
            &mut app,
            KeyCode::KeyT,
            Key::Character("t".into()),
            Some("t"),
            true,
        );
        key(
            &mut app,
            KeyCode::KeyJ,
            Key::Character("j".into()),
            Some("j"),
            true,
        );
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        key(
            &mut app,
            KeyCode::KeyW,
            Key::Character("w".into()),
            None,
            false,
        );
        key(&mut app, KeyCode::Enter, Key::Enter, None, true);
        app.update();
        let state = app.world().resource::<ChatState>();
        assert!(!state.open);
        assert!(state.pending.is_some());
        assert_eq!(state.draft.text, "j");
        assert!(app.world().resource::<InputState>().text_input_captured);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
    }
}
