//! Shared modal helpers (backdrop + panel + cursor sync)

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::input::InputState;
use crate::ui::foundation::{layer, UiButtonStyleExempt, UiRefreshExempt};
use crate::ui::styles::{plate_shadow, LIMEWASH_LIT, MODAL_BACKDROP, PLATE_RULE, RADIUS};

/// Common marker for all windows created by [`spawn_modal`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModalRoot;

/// One central, read-only answer to “is a shared modal open?”. Screen-specific
/// resources still own navigation and data; input code no longer has to know
/// which screen happens to be borrowing an unrelated flag.
#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub struct ModalState {
    pub open_count: usize,
}

impl ModalState {
    pub const fn is_open(&self) -> bool {
        self.open_count > 0
    }
}

#[derive(Clone, Copy)]
pub struct ModalLayout {
    pub panel_size: Vec2,
    pub panel_padding: f32,
}

impl Default for ModalLayout {
    fn default() -> Self {
        Self {
            panel_size: Vec2::new(420.0, 300.0),
            panel_padding: 16.0,
        }
    }
}

pub struct ModalNodes {
    pub root: Entity,
    pub backdrop: Entity,
    pub panel: Entity,
}

/// Canonical full-screen root shared by standard and custom modal layouts.
/// Keeping the input, layer and tab-group policy together prevents a new modal
/// from being visually correct while still leaking world input or keyboard
/// focus.
pub fn modal_root_chrome() -> (Node, Pickable, GlobalZIndex, TabGroup) {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        GlobalZIndex(layer::MODAL),
        TabGroup::modal(),
    )
}

/// Canonical outside-click target for custom modal layouts.
pub fn modal_backdrop_chrome(
    color: Color,
) -> (
    UiRefreshExempt,
    UiButtonStyleExempt,
    Button,
    Node,
    BackgroundColor,
) {
    (
        UiRefreshExempt,
        UiButtonStyleExempt,
        Button,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

pub fn spawn_modal<R, B, P>(
    commands: &mut Commands,
    root_marker: R,
    backdrop_marker: B,
    panel_marker: P,
    layout: ModalLayout,
) -> ModalNodes
where
    R: Component,
    B: Component,
    P: Component,
{
    let root = commands
        .spawn((root_marker, ModalRoot, modal_root_chrome()))
        .id();

    let mut backdrop_entity = None;
    let mut panel_entity = None;
    commands.entity(root).with_children(|root| {
        let backdrop = root
            .spawn((backdrop_marker, modal_backdrop_chrome(MODAL_BACKDROP)))
            .id();
        backdrop_entity = Some(backdrop);
        let panel = root
            .spawn((
                panel_marker,
                Node {
                    width: Val::Px(layout.panel_size.x),
                    height: Val::Px(layout.panel_size.y),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    padding: UiRect::all(Val::Px(layout.panel_padding)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                // `Node` requires `FocusPolicy`, whose default is `Pass`. Without
                // this override the Bevy UI interaction system continues
                // through blank parts of the panel and presses the full-screen
                // backdrop underneath it. The picking backend blocks by default,
                // but declaring both policies makes the modal correct for either
                // interaction path used by Bevy 0.19.
                FocusPolicy::Block,
                Pickable::default(),
                BackgroundColor(LIMEWASH_LIT),
                BorderColor::all(PLATE_RULE),
                plate_shadow(),
            ))
            .id();
        panel_entity = Some(panel);
    });
    ModalNodes {
        root,
        backdrop: backdrop_entity.unwrap_or(root),
        panel: panel_entity.unwrap_or(root),
    }
}

pub fn sync_modal_state(
    roots: Query<(), With<ModalRoot>>,
    mut state: ResMut<ModalState>,
    mut input: ResMut<InputState>,
) {
    let open_count = roots.iter().count();
    if state.open_count != open_count {
        state.open_count = open_count;
    }
    let open = state.is_open();
    if input.modal_open != open {
        input.modal_open = open;
    }
}

/// Whether a freshly-opened modal may act on mouse input yet.
///
/// A window that appears under the cursor puts controls beneath a button that
/// may already be down — the click that opened it, or one held across the
/// transition. Arming only once the button has been observed RELEASED means
/// such a press can never action a freshly-spawned control.
///
/// Call every frame; gate every click handler on the returned value.
pub fn update_modal_click_guard(
    open: bool,
    mouse: &ButtonInput<MouseButton>,
    armed: &mut bool,
) -> bool {
    if !open {
        *armed = false;
        return false;
    }
    if !*armed && !mouse.pressed(MouseButton::Left) {
        *armed = true;
    }
    *armed
}

pub fn handle_backdrop_pressed<B: Component>(
    backdrop: &Query<&Interaction, (With<B>, Changed<Interaction>)>,
) -> bool {
    for interaction in backdrop.iter() {
        if *interaction == Interaction::Pressed {
            return true;
        }
    }
    false
}

/// RTS invariant: the OS cursor stays free and visible in gameplay whether or not a
/// modal is open, so this only ever releases.
pub fn sync_modal_cursor(
    _open: bool,
    _input_state: &InputState,
    windows: &Query<Entity, With<PrimaryWindow>>,
    cursor_opts: &mut Query<&mut CursorOptions>,
) {
    let Ok(window_entity) = windows.single() else {
        return;
    };
    if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[derive(Component)]
    struct Root;
    #[derive(Component)]
    struct Backdrop;
    #[derive(Component)]
    struct Panel;

    #[test]
    fn shared_modal_has_exactly_one_visible_scrim() {
        let mut world = World::new();
        let nodes = spawn_modal(
            &mut world.commands(),
            Root,
            Backdrop,
            Panel,
            ModalLayout::default(),
        );
        world.flush();

        assert_eq!(
            world.get::<BackgroundColor>(nodes.root).unwrap().0,
            Color::NONE
        );
        assert_eq!(
            world.get::<BackgroundColor>(nodes.backdrop).unwrap().0,
            MODAL_BACKDROP
        );
        assert_eq!(
            world.get::<BackgroundColor>(nodes.panel).unwrap().0,
            LIMEWASH_LIT
        );
        assert_eq!(
            world.get::<BorderColor>(nodes.panel).unwrap(),
            &BorderColor::all(PLATE_RULE)
        );
        assert_eq!(
            world.get::<GlobalZIndex>(nodes.root),
            Some(&GlobalZIndex(layer::MODAL))
        );
    }

    #[test]
    fn modal_roots_drive_the_central_input_mutex() {
        let mut world = World::new();
        world.insert_resource(ModalState::default());
        world.insert_resource(InputState::default());
        let root = world.spawn(ModalRoot).id();

        world.run_system_once(sync_modal_state).unwrap();
        assert!(world.resource::<ModalState>().is_open());
        assert!(world.resource::<InputState>().modal_open);

        world.entity_mut(root).despawn();
        world.run_system_once(sync_modal_state).unwrap();
        assert!(!world.resource::<ModalState>().is_open());
        assert!(!world.resource::<InputState>().modal_open);
    }
}
