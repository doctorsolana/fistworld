//! Left click selects one; left DRAG box-selects many.
//!
//! Both gestures come off the same press, and which one it was is only knowable
//! at release — so selection resolves on release, and the box is drawn while the
//! button is down.
//!
//! The two gestures answer different questions, and so have different rules:
//!
//! - A **drag** means "I want to command these", so it only ever grabs units
//!   under your own banner. Dragging across a village full of other people's
//!   heroes and villagers picks up exactly your own and nothing else, instead of
//!   handing you a group where most of it silently refuses orders.
//! - A **click** means "what is that?", so it selects anything — someone else's
//!   hero, a villager — for inspection. It just cannot be ordered.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use shared::components::{Hero, PlayerPosition};

use super::{
    is_owned_by, pick_radius_at, ray_vs_vertical_segment, DragBox, Selectable, Selection,
    BOX_MIN_PX,
};
use crate::camera_rts::{CursorRay, CursorTerrainHit, LocalPeerId};
use crate::hero::control::{placement_armed, HeroSpawnArm, NpcSpawnArm};
use crate::input::InputState;

/// Project a world point into WINDOW pixels, or `None` if it is off screen.
///
/// The conversion is not just `world_to_viewport`: the 3D camera renders to a
/// scaled offscreen image, so that call returns IMAGE pixels while the cursor —
/// and therefore the drag box — is in window pixels. Mapping back through the
/// viewport/window ratio is what keeps a box drawn around a unit actually
/// containing that unit. Getting this wrong gives a selection box that is
/// subtly offset only at non-1.0 render scale: near-invisible in testing and
/// infuriating in play.
fn world_to_window(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    window_size: Vec2,
    world: Vec3,
) -> Option<Vec2> {
    let viewport_size = camera.logical_viewport_size()?;
    if viewport_size.x <= 0.0 || viewport_size.y <= 0.0 {
        return None;
    }
    // Errors here mean "outside the frustum", which rejects off-screen units for
    // free rather than needing a separate visibility test.
    let viewport_pos = camera.world_to_viewport(camera_transform, world).ok()?;
    Some(viewport_pos / viewport_size * window_size)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pick_on_left_click(
    mouse: Res<ButtonInput<MouseButton>>,
    input_state: Res<InputState>,
    spawn_arm: Res<HeroSpawnArm>,
    npc_arm: Res<NpcSpawnArm>,
    cursor_ray: Res<CursorRay>,
    terrain_hit: Res<CursorTerrainHit>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    ui_blockers: Query<&Interaction>,
    local: Option<Res<LocalPeerId>>,
    candidates: Query<(Entity, &Selectable, &PlayerPosition, Option<&Hero>)>,
    mut drag: ResMut<DragBox>,
    mut selection: ResMut<Selection>,
) {
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());

    // --- track the drag -----------------------------------------------------
    if mouse.just_pressed(MouseButton::Left) {
        // A press that starts on the UI, or during an armed placement, can never
        // become a selection. Latched AT PRESS rather than re-tested at release,
        // so press-on-HUD then drag into the world cannot marquee the world.
        let blocked = input_state.ui_blocking()
            || placement_armed(&spawn_arm, &npc_arm)
            || crate::ui::pointer_over_ui(&ui_blockers);
        *drag = DragBox {
            start: if blocked { None } else { cursor },
            current: cursor.unwrap_or_default(),
            active: false,
        };
    }

    if mouse.pressed(MouseButton::Left) {
        if let (Some(start), Some(now)) = (drag.start, cursor) {
            drag.current = now;
            if start.distance(now) > BOX_MIN_PX {
                drag.active = true;
            }
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }

    let box_rect = drag.rect();
    let had_press = drag.start.is_some();
    *drag = DragBox::default();

    if !had_press {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };

    // --- box select ---------------------------------------------------------
    if let Some((min, max)) = box_rect {
        let window_size = window.size();
        let local_id = local.as_ref().map(|local| local.0);
        let mut hits: Vec<(Entity, f32)> = Vec::new();
        for (entity, selectable, position, hero) in candidates.iter() {
            // Yours only. A drag is a command gesture, so anything you cannot
            // order has no business being in the result.
            if !is_owned_by(hero, local_id) {
                continue;
            }
            // Aim at the middle of the body: projecting the FEET means a unit
            // standing at the very bottom edge of the box is missed even though
            // the player clearly dragged over it.
            let centre = position.0 + Vec3::Y * selectable.height * 0.5;
            let Some(screen) = world_to_window(camera, camera_transform, window_size, centre)
            else {
                continue;
            };
            if screen.x >= min.x && screen.x <= max.x && screen.y >= min.y && screen.y <= max.y {
                // Sorted by depth so the order is stable and front-most first,
                // which is what `primary()` should name.
                hits.push((entity, camera_transform.translation().distance(centre)));
            }
        }
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));
        selection.set(hits.into_iter().map(|(entity, _)| entity).collect());
        return;
    }

    // --- single click -------------------------------------------------------
    let Some(ray) = cursor_ray.0 else {
        return;
    };
    let origin = ray.origin;
    let dir = ray.direction.as_vec3();

    // How far along the ray the ground is. Anything beyond it is behind a hill
    // and must not be selectable through it -- picking a unit you cannot see
    // reads as the game ignoring the terrain.
    let terrain_distance = terrain_hit
        .0
        .map(|point| (point - origin).dot(dir))
        .filter(|d| *d > 0.0);

    let mut best: Option<(Entity, f32)> = None;
    for (entity, selectable, position, _hero) in candidates.iter() {
        let Some((distance, gap)) =
            ray_vs_vertical_segment(origin, dir, position.0, selectable.height)
        else {
            continue;
        };
        if gap > pick_radius_at(selectable.radius, distance) {
            continue;
        }
        // Tolerance past the ground hit: a character's feet sit AT the terrain,
        // so an exact comparison rejects the unit that was clicked.
        if let Some(ground) = terrain_distance {
            if distance > ground + selectable.height.max(1.0) {
                continue;
            }
        }
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((entity, distance));
        }
    }

    // Clicking empty ground clears -- the standard RTS deselect.
    selection.set(best.into_iter().map(|(entity, _)| entity).collect());
}
