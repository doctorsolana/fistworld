//! Right click issues a move order to the selected unit.
//!
//! Right button does double duty: held it orbits the camera, tapped it commands.
//! The discriminator lives here; the camera is untouched and keeps reading the
//! raw button, so orbiting still works exactly as before. An order fires on
//! RELEASE, because whether a press was a tap or a drag is not knowable until
//! the button comes up.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};

use shared::components::CommandedBy;
use shared::protocol::{ReliableChannel, UnitMoveOrder, MAX_UNITS_PER_ORDER};

use super::{can_command, formation_targets, is_click, RightDrag, Selection};
use crate::camera_rts::CursorTerrainHit;
use crate::input::InputState;

#[allow(clippy::too_many_arguments)]
pub(super) fn issue_order_on_right_click(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    input_state: Res<InputState>,
    hit: Res<CursorTerrainHit>,
    selection: Res<Selection>,
    ui_blockers: Query<&Interaction>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    units: Query<&CommandedBy>,
    mut drag: ResMut<RightDrag>,
    mut move_sender: Query<
        &mut MessageSender<UnitMoveOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    // Accumulate travel every frame the button is down, even while a modal is
    // open, so a press that started before the modal cannot be mistaken for a
    // fresh tap when it closes.
    let motion: f32 = mouse_motion.read().map(|event| event.delta.length()).sum();

    let cursor = windows.single().ok().and_then(|w| w.cursor_position());

    if mouse.just_pressed(MouseButton::Right) {
        // Void the gesture AT PRESS if the UI owned that press. Checking only at
        // release would let press-on-HUD, drag into the world, release punch a
        // move order through a panel the player was actually interacting with.
        let over_ui = crate::ui::pointer_over_ui(&ui_blockers);
        *drag = RightDrag {
            press_at: cursor,
            motion: 0.0,
            held_secs: 0.0,
            became_drag: over_ui || input_state.ui_blocking(),
        };
    }

    if mouse.pressed(MouseButton::Right) {
        drag.motion += motion;
        drag.held_secs += time.delta_secs();
        let radial = match (drag.press_at, cursor) {
            (Some(start), Some(now)) => start.distance(now),
            _ => 0.0,
        };
        if !is_click(radial, drag.motion, drag.held_secs) {
            drag.became_drag = true;
        }
    }

    if !mouse.just_released(MouseButton::Right) {
        return;
    }

    let was_click = !drag.became_drag && drag.press_at.is_some();
    *drag = RightDrag::default();

    if !was_click {
        return;
    }
    // ...and clean at release too, so a press in the world cannot deliver an
    // order by releasing over the HUD.
    if input_state.ui_blocking() || crate::ui::pointer_over_ui(&ui_blockers) {
        return;
    }

    // Only selected units we OWN take orders. An empty selection means no
    // order, which is what makes right-click safe to also be the orbit button:
    // with nothing selected a tap does nothing at all.
    if selection.is_empty() {
        return;
    }
    let Some(target) = hit.0 else {
        return;
    };
    let my_account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase());

    // Selecting a mixed group (yours and someone else's) orders only yours,
    // silently. The alternative -- refusing the whole order -- would make a
    // box-select over a crowded village feel broken.
    // Box-select already filters to your own, so this normally passes
    // everything through. It stays as the authority anyway: selection can be set
    // by a single click on someone else's unit, and an order must never leak
    // through that.
    let ours: Vec<Entity> = selection
        .entities
        .iter()
        .copied()
        .filter(|entity| can_command(units.get(*entity).ok(), my_account.as_deref()))
        .take(MAX_UNITS_PER_ORDER)
        .collect();
    if ours.is_empty() {
        return;
    }

    let Ok(mut sender) = move_sender.single_mut() else {
        return;
    };
    // ONE message carrying every (unit, point) pair. The old code sent one
    // message per unit with no unit id in it at all, so the server -- which
    // inferred "the sender's hero" -- collapsed the whole order onto one body
    // and kept only the last target.
    //
    // Arrival points are spread so a group sent to one spot arrives as a group
    // rather than stacking into a single body.
    let units_and_points: Vec<(Entity, Vec3)> = ours
        .iter()
        .copied()
        .zip(formation_targets(target, ours.len(), FORMATION_SPACING))
        .collect();
    sender.send::<ReliableChannel>(UnitMoveOrder {
        units: units_and_points,
    });
}

/// Gap between neighbours when a group is ordered to one point, in metres.
/// Roughly two body widths, so a squad reads as a cluster rather than a queue.
const FORMATION_SPACING: f32 = 1.4;
