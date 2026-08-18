//! Right click issues a move order to the selected unit.
//!
//! Right button does double duty: held it orbits the camera, tapped it commands.
//! The discriminator lives here; the camera is untouched and keeps reading the
//! raw button, so orbiting still works exactly as before. An order fires on
//! RELEASE, because whether a press was a tap or a drag is not knowable until
//! the button comes up.

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};

use shared::components::{
    AboardBoat, CommandedBy, ConstructionSite, Hero, OwnedBy, PersonId, PlayerBoat, PlayerPosition,
};
use shared::protocol::{
    DisembarkBoat, HeroConstructionOrder, ReliableChannel, UnitMoveOrder, MAX_UNITS_PER_ORDER,
};

use super::{can_command, formation_targets, is_click, RightDrag, Selection};
use crate::camera_rts::{CursorRay, CursorTerrainHit, CursorTerrainOverride};
use crate::input::InputState;

#[derive(SystemParam)]
pub(super) struct BoatOrderWorld<'w, 's> {
    boats: Query<
        'w,
        's,
        (Entity, &'static CommandedBy, &'static PlayerPosition),
        (With<PlayerBoat>, Without<shared::components::WreckedVessel>),
    >,
    aboard_heroes: Query<'w, 's, &'static CommandedBy, (With<Hero>, With<AboardBoat>)>,
    terrain: Res<'w, shared::terrain::WorldTerrain>,
    sender: Query<
        'w,
        's,
        &'static mut MessageSender<DisembarkBoat>,
        (With<crate::GameClient>, With<Connected>),
    >,
    notice: ResMut<'w, crate::ui::hud::GodNotice>,
}

#[derive(SystemParam)]
pub(super) struct GroundOrderWorld<'w, 's> {
    heroes: Query<'w, 's, (&'static CommandedBy, &'static PersonId), With<Hero>>,
    worksites: Query<
        'w,
        's,
        (
            Entity,
            &'static super::Selectable,
            &'static PlayerPosition,
            Option<&'static Transform>,
            &'static OwnedBy,
        ),
        With<ConstructionSite>,
    >,
    move_sender: Query<
        'w,
        's,
        &'static mut MessageSender<UnitMoveOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    construction_sender: Query<
        'w,
        's,
        &'static mut MessageSender<HeroConstructionOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn issue_order_on_right_click(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    input_state: Res<InputState>,
    hit: Res<CursorTerrainHit>,
    ray: Res<CursorRay>,
    forced_cursor: Option<Res<CursorTerrainOverride>>,
    selection: Res<Selection>,
    ui_blockers: Query<&Interaction>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    units: Query<&CommandedBy>,
    mut boat_world: BoatOrderWorld,
    mut ground_world: GroundOrderWorld,
    mut drag: ResMut<RightDrag>,
) {
    // Accumulate travel every frame the button is down, even while a modal is
    // open, so a press that started before the modal cannot be mistaken for a
    // fresh tap when it closes.
    let motion: f32 = mouse_motion.read().map(|event| event.delta.length()).sum();

    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    // A real mouse press always has a window cursor. The connected capture
    // harness instead supplies an explicit world cursor while its macOS window
    // may not be focused, so give only that opt-in fixture a stable screen
    // coordinate for the tap-vs-orbit discriminator.
    let gesture_cursor = cursor.or_else(|| forced_cursor.as_ref().map(|_| Vec2::ZERO));

    if mouse.just_pressed(MouseButton::Right) {
        // Void the gesture AT PRESS if the UI owned that press. Checking only at
        // release would let press-on-HUD, drag into the world, release punch a
        // move order through a panel the player was actually interacting with.
        let over_ui = crate::ui::pointer_over_ui(&ui_blockers);
        *drag = RightDrag {
            press_at: gesture_cursor,
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
    let Some(mut target) = hit.0 else {
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
    let mut ours: Vec<Entity> = selection
        .entities
        .iter()
        .copied()
        .filter(|entity| can_command(units.get(*entity).ok(), my_account.as_deref()))
        .take(MAX_UNITS_PER_ORDER)
        .collect();
    if ours.is_empty() {
        return;
    }

    // A selected vessel interprets water as navigation and nearby dry terrain
    // as the natural disembark interaction. The server repeats every check;
    // this prediction exists to explain a refused far-shore click immediately.
    let selected_boat = ours.iter().find_map(|entity| {
        boat_world
            .boats
            .get(*entity)
            .ok()
            .map(|(_, _, position)| (*entity, position))
    });
    // The sailor remains individually selectable while seated. Treat selecting
    // that aboard Hero as selecting their vessel for navigation; otherwise one
    // innocent click on the visible character turns every subsequent water
    // order into a silently ignored land-Hero order.
    let aboard_owner = ours
        .iter()
        .find_map(|entity| boat_world.aboard_heroes.get(*entity).ok());
    let paired_boat = aboard_owner.and_then(|hero_owner| {
        boat_world
            .boats
            .iter()
            .find(|(_, boat_owner, _)| boat_owner.0 == hero_owner.0)
            .map(|(entity, _, position)| (entity, position))
    });
    let controlled_boat = selected_boat.or(paired_boat);
    let mut sailing_order = false;
    if let Some((boat, boat_position)) = controlled_boat {
        let target_water = boat_world.terrain.get_water_height(target.x, target.z);
        if let Some(water_height) = target_water {
            target.y = water_height;
            ours.clear();
            ours.push(boat);
            sailing_order = true;
        } else {
            let distance = boat_position.0.xz().distance(target.xz());
            if distance > 11.0 {
                boat_world
                    .notice
                    .show("Sail closer to shore before disembarking");
                return;
            }
            if let Ok(mut sender) = boat_world.sender.single_mut() {
                sender.send::<ReliableChannel>(DisembarkBoat {
                    boat,
                    landing: target,
                });
            }
            return;
        }
    }

    // A worksite click is an interaction order, not a request to walk to the
    // plot centre. Use the same precise pick volume as left-click selection so
    // roofs, long windmill footprints and non-1.0 render scales all agree.
    let selected_hero = ours.iter().find_map(|entity| {
        ground_world
            .heroes
            .get(*entity)
            .ok()
            .map(|(_, person_id)| (*entity, *person_id))
    });
    if let (Some((_, person_id)), Some(ray)) = (selected_hero, ray.0) {
        let origin = ray.origin;
        let direction = ray.direction.as_vec3();
        let terrain_distance = hit
            .0
            .map(|point| (point - origin).dot(direction))
            .filter(|distance| *distance > 0.0);
        let target_site = ground_world
            .worksites
            .iter()
            .filter(|(_, _, _, _, owner)| owner.0 == person_id)
            .filter_map(|(entity, selectable, position, visual, _)| {
                let base = super::pick::selectable_base(selectable, position, visual);
                let distance =
                    super::pick::selectable_ray_distance(selectable, base, origin, direction)?;
                if terrain_distance
                    .is_some_and(|ground| distance > ground + selectable.height.max(1.0))
                {
                    return None;
                }
                Some((entity, distance))
            })
            .min_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(entity, _)| entity);
        if let Some(site) = target_site {
            if let Ok(mut sender) = ground_world.construction_sender.single_mut() {
                sender.send::<ReliableChannel>(HeroConstructionOrder { site });
            }
            return;
        }
    }

    let Ok(mut sender) = ground_world.move_sender.single_mut() else {
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
    if sailing_order {
        info!("Sailing order requested for {:?} to {target:?}", ours);
        boat_world.notice.show("Sailing to destination");
    }
}

/// Gap between neighbours when a group is ordered to one point, in metres.
/// Roughly two body widths, so a squad reads as a cluster rather than a queue.
const FORMATION_SPACING: f32 = 1.4;
