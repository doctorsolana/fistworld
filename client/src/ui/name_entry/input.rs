//! Layout-aware text input. Account format remains validated by the server.

use super::*;
use arboard::Clipboard;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusCause, InputFocus};

/// Byte positions always lie on UTF-8 boundaries. The account protocol's
/// current maximum is sixteen bytes, not sixteen Unicode scalar values.
#[derive(Resource, Default)]
pub(crate) struct NameInputEditing {
    cursor: usize,
    anchor: usize,
    last_name: String,
    revision: u64,
}

impl NameInputEditing {
    pub(crate) fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    fn reconcile(&mut self, name: &str) {
        if self.last_name != name {
            self.cursor = name.len();
            self.anchor = self.cursor;
            self.last_name = name.into();
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn remove_selection(&mut self, name: &mut String) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.cursor = range.start;
        self.anchor = self.cursor;
        name.replace_range(range, "");
        true
    }

    fn insert(&mut self, name: &mut String, text: &str) {
        let filtered: String = text
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if filtered.is_empty() {
            return;
        }
        self.remove_selection(name);
        for ch in filtered.chars() {
            if name.len() + ch.len_utf8() > 16 {
                break;
            }
            name.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
        self.anchor = self.cursor;
    }

    fn erase(&mut self, name: &mut String, backward: bool) {
        if self.remove_selection(name) {
            return;
        }
        let range = if backward {
            let start = name[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            start..self.cursor
        } else {
            let end = name[self.cursor..]
                .chars()
                .next()
                .map_or(self.cursor, |c| self.cursor + c.len_utf8());
            self.cursor..end
        };
        self.cursor = range.start;
        self.anchor = self.cursor;
        name.replace_range(range, "");
    }

    fn navigate(&mut self, name: &str, key: &Key, shift: bool) {
        let selection = self.selection();
        self.cursor = match key {
            Key::Home => 0,
            Key::End => name.len(),
            Key::ArrowLeft if !shift && !selection.is_empty() => selection.start,
            Key::ArrowRight if !shift && !selection.is_empty() => selection.end,
            Key::ArrowLeft => name[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i),
            Key::ArrowRight => name[self.cursor..]
                .chars()
                .next()
                .map_or(self.cursor, |c| self.cursor + c.len_utf8()),
            _ => self.cursor,
        };
        if !shift {
            self.anchor = self.cursor;
        }
    }
}

pub(super) fn handle_text_input(
    mut input: ResMut<PlayerNameInput>,
    phase: Res<NameEntryPhase>,
    mut editing: ResMut<NameInputEditing>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut fields: Query<
        (
            Entity,
            &Interaction,
            &mut crate::ui::foundation::UiButtonStyle,
        ),
        With<NameInputField>,
    >,
    submit_buttons: Query<(), With<SubmitButton>>,
    entities: Query<()>,
    mut focus: Option<ResMut<InputFocus>>,
    mut displays: Query<(Entity, &mut Text), With<NameInputDisplay>>,
    time: Res<Time>,
    mut previous_display: Local<Option<(Entity, u64, bool)>>,
) {
    editing.reconcile(&input.name);
    if let Some(focus) = focus.as_mut() {
        if phase.is_busy() {
            if focus
                .get()
                .is_some_and(|entity| fields.contains(entity) || submit_buttons.contains(entity))
            {
                focus.clear();
            }
        } else if focus.get().is_none_or(|entity| !entities.contains(entity)) {
            if let Some((entity, _, _)) = fields.iter().next() {
                focus.set(entity, FocusCause::Navigated);
            }
        }
    }
    if let Some((entity, _, _)) = fields
        .iter()
        .find(|(_, interaction, _)| !phase.is_busy() && **interaction == Interaction::Pressed)
    {
        if let Some(focus) = focus.as_mut() {
            focus.set(entity, FocusCause::Pressed);
        }
    }
    let focused = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_none_or(|entity| fields.contains(entity));
    let editable = !input.submitted && !phase.is_busy() && focused;
    let shortcut = keyboard.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut changed = false;
    // Always drain events, including while busy: rejected requests must never
    // replay text typed during their round trip.
    for event in events.read() {
        if !editable || !event.state.is_pressed() {
            continue;
        }
        let before = input.name.clone();
        match &event.logical_key {
            Key::Character(key) if shortcut => match key.to_ascii_lowercase().as_str() {
                "a" => {
                    editing.anchor = 0;
                    editing.cursor = input.name.len();
                }
                "v" => {
                    if let Ok(mut clipboard) = Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            editing.insert(&mut input.name, &text);
                        }
                    }
                }
                "c" | "x" => {
                    let selected = &input.name[editing.selection()];
                    if !selected.is_empty() {
                        if let Ok(mut clipboard) = Clipboard::new() {
                            if clipboard.set_text(selected).is_ok() && key.eq_ignore_ascii_case("x")
                            {
                                editing.remove_selection(&mut input.name);
                            }
                        }
                    }
                }
                _ => {}
            },
            Key::Backspace => editing.erase(&mut input.name, true),
            Key::Delete => editing.erase(&mut input.name, false),
            Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End => {
                editing.navigate(&input.name, &event.logical_key, shift)
            }
            Key::Character(key) => editing.insert(
                &mut input.name,
                event.text.as_deref().unwrap_or(key.as_str()),
            ),
            _ => {}
        }
        changed |= input.name != before;
        editing.last_name.clone_from(&input.name);
        editing.revision = editing.revision.wrapping_add(1);
    }
    if changed {
        feedback.error_message = None;
    }
    for (entity, _, mut style) in &mut fields {
        let field_focused = editable
            && focus
                .as_ref()
                .and_then(|focus| focus.get())
                .is_none_or(|current| current == entity);
        let selected = field_focused && !editing.selection().is_empty();
        if style.focused != field_focused {
            style.focused = field_focused;
        }
        if style.selected != selected {
            style.selected = selected;
        }
    }
    for (entity, mut text) in &mut displays {
        let show_caret = editable && (time.elapsed_secs() % 1.0) < 0.55;
        let key = (entity, editing.revision, show_caret);
        if *previous_display == Some(key) {
            continue;
        }
        text.0.clear();
        text.0.push_str(&input.name[..editing.cursor]);
        if show_caret {
            text.0.push('|');
        }
        text.0.push_str(&input.name[editing.cursor..]);
        *previous_display = Some(key);
    }
}

#[derive(Default)]
pub(super) struct NameTextFit {
    name: String,
    width: f32,
}

/// Fit unusually wide names without generating a fresh font atlas for every
/// possible size. The normal 35px face stays unchanged for shorter names.
pub(super) fn fit_name_input(
    input: Res<PlayerNameInput>,
    fields: Query<&ComputedNode, With<NameInputField>>,
    mut displays: Query<
        (
            &Text,
            &bevy::text::TextLayoutInfo,
            &ChildOf,
            &mut UiTransform,
        ),
        With<NameInputDisplay>,
    >,
    mut fit: Local<NameTextFit>,
) {
    for (text, layout, parent, mut transform) in &mut displays {
        let Ok(field) = fields.get(parent.parent()) else {
            continue;
        };
        let available = field.content_box().width() * field.inverse_scale_factor();
        if available <= 0.0 || layout.size.x <= 0.0 {
            continue;
        }
        if fit.name != input.name {
            fit.name.clone_from(&input.name);
            fit.width = 0.0;
        }
        // Keep the largest width for this draft, reserving the thin caret's
        // advance when it is hidden; blinking must not pulse the font size.
        fit.width = fit
            .width
            .max(layout.size.x + if text.0.contains('|') { 0.0 } else { 12.0 });
        let scale = Vec2::splat((available / fit.width).min(1.0));
        if transform.scale != scale {
            transform.scale = scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_navigation_selection_and_delete_preserve_boundaries() {
        let mut text = "Åke_Robin".to_string();
        let mut edit = NameInputEditing::default();
        edit.reconcile(&text);
        edit.navigate(&text, &Key::Home, false);
        edit.navigate(&text, &Key::ArrowRight, false);
        edit.erase(&mut text, true);
        assert_eq!(text, "ke_Robin");
        edit.navigate(&text, &Key::End, false);
        edit.navigate(&text, &Key::ArrowLeft, true);
        edit.navigate(&text, &Key::ArrowLeft, true);
        edit.insert(&mut text, "yn");
        assert_eq!(text, "ke_Robyn");
        edit.navigate(&text, &Key::Home, false);
        edit.erase(&mut text, false);
        assert_eq!(text, "e_Robyn");
    }

    #[test]
    fn paste_replaces_selection_filters_invalid_text_and_respects_wire_limit() {
        let mut text = "Original".to_string();
        let mut edit = NameInputEditing::default();
        edit.reconcile(&text);
        edit.anchor = 0;
        edit.insert(&mut text, "Sir_Åke-2\n /🙂");
        assert_eq!(text, "Sir_Åke-2");
        edit.insert(&mut text, "abcdefghijklmnop");
        assert_eq!(text.len(), 16);
        assert!(text.is_char_boundary(edit.cursor));
    }

    #[test]
    fn busy_input_is_frozen_and_does_not_replay_after_rejection() {
        use bevy::input::ButtonState;
        let mut app = App::new();
        app.init_resource::<PlayerNameInput>()
            .init_resource::<NameEntryPhase>()
            .init_resource::<NameInputEditing>()
            .init_resource::<NameSubmissionFeedback>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, handle_text_input);
        let window = app.world_mut().spawn_empty().id();
        let event = |text: &str| KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character(text.into()),
            state: ButtonState::Pressed,
            text: Some(text.into()),
            repeat: false,
            window,
        };
        app.world_mut().resource_mut::<PlayerNameInput>().name = "Aldric".into();
        app.world_mut().resource_mut::<PlayerNameInput>().submitted = true;
        *app.world_mut().resource_mut::<NameEntryPhase>() = NameEntryPhase::Preparing;
        app.world_mut().write_message(event("Wrong"));
        app.update();
        assert_eq!(app.world().resource::<PlayerNameInput>().name, "Aldric");
        app.world_mut().resource_mut::<PlayerNameInput>().submitted = false;
        *app.world_mut().resource_mut::<NameEntryPhase>() = NameEntryPhase::Editing;
        app.update();
        assert_eq!(app.world().resource::<PlayerNameInput>().name, "Aldric");
        app.world_mut().write_message(event("_Å"));
        app.update();
        assert_eq!(app.world().resource::<PlayerNameInput>().name, "Aldric_Å");
    }
}
