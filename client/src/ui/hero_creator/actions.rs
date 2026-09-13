//! Creator input and retained bindings; appearance remains a client draft
//! until the authoritative CreateHero voyage flow accepts it.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::{
    tab_navigation::{NavAction, TabIndex, TabNavigation},
    FocusCause, InputFocus,
};
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};
use lightyear::prelude::{Connected, MessageSender};
use shared::components::HeroOutfit;
use shared::protocol::{CreateHero, ReliableChannel};

use super::{
    ArrowButton, BeginJourneyButton, CreatorRoot, CreatorRow, CreatorStatusText, HeroCreatorOpen,
    SlotValueText,
};
use crate::hero::control::{SelectedOutfit, WorldPlacementMode};
use crate::hero::{HeroManifest, HeroPreviewRig};
use crate::input::InputState;
use crate::ui::hud::{GodCapability, GodNotice, HudMode};

/// Buttons act on a completed mouse press that began after the modal was
/// armed. Live release coordinates also accept the first macOS click whose
/// press arrives before the window reports a cursor position.
#[derive(Resource, Default)]
pub struct CreatorClickGuard {
    pub armed: bool,
    press_in_flight: bool,
    pub completed_click: bool,
}

#[derive(Resource, Default)]
pub(super) struct CreatorFeedback {
    text: String,
}

#[derive(Resource, Default)]
pub(super) struct CreatorFocus {
    root: Option<Entity>,
    previous: Option<Entity>,
    last_control: Option<Entity>,
}

/// Bevy only tabs through a modal group when focus is already inside it.
/// Run after layout, once foundation has assigned the controls' TabIndex.
pub(super) fn initialize_creator_focus(
    open: Res<HeroCreatorOpen>,
    roots: Query<Entity, With<CreatorRoot>>,
    parents: Query<&ChildOf>,
    controls: Query<(), (With<TabIndex>, Without<InteractionDisabled>)>,
    entities: Query<Entity>,
    navigation: TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut owner: ResMut<CreatorFocus>,
) {
    if !open.0 {
        return;
    }
    let Ok(root) = roots.single() else { return };
    let current = focus.get();
    let inside = current.is_some_and(|entity| {
        controls.contains(entity) && parents.iter_ancestors(entity).any(|parent| parent == root)
    });
    if owner.root == Some(root) {
        if inside {
            owner.last_control = current;
            return;
        }
        // Respect another screen that deliberately acquired focus. A blank
        // click clears focus; recover that case so Tab remains usable.
        if current.is_some_and(|entity| entities.contains(entity)) {
            return;
        }
    }
    let target = if inside {
        current
    } else {
        owner
            .last_control
            .filter(|entity| {
                controls.contains(*entity)
                    && parents.iter_ancestors(*entity).any(|parent| parent == root)
            })
            .or_else(|| navigation.initialize(root, NavAction::First).ok())
    };
    let Some(target) = target.filter(|entity| controls.contains(*entity)) else {
        return;
    };
    if owner.root != Some(root) {
        owner.previous = current.filter(|entity| !inside && entities.contains(*entity));
        owner.root = Some(root);
    }
    owner.last_control = Some(target);
    if current != Some(target) {
        focus.set(target, FocusCause::Navigated);
    }
}

/// Release ownership before the tree disappears; never restore a removed or
/// disabled control, and never overwrite focus acquired by another screen.
pub(super) fn release_creator_focus(
    open: Res<HeroCreatorOpen>,
    parents: Query<&ChildOf>,
    controls: Query<(), (With<TabIndex>, Without<InteractionDisabled>)>,
    entities: Query<Entity>,
    mut focus: ResMut<InputFocus>,
    mut owner: ResMut<CreatorFocus>,
) {
    if open.0 {
        return;
    }
    let Some(root) = owner.root else { return };
    let current = focus.get();
    if current.is_none_or(|entity| {
        !entities.contains(entity)
            || entity == root
            || Some(entity) == owner.last_control
            || parents.iter_ancestors(entity).any(|parent| parent == root)
    }) {
        if let Some(previous) = owner.previous.filter(|entity| controls.contains(*entity)) {
            focus.set(previous, FocusCause::Navigated);
        } else if current.is_some() {
            focus.clear();
        }
    }
    *owner = CreatorFocus::default();
}

impl CreatorClickGuard {
    fn advance(&mut self, open: bool, pressed: bool, just_pressed: bool, just_released: bool) {
        self.completed_click = false;
        if !open {
            self.armed = false;
            self.press_in_flight = false;
            return;
        }
        if !self.armed {
            // A complete opening press/release can arrive in one frame. It
            // must not arm and then activate the newly appeared controls.
            self.armed = !pressed && !just_pressed;
            return;
        }
        if just_pressed {
            self.press_in_flight = true;
        }
        if just_released {
            self.completed_click = self.press_in_flight;
            self.press_in_flight = false;
        }
    }
}

pub(super) fn update_click_guard(
    open: Res<HeroCreatorOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<CreatorClickGuard>,
    mut feedback: ResMut<CreatorFeedback>,
) {
    if open.is_changed() && !feedback.text.is_empty() {
        feedback.text.clear();
    }
    guard.advance(
        open.0,
        mouse.pressed(MouseButton::Left),
        mouse.just_pressed(MouseButton::Left),
        mouse.just_released(MouseButton::Left),
    );
}

/// Uses the same focus resource that foundation's TabNavigationPlugin and
/// focus chrome own. Keyboard activation does not simulate a mouse press.
#[derive(SystemParam)]
pub(super) struct CreatorActivation<'w> {
    guard: Res<'w, CreatorClickGuard>,
    focus: Res<'w, InputFocus>,
    keyboard: Res<'w, ButtonInput<KeyCode>>,
}

impl CreatorActivation<'_> {
    fn activated(&self, entity: Entity, cursor: &RelativeCursorPosition) -> bool {
        let keyboard = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]
            .into_iter()
            .any(|key| self.keyboard.just_pressed(key));
        if keyboard {
            self.guard.armed && self.focus.get() == Some(entity)
        } else {
            self.guard.completed_click && cursor.cursor_over
        }
    }
}

pub(super) fn force_close_creator(
    mut open: ResMut<HeroCreatorOpen>,
    mut guard: ResMut<CreatorClickGuard>,
) {
    open.0 = false;
    *guard = CreatorClickGuard::default();
}

pub(super) fn sync_creator_open_state(open: Res<HeroCreatorOpen>, mut input: ResMut<InputState>) {
    if input.hero_creator_open != open.0 {
        input.hero_creator_open = open.0;
    }
}

fn cycle_outfit(outfit: &mut HeroOutfit, row: CreatorRow, direction: i8, manifest: &HeroManifest) {
    match row {
        CreatorRow::Slot(index) => {
            if let Some(slot) = manifest.slots.get(index) {
                outfit.cycle_slot(index, i16::from(direction), slot.items.len());
            }
        }
        CreatorRow::Skin => outfit.cycle_skin(i16::from(direction), manifest.skin.tones.len()),
    }
}

pub(super) fn handle_arrow_buttons(
    open: Res<HeroCreatorOpen>,
    activation: CreatorActivation,
    manifest: Res<HeroManifest>,
    mut selected: ResMut<SelectedOutfit>,
    buttons: Query<(Entity, &RelativeCursorPosition, &ArrowButton), Without<InteractionDisabled>>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    if !open.0 {
        return;
    }
    let Some((_, _, arrow)) = buttons
        .iter()
        .find(|(entity, cursor, _)| activation.activated(*entity, cursor))
    else {
        return;
    };
    let mut next = selected.0;
    cycle_outfit(&mut next, arrow.row, arrow.dir, &manifest);
    if selected.0 != next {
        selected.0 = next;
        sounds.emit(crate::audio::sfx::SfxCue::UiClick);
    }
}

/// Covers arrow presses, capture fixtures and other deliberate draft updates.
/// A new preview also receives the current draft without dirtying unchanged rigs.
pub(super) fn sync_preview_outfit(
    selected: Res<SelectedOutfit>,
    mut previews: Query<&mut HeroOutfit, With<HeroPreviewRig>>,
) {
    for mut outfit in &mut previews {
        if *outfit != selected.0 {
            *outfit = selected.0;
        }
    }
}

pub(super) fn handle_confirm_buttons(
    activation: CreatorActivation,
    mut open: ResMut<HeroCreatorOpen>,
    selected: Res<SelectedOutfit>,
    mut placement: ResMut<WorldPlacementMode>,
    mut notice: ResMut<GodNotice>,
    mut feedback: ResMut<CreatorFeedback>,
    confirm: Query<
        (Entity, &RelativeCursorPosition),
        (With<BeginJourneyButton>, Without<InteractionDisabled>),
    >,
    mut sender: Query<&mut MessageSender<CreateHero>, (With<crate::GameClient>, With<Connected>)>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    if !open.0 {
        return;
    }
    if !confirm
        .iter()
        .any(|(entity, cursor)| activation.activated(entity, cursor))
    {
        return;
    }
    if !feedback.text.is_empty() {
        feedback.text.clear();
    }
    let Ok(mut sender) = sender.single_mut() else {
        let message = "Still connecting - try again in a moment";
        notice.show(message);
        feedback.text = message.into();
        sounds.emit(crate::audio::sfx::SfxCue::UiReject);
        return;
    };
    sender.send::<ReliableChannel>(CreateHero { outfit: selected.0 });
    // Acknowledges the accepted local submission, not a server creation result.
    sounds.emit(crate::audio::sfx::SfxCue::UiConfirm);
    *placement = WorldPlacementMode::None;
    open.0 = false;
}

/// Explicit development bypass for inspecting a world without starting a voyage.
/// It cannot reopen the creator; normal players must finish character creation.
pub(super) fn handle_developer_skip(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<HeroCreatorOpen>,
    capability: Res<GodCapability>,
    mut hud_mode: ResMut<HudMode>,
    mut cinematic: ResMut<crate::boat::OpeningCinematic>,
) {
    if open.0 && capability.0 && keyboard.just_pressed(KeyCode::KeyG) {
        *hud_mode = HudMode::God;
        cinematic.cancel();
        open.0 = false;
    }
}

/// Asset node names are content, not typography: 'Bottom_Shorts_Long' becomes
/// 'Shorts long', while the layout owns uppercase category headings.
fn value_label(raw: &str, prefix: Option<&str>) -> String {
    let trimmed = prefix
        .and_then(|p| raw.strip_prefix(p))
        .unwrap_or(raw)
        .trim_matches('_');
    let mut label = String::with_capacity(trimmed.len());
    let mut previous_lower = false;
    for ch in trimmed.chars() {
        if ch.is_uppercase() && previous_lower {
            label.push(' ');
        }
        if ch == '_' {
            label.push(' ');
        } else if label.is_empty() {
            label.extend(ch.to_uppercase());
        } else {
            label.extend(ch.to_lowercase());
        }
        previous_lower = ch.is_lowercase();
    }
    label
}

fn row_label(row: CreatorRow, selected: &HeroOutfit, manifest: &HeroManifest) -> String {
    match row {
        CreatorRow::Slot(index) => manifest
            .slots
            .get(index)
            .and_then(|slot| {
                // Borrow the existing prefix, rather than allocating a temporary
                // '<Slot>_' string for each row.
                let prefix = slot
                    .items
                    .first()
                    .and_then(|s| s.find('_').map(|i| &s[..=i]));
                slot.item(selected.slot(index))
                    .map(|item| value_label(item, prefix))
            })
            .unwrap_or_else(|| "-".into()),
        CreatorRow::Skin => manifest
            .skin_tone(selected.skin)
            .map(|tone| value_label(&tone.name, None))
            .unwrap_or_else(|| "-".into()),
    }
}

pub(super) fn sync_slot_labels(
    manifest: Res<HeroManifest>,
    selected: Res<SelectedOutfit>,
    mut labels: Query<(Ref<SlotValueText>, &mut Text)>,
    mut previous: Local<Option<HeroOutfit>>,
) {
    for (marker, mut text) in &mut labels {
        let old_value = previous.as_ref().map(|old| match marker.0 {
            CreatorRow::Slot(index) => old.slot(index) == selected.0.slot(index),
            CreatorRow::Skin => old.skin == selected.0.skin,
        });
        if !manifest.is_changed() && !marker.is_added() && old_value == Some(true) {
            continue;
        }
        let value = row_label(marker.0, &selected.0, &manifest);
        if text.0 != value {
            text.0 = value;
        }
    }
    *previous = Some(selected.0);
}

pub(super) fn sync_status_text(
    feedback: Res<CreatorFeedback>,
    mut labels: Query<(Ref<CreatorStatusText>, &mut Text)>,
) {
    for (marker, mut text) in &mut labels {
        if !feedback.is_changed() && !marker.is_added() {
            continue;
        }
        let value = feedback.text.as_str();
        if text.0 != value {
            text.0 = value.into();
        }
    }
}

#[cfg(test)]
mod tests;
