//! Retained directory search: local knowledge only, with native keyboard focus.
//!
//! Editing owns the current and closing frame after chat input. Query terms are
//! normalized only when edited; list owners retain their existing row signatures.

use arboard::Clipboard;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{
    FocusCause, InputFocus,
    tab_navigation::{TabGroup, TabIndex},
};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::{ClickGuard, EncyclopediaOpen, EncyclopediaTab, PersonRecord, TabBody};
use crate::input::InputState;
use crate::ui::foundation::{UiArtworkFocus, UiButtonLabel, UiButtonVariant, button_chrome};
use crate::ui::ledger;
use crate::ui::styles::{BRASS, INK, INK_MUTED};

const MAX_CHARACTERS: usize = 96;

#[derive(Default)]
struct SearchDraft {
    text: String,
    cursor: usize,
    anchor: usize,
    terms: Vec<String>,
    revision: u64,
}

impl SearchDraft {
    fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    fn changed(&mut self) {
        self.terms = self
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.revision = self.revision.wrapping_add(1);
    }

    fn clear(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.changed();
    }

    fn insert(&mut self, value: &str) {
        let range = self.selection();
        let remaining =
            MAX_CHARACTERS - (self.text.chars().count() - self.text[range.clone()].chars().count());
        // A paste stays one line. Do not allow control characters to disguise a name.
        let value: String = value
            .chars()
            .filter(|c| !c.is_control())
            .take(remaining)
            .collect();
        if value.is_empty() {
            return;
        }
        self.text.replace_range(range.clone(), &value);
        self.cursor = range.start + value.len();
        self.anchor = self.cursor;
        self.changed();
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    fn erase(&mut self, backward: bool) {
        let selection = self.selection();
        let range = if !selection.is_empty() {
            selection
        } else if backward {
            self.previous()..self.cursor
        } else {
            self.cursor..self.next()
        };
        if !range.is_empty() {
            self.text.replace_range(range.clone(), "");
            self.cursor = range.start;
            self.anchor = self.cursor;
            self.changed();
        }
    }

    fn navigate(&mut self, key: &Key, shift: bool) {
        let selection = self.selection();
        self.cursor = match key {
            Key::Home => 0,
            Key::End => self.text.len(),
            Key::ArrowLeft if !shift && !selection.is_empty() => selection.start,
            Key::ArrowRight if !shift && !selection.is_empty() => selection.end,
            Key::ArrowLeft => self.previous(),
            Key::ArrowRight => self.next(),
            _ => self.cursor,
        };
        if !shift {
            self.anchor = self.cursor;
        }
    }

    fn matches(&self, fields: &[&str]) -> bool {
        if self.terms.is_empty() {
            return true;
        }
        let fields: Vec<_> = fields.iter().map(|field| field.to_lowercase()).collect();
        self.terms
            .iter()
            .all(|term| fields.iter().any(|field| field.contains(term)))
    }
}

#[derive(Resource, Default)]
pub(crate) struct EncyclopediaSearch {
    people: SearchDraft,
    places: SearchDraft,
    focused: Option<EncyclopediaTab>,
}

impl EncyclopediaSearch {
    fn draft(&self, tab: EncyclopediaTab) -> &SearchDraft {
        if tab == EncyclopediaTab::Places {
            &self.places
        } else {
            &self.people
        }
    }

    fn draft_mut(&mut self, tab: EncyclopediaTab) -> &mut SearchDraft {
        if tab == EncyclopediaTab::Places {
            &mut self.places
        } else {
            &mut self.people
        }
    }

    /// Read-only presentation evidence for the real-input capture tour.
    pub(crate) fn query(&self, tab: EncyclopediaTab) -> &str {
        &self.draft(tab).text
    }

    #[cfg(test)]
    pub(super) fn set_query(&mut self, tab: EncyclopediaTab, text: &str) {
        let draft = self.draft_mut(tab);
        draft.clear();
        draft.insert(text);
    }

    /// Cross-page links reveal their destination without discarding the other tab's search.
    pub(crate) fn clear_people(&mut self) {
        self.people.clear();
    }

    pub(crate) fn clear_places(&mut self) {
        self.places.clear();
    }

    pub(super) fn active(&self, tab: EncyclopediaTab) -> bool {
        !self.draft(tab).terms.is_empty()
    }

    pub(super) fn revision(&self, tab: EncyclopediaTab) -> u64 {
        self.draft(tab).revision
    }

    /// Call only on records that passed KnownPeople::visible: search cannot reveal strangers.
    pub(super) fn matches_person(&self, person: &PersonRecord) -> bool {
        self.people.matches(&[
            &person.name,
            person.residence.as_deref().unwrap_or_default(),
            person.workplace.as_deref().unwrap_or_default(),
        ])
    }

    pub(super) fn matches_place(&self, name: &str) -> bool {
        self.places.matches(&[name])
    }
}

#[derive(Component)]
pub(crate) struct SearchField(pub EncyclopediaTab);
#[derive(Component)]
pub(crate) struct ClearSearch(pub EncyclopediaTab);
#[derive(Component)]
pub(super) struct SearchText(EncyclopediaTab);
#[derive(Component)]
pub(super) struct SearchCaret(EncyclopediaTab);
#[derive(Component)]
pub(super) struct SearchSuffix(EncyclopediaTab);

/// This stays outside the list's rebuilt subtree, retaining keyboard focus and its glyph atlas.
pub(super) fn spawn(parent: &mut ChildSpawnerCommands<'_>, tab: EncyclopediaTab) {
    let (name, clear) = if tab == EncyclopediaTab::People {
        ("Search people", "Clear people search")
    } else {
        ("Search places", "Clear places search")
    };
    parent
        .spawn((
            TabGroup {
                order: 0,
                modal: true,
            },
            Node {
                margin: UiRect::new(px(16), px(16), px(0), px(12)),
                align_items: AlignItems::Center,
                column_gap: px(6),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                Button,
                SearchField(tab),
                Name::new(name),
                UiArtworkFocus,
                button_chrome(UiButtonVariant::Secondary),
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    height: px(39),
                    border: UiRect::all(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|field| {
                field
                    .spawn((
                        SearchText(tab),
                        Text::new(name),
                        ledger::reading(14.0),
                        TextColor(INK_MUTED),
                        TextLayout::no_wrap(),
                        Pickable::IGNORE,
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(13),
                            top: px(10),
                            ..default()
                        },
                        UiTransform::default(),
                    ))
                    .with_children(|text| {
                        text.spawn((
                            SearchCaret(tab),
                            TextSpan::new("|"),
                            ledger::reading(14.0),
                            TextColor(Color::NONE),
                        ));
                        text.spawn((
                            SearchSuffix(tab),
                            TextSpan::default(),
                            ledger::reading(14.0),
                            TextColor(INK),
                        ));
                    });
            });
            row.spawn((
                Button,
                ClearSearch(tab),
                Name::new(clear),
                UiArtworkFocus,
                button_chrome(UiButtonVariant::Secondary),
                Node {
                    width: px(35),
                    height: px(35),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_child((UiButtonLabel, ledger::heading("×", 23.0)));
        });
}

/// Chat resets shared capture first; this field then contributes its ownership.
/// Mouse defocus is independent of text capture, so other ledger controls remain clickable.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_input(
    mut commands: Commands,
    mut search: ResMut<EncyclopediaSearch>,
    mut input: ResMut<InputState>,
    open: Res<EncyclopediaOpen>,
    tab: Res<EncyclopediaTab>,
    guard: Res<ClickGuard>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut events: MessageReader<KeyboardInput>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(Entity, &SearchField, &Interaction, Has<InteractionDisabled>)>,
    clear: Query<(Entity, &ClearSearch, &Interaction, Has<InteractionDisabled>)>,
    bodies: Query<(&TabBody, &Node)>,
    windows: Query<&Window>,
) {
    let was_focused = search.focused.is_some();
    let visible = open.0
        && matches!(*tab, EncyclopediaTab::People | EncyclopediaTab::Places)
        && bodies
            .iter()
            .any(|(body, node)| body.0 == *tab && node.display != Display::None)
        && !windows.iter().any(|window| !window.focused);
    for (entity, field, _, disabled) in &fields {
        let enabled = visible && field.0 == *tab;
        if enabled && disabled {
            commands
                .entity(entity)
                .remove::<InteractionDisabled>()
                .insert(TabIndex(0));
        } else if !enabled && !disabled {
            commands
                .entity(entity)
                .insert(InteractionDisabled)
                .remove::<TabIndex>();
        }
    }
    for (entity, button, _, disabled) in &clear {
        let enabled = visible && button.0 == *tab && !search.draft(button.0).text.is_empty();
        if enabled && disabled {
            commands
                .entity(entity)
                .remove::<InteractionDisabled>()
                .insert(TabIndex(0));
        } else if !enabled && !disabled {
            commands
                .entity(entity)
                .insert(InteractionDisabled)
                .remove::<TabIndex>();
        }
    }
    let field = fields
        .iter()
        .find(|(_, field, _, _)| visible && field.0 == *tab);
    if mouse.just_pressed(MouseButton::Left) && guard.0 {
        if let Some((entity, _, Interaction::Pressed, _)) = field {
            focus.set(entity, FocusCause::Pressed);
        } else if clear.iter().any(|(_, button, interaction, _)| {
            visible && button.0 == *tab && *interaction == Interaction::Pressed
        }) {
            search.draft_mut(*tab).clear();
            if let Some((entity, _, _, _)) = field {
                focus.set(entity, FocusCause::Pressed);
            }
        } else if fields
            .iter()
            .any(|(entity, _, _, _)| focus.get() == Some(entity))
        {
            focus.clear();
        }
    }
    let editing = field.is_some_and(|(entity, _, _, _)| focus.get() == Some(entity));
    if !visible
        && fields
            .iter()
            .any(|(entity, _, _, _)| focus.get() == Some(entity))
    {
        focus.clear();
    }
    let next_focus = editing.then_some(*tab);
    if search.focused != next_focus {
        search.focused = next_focus;
    }
    let shortcut = keyboard.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut closed = false;
    let clear_key = visible
        && keyboard.any_just_pressed([KeyCode::Enter, KeyCode::Space])
        && clear.iter().any(|(entity, button, _, disabled)| {
            !disabled && button.0 == *tab && focus.get() == Some(entity)
        });
    if clear_key {
        search.draft_mut(*tab).clear();
        input.text_input_captured = true;
    }
    for event in events.read() {
        if !editing || closed || !event.state.is_pressed() {
            continue;
        }
        if matches!(event.logical_key, Key::Escape | Key::Enter) {
            focus.clear();
            search.focused = None;
            closed = true;
            continue;
        }
        let draft = search.draft_mut(*tab);
        match &event.logical_key {
            Key::Character(key) if shortcut => match key.to_ascii_lowercase().as_str() {
                "a" => {
                    draft.anchor = 0;
                    draft.cursor = draft.text.len();
                }
                "v" => {
                    if let Ok(mut clipboard) = Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            draft.insert(&text);
                        }
                    }
                }
                "c" | "x" => {
                    let range = draft.selection();
                    if !range.is_empty() {
                        if let Ok(mut clipboard) = Clipboard::new() {
                            if clipboard.set_text(draft.text[range].to_owned()).is_ok()
                                && key.eq_ignore_ascii_case("x")
                            {
                                draft.erase(true);
                            }
                        }
                    }
                }
                _ => (),
            },
            Key::Character(key) => draft.insert(event.text.as_deref().unwrap_or(key.as_str())),
            Key::Space => draft.insert(" "),
            Key::Backspace => draft.erase(true),
            Key::Delete => draft.erase(false),
            Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End => {
                draft.navigate(&event.logical_key, shift)
            }
            _ => (),
        }
    }
    // Capture and controller input may supply the physical edge without a text event.
    if editing && keyboard.any_just_pressed([KeyCode::Escape, KeyCode::Enter]) {
        focus.clear();
        search.focused = None;
    }
    input.text_input_active |= search.focused.is_some();
    input.text_input_captured |= was_focused || editing;
}

pub(super) fn sync_view(
    search: Res<EncyclopediaSearch>,
    time: Res<Time>,
    mut labels: Query<(&SearchText, &mut Text, &mut TextColor)>,
    mut suffixes: Query<(&SearchSuffix, &mut TextSpan)>,
    mut carets: Query<(&SearchCaret, &mut TextColor), Without<SearchText>>,
) {
    for (field, mut text, mut color) in &mut labels {
        let draft = search.draft(field.0);
        let focused = search.focused == Some(field.0);
        let placeholder = draft.text.is_empty() && !focused;
        let label = if placeholder {
            if field.0 == EncyclopediaTab::People {
                "Search name or place…"
            } else {
                "Search places…"
            }
        } else {
            &draft.text[..draft.cursor]
        };
        if text.0 != label {
            text.0 = label.into();
        }
        let ink = if placeholder { INK_MUTED } else { INK };
        if color.0 != ink {
            color.0 = ink;
        }
    }
    for (field, mut text) in &mut suffixes {
        let draft = search.draft(field.0);
        let label = &draft.text[draft.cursor..];
        if text.0 != label {
            text.0 = label.into();
        }
    }
    for (field, mut color) in &mut carets {
        let show =
            search.focused == Some(field.0) && (time.elapsed_secs_f64() * 2.0) as u64 % 2 == 0;
        let ink = if show { BRASS } else { Color::NONE };
        if color.0 != ink {
            color.0 = ink;
        }
    }
}

pub(super) fn fit_text(
    fields: Query<&ComputedNode, With<SearchField>>,
    mut labels: Query<(&bevy::text::TextLayoutInfo, &ChildOf, &mut UiTransform), With<SearchText>>,
) {
    for (layout, parent, mut transform) in &mut labels {
        let Ok(field) = fields.get(parent.parent()) else {
            continue;
        };
        let width = field.content_box().width() * field.inverse_scale_factor() - 26.0;
        if width <= 0.0 {
            continue;
        }
        let Some(caret) = layout.glyphs.iter().find(|glyph| glyph.section_index == 1) else {
            continue;
        };
        let x = caret.position.x / layout.scale_factor;
        let next = bevy::ui::Val2::px(-(x + 12.0 - width).max(0.0), 0.0);
        if transform.translation != next {
            transform.translation = next;
        }
    }
}

#[cfg(test)]
mod tests;
