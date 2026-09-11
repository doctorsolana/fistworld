//! Small, contextual controls for the ordinary arrival and trading loop.
//!
//! These change the local view/selection only. The existing right-click and
//! market systems remain the sole senders of movement and trading intent.

use super::*;
use crate::camera_rts::{CommanderCamera, LocalPeerId};
use shared::components::{
    AboardBoat, BuildingOf, CharacterName, Hero, PlayerPosition, PlayerRotation,
    SettlementBuilding, SettlementBuildingKind, SettlementId, SettlementSummary,
};
use shared::economy::{format_money, GoodsInventory, MootMarket, Wallet};

#[derive(Component)]
struct JourneyPlate;

#[derive(Component)]
enum JourneyText {
    Hero,
    Town,
    Hint,
}

#[derive(Component, Clone, Copy)]
enum JourneyAction {
    Hero,
    Town(Option<Vec3>),
    Map,
}

pub(super) fn install(app: &mut App) {
    app.add_systems(OnEnter(GameState::Playing), spawn);
    app.add_systems(
        Update,
        (
            handle_actions.after(crate::selection::SelectionGestureSet),
            bind,
        )
            .run_if(in_state(GameState::Playing)),
    );
}

fn spawn(mut commands: Commands, capture: Option<Res<crate::capture::CaptureConfig>>) {
    if capture.is_some() && std::env::var_os("FISTFORCE_CAPTURE_HUD").is_none() {
        return;
    }
    commands
        .spawn((
            HudRoot,
            JourneyPlate,
            Name::new("Player journey"),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
                left: Val::Px(18.0),
                width: Val::Px(460.0),
                max_width: Val::Percent(46.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE),
            plate_shadow(),
            Interaction::default(),
            GlobalZIndex(crate::ui::foundation::layer::HUD),
        ))
        .with_children(|plate| {
            plate.spawn((
                Text::new("YOUR JOURNEY"),
                crate::ui::typography::heading(18.0),
                TextColor(INK),
            ));
            for field in [JourneyText::Hero, JourneyText::Town, JourneyText::Hint] {
                plate.spawn((
                    field,
                    Text::new(""),
                    crate::ui::typography::body(14.0),
                    TextColor(INK),
                ));
            }
            plate
                .spawn(Node {
                    column_gap: Val::Px(8.0),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|row| {
                    for (action, label) in [
                        (JourneyAction::Hero, "HERO [HOME]"),
                        (JourneyAction::Town(None), "VIEW TOWN"),
                        (JourneyAction::Map, "MAP [M]"),
                    ] {
                        row.spawn((
                            action,
                            Button,
                            Name::new(format!("journey-{label}")),
                            Node {
                                min_height: Val::Px(32.0),
                                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            button_chrome(UiButtonVariant::Secondary),
                        ))
                        .with_child((
                            Text::new(label),
                            UiButtonLabel,
                            crate::ui::typography::body(13.0),
                            TextColor(INK),
                            Pickable::IGNORE,
                        ));
                    }
                });
        });
}

fn handle_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(Entity, &Hero, &PlayerPosition)>,
    buttons: Query<(&Interaction, &JourneyAction), Changed<Interaction>>,
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
    input: Res<InputState>,
    opening: Res<crate::boat::OpeningCinematic>,
    combat: Res<crate::combat_mode::CombatMode>,
    notice: Res<GodNotice>,
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
    mut plates: Query<&mut Node, With<JourneyPlate>>,
    mut labels: Query<(&JourneyText, &mut Text)>,
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
    let showing = hero.is_some() && !input.ui_blocking() && !opening.is_active() && !combat.0;
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
    *elapsed += time.delta_secs();
    if !showing || *elapsed < 0.2 {
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
    let hint = if notice.seconds_left > 0.0 {
        notice.text.as_str()
    } else if aboard {
        "Right-click water to sail, or land to sail ashore and walk there. VIEW TOWN helps you choose a destination."
    } else if at_counter {
        "E opens this market. Buy goods or list carried goods for sale; you are paid only when someone buys them."
    } else {
        "Right-click to walk. Visit a Hall or Marketplace and press E to trade. HOME brings your hero back into view."
    };
    for (field, mut text) in &mut labels {
        let value = match field {
            JourneyText::Hero => &hero_text,
            JourneyText::Town => &town_text,
            JourneyText::Hint => hint,
        };
        if text.0 != value {
            text.0.clear();
            text.0.push_str(value);
        }
    }
    for (entity, mut action, disabled) in &mut buttons {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_finds_only_the_local_hero_and_respects_modal_input() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<InputState>()
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
