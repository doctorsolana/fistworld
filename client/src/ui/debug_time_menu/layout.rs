//! layout systems.

use super::*;

pub(super) fn spawn_debug_time_menu(
    mut commands: Commands,
    open: Res<DebugTimeMenuOpen>,
    god: Res<GodCapability>,
    cloud_override: Res<CloudCoverOverride>,
    perf: Res<DebugPerfSettings>,
    existing: Query<Entity, With<DebugMenuRoot>>,
) {
    if !open.0 || !existing.is_empty() {
        return;
    }

    let panel_height = if god.0 { 650.0 } else { 330.0 };
    let layout = ModalLayout {
        panel_size: Vec2::new(560.0, panel_height),
        panel_padding: 0.0,
    };

    let nodes = spawn_modal(
        &mut commands,
        DebugMenuRoot,
        DebugMenuBackdrop,
        DebugMenuPanel,
        layout,
    );

    commands.entity(nodes.panel).insert((
        debug_panel_node(panel_height),
        BackgroundColor(LIMEWASH_LIT),
        BorderColor::all(PLATE_RULE),
        plate_shadow(),
    ));
    commands.entity(nodes.panel).with_children(|panel| {
        if !god.0 {
            spawn_debug_header(panel, "SERVER ADMIN", "GOD ACCESS / CONNECTION SECURITY");
            spawn_god_access_panel(panel);
            spawn_debug_footer(panel, "ENTER submit   ESC close");
            return;
        }
        spawn_debug_header(panel, "WORLD CONTROL", "DEVELOPER CONSOLE / J");
        panel
            .spawn((
                DebugMenuViewport,
                ScrollPosition::default(),
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    row_gap: Val::Px(18.0),
                    padding: UiRect::all(Val::Px(22.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 8.0,
                    ..default()
                },
            ))
            .with_children(|body| {
                spawn_section_header(
                    body,
                    "WORLD CLOCK",
                    "Move the authoritative server clock to a stable daylight preset.",
                );
                body.spawn(debug_button_row()).with_children(|row| {
                    spawn_time_button(row, "NIGHT", TimeOfDayPreset::Night);
                    spawn_time_button(row, "MORNING", TimeOfDayPreset::Morning);
                });
                body.spawn(debug_button_row()).with_children(|row| {
                    spawn_time_button(row, "MIDDAY", TimeOfDayPreset::Midday);
                    spawn_time_button(row, "SUNSET", TimeOfDayPreset::Sunset);
                });

                spawn_section_header(
                    body,
                    "WEATHER",
                    "Override cloud cover for lighting, visibility and storm testing.",
                );
                body.spawn(debug_button_row()).with_children(|row| {
                    spawn_cloud_cover_button(
                        row,
                        "AUTO CLOUDS",
                        CloudCoverMode::Auto,
                        cloud_override.mode == CloudCoverMode::Auto,
                    );
                    spawn_cloud_cover_button(
                        row,
                        "FORCE CLEAR",
                        CloudCoverMode::Clear,
                        cloud_override.mode == CloudCoverMode::Clear,
                    );
                });
                body.spawn(debug_button_row()).with_children(|row| {
                    spawn_cloud_cover_button(
                        row,
                        "FORCE CLOUDY",
                        CloudCoverMode::Cloudy,
                        cloud_override.mode == CloudCoverMode::Cloudy,
                    );
                    spawn_cloud_cover_button(
                        row,
                        "FORCE STORM",
                        CloudCoverMode::Storm,
                        cloud_override.mode == CloudCoverMode::Storm,
                    );
                });

                spawn_section_header(
                    body,
                    "DIAGNOSTICS",
                    "Opt-in instrumentation for investigating rendering and terrain cost.",
                );
                body.spawn(debug_button_row()).with_children(|row| {
                    spawn_weightmap_stats_button(row, perf.weightmap_stats);
                    spawn_render_diag_button(row, perf.render_diag_logging);
                });
            });
        spawn_debug_footer(panel, "J or ESC close");
    });
}

fn debug_panel_node(height: f32) -> Node {
    Node {
        width: Val::Vw(88.0),
        max_width: Val::Px(560.0),
        height: Val::Vh(84.0),
        max_height: Val::Px(height),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(RADIUS)),
        overflow: Overflow::clip(),
        ..default()
    }
}

fn spawn_debug_header(panel: &mut ChildSpawnerCommands<'_>, title: &str, subtitle: &str) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH_HEADER),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(title),
                        crate::ui::typography::text(type_scale::HEADING),
                        TextColor(EMBER),
                    ));
                    copy.spawn((
                        Text::new(subtitle),
                        crate::ui::typography::text(type_scale::CAPTION),
                        TextColor(INK_MUTED),
                    ));
                });
            header
                .spawn((
                    Button,
                    CloseButton,
                    Node {
                        width: Val::Px(30.0),
                        height: Val::Px(30.0),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Ghost),
                ))
                .with_child((
                    Text::new("X"),
                    UiButtonLabel,
                    crate::ui::typography::text(type_scale::VALUE),
                    TextColor(INK_MUTED),
                ));
        });
}

fn spawn_section_header(parent: &mut ChildSpawnerCommands<'_>, title: &str, detail: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|copy| {
            copy.spawn((
                Text::new(title),
                crate::ui::typography::text(type_scale::VALUE),
                TextColor(INK),
            ));
            copy.spawn((
                Text::new(detail),
                crate::ui::typography::text(type_scale::CAPTION),
                TextColor(INK_MUTED),
            ));
        });
}

fn debug_button_row() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Row,
        justify_content: JustifyContent::SpaceBetween,
        column_gap: Val::Px(8.0),
        ..default()
    }
}

fn spawn_debug_footer(panel: &mut ChildSpawnerCommands<'_>, hint: &str) {
    panel.spawn((
        Text::new(hint),
        crate::ui::typography::text(type_scale::CAPTION),
        TextColor(INK_MUTED),
        Node {
            width: Val::Percent(100.0),
            flex_shrink: 0.0,
            padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(PLATE_RULE_SOFT),
    ));
}

fn spawn_god_access_panel(panel: &mut ChildSpawnerCommands<'_>) {
    panel
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(12.0),
            padding: UiRect::all(Val::Px(22.0)),
            ..default()
        })
        .with_children(|body| {
            body.spawn((
                Text::new(
                    "Enter the hosted-server access key to unlock developer controls for this connection. The key is never saved locally.",
                ),
                crate::ui::typography::text(type_scale::BODY),
                TextColor(INK_MUTED),
            ));
            body
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(42.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    BackgroundColor(SLATE),
                    BorderColor::all(PLATE_RULE),
                ))
                .with_children(|field| {
                    field.spawn((
                        GodAccessInputDisplay,
                        Text::new("_"),
                        crate::ui::typography::text(type_scale::HEADING),
                        TextColor(INK_INVERSE),
                    ));
                });
            body.spawn((
                GodAccessFeedbackText,
                Text::new(""),
                crate::ui::typography::text(type_scale::CAPTION),
                TextColor(INK_MUTED),
                Node {
                    min_height: Val::Px(20.0),
                    ..default()
                },
            ));
            body
                .spawn((
                    Button,
                    GodAccessSubmitButton,
                    Node {
                        width: Val::Px(200.0),
                        height: Val::Px(40.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Developer),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new("UNLOCK GOD MODE"),
                        UiButtonLabel,
                        crate::ui::typography::text(type_scale::BODY),
                        TextColor(INK),
                    ));
                });
        });
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
            debug_button_node(),
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
    selected: bool,
) {
    parent
        .spawn((
            Button,
            CloudCoverButton(mode),
            debug_button_node(),
            selected_button_chrome(UiButtonVariant::Secondary, selected),
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

pub(super) fn spawn_weightmap_stats_button(parent: &mut ChildSpawnerCommands<'_>, selected: bool) {
    parent
        .spawn((
            Button,
            PerfWeightmapToggleButton,
            debug_button_node(),
            selected_button_chrome(UiButtonVariant::Developer, selected),
        ))
        .with_children(|btn| {
            btn.spawn((
                PerfWeightmapLabel,
                Text::new(if selected {
                    "WEIGHTMAP STATS: ON"
                } else {
                    "WEIGHTMAP STATS: OFF"
                }),
                UiButtonLabel,
                debug_button_font(),
                TextColor(INK),
            ));
        });
}

pub(super) fn spawn_render_diag_button(parent: &mut ChildSpawnerCommands<'_>, selected: bool) {
    parent
        .spawn((
            Button,
            PerfRenderDiagToggleButton,
            debug_button_node(),
            selected_button_chrome(UiButtonVariant::Developer, selected),
        ))
        .with_children(|btn| {
            btn.spawn((
                PerfRenderDiagLabel,
                Text::new(if selected {
                    "RENDER DIAG LOGGING: ON"
                } else {
                    "RENDER DIAG LOGGING: OFF"
                }),
                UiButtonLabel,
                debug_button_font(),
                TextColor(INK),
            ));
        });
}

fn debug_button_node() -> Node {
    Node {
        // Percentage sizing keeps the paired rows stable at both the 560 px
        // desktop cap and the narrower 88 vw fallback.
        width: Val::Percent(49.0),
        height: Val::Px(40.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(RADIUS)),
        ..default()
    }
}

fn debug_button_font() -> TextFont {
    crate::ui::typography::text(type_scale::BODY)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_menu_uses_the_responsive_ledger_dimensions() {
        let panel = debug_panel_node(650.0);
        assert_eq!(panel.width, Val::Vw(88.0));
        assert_eq!(panel.max_width, Val::Px(560.0));
        assert_eq!(panel.height, Val::Vh(84.0));
        assert_eq!(panel.max_height, Val::Px(650.0));
        assert_eq!(panel.overflow, Overflow::clip());

        let button = debug_button_node();
        assert_eq!(button.width, Val::Percent(49.0));
        assert_eq!(button.height, Val::Px(40.0));
    }
}
