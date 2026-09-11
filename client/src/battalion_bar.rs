//! The battalion bar: Rome-style unit cards along the bottom of the screen.
//!
//! Springs up when combat mode arms and
//! shows one card per battalion: its Roman numeral in the display face, its
//! strength in men, and a thin health bar that thins as the unit bleeds.
//! Clicking a card selects the whole battalion; cards of battalions with
//! soldiers in the current selection wear the ember highlight.

use bevy::prelude::*;

use crate::army_roster::ArmyRoster;

use crate::combat_mode::CombatMode;
use crate::states::GameState;
use crate::ui::{
    foundation::{UiButtonLabel, UiButtonStyle, UiButtonVariant, button_chrome},
    motion::Spring,
    styles::PARCHMENT,
};

mod navigation;

// Match the corner HUD's independent selection and map controls. These are
// design pixels; the shared resolution-aware UiScale preserves the clearance.
const BAR_LEFT: f32 = 438.0;
const BAR_RIGHT: f32 = 180.0;
const BAR_SHOWN_BOTTOM: f32 = 18.0;
const BAR_HIDDEN_BOTTOM: f32 = -160.0;
const CARD_WIDTH: f32 = 84.0;
const CARD_HEIGHT: f32 = 96.0;
const CARD_GAP: f32 = 8.0;
/// Health bar colors: full reads as parchment-gold, the loss as dried blood.
const HEALTH_FILL: Color = Color::srgb(0.82, 0.68, 0.42);
const HEALTH_LOSS: Color = Color::srgb(0.30, 0.10, 0.08);

pub struct BattalionBarPlugin;

impl Plugin for BattalionBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_battalion_bar);
        app.add_systems(OnExit(GameState::Playing), despawn_battalion_bar);
        app.add_systems(
            Update,
            (
                rebuild_battalion_cards,
                handle_card_clicks,
                handle_muster_card_clicks,
                navigation::handle_navigation,
            )
                .chain()
                .after(crate::army_roster::ArmyRosterSet)
                .before(crate::selection::SelectionGestureSet)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                bind_battalion_cards,
                bind_muster_card,
                animate_battalion_bar,
                navigation::bind_navigation,
            )
                .after(crate::selection::SelectionGestureSet)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// The animated container; cards live inside a centered row.
#[derive(Component)]
struct BattalionBarRoot {
    spring: Spring,
}

#[derive(Component)]
struct BattalionCardRow;

#[derive(Component, Clone, Copy)]
struct BattalionCard(Entity);

#[derive(Component, Clone, Copy)]
struct CardCountText(Entity);

#[derive(Component, Clone, Copy)]
struct CardHealthFill(Entity);

#[derive(Component)]
struct MusterCard;

#[derive(Component)]
struct MusterCardLabel;

/// 1 -> "I", 4 -> "IV" ... the card face is a war banner, not a spreadsheet.
fn roman_numeral(mut value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    const TABLE: [(u64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (worth, glyph) in TABLE {
        while value >= worth {
            out.push_str(glyph);
            value -= worth;
        }
    }
    out
}

fn spawn_battalion_bar(
    mut commands: Commands,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    // Same suppression contract as the HUD and the combat overlay.
    let hud_requested = std::env::var("FISTFORCE_CAPTURE_HUD").is_ok_and(|value| !value.is_empty());
    if capture.is_some() && !hud_requested {
        return;
    }
    commands
        .spawn((
            Name::new("Combat battalion dock"),
            BattalionBarRoot {
                spring: Spring::new(BAR_HIDDEN_BOTTOM),
            },
            Pickable::IGNORE,
            GlobalZIndex(crate::ui::foundation::layer::HUD + 56),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(BAR_LEFT),
                right: Val::Px(BAR_RIGHT),
                bottom: Val::Px(BAR_HIDDEN_BOTTOM),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(CARD_GAP),
                align_items: AlignItems::FlexEnd,
                ..default()
            },
        ))
        .with_children(|dock| {
            navigation::spawn_button(dock, -1);
            dock.spawn((
                Name::new("Battalion card viewport"),
                navigation::CardViewport,
                crate::ui::foundation::surface_block(),
                ScrollPosition::default(),
                Node {
                    min_width: Val::Px(0.0),
                    height: Val::Px(CARD_HEIGHT),
                    flex_shrink: 1.0,
                    overflow: Overflow::scroll_x(),
                    ..default()
                },
                children![(
                    BattalionCardRow,
                    Pickable::IGNORE,
                    Node {
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(CARD_GAP),
                        align_items: AlignItems::FlexEnd,
                        ..default()
                    },
                )],
            ));
            navigation::spawn_button(dock, 1);
            // Raising a battalion remains available even when all other cards
            // are scrolled away. Its message and recruitment rules are unchanged.
            dock.spawn((
                Name::new("Muster battalion"),
                MusterCard,
                Button,
                button_chrome(UiButtonVariant::Inverse),
                Node {
                    width: Val::Px(76.0),
                    height: Val::Px(CARD_HEIGHT),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::all(Val::Px(6.0)),
                    row_gap: Val::Px(4.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                crate::ui::styles::plate_shadow(),
                children![
                    (
                        Text::new("+"),
                        UiButtonLabel,
                        crate::ui::typography::heading(27.0),
                        TextColor(PARCHMENT),
                        Pickable::IGNORE,
                    ),
                    (
                        MusterCardLabel,
                        Text::new("MUSTER"),
                        crate::ui::typography::heading(11.0),
                        TextColor(PARCHMENT),
                        TextLayout::justify(Justify::Center),
                        Pickable::IGNORE,
                    ),
                ],
            ));
        });
}

/// Cards are rebuilt only when the set of battalions changes (mustered,
/// disbanded, renamed); everything that moves per-frame is bound in place.
#[allow(clippy::type_complexity)]
fn rebuild_battalion_cards(
    mut commands: Commands,
    roster: Res<ArmyRoster>,
    row: Query<Entity, With<BattalionCardRow>>,
    existing: Query<Entity, With<BattalionCard>>,
    mut signature: Local<u64>,
) {
    use std::hash::{Hash, Hasher};
    let Ok(row) = row.single() else {
        return;
    };
    if !roster.is_changed() && (!existing.is_empty() || roster.battalions.is_empty()) {
        return;
    }
    let mine: Vec<_> = roster
        .battalions
        .iter()
        .map(|b| (b.entity, b.ordinal, b.name.clone()))
        .collect();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (entity, ordinal, name) in &mine {
        entity.hash(&mut hasher);
        ordinal.hash(&mut hasher);
        name.hash(&mut hasher);
    }
    let next = hasher.finish();
    let fresh = existing.is_empty() && !mine.is_empty();
    if !fresh && *signature == next {
        return;
    }
    *signature = next;
    for card in existing.iter() {
        commands.entity(card).despawn();
    }

    commands.entity(row).with_children(|row| {
        for (battalion, ordinal, _) in &mine {
            row.spawn((
                BattalionCard(*battalion),
                Button,
                Name::new(format!("Select battalion {}", roman_numeral(*ordinal))),
                button_chrome(UiButtonVariant::Inverse),
                Node {
                    width: Val::Px(CARD_WIDTH),
                    height: Val::Px(CARD_HEIGHT),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(8.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                crate::ui::styles::plate_shadow(),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(roman_numeral(*ordinal)),
                    UiButtonLabel,
                    crate::ui::typography::heading(27.0),
                    TextColor(PARCHMENT),
                    TextShadow {
                        offset: Vec2::new(0.0, 1.5),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.6),
                    },
                    Pickable::IGNORE,
                ));
                card.spawn((
                    CardCountText(*battalion),
                    Text::new(""),
                    crate::ui::typography::heading(11.5),
                    TextColor(PARCHMENT.with_alpha(0.82)),
                    Pickable::IGNORE,
                ));
                // The health trough with its bound fill.
                card.spawn((
                    Pickable::IGNORE,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(6.0),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(HEALTH_LOSS),
                    children![(
                        CardHealthFill(*battalion),
                        Pickable::IGNORE,
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(HEALTH_FILL),
                    )],
                ));
            });
        }
    });
}

/// Everything that moves: soldier counts, health fills, the ember highlight
/// on cards whose soldiers are in the current selection. All writes diffed.
fn bind_battalion_cards(
    mode: Res<CombatMode>,
    roster: Res<ArmyRoster>,
    selection: Res<crate::selection::Selection>,
    mut cards: Query<(&BattalionCard, &mut UiButtonStyle)>,
    mut counts: Query<(&CardCountText, &mut Text)>,
    mut fills: Query<(&CardHealthFill, &mut Node)>,
) {
    if !mode.0 {
        return;
    }
    if !roster.is_changed() && !selection.is_changed() && !mode.is_changed() {
        return;
    }
    let selected: std::collections::HashMap<_, _> = roster
        .battalions
        .iter()
        .map(|b| {
            (
                b.entity,
                b.members
                    .iter()
                    .filter(|e| selection.is_selected(**e))
                    .count(),
            )
        })
        .collect();
    for (card, mut style) in &mut cards {
        let next = selected.get(&card.0).is_some_and(|count| *count > 0);
        if style.selected != next {
            style.selected = next;
        }
    }
    for (marker, mut text) in &mut counts {
        let Some(b) = roster.battalions.iter().find(|b| b.entity == marker.0) else {
            continue;
        };
        let n = selected[&b.entity];
        let next = if n > 0 && n < b.count {
            format!("{n}/{} SELECTED", b.count)
        } else {
            format!(
                "{} {}",
                b.count,
                match b.role {
                    shared::components::SoldierRole::Archer => "BOWS",
                    shared::components::SoldierRole::Cavalry => "RIDERS",
                    shared::components::SoldierRole::Infantry => "MEN",
                }
            )
        };
        if text.0 != next {
            text.0 = next;
        }
    }
    for (marker, mut node) in &mut fills {
        let Some(b) = roster.battalions.iter().find(|b| b.entity == marker.0) else {
            continue;
        };
        let next = Val::Percent(b.health_fraction * 100.0);
        if node.width != next {
            node.width = next;
        }
    }
}

fn handle_card_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    mode: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    keys: Res<ButtonInput<KeyCode>>,
    cards: Query<(&Interaction, &BattalionCard)>,
    roster: Res<ArmyRoster>,
    mut selection: ResMut<crate::selection::Selection>,
) {
    if !mode.0 || input.ui_blocking() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, card) in &cards {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(battalion) = roster.battalions.iter().find(|b| b.entity == card.0) else {
            continue;
        };
        if !battalion.members.is_empty() {
            selection.apply_group(
                battalion.members.clone(),
                keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
                true,
            );
        }
    }
}

/// The muster card narrates what a click would do: dimmed "MUSTER" when
/// nothing is selected, an ember "MUSTER 5" when five of your soldiers are.
#[allow(clippy::type_complexity)]
fn bind_muster_card(
    selection: Res<crate::selection::Selection>,
    roster: Res<ArmyRoster>,
    mut cards: Query<&mut UiButtonStyle, With<MusterCard>>,
    mut labels: Query<&mut Text, With<MusterCardLabel>>,
) {
    let eligible = roster.muster_candidates(&selection).len();
    let next_label = if eligible == 0 {
        // Creates an empty battalion; fill it from the army page.
        "NEW BATTALION".to_string()
    } else {
        format!("MUSTER {eligible}")
    };
    for mut label in labels.iter_mut() {
        if label.0 != next_label {
            label.0 = next_label.clone();
        }
    }
    for mut style in &mut cards {
        let selected = eligible > 0;
        if style.selected != selected {
            style.selected = selected;
        }
    }
}

#[allow(clippy::type_complexity)]
fn handle_muster_card_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    mode: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    time: Res<Time>,
    mut last_muster: Local<Option<f32>>,
    cards: Query<&Interaction, With<MusterCard>>,
    selection: Res<crate::selection::Selection>,
    roster: Res<ArmyRoster>,
    mut senders: Query<
        &mut lightyear::prelude::MessageSender<shared::protocol::ArmyOrder>,
        (With<crate::GameClient>, With<lightyear::prelude::Connected>),
    >,
) {
    if !mode.0 || input.ui_blocking() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let pressed = cards
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);
    if !pressed {
        return;
    }
    // Same double-click guard as the encyclopedia's muster button: one
    // gesture, one battalion.
    let now = time.elapsed_secs();
    if last_muster.is_some_and(|sent| now - sent < 1.0) {
        return;
    }
    let recruits = roster.muster_candidates(&selection);
    *last_muster = Some(now);
    if let Ok(mut sender) = senders.single_mut() {
        sender.send::<shared::protocol::ReliableChannel>(shared::protocol::ArmyOrder::Muster {
            members: recruits,
        });
    }
}

/// The bar rises whenever combat mode arms - even with no battalions yet,
/// because the muster card on it IS how you raise the first one - and sinks
/// out of sight otherwise. Same underdamped spring as the banner.
fn animate_battalion_bar(
    selection: Res<crate::selection::Selection>,
    machines: Query<(), With<shared::components::Catapult>>,
    time: Res<Time>,
    mode: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    mut roots: Query<(&mut BattalionBarRoot, &mut Node, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let target = if mode.0
        && !(selection.len() > 0 && selection.entities.iter().all(|e| machines.contains(*e)))
    {
        BAR_SHOWN_BOTTOM
    } else {
        BAR_HIDDEN_BOTTOM
    };
    for (mut root, mut node, mut visibility) in roots.iter_mut() {
        visibility.set_if_neq(if input.ui_blocking() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        if !root.spring.step(target, dt, 220.0, 16.0) {
            continue;
        }
        let next = Val::Px(root.spring.value);
        if node.bottom != next {
            node.bottom = next;
        }
    }
}

fn despawn_battalion_bar(mut commands: Commands, roots: Query<Entity, With<BattalionBarRoot>>) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
}
