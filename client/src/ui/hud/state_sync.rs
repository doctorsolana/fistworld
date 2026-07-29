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
    let (label, color) = if wt.is_day() {
        (format!("DAY {}", wt.day), ACCENT_COLOR)
    } else {
        (format!("NIGHT {}", wt.day), TEXT_MUTED)
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
    let (label, text_color, border) = match *mode {
        HudMode::God => ("GOD MODE", ACCENT_COLOR, ACCENT_COLOR),
        HudMode::Play => ("PLAY MODE", TEXT_MUTED, BUTTON_BORDER),
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
            ACCENT_COLOR
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
            TEXT_COLOR
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

/// Selected outfit swatches fill with the accent, mirroring the warp row.
pub(super) fn style_outfit_buttons(
    selected: Res<crate::hero::control::SelectedOutfit>,
    mut hair: Query<
        (&Interaction, &HairButton, &mut BackgroundColor, &Children),
        (Without<ShortsButton>, Without<ShirtToggleButton>),
    >,
    mut shorts: Query<
        (&Interaction, &ShortsButton, &mut BackgroundColor, &Children),
        (Without<HairButton>, Without<ShirtToggleButton>),
    >,
    mut shirt: Query<
        (&Interaction, &mut BackgroundColor, &Children),
        (
            With<ShirtToggleButton>,
            Without<HairButton>,
            Without<ShortsButton>,
        ),
    >,
    mut labels: Query<&mut TextColor>,
) {
    let mut apply = |interaction: &Interaction,
                     is_active: bool,
                     bg: &mut BackgroundColor,
                     children: &Children,
                     labels: &mut Query<&mut TextColor>| {
        let background = if is_active {
            ACCENT_COLOR
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
        let text_color = if is_active { WARP_ACTIVE_TEXT } else { TEXT_COLOR };
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                if color.0 != text_color {
                    color.0 = text_color;
                }
            }
        }
    };

    for (interaction, HairButton(index), mut bg, children) in hair.iter_mut() {
        apply(
            interaction,
            *index == selected.0.hair,
            &mut bg,
            children,
            &mut labels,
        );
    }
    for (interaction, ShortsButton(index), mut bg, children) in shorts.iter_mut() {
        apply(
            interaction,
            *index == selected.0.shorts,
            &mut bg,
            children,
            &mut labels,
        );
    }
    for (interaction, mut bg, children) in shirt.iter_mut() {
        apply(interaction, selected.0.shirt, &mut bg, children, &mut labels);
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
        ("HERO ACTIVE", TEXT_MUTED, BUTTON_BORDER)
    } else if arm.0 {
        ("CLICK TERRAIN...", ACCENT_COLOR, ACCENT_COLOR)
    } else {
        ("SPAWN HERO", TEXT_COLOR, ACCENT_COLOR)
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
