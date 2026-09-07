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
    AboardBoat, CharacterActivity, CommandedBy, ConstructionSite, Hero, OwnedBy, PersonId,
    PlayerBoat, PlayerPosition,
};
use shared::protocol::{
    DisembarkBoat, HeroConstructionOrder, MovementMode, ReliableChannel, SailToLanding,
    UnitCommand, UnitOrder, MAX_UNITS_PER_ORDER,
};

use super::{can_command, is_click, RightDrag, Selection};
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
    landing_sender: Query<
        'w,
        's,
        &'static mut MessageSender<SailToLanding>,
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
    sender: Query<
        'w,
        's,
        &'static mut MessageSender<UnitOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    construction_sender: Query<
        'w,
        's,
        &'static mut MessageSender<HeroConstructionOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    siege_aim: ResMut<'w, crate::siege::SiegeAim>,
    catapults: Query<'w, 's, (), With<shared::components::Catapult>>,
    combat_mode: Res<'w, crate::combat_mode::CombatMode>,
    command_mode: ResMut<'w, super::commands::CommandMode>,
    roster: Res<'w, crate::army_roster::ArmyRoster>,
    // Everything a combat-mode click might land on, the same tuple the picker
    // uses so the attack pick and the selection pick can never drift apart.
    characters: Query<
        'w,
        's,
        (
            Entity,
            &'static super::Selectable,
            &'static PlayerPosition,
            Option<&'static Transform>,
            Option<&'static CommandedBy>,
            Option<&'static CharacterActivity>,
            Has<shared::components::Catapult>,
        ),
    >,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn issue_order_on_right_click(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
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
        let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        *drag = RightDrag {
            formation_start: hit.0,
            formation: ground_world.combat_mode.0
                && !ground_world.siege_aim.0
                && selection
                    .entities
                    .iter()
                    .any(|e| !ground_world.catapults.contains(*e))
                && !alt
                && !selection.is_empty()
                && !over_ui
                && !input_state.ui_blocking(),
            press_at: gesture_cursor,
            motion: 0.0,
            held_secs: 0.0,
            became_drag: over_ui || input_state.ui_blocking() || alt,
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

    let placement = if drag.formation {
        drag.formation_start
            .zip(hit.0)
            .and_then(|(start, end)| super::formation_preview::frontage_from_drag(start, end))
    } else {
        None
    };
    let was_click = drag.press_at.is_some() && (!drag.became_drag || drag.formation);
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
    if let Some((centre, _)) = placement {
        target = centre;
    }
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
        .collect();
    if ours.is_empty() {
        return;
    }
    if ours.len() > MAX_UNITS_PER_ORDER {
        boat_world
            .notice
            .show("Selection exceeds the 1024-unit command limit");
        return;
    }

    if ground_world.siege_aim.0 {
        let units = ours
            .iter()
            .copied()
            .filter(|e| ground_world.catapults.contains(*e))
            .collect();
        if let Ok(mut sender) = ground_world.sender.single_mut() {
            sender.send::<ReliableChannel>(UnitOrder {
                selection: shared::protocol::UnitSelection {
                    units,
                    battalions: vec![],
                },
                command: UnitCommand::AttackGround { target },
            });
        }
        ground_world.siege_aim.0 = false;
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
            // A land click while troops stand alongside the sailor belongs
            // to the TROOPS: without this carve-out the boat interaction
            // consumes the whole click and a box-selected battalion on the
            // pier silently receives no order at all.
            let land_units: Vec<Entity> = ours
                .iter()
                .copied()
                .filter(|entity| {
                    boat_world.boats.get(*entity).is_err()
                        && boat_world.aboard_heroes.get(*entity).is_err()
                })
                .collect();
            if land_units.is_empty() {
                let distance = boat_position.0.xz().distance(target.xz());
                if distance > 11.0 {
                    // An inland click is a complete intent: the server picks
                    // the coast, sails there, puts the sailor ashore and walks
                    // them the rest of the way. No "sail closer" homework.
                    if let Ok(mut sender) = boat_world.landing_sender.single_mut() {
                        sender.send::<ReliableChannel>(SailToLanding { boat, target });
                        boat_world.notice.show("Making for shore");
                    }
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
            ours = land_units;
        }
    }

    // In combat mode, a click on somebody else's character is an attack order
    // for everyone selected. The pick is the SAME function that drives the
    // hover ring, so the person marked red and the person the order names can
    // never disagree. A combat-mode click on empty ground still falls through
    // to movement: repositioning mid-fight must not require leaving the mode.
    if ground_world.combat_mode.0
        && placement.is_none()
        && ground_world.command_mode.0 == MovementMode::Move
    {
        let victim = super::attack_ring::find_enemy_under_cursor(
            &ground_world.combat_mode,
            &input_state,
            &ui_blockers,
            &ray,
            &hit,
            account.as_deref(),
            &ground_world.characters,
        );
        if let Some(victim) = victim {
            // People fight; vessels do not. A mixed selection quietly sends
            // only the fighters, mirroring the ours-only filter above.
            let fighters: Vec<Entity> = ours
                .iter()
                .copied()
                .filter(|entity| boat_world.boats.get(*entity).is_err())
                .collect();
            if !fighters.is_empty() {
                if let Ok(mut sender) = ground_world.sender.single_mut() {
                    sender.send::<ReliableChannel>(UnitOrder {
                        selection: ground_world.roster.selection(&fighters),
                        command: UnitCommand::Attack {
                            target: victim,
                            mode: if keys.any_pressed([
                                KeyCode::ControlLeft,
                                KeyCode::ControlRight,
                                KeyCode::SuperLeft,
                                KeyCode::SuperRight,
                            ]) {
                                shared::protocol::AttackMode::Focus
                            } else {
                                shared::protocol::AttackMode::EngageLine
                            },
                        },
                    });
                }
                return;
            }
        }
    }

    // A worksite click is an interaction order, not a request to walk to the
    // plot centre. Use the same precise pick volume as left-click selection so
    // roofs, long windmill footprints and non-1.0 render scales all agree.
    let selected_hero = ours
        .iter()
        .filter(|_| placement.is_none() && ground_world.command_mode.0 == MovementMode::Move)
        .find_map(|entity| {
            ground_world
                .heroes
                .get(*entity)
                .ok()
                .map(|(_, person_id)| (*entity, *person_id))
        });
    if let (Some((hero_entity, person_id)), Some(ray)) = (selected_hero, ray.0) {
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
            // The build order claims only the hero. Everyone else selected
            // still gets the walk - a mixed selection's click must never be
            // silently consumed by one unit's special interaction.
            ours.retain(|entity| *entity != hero_entity);
            if ours.is_empty() {
                return;
            }
        }
    }

    if let Ok(mut sender) = ground_world.sender.single_mut() {
        sender.send::<ReliableChannel>(UnitOrder {
            selection: ground_world.roster.selection(&ours),
            command: UnitCommand::Move {
                target,
                frontage: placement.map(|(_, frontage)| frontage),
                mode: ground_world.command_mode.0,
            },
        });
        ground_world.command_mode.0 = MovementMode::Move;
        if sailing_order {
            boat_world.notice.show("Sailing to destination");
        }
    }
}
