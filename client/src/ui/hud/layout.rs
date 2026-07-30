//! HUD layout: two plates, and nothing else.
//!
//! Composition:
//!
//! ```text
//!                                                  +---------------------+
//!                                                  | DAY 3  14:22   PLAY |  <- one plate,
//!                                                  +---------------------+     grows down
//!                                                                              in god mode
//!
//!                              ( the world is the interface )
//!
//!                         +----------------------------------+
//!                         | (o) |  SIGRUN         ON THE MOVE |  <- only when
//!                         +----------------------------------+     selected
//! ```
//!
//! Two things here are deliberate and easy to undo by accident.
//!
//! **The god panel is ONE plate with hairline dividers**, not a stack of nested
//! bordered boxes. It also has no `SIMULATION SPEED` or `HERO` captions: four
//! buttons reading `II 1x 10x 100x` do not need a label telling you they are
//! speeds, and deleting the caption deletes a whole row of chrome.
//!
//! **The mode toggle lives INSIDE the clock row.** As its own bordered chip it
//! was a second surface competing with the clock for the same corner.

use super::*;

pub(super) fn spawn_hud(
    mut commands: Commands,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    // The offline capture tool enters Playing too; its screenshots must stay
    // clean of UI unless a capture explicitly asks for the HUD.
    let hud_requested = std::env::var("FISTFORCE_CAPTURE_HUD").is_ok_and(|v| !v.is_empty());
    if capture.is_some() && !hud_requested {
        return;
    }
    commands.spawn((
        HudRoot,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        // NO `Interaction` on this root, and this is load-bearing. It spans the
        // whole screen, and the world-click guard treats any hovered
        // `Interaction` as UI -- so a full-screen node carrying one would veto
        // every world click for as long as the cursor is on screen. The plates
        // inside it carry `Interaction` and swallow clicks; this does not.
        Pickable::IGNORE,
        children![top_right_column(), selection_plate(), selection_box()],
    ));
}

/// The world-state corner: clock, and the god tools beneath it.
fn top_right_column() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(12.0),
            top: Val::Px(12.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: Val::Px(8.0),
            ..default()
        },
        Pickable::IGNORE,
        children![clock_plate(), god_plate()],
    )
}

/// Shared plate chrome: fill, carved rule, radius, the two-layer shadow, and
/// the marker that makes it swallow world clicks.
fn plate(fill: Color) -> impl Bundle {
    (
        BackgroundColor(fill),
        BorderColor::from(PLATE_RULE),
        plate_shadow(),
        Interaction::default(),
    )
}

fn clock_plate() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(9.0),
            padding: UiRect::axes(Val::Px(11.0), Val::Px(7.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        plate(LIMEWASH),
        children![
            (
                ClockPeriodText,
                Text::new("DAY 0"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ),
            (
                ClockTimeText,
                Text::new("--:--"),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(INK),
            ),
            (
                ClockWarpText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(EMBER),
                Node {
                    display: Display::None,
                    ..default()
                },
            ),
            mode_toggle(),
        ],
    )
}

/// The play/god toggle, folded into the clock row as a ghost button: no fill and
/// no border at rest, so it reads as a word in the row rather than a second
/// surface. Hidden entirely until the server grants god capability.
fn mode_toggle() -> impl Bundle {
    (
        ModeChipButton,
        Button,
        Node {
            display: Display::None,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            ModeChipText,
            Text::new("PLAY"),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(INK_MUTED),
        )],
    )
}

/// God tools. One plate, hairline-separated, no section captions.
fn god_plate() -> impl Bundle {
    (
        GodPanel,
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: Val::Px(7.0),
            padding: UiRect::all(Val::Px(9.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        plate(LIMEWASH),
        children![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(5.0),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    warp_button("II", 0.0),
                    warp_button("1x", 1.0),
                    warp_button("10x", 10.0),
                    warp_button("100x", 100.0),
                ],
            ),
            hairline(),
            spawn_hero_button(),
            spawn_npc_button(),
            hairline(),
            (
                Text::new("G god   J time   N people   M map"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ),
        ],
    )
}

/// A 1px internal divider. Replaces wrapping each section in its own bordered
/// box, which is what made the old panel read as boxes-inside-boxes.
fn hairline() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            ..default()
        },
        BackgroundColor(PLATE_RULE_SOFT),
        Pickable::IGNORE,
    )
}

fn warp_button(text: &str, factor: f32) -> impl Bundle {
    (
        Button,
        WarpButton(factor),
        Node {
            width: Val::Px(40.0),
            height: Val::Px(24.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(BUTTON_NORMAL),
        BorderColor::from(PLATE_RULE_SOFT),
        children![(
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(INK),
        )],
    )
}

fn spawn_hero_button() -> impl Bundle {
    (
        SpawnHeroButton,
        Button,
        Node {
            // Full width so the column reads as one stack rather than a
            // right-aligned button floating in its own row of empty plate.
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(BUTTON_NORMAL),
        BorderColor::from(PLATE_RULE_SOFT),
        children![(
            SpawnHeroLabel,
            Text::new("SPAWN HERO"),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(INK),
        )],
    )
}

/// The drag-select marquee.
///
/// A hairline outline with a barely-there wash, not a filled rectangle: the
/// player is selecting things they need to keep SEEING while they drag, and a
/// tinted overlay is exactly what stops them judging which units are inside it.
fn selection_box() -> impl Bundle {
    (
        SelectionBox,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.973, 0.961, 0.929, 0.10)),
        BorderColor::from(Color::srgba(0.129, 0.118, 0.102, 0.75)),
        // Never blocks: the marquee sits under the cursor for its whole life,
        // and a marquee that ate the release event could not finish a selection.
        Pickable::IGNORE,
    )
}

/// Drop a villager: a named person who lives in the world and belongs to
/// nobody. A test tool until settlements produce their own population.
fn spawn_npc_button() -> impl Bundle {
    (
        SpawnNpcButton,
        Button,
        Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(BUTTON_NORMAL),
        BorderColor::from(PLATE_RULE_SOFT),
        children![(
            SpawnNpcLabel,
            Text::new("SPAWN VILLAGER"),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(INK),
        )],
    )
}

/// The selected-unit plate.
///
/// The leading mark is a hollow ring: the SAME form as the mark on the ground,
/// in the same ink. That rhyme is the whole "unmistakable" mechanism -- the
/// player never has to be told the plate refers to the ringed unit, because the
/// shapes match. No icon, no arrow, no label saying SELECTED.
///
/// It carries `Interaction` because it sits in the bottom-centre cursor zone:
/// without it, clicking your own readout would fall through to the world and
/// deselect the very thing the readout describes.
fn selection_plate() -> impl Bundle {
    (
        SelectionPlate,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            bottom: Val::Px(26.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-112.0)),
            width: Val::Px(224.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::axes(Val::Px(11.0), Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        plate(LIMEWASH_LIT),
        children![
            (
                SelectionRingGlyph,
                Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::from(EMBER_RULE),
                Pickable::IGNORE,
            ),
            // Fixed width, so a longer name can never reflow the status word
            // sideways. Things that move under a settled cursor are the bug
            // players actually feel.
            (
                SelectionNameText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ),
            (
                SelectionStatusText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ),
        ],
    )
}

pub(super) fn despawn_hud(mut commands: Commands, roots: Query<Entity, With<HudRoot>>) {
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}
