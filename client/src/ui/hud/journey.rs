//! Optional notice tray and local view controls for ordinary exploration.
//!
//! These change the local view/selection only. The existing right-click and
//! market systems remain the sole senders of movement and trading intent.

mod notices;
mod view;

use super::*;
use crate::camera_rts::{CommanderCamera, LocalPeerId};
use notices::Notices;
use shared::components::{
    AboardBoat, BuildingOf, CharacterName, Hero, PlayerPosition, PlayerRotation,
    SettlementBuilding, SettlementBuildingKind, SettlementId, SettlementSummary,
};
use shared::economy::{format_money, GoodsInventory, MootMarket, Wallet};
pub(super) use view::view;

#[derive(Component)]
struct JourneyPlate;

#[derive(Component)]
struct JourneyDetails;

#[derive(Component)]
enum JourneyText {
    Unread,
    Toggle,
    Message(usize),
    Hero,
    Town,
    Hint,
}

#[derive(Component, Clone, Copy)]
enum JourneyAction {
    Toggle,
    Clear,
    Hero,
    Town(Option<Vec3>),
    Map,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Notices>();
    app.add_systems(OnEnter(GameState::Playing), reset);
    app.add_systems(Update, collect_notices.run_if(in_state(GameState::Playing)));
    app.add_systems(
        Update,
        (
            handle_actions.after(crate::selection::SelectionGestureSet),
            bind.after(handle_actions).after(collect_notices),
        )
            .run_if(in_state(GameState::Playing)),
    );
}

fn reset(mut notices: ResMut<Notices>, notice: Res<GodNotice>) {
    *notices = Notices::default();
    // Ignore the previous connection's last action result.
    notices.observe(notice.sequence, "");
}

fn collect_notices(notice: Res<GodNotice>, mut notices: ResMut<Notices>) {
    if !notices.has_seen(notice.sequence) {
        notices.observe(notice.sequence, &notice.text);
    }
}

fn handle_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(Entity, &Hero, &PlayerPosition)>,
    buttons: Query<
        (&Interaction, &JourneyAction),
        (Changed<Interaction>, Without<bevy::ui::InteractionDisabled>),
    >,
    mut notices: ResMut<Notices>,
    mut selection: ResMut<crate::selection::Selection>,
    mut cameras: Query<&mut CommanderCamera>,
    mut map: ResMut<crate::ui::world_map::MapOpen>,
) {
    if input.ui_blocking()
        || opening
            .as_deref()
            .is_some_and(|opening| opening.is_active())
    {
        return;
    }
    let Some(local) = local else {
        return;
    };
    let Some((entity, _, position)) = heroes
        .iter()
        .find(|(_, hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    else {
        return;
    };
    let clicked = mouse
        .just_pressed(MouseButton::Left)
        .then(|| {
            buttons
                .iter()
                .find(|(interaction, _)| **interaction == Interaction::Pressed)
                .map(|(_, action)| *action)
        })
        .flatten();
    let action = if keys.just_pressed(KeyCode::Home) {
        Some(JourneyAction::Hero)
    } else {
        clicked
    };
    match action {
        Some(JourneyAction::Toggle) => notices.toggle(),
        Some(JourneyAction::Clear) => notices.clear(),
        Some(JourneyAction::Hero) => {
            // Selecting the aboard hero already controls their paired vessel
            // through the normal order handler; no special sailing command.
            selection.set(vec![entity]);
            for mut camera in &mut cameras {
                camera.focus_target = position.0;
                camera.zoom_target = 82.0_f32.clamp(camera.zoom_min, camera.zoom_max);
            }
        }
        Some(JourneyAction::Town(Some(position))) => {
            for mut camera in &mut cameras {
                camera.focus_target = position;
                camera.zoom_target = 180.0_f32.clamp(camera.zoom_min, camera.zoom_max);
            }
        }
        Some(JourneyAction::Map) => map.0 = true,
        _ => {}
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn bind(
    time: Res<Time>,
    mut elapsed: Local<f32>,
    local: Option<Res<LocalPeerId>>,
    (input, opening, combat, mode): (
        Res<InputState>,
        Res<crate::boat::OpeningCinematic>,
        Res<crate::combat_mode::CombatMode>,
        Res<HudMode>,
    ),
    mut notices: ResMut<Notices>,
    heroes: Query<(
        &Hero,
        &PlayerPosition,
        Has<AboardBoat>,
        Option<&CharacterName>,
        Option<&Wallet>,
        Option<&GoodsInventory>,
    )>,
    towns: Query<(&SettlementSummary, &PlayerPosition)>,
    halls: Query<(&SettlementId, &PlayerPosition, Option<&PlayerRotation>), With<MootMarket>>,
    marketplaces: Query<(
        &SettlementBuilding,
        &BuildingOf,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    mut plates: Query<
        &mut Node,
        (
            With<JourneyPlate>,
            Without<JourneyDetails>,
            Without<JourneyText>,
        ),
    >,
    mut details: Query<
        &mut Node,
        (
            With<JourneyDetails>,
            Without<JourneyPlate>,
            Without<JourneyText>,
        ),
    >,
    mut labels: Query<
        (&JourneyText, &mut Text, &mut Node),
        (Without<JourneyPlate>, Without<JourneyDetails>),
    >,
    mut buttons: Query<(
        Entity,
        &mut JourneyAction,
        Has<bevy::ui::InteractionDisabled>,
    )>,
    mut commands: Commands,
) {
    let hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let showing = hero.is_some()
        && !input.ui_blocking()
        && !opening.is_active()
        && !combat.0
        && *mode == HudMode::Play;
    for mut node in &mut plates {
        let display = if showing {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in &mut details {
        set_display(&mut node, showing && notices.expanded);
    }
    if showing && notices.expanded && notices.unread() > 0 {
        notices.mark_read();
    }
    // Expansion and unread state respond immediately; world facts only bind
    // while expanded and at most five times per second.
    if notices.is_changed() {
        for (field, mut text, mut node) in &mut labels {
            match field {
                JourneyText::Unread => {
                    let count = notices.unread();
                    set_display(&mut node, count > 0);
                    if count > 0 {
                        set_text(&mut text, &count.to_string());
                    }
                }
                JourneyText::Toggle => {
                    set_text(&mut text, if notices.expanded { "-" } else { "+" })
                }
                JourneyText::Message(index) if notices.expanded => {
                    let message = notices.entry(*index);
                    set_display(&mut node, *index == 0 || message.is_some());
                    set_text(&mut text, message.unwrap_or("No recent messages."));
                }
                _ => {}
            }
        }
    }
    *elapsed += time.delta_secs();
    if !showing || !notices.expanded || (*elapsed < 0.2 && !notices.is_changed()) {
        return;
    }
    *elapsed = 0.0;
    let (_, position, aboard, name, wallet, cargo) = hero.unwrap();
    let nearest = towns
        .iter()
        .filter(|(town, _)| town.residents > 0)
        .min_by(|(a, pa), (b, pb)| {
            pa.0.xz()
                .distance_squared(position.0.xz())
                .total_cmp(&pb.0.xz().distance_squared(position.0.xz()))
                .then(a.id.cmp(&b.id))
        });
    let at_counter = !aboard
        && halls.iter().any(|(id, hall, rotation)| {
            let entrance = SettlementBuildingKind::Hall
                .entrance_position(hall.0, rotation.map_or(0.0, |r| r.0));
            let counter = crate::ui::market::nearest_public_market_entrance(
                position.0,
                entrance,
                marketplaces
                    .iter()
                    .filter(|(building, owner, ..)| {
                        building.kind == SettlementBuildingKind::Market && owner.0 == *id
                    })
                    .map(|(building, _, position, rotation)| {
                        building.kind.entrance_position(position.0, rotation.0)
                    }),
            );
            position.0.xz().distance(counter.xz())
                <= crate::ui::market::HERO_MARKET_INTERACTION_RANGE
        });
    let hero_text = format!(
        "{} / {} coin / {} of {} bulk carried",
        name.map_or("Your hero", |name| name.0.as_str()),
        wallet.map_or_else(|| "...".into(), |wallet| format_money(wallet.balance())),
        cargo.map_or(0, GoodsInventory::used_bulk),
        cargo.map_or(0, GoodsInventory::bulk_capacity)
    );
    let town_text = nearest.map_or_else(
        || "Explore the coast to find an inhabited town.".into(),
        |(town, town_position)| {
            format!(
                "Nearest town: {} / {:.0}m",
                town.name,
                position.0.xz().distance(town_position.0.xz())
            )
        },
    );
    let hint = if aboard {
        "At sea. Choose a shore to land and continue on foot."
    } else if at_counter {
        "Market in reach. Press E to trade."
    } else {
        "Trade at the town Hall or market stalls."
    };
    for (field, mut text, _) in &mut labels {
        let value = match field {
            JourneyText::Hero => &hero_text,
            JourneyText::Town => &town_text,
            JourneyText::Hint => hint,
            _ => continue,
        };
        set_text(&mut text, value);
    }
    for (entity, mut action, disabled) in &mut buttons {
        if matches!(*action, JourneyAction::Clear) {
            let empty = notices.entry(0).is_none();
            if empty != disabled {
                if empty {
                    commands
                        .entity(entity)
                        .insert(bevy::ui::InteractionDisabled);
                } else {
                    commands
                        .entity(entity)
                        .remove::<bevy::ui::InteractionDisabled>();
                }
            }
        }
        if let JourneyAction::Town(current) = &mut *action {
            let destination = nearest.map(|(_, position)| position.0);
            if *current != destination {
                *current = destination;
            }
            if destination.is_some() && disabled {
                commands
                    .entity(entity)
                    .remove::<bevy::ui::InteractionDisabled>();
            }
            if destination.is_none() && !disabled {
                commands
                    .entity(entity)
                    .insert(bevy::ui::InteractionDisabled);
            }
        }
    }
}

fn set_text(text: &mut Mut<Text>, value: &str) {
    if text.0 != value {
        text.0.clear();
        text.0.push_str(value);
    }
}

fn set_display(node: &mut Mut<Node>, showing: bool) {
    let display = if showing {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_countdown_does_not_dirty_retained_history() {
        let mut app = App::new();
        app.init_resource::<GodNotice>()
            .init_resource::<Notices>()
            .add_systems(Update, collect_notices);
        app.world_mut().resource_mut::<GodNotice>().show("Moving");
        app.update();
        app.world_mut().clear_trackers();
        app.world_mut().resource_mut::<GodNotice>().seconds_left -= 0.1;
        app.update();
        assert!(!app
            .world()
            .get_resource_ref::<Notices>()
            .unwrap()
            .is_changed());
        assert_eq!(app.world().resource::<Notices>().unread(), 1);
    }

    #[test]
    fn home_finds_only_the_local_hero_and_respects_modal_input() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<InputState>()
            .init_resource::<Notices>()
            .init_resource::<crate::selection::Selection>()
            .init_resource::<crate::ui::world_map::MapOpen>()
            .insert_resource(LocalPeerId(41))
            .add_systems(Update, handle_actions);
        let camera = app.world_mut().spawn(CommanderCamera::default()).id();
        app.world_mut().spawn((
            Hero {
                owner: PeerId::Netcode(42),
            },
            PlayerPosition(Vec3::ZERO),
        ));
        let destination = Vec3::new(3400.0, 4.0, 150.0);
        let hero = app
            .world_mut()
            .spawn((
                Hero {
                    owner: PeerId::Netcode(41),
                },
                PlayerPosition(destination),
            ))
            .id();
        app.world_mut()
            .resource_mut::<InputState>()
            .hero_creator_open = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Home);
        app.update();
        assert!(app
            .world()
            .resource::<crate::selection::Selection>()
            .is_empty());
        app.world_mut()
            .resource_mut::<InputState>()
            .hero_creator_open = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::Home);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Home);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::selection::Selection>()
                .primary(),
            Some(hero)
        );
        let camera = app.world().get::<CommanderCamera>(camera).unwrap();
        assert_eq!(camera.focus_target, destination);
        assert_eq!(camera.zoom_target, 82.0);
    }
}
