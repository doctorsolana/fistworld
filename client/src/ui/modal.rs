//! Shared modal helpers (backdrop + panel + cursor sync)

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::input::InputState;
use crate::ui::styles::{BUTTON_BORDER, MENU_BACKGROUND};

pub const MODAL_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.4);
pub const MODAL_BACKDROP_HIT: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);

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
    pub panel: Entity,
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
        .spawn((
            root_marker,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(MODAL_BACKDROP),
        ))
        .id();

    let mut panel_entity = None;
    commands.entity(root).with_children(|root| {
        root.spawn((
            backdrop_marker,
            Button,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            },
            BackgroundColor(MODAL_BACKDROP_HIT),
        ));
        let panel = root
            .spawn((
                panel_marker,
                Node {
                    width: Val::Px(layout.panel_size.x),
                    height: Val::Px(layout.panel_size.y),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(Val::Px(2.0)),
                    padding: UiRect::all(Val::Px(layout.panel_padding)),
                    ..default()
                },
                BackgroundColor(MENU_BACKGROUND),
                BorderColor::from(BUTTON_BORDER),
            ))
            .id();
        panel_entity = Some(panel);
    });
    ModalNodes {
        panel: panel_entity.unwrap_or(root),
    }
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
