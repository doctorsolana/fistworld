//! state sync systems.

use super::*;

pub(super) fn reset_dev_grant(
    mut capability: ResMut<GodCapability>,
    mut mode: ResMut<HudMode>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    mut immigrant_watch: ResMut<ImmigrantBoatWatch>,
) {
    capability.0 = false;
    *mode = HudMode::Play;
    // A stale armed placement surviving a reconnect would fire on the first
    // innocent click of the new session.
    *placement = crate::hero::control::WorldPlacementMode::None;
    immigrant_watch.clear();
}

/// Keep the arrival launcher's label and active treatment in sync with its
/// camera watcher. The containing God panel owns access and visibility.
pub(super) fn sync_immigrant_boat_button(
    watch: Res<ImmigrantBoatWatch>,
    mut buttons: Query<&mut UiButtonStyle, With<SpawnImmigrantBoatButton>>,
    mut labels: Query<&mut Text, With<SpawnImmigrantBoatLabel>>,
) {
    let label = if watch.following.is_some() {
        "STOP WATCHING BOAT"
    } else if watch.waiting {
        "FINDING IMMIGRANT BOAT…"
    } else {
        "SPAWN IMMIGRANT BOAT"
    };
    for mut style in buttons.iter_mut() {
        style.variant = if watch.active() {
            UiButtonVariant::Developer
        } else {
            UiButtonVariant::Secondary
        };
    }
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
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
    mut chips: Query<(&mut Node, &mut UiButtonStyle), With<ModeChipButton>>,
    mut labels: Query<&mut Text, With<ModeChipText>>,
) {
    let display = if capability.0 {
        Display::Flex
    } else {
        Display::None
    };
    // GOD reads as a neutral dark inversion, not as the accent: dev chrome must
    // never be mistakable for game state. It is also a ghost button at rest, so
    // PLAY is just a word in the clock row.
    let (label, variant) = match *mode {
        HudMode::God => ("GOD", UiButtonVariant::Developer),
        HudMode::Play => ("PLAY", UiButtonVariant::Ghost),
    };
    for (mut node, mut style) in chips.iter_mut() {
        if node.display != display {
            node.display = display;
        }
        style.variant = variant;
    }
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
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
    mut buttons: Query<(&WarpButton, &mut UiButtonStyle)>,
) {
    let current = warp.iter().next().map_or(1.0, |w| w.0);

    for (WarpButton(factor), mut style) in buttons.iter_mut() {
        // Exact match only: a non-preset factor (e.g. clamped or set by another god)
        // highlights nothing — the clock chip's suffix already shows the true value.
        let is_active = (current - factor).abs() < 1e-3;
        style.variant = if is_active {
            UiButtonVariant::Developer
        } else {
            UiButtonVariant::Secondary
        };
    }
}

/// Spawn button reflects the real gate: armed, ready, or already spawned.
pub(super) fn sync_spawn_hero_button(
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<&shared::components::Hero>,
    mut buttons: Query<&mut UiButtonStyle, With<SpawnHeroButton>>,
    mut labels: Query<&mut Text, With<SpawnHeroLabel>>,
) {
    let owns_hero = local
        .as_ref()
        .is_some_and(|local| crate::hero::control::local_hero_exists(&heroes, local));
    if owns_hero && placement.is_spawn_hero() {
        *placement = crate::hero::control::WorldPlacementMode::None;
    }

    let label = if owns_hero {
        "HERO ACTIVE"
    } else if placement.is_spawn_hero() {
        // Armed is the one dev affordance that genuinely needs to shout, so it
        // takes the slate inversion rather than the reserved accent.
        "CLICK TERRAIN"
    } else {
        "SPAWN HERO"
    };

    for mut style in buttons.iter_mut() {
        style.variant = if placement.is_spawn_hero() {
            UiButtonVariant::Developer
        } else {
            UiButtonVariant::Secondary
        };
    }
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
}

/// Show the selected-unit plate, and say who is selected and what they are.
///
/// Reads `CharacterName` rather than the player's profile: every person in the
/// world now has a name, so a selected villager is named as themselves instead
/// of showing an empty plate.
///
/// A single selected person exposes the three real shared attributes immediately;
/// EXPAND opens the complete live record (job, wage, home, food and inventory).
#[allow(clippy::type_complexity)] // Disjoint Bevy UI mutations require one ParamSet.
pub(super) fn sync_selection_plate(
    selection: Res<crate::selection::Selection>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    characters: Query<(
        &shared::components::CharacterName,
        &shared::components::CharacterKind,
        &shared::components::PlayerPosition,
        Option<&shared::components::CommandedBy>,
        Option<&shared::components::CharacterAttributes>,
        Option<&shared::components::CharacterObjective>,
        Option<&shared::components::CharacterNavigationStatus>,
        Option<&shared::components::Health>,
    )>,
    mut glyphs: Query<&mut BorderColor, With<SelectionRingGlyph>>,
    mut names: Query<&mut Text, (With<SelectionNameText>, Without<SelectionStatusText>)>,
    mut statuses: Query<&mut Text, (With<SelectionStatusText>, Without<SelectionNameText>)>,
    mut nodes: ParamSet<(
        Query<&mut Node, (With<SelectionPlate>, Without<SelectionExpandButton>)>,
        Query<&mut Node, (With<SelectionHealthTrack>, Without<SelectionHealthFill>)>,
        Query<
            (&mut Node, &mut BackgroundColor),
            (With<SelectionHealthFill>, Without<SelectionHealthTrack>),
        >,
        Query<&mut Node, (With<SelectionExpandButton>, Without<SelectionPlate>)>,
    )>,
    visuals: Query<&crate::hero::HeroVisual>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
) {
    let mut _ui_scope = ui_perf.scope("sync_selection_plate");
    let count = selection.len();
    // A selected SETTLEMENT is not a unit and gets its own panel. Without this
    // the plate would stay up showing whoever was selected before, because the
    // character lookup below simply fails and returns.
    let is_person = selection
        .primary()
        .is_some_and(|entity| characters.contains(entity));
    let display = if count > 0 && is_person {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in nodes.p0().iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }
    let show_expand = count == 1 && is_person;
    for mut node in nodes.p3().iter_mut() {
        let display = if show_expand {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let Some(primary) = selection.primary().and_then(|e| characters.get(e).ok()) else {
        return;
    };
    // Position is no longer read here: movement comes from the smoothed
    // visual, not from diffing the replicated position frame to frame. The
    // compact plate leads with the person's purpose; attributes remain in the
    // expanded encyclopedia record.
    let (name, kind, _position, commanded, attributes, objective, navigation, health) = primary;

    let health_percentage = health.map(shared::components::Health::percentage);
    for mut node in nodes.p1().iter_mut() {
        node.display = if count == 1 && health_percentage.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some(percentage) = health_percentage {
        let color = if percentage > 0.6 {
            Color::srgb(0.29, 0.58, 0.29)
        } else if percentage > 0.3 {
            Color::srgb(0.78, 0.55, 0.18)
        } else {
            Color::srgb(0.72, 0.20, 0.16)
        };
        for (mut node, mut background) in nodes.p2().iter_mut() {
            node.width = Val::Percent(percentage * 100.0);
            background.0 = color;
        }
    }

    let my_account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase());
    let owns = |commanded: Option<&shared::components::CommandedBy>| {
        crate::selection::can_command(commanded, my_account.as_deref())
    };
    let is_mine = owns(commanded);

    // How many of the selection actually take orders. A box-drag over a village
    // grabs a mixed crowd, and the plate has to say what will move.
    let commandable = selection
        .entities
        .iter()
        .filter(|entity| {
            characters
                .get(**entity)
                .is_ok_and(|(_, _, _, c, _, _, _, _)| owns(c))
        })
        .count();

    // The one saturated colour means "you command this". Grey means you are
    // merely looking at someone.
    let glyph_color = if commandable > 0 {
        EMBER_RULE
    } else {
        INK_MUTED
    };
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
    // Ask the SMOOTHED visual whether it is walking.
    //
    // This used to compare the replicated position against last frame's. That
    // answers "did a packet land this frame?", not "is this character moving?"
    // -- replication is ~20 Hz and rendering is not, so a walking villager read
    // HOLDING on the two frames out of three with no packet and flipped to ON
    // THE MOVE on the third. The plate changed its mind dozens of times a
    // second while the character walked in a straight line.
    //
    // `HeroVisual::speed` is already smoothed for the walk animation, so the
    // feet and the label now agree by construction.
    const MOVING_ABOVE: f32 = 0.25;
    let moved = selection
        .primary()
        .and_then(|entity| visuals.get(entity).ok())
        .is_some_and(|visual| visual.speed() > MOVING_ABOVE);
    // A box-drag only ever selects your own, so a group is normally all
    // commandable. The mixed branches stay because a selection can also be set
    // by other means, and a group that silently would not move must say so.
    // Phrased as the CONSEQUENCE of the next right-click, not as a property.
    // "UNDER YOUR BANNER" was both wrong -- a banner is affiliation, not command
    // -- and the exact phrase that made a player expect clanmates to obey.
    let status = if count > 1 {
        match commandable {
            0 => "NONE WILL MOVE".to_string(),
            n if n == count => format!("{n} WILL MOVE"),
            n => format!("{n} OF {count} WILL MOVE"),
        }
    } else if let Some(objective) = objective {
        navigation.and_then(|state| state.label()).map_or_else(
            || objective.label().to_uppercase(),
            |state| format!("{} · {}", objective.label(), state).to_uppercase(),
        )
    } else if let Some(attributes) = attributes {
        format!(
            "P{} I{} C{}",
            attributes.physique(),
            attributes.intelligence(),
            attributes.charm(),
        )
    } else if is_mine {
        if moved {
            "ON THE MOVE".to_string()
        } else {
            "HOLDING".to_string()
        }
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
/// The drag box is in WINDOW pixels and Bevy UI lengths are in UI pixels, and on
/// this app those are NOT the same unit. On macOS the window takes a scale
/// factor override of 1.0 and the Retina factor is moved into `UiScale`
/// (see `app_wiring::window::apply_window_mode`), so `cursor_position` comes
/// back in physical pixels while every `Val::Px` is multiplied by `UiScale` on
/// the way to the screen.
///
/// Writing cursor coordinates straight into `Val::Px` therefore draws the
/// marquee at roughly double the intended position -- down and to the right by
/// the scale factor. Dividing here is the conversion between the two spaces.
///
/// Note the box TEST does not need this: it compares world-projected points
/// against the cursor, both already in window pixels. Only drawing crosses into
/// UI space.
pub(super) fn sync_selection_box(
    drag: Res<crate::selection::DragBox>,
    ui_scale: Res<bevy::ui::UiScale>,
    mut boxes: Query<&mut Node, With<SelectionBox>>,
) {
    let rect = drag.rect();
    let scale = if ui_scale.0 > 0.0 { ui_scale.0 } else { 1.0 };
    for mut node in boxes.iter_mut() {
        match rect {
            Some((min, max)) => {
                // Convert both corners once, then measure -- converting the
                // corner and then un-converting to get the size is the kind of
                // arithmetic that looks right and drifts by a pixel.
                let (min, max) = (min / scale, max / scale);
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
    placement: Res<crate::hero::control::WorldPlacementMode>,
    mut buttons: Query<&mut UiButtonStyle, With<SpawnNpcButton>>,
    mut labels: Query<&mut Text, With<SpawnNpcLabel>>,
) {
    let armed = placement.is_spawn_npc();
    let label = if armed {
        "CLICK TO PLACE"
    } else {
        "SPAWN VILLAGER"
    };
    for mut style in buttons.iter_mut() {
        style.variant = if armed {
            UiButtonVariant::Developer
        } else {
            UiButtonVariant::Secondary
        };
    }
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
}

/// Found button reflects whether founding is armed.
pub(super) fn sync_found_village_button(
    placement: Res<crate::hero::control::WorldPlacementMode>,
    mut buttons: Query<&mut UiButtonStyle, With<FoundVillageButton>>,
    mut labels: Query<&mut Text, With<FoundVillageLabel>>,
) {
    let armed = placement.is_found_settlement();
    let label = if armed {
        "CLICK TO FOUND"
    } else {
        "FOUND VILLAGE"
    };
    for mut style in buttons.iter_mut() {
        style.variant = if armed {
            UiButtonVariant::Developer
        } else {
            UiButtonVariant::Secondary
        };
    }
    for mut text in labels.iter_mut() {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
}
