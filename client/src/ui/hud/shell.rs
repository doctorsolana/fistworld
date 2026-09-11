//! Stable exploration frame: the viewed place, personal purse, map and ledger.
//!
//! Settlement names come from the global directory. Viewing a town preserves
//! selected people so the next world order still moves the same hero or army.
//! These controls change only local selection/camera intent, never world state.

use bevy::{prelude::*, ui::InteractionDisabled};
use shared::{
    components::{Hero, PlayerPosition, SettlementId, SettlementSummary},
    economy::{format_money, Wallet, PENNIES_PER_COIN},
};

use super::{chrome, HudMode};
use crate::{
    camera_rts::{CommanderCamera, LocalPeerId},
    input::InputState,
    selection::Selection,
    states::GameState,
    ui::{
        encyclopedia::{ClickGuard, EncyclopediaOpen},
        foundation::{button_chrome, surface_block, UiButtonLabel, UiButtonVariant},
        styles::{plate_shadow, BRASS_DARK, PARCHMENT},
        typography,
        world_map::MapOpen,
    },
};

#[derive(Component)]
struct ShellRoot;

#[derive(Component, Clone, Copy)]
enum ShellAction {
    Hero,
    Town,
    Map,
    Ledger,
}

#[derive(Component)]
enum ShellText {
    Place,
    Coins,
}

#[derive(Component)]
struct CompassRose;

#[derive(Resource, Default)]
struct ViewedPlace(Option<(SettlementId, Vec3)>);

pub(super) fn install(app: &mut App) {
    app.init_resource::<ViewedPlace>()
        .add_systems(OnEnter(GameState::Playing), reset)
        .add_systems(
            Update,
            (
                handle_actions.after(crate::selection::SelectionGestureSet),
                bind.after(handle_actions),
                sync_visibility,
                turn_compass.after(crate::camera_rts::update_commander_camera),
            )
                .run_if(in_state(GameState::Playing)),
        );
}

fn reset(mut place: ResMut<ViewedPlace>) {
    place.0 = None;
}

pub(super) fn location() -> impl Bundle {
    (
        ShellRoot,
        Name::new("Exploration location"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            top: Val::Px(18.0),
            width: Val::Px(320.0),
            height: Val::Px(52.0),
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        children![
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(21.0),
                    top: Val::Px(6.0),
                    right: Val::Px(0.0),
                    height: Val::Px(40.0),
                    padding: UiRect {
                        left: Val::Px(36.0),
                        right: Val::Px(12.0),
                        ..default()
                    },
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                chrome::pill_panel(),
                surface_block(),
                Interaction::default(),
                plate_shadow(),
                children![
                    (
                        ShellAction::Town,
                        Name::new("hud-VIEW TOWN"),
                        AccessibleLabel::new("View nearest town"),
                        Button,
                        Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            min_width: Val::Px(0.0),
                            height: Val::Px(30.0),
                            padding: UiRect::horizontal(Val::Px(4.0)),
                            align_items: AlignItems::Center,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Ribbon),
                        children![(
                            ShellText::Place,
                            Text::new("Exploring"),
                            TextLayout::no_wrap(),
                            UiButtonLabel,
                            typography::heading(14.0),
                            TextColor(PARCHMENT),
                            Pickable::IGNORE,
                        )],
                    ),
                    (
                        Node {
                            width: Val::Px(1.0),
                            height: Val::Px(21.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(BRASS_DARK),
                        Pickable::IGNORE,
                    ),
                    (
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(5.0),
                            width: Val::Px(90.0),
                            flex_shrink: 0.0,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        AccessibleLabel::new("Personal money"),
                        Pickable::IGNORE,
                        children![
                            chrome::icon(chrome::HudIcon::Purse, 21.0),
                            (
                                ShellText::Coins,
                                Text::new("--"),
                                TextLayout::no_wrap(),
                                typography::body(14.0),
                                TextColor(PARCHMENT),
                                Pickable::IGNORE,
                            ),
                        ],
                    ),
                ],
            ),
            (
                ShellAction::Hero,
                Name::new("hud-HERO"),
                AccessibleLabel::new("Select your hero and centre the view (Home)"),
                Button,
                Node {
                    width: Val::Px(52.0),
                    height: Val::Px(52.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                button_chrome(UiButtonVariant::Ribbon),
                chrome::medallion(),
                plate_shadow(),
                children![chrome::icon(chrome::HudIcon::Crest, 33.0)],
            ),
        ],
    )
}

pub(super) fn navigation() -> impl Bundle {
    (
        ShellRoot,
        Name::new("Exploration navigation"),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(18.0),
            bottom: Val::Px(18.0),
            width: Val::Px(145.0),
            height: Val::Px(110.0),
            ..default()
        },
        Pickable::IGNORE,
        children![
            (
                ShellAction::Map,
                Name::new("hud-MAP"),
                AccessibleLabel::new("Open world map (M)"),
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(108.0),
                    height: Val::Px(108.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                button_chrome(UiButtonVariant::Ribbon),
                chrome::medallion(),
                plate_shadow(),
                children![(
                    CompassRose,
                    Node {
                        width: Val::Px(84.0),
                        height: Val::Px(84.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    UiTransform::default(),
                    Pickable::IGNORE,
                    children![
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                top: Val::Px(0.0),
                                ..default()
                            },
                            Text::new("N"),
                            typography::heading(13.0),
                            TextColor(PARCHMENT),
                            Pickable::IGNORE,
                        ),
                        chrome::icon(chrome::HudIcon::Compass, 52.0),
                    ],
                )],
            ),
            (
                ShellAction::Ledger,
                Name::new("hud-LEDGER"),
                AccessibleLabel::new("Open encyclopedia (N)"),
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    bottom: Val::Px(18.0),
                    width: Val::Px(45.0),
                    height: Val::Px(45.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                button_chrome(UiButtonVariant::Ribbon),
                chrome::medallion(),
                plate_shadow(),
                children![chrome::icon(chrome::HudIcon::Book, 25.0)],
            ),
        ],
    )
}

fn sync_visibility(
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mode: Res<HudMode>,
    mut roots: Query<&mut Node, With<ShellRoot>>,
) {
    let showing = !input.ui_blocking()
        && !opening.is_some_and(|opening| opening.is_active())
        && *mode == HudMode::Play;
    let display = if showing {
        Display::Flex
    } else {
        Display::None
    };
    for mut root in &mut roots {
        if root.display != display {
            root.display = display;
        }
    }
}

fn turn_compass(
    cameras: Query<&CommanderCamera>,
    mut roses: Query<&mut UiTransform, With<CompassRose>>,
) {
    let Ok(camera) = cameras.single() else { return };
    // The screen's positive y points down: viewed north rotates clockwise as
    // the camera's world yaw increases. Rotate the N label with the rose.
    let rotation = Rot2::radians(camera.yaw);
    for mut rose in &mut roses {
        if rose.rotation != rotation {
            rose.rotation = rotation;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bind(
    time: Res<Time>,
    mut elapsed: Local<f32>,
    cameras: Query<&CommanderCamera>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(&Hero, Option<&Wallet>)>,
    towns: Query<(&SettlementSummary, &PlayerPosition)>,
    mut viewed: ResMut<ViewedPlace>,
    mut labels: Query<(&ShellText, &mut Text)>,
    buttons: Query<(Entity, &ShellAction, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.2 {
        return;
    }
    *elapsed = 0.0;
    let camera = cameras.single().ok();
    let nearest = camera.and_then(|camera| {
        towns
            .iter()
            .filter(|(town, _)| town.residents > 0)
            .min_by(|(a, pa), (b, pb)| {
                pa.0.xz()
                    .distance_squared(camera.focus.xz())
                    .total_cmp(&pb.0.xz().distance_squared(camera.focus.xz()))
                    .then(a.id.cmp(&b.id))
            })
    });
    let hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let next = nearest.map(|(town, position)| (town.id, position.0));
    if viewed.0 != next {
        viewed.0 = next;
    }
    let location = nearest.map_or("Exploring", |(town, _)| town.name.as_str());
    let coins = hero
        .and_then(|(_, wallet)| wallet)
        .map_or_else(|| "--".into(), |wallet| compact_money(wallet.balance()));
    for (field, mut text) in &mut labels {
        let value = match field {
            ShellText::Place => location,
            ShellText::Coins => coins.as_str(),
        };
        if text.0 != value {
            text.0 = value.into();
        }
    }
    for (entity, action, disabled) in &buttons {
        let available = match action {
            ShellAction::Hero => hero.is_some(),
            ShellAction::Town => nearest.is_some(),
            ShellAction::Map | ShellAction::Ledger => true,
        };
        if available && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !available && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

/// Keep the persistent purse bounded even after a fortune has accumulated.
/// Transaction pages retain exact pennies; this overview rounds down, never
/// displaying money the hero cannot actually spend.
fn compact_money(pennies: u64) -> String {
    let coins = pennies / PENNIES_PER_COIN;
    if coins < 10_000 {
        return format_money(pennies);
    }
    let (scale, suffix) = [
        (1_000_000_000_000_000, "P"),
        (1_000_000_000_000, "T"),
        (1_000_000_000, "B"),
        (1_000_000, "M"),
        (1_000, "k"),
    ]
    .into_iter()
    .find(|(scale, _)| coins >= *scale)
    .unwrap();
    format!("{}.{}{suffix}", coins / scale, coins % scale / (scale / 10))
}

#[allow(clippy::too_many_arguments)]
fn handle_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mode: Res<HudMode>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(Entity, &Hero, &PlayerPosition)>,
    viewed: Res<ViewedPlace>,
    buttons: Query<
        (&Interaction, &ShellAction),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    mut selection: ResMut<Selection>,
    mut cameras: Query<&mut CommanderCamera>,
    mut map: ResMut<MapOpen>,
    mut ledger: ResMut<EncyclopediaOpen>,
    mut ledger_guard: ResMut<ClickGuard>,
) {
    if input.ui_blocking()
        || opening.is_some_and(|opening| opening.is_active())
        || *mode != HudMode::Play
    {
        return;
    }
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
        Some(ShellAction::Hero)
    } else {
        clicked
    };
    match action {
        Some(ShellAction::Hero) => {
            let Some(local) = local else { return };
            let Some((entity, _, position)) = heroes
                .iter()
                .find(|(_, hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            else {
                return;
            };
            selection.set(vec![entity]);
            for mut camera in &mut cameras {
                camera.focus_target = position.0;
                camera.zoom_target = 82.0_f32.clamp(camera.zoom_min, camera.zoom_max);
            }
        }
        Some(ShellAction::Town) => {
            let Some((_, position)) = viewed.0 else {
                return;
            };
            // Reframing is not a selection command: keep the hero/army ready
            // for a right-click at the destination, including while sailing.
            for mut camera in &mut cameras {
                camera.focus_target = position;
                camera.zoom_target = 180.0_f32.clamp(camera.zoom_min, camera.zoom_max);
            }
        }
        Some(ShellAction::Map) => map.0 = !map.0,
        Some(ShellAction::Ledger) => {
            ledger_guard.0 = false;
            ledger.0 = true;
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightyear::prelude::PeerId;

    #[test]
    fn purse_labels_remain_bounded_at_large_balances_without_overstating_funds() {
        assert_eq!(compact_money(1999), "19.99");
        assert_eq!(compact_money(999_999), "9999.99");
        assert_eq!(compact_money(1_000_000), "10.0k");
        assert_eq!(compact_money(123_456_789), "1.2M");
        assert_eq!(compact_money(u64::MAX), "184.4P");
    }

    #[test]
    fn home_keeps_local_ownership_and_modal_gate() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<InputState>()
            .init_resource::<HudMode>()
            .init_resource::<ViewedPlace>()
            .init_resource::<Selection>()
            .init_resource::<MapOpen>()
            .init_resource::<EncyclopediaOpen>()
            .init_resource::<ClickGuard>()
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
        assert!(app.world().resource::<Selection>().is_empty());
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
        assert_eq!(app.world().resource::<Selection>().primary(), Some(hero));
        assert_eq!(
            app.world()
                .get::<CommanderCamera>(camera)
                .unwrap()
                .focus_target,
            destination
        );
        assert_eq!(
            app.world()
                .get::<CommanderCamera>(camera)
                .unwrap()
                .zoom_target,
            82.0
        );
        let town_position = Vec3::new(80.0, 2.0, 40.0);
        app.world_mut().resource_mut::<ViewedPlace>().0 = Some((SettlementId(1), town_position));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::Home);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .spawn((ShellAction::Town, Interaction::Pressed));
        app.update();
        assert_eq!(
            app.world().resource::<Selection>().primary(),
            Some(hero),
            "viewing a town must keep the hero available for the next movement order"
        );
        assert_eq!(
            app.world()
                .get::<CommanderCamera>(camera)
                .unwrap()
                .focus_target,
            town_position
        );
    }
}
