//! Persistent edge HUD. Transparent layout containers never consume world input.

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
        children![
            top_right_column(),
            super::shell::location(),
            super::shell::navigation(),
            super::selection_card::view(),
            selection_box()
        ],
    ));
}

/// The world-state corner: clock, and the god tools beneath it.
fn top_right_column() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(18.0),
            top: Val::Px(18.0),
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
        Name::new("World clock"),
        Node {
            height: Val::Px(46.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::axes(Val::Px(18.0), Val::Px(4.0)),
            ..default()
        },
        super::chrome::pill_panel(),
        crate::ui::foundation::surface_block(),
        Interaction::default(),
        children![
            (
                ClockIcon,
                super::chrome::icon(super::chrome::HudIcon::Sun, 24.0)
            ),
            (
                ClockPeriodText,
                Text::new("DAY 0"),
                crate::ui::typography::heading(14.0),
                TextColor(crate::ui::styles::INK_INVERSE),
                Pickable::IGNORE
            ),
            (
                Text::new("|"),
                crate::ui::typography::body(14.0),
                TextColor(crate::ui::styles::BRASS),
                Pickable::IGNORE
            ),
            (
                ClockTimeText,
                Text::new("--:--"),
                crate::ui::typography::heading(13.0),
                TextColor(crate::ui::styles::INK_INVERSE),
                Pickable::IGNORE
            ),
            (
                ClockWarpText,
                Text::new(""),
                crate::ui::typography::body(13.0),
                TextColor(crate::ui::styles::BRASS),
                Node {
                    display: Display::None,
                    ..default()
                },
                Pickable::IGNORE
            ),
            mode_toggle(),
            super::journey::notice_button(),
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
        button_chrome(UiButtonVariant::Ribbon),
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

/// A hidden modal HUD must not leave invisible click-catching rectangles.
pub(super) fn sync_visibility(
    input: Res<InputState>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mut roots: Query<&mut Node, With<HudRoot>>,
) {
    let hidden = input.ui_blocking()
        || opening
            .as_deref()
            .is_some_and(|opening| opening.is_active());
    let display = if hidden { Display::None } else { Display::Flex };
    for mut node in &mut roots {
        if node.display != display {
            node.display = display;
        }
    }
}
