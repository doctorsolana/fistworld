//! layout systems.

use super::*;

pub(super) fn spawn_debug_time_menu(
    mut commands: Commands,
    open: Res<DebugTimeMenuOpen>,
    existing: Query<Entity, With<DebugMenuRoot>>,
) {
    if !open.0 || !existing.is_empty() {
        return;
    }

    let layout = ModalLayout {
        panel_size: Vec2::new(420.0, 580.0),
        panel_padding: 18.0,
    };

    let nodes = spawn_modal(
        &mut commands,
        DebugMenuRoot,
        DebugMenuBackdrop,
        DebugMenuPanel,
        layout,
    );

    commands.entity(nodes.panel).with_children(|panel| {
        panel.spawn((
            Text::new("DEBUG TIME"),
            TextFont {
                font_size: FontSize::Px(26.0),
                ..default()
            },
            TextColor(ACCENT_COLOR),
            Node {
                margin: UiRect::bottom(Val::Px(8.0)),
                ..default()
            },
        ));

        panel.spawn((
            Text::new("Set server time of day"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                margin: UiRect::bottom(Val::Px(16.0)),
                ..default()
            },
        ));

        spawn_time_button(panel, "NIGHT", TimeOfDayPreset::Night);
        spawn_time_button(panel, "MORNING", TimeOfDayPreset::Morning);
        spawn_time_button(panel, "MIDDAY", TimeOfDayPreset::Midday);
        spawn_time_button(panel, "SUNSET", TimeOfDayPreset::Sunset);

        panel.spawn((
            Text::new("Cloud cover test"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                margin: UiRect::top(Val::Px(12.0)),
                ..default()
            },
        ));

        spawn_cloud_cover_button(panel, "AUTO CLOUDS", CloudCoverMode::Auto);
        spawn_cloud_cover_button(panel, "FORCE CLEAR", CloudCoverMode::Clear);
        spawn_cloud_cover_button(panel, "FORCE CLOUDY", CloudCoverMode::Cloudy);
        spawn_cloud_cover_button(panel, "FORCE STORM", CloudCoverMode::Storm);

        panel.spawn((
            Text::new("Performance debug"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                margin: UiRect::top(Val::Px(12.0)),
                ..default()
            },
        ));

        spawn_weightmap_stats_button(panel);
        spawn_render_diag_button(panel);

        // Character selection section
        panel.spawn((
            Text::new("Player character"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                margin: UiRect::top(Val::Px(12.0)),
                ..default()
            },
        ));

        panel.spawn((
            Text::new("NPC debug"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                margin: UiRect::top(Val::Px(12.0)),
                ..default()
            },
        ));

        panel
            .spawn((
                Button,
                CloseButton,
                Node {
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..button_style()
                },
                BackgroundColor(BUTTON_NORMAL),
            ))
            .with_children(|btn| {
                btn.spawn((
                    Text::new("CLOSE"),
                    button_text_style(),
                    TextColor(TEXT_COLOR),
                ));
            });
    });
}

pub(super) fn style_debug_time_menu(
    mut colors: ParamSet<(
        Query<&mut BackgroundColor, With<DebugMenuRoot>>,
        Query<&mut BackgroundColor, With<DebugMenuBackdrop>>,
        Query<&mut BackgroundColor, With<DebugMenuPanel>>,
    )>,
) {
    let clear = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0));
    for mut bg in colors.p0().iter_mut() {
        *bg = clear;
    }
    for mut bg in colors.p1().iter_mut() {
        *bg = clear;
    }
    for mut bg in colors.p2().iter_mut() {
        *bg = clear;
    }
}

pub(super) fn spawn_time_button(
    parent: &mut ChildSpawnerCommands<'_>,
    text: &str,
    preset: TimeOfDayPreset,
) {
    parent
        .spawn((
            Button,
            TimeButton(preset),
            Node {
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..button_style()
            },
            BackgroundColor(BUTTON_NORMAL),
        ))
        .with_children(|btn| {
            btn.spawn((Text::new(text), button_text_style(), TextColor(TEXT_COLOR)));
        });
}

pub(super) fn spawn_cloud_cover_button(
    parent: &mut ChildSpawnerCommands<'_>,
    text: &str,
    mode: CloudCoverMode,
) {
    parent
        .spawn((
            Button,
            CloudCoverButton(mode),
            Node {
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..button_style()
            },
            BackgroundColor(BUTTON_NORMAL),
        ))
        .with_children(|btn| {
            btn.spawn((Text::new(text), button_text_style(), TextColor(TEXT_COLOR)));
        });
}

pub(super) fn spawn_weightmap_stats_button(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Button,
            PerfWeightmapToggleButton,
            Node {
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..button_style()
            },
            BackgroundColor(BUTTON_NORMAL),
        ))
        .with_children(|btn| {
            btn.spawn((
                PerfWeightmapLabel,
                Text::new("WEIGHTMAP STATS: OFF"),
                button_text_style(),
                TextColor(TEXT_COLOR),
            ));
        });
}

pub(super) fn spawn_render_diag_button(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Button,
            PerfRenderDiagToggleButton,
            Node {
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..button_style()
            },
            BackgroundColor(BUTTON_NORMAL),
        ))
        .with_children(|btn| {
            btn.spawn((
                PerfRenderDiagLabel,
                Text::new("RENDER DIAG LOGGING: OFF"),
                button_text_style(),
                TextColor(TEXT_COLOR),
            ));
        });
}

pub(super) fn despawn_debug_time_menu(
    mut commands: Commands,
    query: Query<Entity, With<DebugMenuRoot>>,
    children: Query<&Children>,
    mut input_state: ResMut<InputState>,
    open: Res<DebugTimeMenuOpen>,
) {
    if open.0 {
        return;
    }
    for entity in query.iter() {
        despawn_recursive(&mut commands, entity, &children);
    }
    input_state.debug_menu_open = false;
}

pub(super) fn despawn_recursive(
    commands: &mut Commands,
    entity: Entity,
    children: &Query<&Children>,
) {
    if let Ok(kids) = children.get(entity) {
        for child in kids.iter() {
            despawn_recursive(commands, child, children);
        }
    }
    commands.entity(entity).despawn();
}
