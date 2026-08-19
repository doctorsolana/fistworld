//! layout systems.

use super::*;

pub(super) fn spawn_debug_time_menu(
    mut commands: Commands,
    open: Res<DebugTimeMenuOpen>,
    god: Res<GodCapability>,
    existing: Query<Entity, With<DebugMenuRoot>>,
) {
    if !open.0 || !existing.is_empty() {
        return;
    }

    let layout = ModalLayout {
        panel_size: if god.0 {
            Vec2::new(420.0, 580.0)
        } else {
            Vec2::new(420.0, 330.0)
        },
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
        if !god.0 {
            spawn_god_access_panel(panel);
            return;
        }
        panel.spawn((
            Text::new("DEBUG TIME"),
            TextFont {
                font_size: FontSize::Px(26.0),
                ..default()
            },
            TextColor(EMBER),
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
            TextColor(INK_MUTED),
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
            TextColor(INK_MUTED),
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
            TextColor(INK_MUTED),
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
            TextColor(INK_MUTED),
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
            TextColor(INK_MUTED),
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
                    ..debug_button_node()
                },
                button_chrome(UiButtonVariant::Secondary),
            ))
            .with_children(|btn| {
                btn.spawn((
                    Text::new("CLOSE"),
                    UiButtonLabel,
                    debug_button_font(),
                    TextColor(INK),
                ));
            });
    });
}

fn spawn_god_access_panel(panel: &mut ChildSpawnerCommands<'_>) {
    panel.spawn((
        Text::new("SERVER ADMIN"),
        TextFont {
            font_size: FontSize::Px(26.0),
            ..default()
        },
        TextColor(EMBER),
    ));
    panel.spawn((
        Text::new("Enter the hosted server access key to unlock God Mode for this connection."),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            max_width: Val::Px(340.0),
            margin: UiRect::vertical(Val::Px(12.0)),
            ..default()
        },
    ));
    panel
        .spawn((
            Node {
                width: Val::Px(340.0),
                height: Val::Px(42.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(crate::ui::styles::SLATE),
            BorderColor::from(INK_MUTED),
        ))
        .with_children(|field| {
            field.spawn((
                GodAccessInputDisplay,
                Text::new("_"),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(crate::ui::styles::INK_INVERSE),
            ));
        });
    panel.spawn((
        GodAccessFeedbackText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            min_height: Val::Px(20.0),
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        },
    ));
    panel
        .spawn((
            Button,
            GodAccessSubmitButton,
            Node {
                width: Val::Px(180.0),
                height: Val::Px(42.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Developer),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("UNLOCK GOD MODE"),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK),
            ));
        });
    panel.spawn((
        Text::new("ENTER submit   ESC close"),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            margin: UiRect::top(Val::Px(10.0)),
            ..default()
        },
    ));
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
                ..debug_button_node()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(text),
                UiButtonLabel,
                debug_button_font(),
                TextColor(INK),
            ));
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
                ..debug_button_node()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(text),
                UiButtonLabel,
                debug_button_font(),
                TextColor(INK),
            ));
        });
}

pub(super) fn spawn_weightmap_stats_button(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Button,
            PerfWeightmapToggleButton,
            Node {
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..debug_button_node()
            },
            button_chrome(UiButtonVariant::Developer),
        ))
        .with_children(|btn| {
            btn.spawn((
                PerfWeightmapLabel,
                Text::new("WEIGHTMAP STATS: OFF"),
                UiButtonLabel,
                debug_button_font(),
                TextColor(INK),
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
                ..debug_button_node()
            },
            button_chrome(UiButtonVariant::Developer),
        ))
        .with_children(|btn| {
            btn.spawn((
                PerfRenderDiagLabel,
                Text::new("RENDER DIAG LOGGING: OFF"),
                UiButtonLabel,
                debug_button_font(),
                TextColor(INK),
            ));
        });
}

fn debug_button_node() -> Node {
    Node {
        width: Val::Px(280.0),
        height: Val::Px(55.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        margin: UiRect::all(Val::Px(8.0)),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(4.0)),
        ..default()
    }
}

fn debug_button_font() -> TextFont {
    TextFont {
        font_size: FontSize::Px(22.0),
        ..default()
    }
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
