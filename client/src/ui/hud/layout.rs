//! Clock/developer and selection plates. The optional notice tray is composed
//! below the clock; its contents and actions are owned by `journey.rs`.
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
//! bordered boxes. It also has no `SIMULATION SPEED` or `HERO` captions: five
//! buttons reading `II 1x 10x 25x 100x` do not need a label telling you they are
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
        GlobalZIndex(crate::ui::foundation::layer::HUD),
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
        children![clock_plate(), god_plate(), super::journey::view()],
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
                crate::ui::typography::text(11.0),
                TextColor(INK_MUTED),
            ),
            (
                ClockTimeText,
                Text::new("--:--"),
                crate::ui::typography::text(15.0),
                TextColor(INK),
            ),
            (
                ClockWarpText,
                Text::new(""),
                crate::ui::typography::text(13.0),
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
        button_chrome(UiButtonVariant::Ghost),
        children![(
            ModeChipText,
            UiButtonLabel,
            Text::new("PLAY"),
            crate::ui::typography::text(10.0),
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
                    warp_button("25x", 25.0),
                    warp_button("100x", 100.0),
                ],
            ),
            hairline(),
            spawn_hero_button(),
            spawn_npc_button(),
            spawn_catapult_button(),
            spawn_immigrant_boat_button(),
            found_village_button(),
            god_notice(),
            hairline(),
            (
                Text::new("G god   J time   N people   M map"),
                crate::ui::typography::text(9.0),
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
        button_chrome(UiButtonVariant::Secondary),
        children![(
            Text::new(text),
            UiButtonLabel,
            crate::ui::typography::text(13.0),
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
        button_chrome(UiButtonVariant::Secondary),
        children![(
            SpawnHeroLabel,
            UiButtonLabel,
            Text::new("SPAWN HERO"),
            crate::ui::typography::text(11.0),
            TextColor(INK),
        )],
    )
}

/// Found a settlement: arm placement, next terrain click raises a moot hall.
fn found_village_button() -> impl Bundle {
    (
        FoundVillageButton,
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
        button_chrome(UiButtonVariant::Secondary),
        children![(
            FoundVillageLabel,
            UiButtonLabel,
            Text::new("FOUND VILLAGE"),
            crate::ui::typography::text(11.0),
            TextColor(INK),
        )],
    )
}

/// Why the last god action was refused. Hidden until there is something to say.
fn god_notice() -> impl Bundle {
    (
        super::GodNoticeText,
        Node {
            display: Display::None,
            max_width: Val::Px(190.0),
            ..default()
        },
        Pickable::IGNORE,
        Text::new(String::new()),
        crate::ui::typography::text(10.0),
        // Madder, the reserved danger colour: this is the one place in god
        // chrome that reports a refusal, and it must not read as a label.
        TextColor(crate::ui::styles::ACCENT_RED),
        TextLayout::justify(Justify::Right),
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
        button_chrome(UiButtonVariant::Secondary),
        children![(
            SpawnNpcLabel,
            UiButtonLabel,
            Text::new("SPAWN VILLAGER"),
            crate::ui::typography::text(11.0),
            TextColor(INK),
        )],
    )
}

/// Launch one real natural immigrant and follow the voyage. This is a God-mode
/// world-testing instrument on every playable map.
fn spawn_immigrant_boat_button() -> impl Bundle {
    (
        SpawnImmigrantBoatButton,
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
        button_chrome(UiButtonVariant::Secondary),
        children![(
            SpawnImmigrantBoatLabel,
            UiButtonLabel,
            Text::new("SPAWN IMMIGRANT BOAT"),
            crate::ui::typography::text(10.0),
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
            margin: UiRect::left(Val::Px(-178.0)),
            width: Val::Px(356.0),
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
                crate::ui::typography::text(14.0),
                TextColor(INK),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ),
            (
                SelectionStatusText,
                Text::new(""),
                crate::ui::typography::text(10.0),
                TextColor(INK_MUTED),
            ),
            (
                SelectionHealthTrack,
                Node {
                    display: Display::None,
                    width: Val::Px(58.0),
                    height: Val::Px(7.0),
                    padding: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(PLATE_RULE_SOFT),
                Pickable::IGNORE,
                children![(
                    SelectionHealthFill,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.29, 0.58, 0.29)),
                    Pickable::IGNORE,
                )],
            ),
            (
                SelectionExpandButton,
                Button,
                Node {
                    display: Display::None,
                    height: Val::Px(27.0),
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                button_chrome(UiButtonVariant::Secondary),
                children![(
                    Text::new("EXPAND"),
                    UiButtonLabel,
                    crate::ui::typography::text(9.0),
                    TextColor(INK),
                    Pickable::IGNORE,
                )],
            ),
        ],
    )
}

pub(super) fn despawn_hud(mut commands: Commands, roots: Query<Entity, With<HudRoot>>) {
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}

fn spawn_catapult_button() -> impl Bundle {
    (
        SpawnCatapultButton,
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
        button_chrome(UiButtonVariant::Secondary),
        children![(
            SpawnCatapultLabel,
            UiButtonLabel,
            Text::new("SPAWN CATAPULT"),
            crate::ui::typography::text(11.0),
            TextColor(INK),
        )],
    )
}
