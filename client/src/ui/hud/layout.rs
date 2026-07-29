//! layout systems.

use super::*;

pub(super) fn spawn_hud(
    mut commands: Commands,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    // The offline capture tool enters Playing too; its screenshots must stay clean of UI.
    if capture.is_some() {
        return;
    }
    commands
        .spawn((
            HudRoot,
            // Interaction makes the whole HUD rect (panels, labels, the gaps
            // between swatches) register as UI to the world-click guard —
            // Buttons alone left every non-button pixel click-through.
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(8.0),
                ..default()
            },
        ))
        .with_children(|root| {
            spawn_clock_chip(root);
            spawn_mode_chip(root);
            spawn_god_panel(root);
        });
}

fn spawn_clock_chip(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::from(BUTTON_BORDER),
        ))
        .with_children(|chip| {
            chip.spawn((
                ClockPeriodText,
                Text::new("DAY 0"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(ACCENT_COLOR),
            ));
            chip.spawn((
                ClockTimeText,
                Text::new("--:--"),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
            chip.spawn((
                ClockWarpText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(ACCENT_COLOR),
                Node {
                    display: Display::None,
                    ..default()
                },
            ));
        });
}

fn spawn_mode_chip(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            ModeChipButton,
            Button,
            Node {
                // Hidden until the server grants god capability.
                display: Display::None,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(BUTTON_BORDER),
        ))
        .with_children(|btn| {
            btn.spawn((
                ModeChipText,
                Text::new("PLAY MODE"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        });
}

fn spawn_god_panel(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            GodPanel,
            Node {
                // Hidden outside god mode.
                display: Display::None,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::from(BUTTON_BORDER),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("SIMULATION SPEED"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    spawn_warp_button(row, "II", 0.0);
                    spawn_warp_button(row, "1x", 1.0);
                    spawn_warp_button(row, "10x", 10.0);
                    spawn_warp_button(row, "100x", 100.0);
                });
            spawn_hero_section(panel);
            panel.spawn((
                Text::new("G god mode   J time of day"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        });
}

fn spawn_warp_button(parent: &mut ChildSpawnerCommands<'_>, text: &str, factor: f32) {
    parent
        .spawn((
            Button,
            WarpButton(factor),
            Node {
                width: Val::Px(44.0),
                height: Val::Px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(BUTTON_BORDER),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(text),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
        });
}

/// Hero spawn section: outfit swatches + the spawn button. Lives inside the
/// god panel so its visibility rides `sync_god_panel` for free.
fn spawn_hero_section(panel: &mut ChildSpawnerCommands<'_>) {
    panel.spawn((
        Text::new("HERO"),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        Node {
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        },
    ));

    panel
        .spawn((
            SpawnHeroButton,
            Button,
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(14.0), Val::Px(7.0)),
                margin: UiRect::top(Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(ACCENT_COLOR),
        ))
        .with_children(|btn| {
            btn.spawn((
                SpawnHeroLabel,
                Text::new("SPAWN HERO"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
        });
}

pub(super) fn despawn_hud(mut commands: Commands, roots: Query<Entity, With<HudRoot>>) {
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}
