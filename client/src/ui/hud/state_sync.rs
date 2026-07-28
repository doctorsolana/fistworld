//! state sync systems.

use super::*;

pub(super) fn reset_dev_grant(mut capability: ResMut<GodCapability>, mut mode: ResMut<HudMode>) {
    capability.0 = false;
    *mode = HudMode::Play;
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
