//! state sync systems.

use super::*;

pub(super) fn reset_dev_grant(
    mut capability: ResMut<GodCapability>,
    mut mode: ResMut<HudMode>,
    mut arm: ResMut<crate::hero::control::HeroSpawnArm>,
) {
    capability.0 = false;
    *mode = HudMode::Play;
    // A stale armed placement surviving a reconnect would fire on the first
    // innocent click of the new session.
    arm.0 = false;
}

pub(super) fn receive_dev_status(
    mut receivers: Query<&mut MessageReceiver<DevStatus>, With<crate::GameClient>>,
    mut capability: ResMut<GodCapability>,
    mut mode: ResMut<HudMode>,
) {
    for mut receiver in receivers.iter_mut() {
        for status in receiver.receive() {
            capability.0 = status.god;
            if !status.god {
                *mode = HudMode::Play;
            }
        }
    }
}

/// Suffix shown when the world clock runs off real time.
fn warp_label(factor: f32) -> String {
    if factor == 0.0 {
        "II".to_string()
    } else if (factor - factor.round()).abs() < 0.01 {
        format!("{}x", factor.round() as i32)
    } else {
        format!("{factor:.1}x")
    }
}

pub(super) fn sync_clock_chip(
    world_time: Query<&WorldTime>,
    warp: Query<&TimeWarp>,
    mut period: Query<
        (&mut Text, &mut TextColor),
        (
            With<ClockPeriodText>,
            Without<ClockTimeText>,
            Without<ClockWarpText>,
        ),
    >,
    mut clock: Query<
        &mut Text,
        (
            With<ClockTimeText>,
            Without<ClockPeriodText>,
            Without<ClockWarpText>,
        ),
    >,
    mut suffix: Query<
        (&mut Text, &mut Node),
        (
            With<ClockWarpText>,
            Without<ClockPeriodText>,
            Without<ClockTimeText>,
        ),
    >,
) {
    let Ok(wt) = world_time.single() else {
        return;
    };

    let hours = wt.normalized_time() * 24.0;
    let hh = (hours as u32).min(23);
    let mm = (((hours - hh as f32) * 60.0) as u32).min(59);
    let time_str = format!("{hh:02}:{mm:02}");
    for mut text in clock.iter_mut() {
        if text.0 != time_str {
            text.0 = time_str.clone();
        }
    }

    // The calendar starts at day 0 and rides the replicated WorldTime, so every client
    // shows the same date even under warp.
    // Day/night differ by WORD and by value, never by the accent: EMBER is
    // reserved for selection, and a permanently-lit accent in the corner would
    // compete with the one thing that must catch the eye.
    let (label, color) = if wt.is_day() {
        (format!("DAY {}", wt.day), INK)
    } else {
        (format!("NIGHT {}", wt.day), INK_MUTED)
    };
    for (mut text, mut text_color) in period.iter_mut() {
        if text.0 != label {
            text.0 = label.clone();
        }
        if text_color.0 != color {
            text_color.0 = color;
        }
    }

    let factor = warp.iter().next().map_or(1.0, |w| w.0);
    let warped = (factor - 1.0).abs() > f32::EPSILON;
    for (mut text, mut node) in suffix.iter_mut() {
        let display = if warped { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        if warped {
            let label = warp_label(factor);
            if text.0 != label {
                text.0 = label;
            }
        }
    }
}

pub(super) fn sync_mode_chip(
    capability: Res<GodCapability>,
    mode: Res<HudMode>,
    mut chips: Query<(&mut Node, &mut BorderColor), With<ModeChipButton>>,
    mut labels: Query<(&mut Text, &mut TextColor), With<ModeChipText>>,
) {
    let display = if capability.0 {
        Display::Flex
    } else {
        Display::None
    };
    // GOD reads as a neutral dark inversion, not as the accent: dev chrome must
    // never be mistakable for game state. It is also a ghost button at rest, so
    // PLAY is just a word in the clock row.
    let (label, text_color, border) = match *mode {
        HudMode::God => ("GOD", INK_INVERSE, PLATE_RULE),
        HudMode::Play => ("PLAY", INK_MUTED, Color::NONE),
    };
    for (mut node, mut border_color) in chips.iter_mut() {
        if node.display != display {
            node.display = display;
        }
        let border = BorderColor::from(border);
        if *border_color != border {
            *border_color = border;
        }
    }
    for (mut text, mut color) in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
        if color.0 != text_color {
            color.0 = text_color;
        }
    }
}

pub(super) fn sync_god_panel(
    capability: Res<GodCapability>,
    mode: Res<HudMode>,
    mut panels: Query<&mut Node, With<GodPanel>>,
) {
    let display = if capability.0 && *mode == HudMode::God {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in panels.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }
}

/// Active-state derives from the replicated [`TimeWarp`] (server truth), never from
/// local click state.
pub(super) fn style_warp_buttons(
    warp: Query<&TimeWarp>,
    mut buttons: Query<(&Interaction, &WarpButton, &mut BackgroundColor, &Children)>,
    mut labels: Query<&mut TextColor>,
) {
    let current = warp.iter().next().map_or(1.0, |w| w.0);

    for (interaction, WarpButton(factor), mut bg, children) in buttons.iter_mut() {
        // Exact match only: a non-preset factor (e.g. clamped or set by another god)
        // highlights nothing — the clock chip's suffix already shows the true value.
        let is_active = (current - factor).abs() < 1e-3;
        let background = if is_active {
            SLATE
        } else {
            match *interaction {
                Interaction::Pressed => BUTTON_PRESSED,
                Interaction::Hovered => BUTTON_HOVERED,
                Interaction::None => BUTTON_NORMAL,
            }
        };
        if bg.0 != background {
            bg.0 = background;
        }
        let text_color = if is_active {
            WARP_ACTIVE_TEXT
        } else {
            INK
        };
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                if color.0 != text_color {
                    color.0 = text_color;
                }
            }
        }
    }
}

/// Spawn button reflects the real gate: armed, ready, or already spawned.
pub(super) fn sync_spawn_hero_button(
    mut arm: ResMut<crate::hero::control::HeroSpawnArm>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<&shared::components::Hero>,
    mut buttons: Query<(&mut BackgroundColor, &mut BorderColor), With<SpawnHeroButton>>,
    mut labels: Query<(&mut Text, &mut TextColor), With<SpawnHeroLabel>>,
) {
    let owns_hero = local
        .as_ref()
        .is_some_and(|local| crate::hero::control::local_hero_exists(&heroes, local));
    if owns_hero && arm.0 {
        arm.0 = false;
    }

    let (label, text_color, border) = if owns_hero {
        ("HERO ACTIVE", INK_MUTED, PLATE_RULE_SOFT)
    } else if arm.0 {
        // Armed is the one dev affordance that genuinely needs to shout, so it
        // takes the slate inversion rather than the reserved accent.
        ("CLICK TERRAIN", INK_INVERSE, PLATE_RULE)
    } else {
        ("SPAWN HERO", INK, PLATE_RULE_SOFT)
    };

    for (mut bg, mut border_color) in buttons.iter_mut() {
        let background = if arm.0 { BUTTON_PRESSED } else { BUTTON_NORMAL };
        if bg.0 != background {
            bg.0 = background;
        }
        let border = BorderColor::from(border);
        if *border_color != border {
            *border_color = border;
        }
    }
    for (mut text, mut color) in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
        if color.0 != text_color {
            color.0 = text_color;
        }
    }
}

/// Show the selected-unit plate, and say who is selected and what they are.
///
/// Reads `CharacterName` rather than the player's profile: every person in the
/// world now has a name, so a selected villager is named as themselves instead
/// of showing an empty plate.
///
/// There is deliberately no health or stamina bar. A character carries position,
/// rotation, a name and an outfit and nothing else, so any bar would be a lie or
/// a hardcoded 100%, and a fake gauge is worse than an absent one.
pub(super) fn sync_selection_plate(
    selection: Res<crate::selection::Selection>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    characters: Query<(
        &shared::components::CharacterName,
        &shared::components::CharacterKind,
        &shared::components::PlayerPosition,
        Option<&shared::components::Hero>,
    )>,
    mut plates: Query<&mut Node, With<SelectionPlate>>,
    mut glyphs: Query<&mut BorderColor, With<SelectionRingGlyph>>,
    mut names: Query<&mut Text, (With<SelectionNameText>, Without<SelectionStatusText>)>,
    mut statuses: Query<&mut Text, (With<SelectionStatusText>, Without<SelectionNameText>)>,
    mut last: Local<Option<Vec3>>,
) {
    let count = selection.len();
    let display = if count > 0 { Display::Flex } else { Display::None };
    for mut node in plates.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }
    let Some(primary) = selection.primary().and_then(|e| characters.get(e).ok()) else {
        *last = None;
        return;
    };
    let (name, kind, position, hero) = primary;

    let owns = |hero: Option<&shared::components::Hero>| {
        matches!((hero, local.as_ref()), (Some(hero), Some(local))
            if shared::player::peer_id_to_u64(hero.owner) == local.0)
    };
    let is_mine = owns(hero);

    // How many of the selection actually take orders. A box-drag over a village
    // grabs a mixed crowd, and the plate has to say what will move.
    let commandable = selection
        .entities
        .iter()
        .filter(|entity| characters.get(**entity).is_ok_and(|(_, _, _, h)| owns(h)))
        .count();

    // The one saturated colour means "you command this". Grey means you are
    // merely looking at someone.
    let glyph_color = if commandable > 0 { EMBER_RULE } else { INK_MUTED };
    for mut border in glyphs.iter_mut() {
        let next = BorderColor::from(glyph_color);
        if *border != next {
            *border = next;
        }
    }

    // A group is named by its SIZE, not by whoever happens to be front-most:
    // one name over a squad of six is a lie about what the next order moves.
    let label = if count > 1 {
        format!("{count} SELECTED")
    } else {
        name.0.to_uppercase()
    };
    for mut text in names.iter_mut() {
        if text.0 != label {
            text.0 = label.clone();
        }
    }

    // Moving or standing, derived from whether the replicated position changed.
    // The client holds no copy of the server's move target, and inventing a
    // replicated "is moving" flag for one cosmetic word is not worth the traffic.
    let moved = last.is_some_and(|previous| previous.distance_squared(position.0) > 1e-4);
    *last = Some(position.0);
    let status = if count > 1 {
        match commandable {
            0 => "NONE YOURS".to_string(),
            n if n == count => "READY".to_string(),
            n => format!("{n} YOURS"),
        }
    } else if is_mine {
        if moved { "ON THE MOVE".to_string() } else { "HOLDING".to_string() }
    } else {
        kind.label().to_string()
    };
    for mut text in statuses.iter_mut() {
        if text.0 != status {
            text.0 = status.clone();
        }
    }
}

/// Draw the drag-select marquee where the cursor actually is.
///
/// Both the drag box and Bevy UI use top-left-origin WINDOW pixels, so this is a
/// direct mapping with no conversion -- unlike the world-to-screen projection
/// the box test needs, which has to undo the render-target scaling.
pub(super) fn sync_selection_box(
    drag: Res<crate::selection::DragBox>,
    mut boxes: Query<&mut Node, With<SelectionBox>>,
) {
    let rect = drag.rect();
    for mut node in boxes.iter_mut() {
        match rect {
            Some((min, max)) => {
                let size = (max - min).max(Vec2::ZERO);
                node.display = Display::Flex;
                node.left = Val::Px(min.x);
                node.top = Val::Px(min.y);
                node.width = Val::Px(size.x);
                node.height = Val::Px(size.y);
            }
            None => {
                if node.display != Display::None {
                    node.display = Display::None;
                }
            }
        }
    }
}

/// Villager button reflects whether placement is armed.
pub(super) fn sync_spawn_npc_button(
    npc_arm: Res<crate::hero::control::NpcSpawnArm>,
    mut buttons: Query<(&mut BackgroundColor, &mut BorderColor), With<SpawnNpcButton>>,
    mut labels: Query<(&mut Text, &mut TextColor), With<SpawnNpcLabel>>,
) {
    let (label, text_color, border) = if npc_arm.0 {
        ("CLICK TO PLACE", INK_INVERSE, PLATE_RULE)
    } else {
        ("SPAWN VILLAGER", INK, PLATE_RULE_SOFT)
    };
    for (mut bg, mut border_color) in buttons.iter_mut() {
        let background = if npc_arm.0 { BUTTON_PRESSED } else { BUTTON_NORMAL };
        if bg.0 != background {
            bg.0 = background;
        }
        let next = BorderColor::from(border);
        if *border_color != next {
            *border_color = next;
        }
    }
    for (mut text, mut color) in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
        if color.0 != text_color {
            color.0 = text_color;
        }
    }
}
