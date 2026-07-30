//! Left click picks an entity, or clears the selection on empty ground.

use bevy::prelude::*;

use shared::components::PlayerPosition;

use super::{pick_radius_at, ray_vs_vertical_segment, Selectable, Selection};
use crate::camera_rts::{CursorRay, CursorTerrainHit};
use crate::hero::control::HeroSpawnArm;
use crate::input::InputState;

pub(super) fn pick_on_left_click(
    mouse: Res<ButtonInput<MouseButton>>,
    input_state: Res<InputState>,
    spawn_arm: Res<HeroSpawnArm>,
    cursor_ray: Res<CursorRay>,
    terrain_hit: Res<CursorTerrainHit>,
    ui_blockers: Query<&Interaction, With<crate::ui::BlocksWorldClicks>>,
    candidates: Query<(Entity, &Selectable, &PlayerPosition)>,
    mut selection: ResMut<Selection>,
) {
    if !mouse.just_pressed(MouseButton::Left) || input_state.ui_blocking() {
        return;
    }
    // An armed god-mode placement owns this click: it consumes the left button to
    // put the hero down. Selecting as well would leave the player having both
    // placed and selected in one gesture, which reads as the click doing two
    // things.
    if spawn_arm.0 {
        return;
    }
    // A click on a HUD surface must never reach the world.
    if crate::ui::pointer_over_ui(&ui_blockers) {
        return;
    }
    let Some(ray) = cursor_ray.0 else {
        return;
    };

    let origin = ray.origin;
    let dir = ray.direction.as_vec3();

    // How far along the ray the ground is. Anything further than this is behind
    // a hill and must not be selectable through it -- a click that picks a unit
    // you cannot see reads as the game ignoring the terrain.
    let terrain_distance = terrain_hit
        .0
        .map(|point| (point - origin).dot(dir))
        .filter(|d| *d > 0.0);

    let mut best: Option<(Entity, f32)> = None;
    for (entity, selectable, position) in candidates.iter() {
        let Some((distance, gap)) = ray_vs_vertical_segment(
            origin,
            dir,
            position.0,
            selectable.height,
        ) else {
            continue;
        };
        if gap > pick_radius_at(selectable.radius, distance) {
            continue;
        }
        // Allow a little tolerance past the ground hit: a character's feet sit
        // AT the terrain, so an exact comparison rejects the unit you clicked.
        if let Some(ground) = terrain_distance {
            if distance > ground + selectable.height.max(1.0) {
                continue;
            }
        }
        // Nearest wins, so overlapping units resolve to the front one.
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((entity, distance));
        }
    }

    let picked = best.map(|(entity, _)| entity);
    // Clicking empty ground clears -- the standard RTS deselect.
    if selection.entity != picked {
        selection.entity = picked;
    }
}
