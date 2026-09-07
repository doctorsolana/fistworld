//! The battalion bar: Rome-style unit cards along the bottom of the screen.
//!
//! Springs up when combat mode arms (the same movement as the COMBAT MODE
//! sign dropping from the top - the war UI arrives as one gesture), and
//! shows one card per battalion: its Roman numeral in the display face, its
//! strength in men, and a thin health bar that thins as the unit bleeds.
//! Clicking a card selects the whole battalion; cards of battalions with
//! soldiers in the current selection wear the ember highlight.

use bevy::prelude::*;

use crate::army_roster::ArmyRoster;

use crate::combat_mode::CombatMode;
use crate::states::GameState;
use crate::ui::{
    motion::Spring,
    styles::{CRIMSON, PARCHMENT, SIGN_WOOD},
};

/// Clears the selection plate that hugs the bottom edge; the cards hover
/// just above it rather than fighting it for the same pixels.
const BAR_SHOWN_BOTTOM: f32 = 58.0;
const BAR_HIDDEN_BOTTOM: f32 = -160.0;
const CARD_WIDTH: f32 = 84.0;
const CARD_HEIGHT: f32 = 96.0;
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
    commands.spawn((
        BattalionBarRoot {
            spring: Spring::new(BAR_HIDDEN_BOTTOM),
        },
        Pickable::IGNORE,
        GlobalZIndex(56),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            bottom: Val::Px(BAR_HIDDEN_BOTTOM),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(10.0),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
        children![
            (
                BattalionCardRow,
                Pickable::IGNORE,
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(10.0),
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
            ),
            // The muster card: raising a NEW battalion lives right here on
            // the war bar, not three menus deep. Select troops, click, done.
            (
                MusterCard,
                Button,
                crate::ui::foundation::UiButtonStyleExempt,
                Node {
                    width: Val::Px(CARD_WIDTH),
                    height: Val::Px(CARD_HEIGHT),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    row_gap: Val::Px(2.0),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(7.0)),
                    ..default()
                },
                BackgroundColor(SIGN_WOOD.with_alpha(0.72)),
                BorderColor::all(PARCHMENT.with_alpha(0.25)),
                children![
                    (
                        Text::new("+"),
                        crate::ui::typography::heading(30.0),
                        TextColor(PARCHMENT.with_alpha(0.55)),
                        Pickable::IGNORE,
                    ),
                    (
                        MusterCardLabel,
                        Text::new("MUSTER"),
                        crate::ui::typography::heading(11.0),
                        TextColor(PARCHMENT.with_alpha(0.55)),
                        Pickable::IGNORE,
                    ),
                ],
            ),
        ],
    ));
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
                // The card paints its own selected/rest states (war palette,
                // not the panel chrome family), so it opts out of the shared
                // style contract instead of wearing the wrong coat.
                crate::ui::foundation::UiButtonStyleExempt,
                Node {
                    width: Val::Px(CARD_WIDTH),
                    height: Val::Px(CARD_HEIGHT),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(8.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(7.0)),
                    ..default()
                },
                BackgroundColor(SIGN_WOOD),
                BorderColor::all(CRIMSON.with_alpha(0.55)),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(roman_numeral(*ordinal)),
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
    mut cards: Query<(&BattalionCard, &mut BorderColor, &mut BackgroundColor)>,
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
    for (card, mut border, mut background) in &mut cards {
        let Some(battalion) = roster.battalions.iter().find(|b| b.entity == card.0) else {
            continue;
        };
        let count = selected[&card.0];
        let full = count > 0 && count == battalion.count;
        border.set_if_neq(BorderColor::all(if full {
            Color::srgb(0.95, 0.72, 0.35)
        } else if count > 0 {
            Color::srgb(0.90, 0.42, 0.20)
        } else {
            CRIMSON.with_alpha(0.55)
        }));
        background.set_if_neq(BackgroundColor(if count > 0 {
            Color::srgba(0.20, 0.13, 0.08, 0.96)
        } else {
            SIGN_WOOD
        }));
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
                if b.role == shared::components::SoldierRole::Archer {
                    "BOWS"
                } else {
                    "MEN"
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
    keys: Res<ButtonInput<KeyCode>>,
    cards: Query<(&Interaction, &BattalionCard)>,
    roster: Res<ArmyRoster>,
    mut selection: ResMut<crate::selection::Selection>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
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
    mut cards: Query<&mut BorderColor, With<MusterCard>>,
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
    let next_border = if eligible == 0 {
        BorderColor::all(PARCHMENT.with_alpha(0.25))
    } else {
        BorderColor::all(Color::srgb(0.95, 0.72, 0.35))
    };
    for mut border in cards.iter_mut() {
        if *border != next_border {
            *border = next_border;
        }
    }
}

#[allow(clippy::type_complexity)]
fn handle_muster_card_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
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
    if !mouse.just_pressed(MouseButton::Left) {
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
    mut roots: Query<(&mut BattalionBarRoot, &mut Node)>,
) {
    let dt = time.delta_secs();
    let target = if mode.0
        && !(selection.len() > 0 && selection.entities.iter().all(|e| machines.contains(*e)))
    {
        BAR_SHOWN_BOTTOM
    } else {
        BAR_HIDDEN_BOTTOM
    };
    for (mut root, mut node) in roots.iter_mut() {
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
