//! layout systems.

use super::*;

pub(super) fn icon_bg_color(item_type: ItemType) -> Color {
    item_type.color()
}

pub(super) fn preview_icon_handle(
    item_type: ItemType,
    previews: &ItemPreviewAssets,
) -> Option<Handle<Image>> {
    previews.item_icons.get(&item_type).cloned()
}

pub(super) fn setup_item_preview_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let mut item_icons = HashMap::new();
    // Ammo previews
    item_icons.insert(
        ItemType::RifleAmmo,
        asset_server.load("ui/item_preview/bullet512.png"),
    );
    item_icons.insert(
        ItemType::ShotgunShells,
        asset_server.load("ui/item_preview/shotgun_bullet512.png"),
    );
    item_icons.insert(
        ItemType::SniperRounds,
        asset_server.load("ui/item_preview/rifle_bullet512.png"),
    );
    // Resource previews
    item_icons.insert(
        ItemType::Stone,
        asset_server.load("ui/item_preview/stone512.png"),
    );
    item_icons.insert(
        ItemType::Wood,
        asset_server.load("ui/item_preview/wood512.png"),
    );
    item_icons.insert(
        ItemType::GoldCoin,
        asset_server.load("ui/item_preview/bullet512.png"),
    );

    commands.insert_resource(ItemPreviewAssets { item_icons });
}

/// Toggle inventory open/close with I key
pub(super) fn toggle_inventory(
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<State<GameState>>,
    mut inventory_open: ResMut<InventoryOpen>,
    mut input_state: ResMut<InputState>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    // Only allow inventory toggle in Playing state
    if game_state.get() != &GameState::Playing
        || input_state.map_open
        || input_state.pause_menu_open
        || input_state.debug_menu_open
    {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyI) {
        inventory_open.0 = !inventory_open.0;
        input_state.inventory_open = inventory_open.0;
        sync_modal_cursor(inventory_open.0, &input_state, &windows, &mut cursor_opts);

        info!(
            "Inventory {}",
            if inventory_open.0 { "opened" } else { "closed" }
        );
    }

    // Also close with Escape
    if keyboard.just_pressed(KeyCode::Escape) && inventory_open.0 {
        inventory_open.0 = false;
        input_state.inventory_open = false;
        sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);
    }
}

/// Spawn inventory UI when opened
pub(super) fn spawn_inventory_ui(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    open_chest: Res<OpenChest>,
    existing_ui: Query<Entity, With<InventoryUI>>,
) {
    // Only spawn if opened and not already existing
    if !inventory_open.0 || !existing_ui.is_empty() {
        return;
    }

    let chest_is_open = open_chest.entity.is_some();

    // Root container - centered on screen
    commands.spawn((
        InventoryUI,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: Val::Px(20.0), // Space between panels
            ..default()
        },
        // Semi-transparent background
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
    )).with_children(|parent| {
        // Chest panel (only when chest is open) - LEFT side
        if chest_is_open {
            parent.spawn((
                ChestPanel,
                Node {
                    width: Val::Px(340.0),
                    height: Val::Px(180.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    border: UiRect::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(MENU_BACKGROUND),
                BorderColor::from(Color::srgb(0.5, 0.35, 0.2)), // Wood-ish
            )).with_children(|panel| {
                // Title
                panel.spawn((
                    Text::new("CHEST"),
                    TextFont {
                        font_size: 24.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.8, 0.6, 0.3)), // Golden-brown
                    Node {
                        margin: UiRect::bottom(Val::Px(12.0)),
                        ..default()
                    },
                ));

                // Chest grid (6 columns x 1 row = 6 slots)
                panel.spawn((
                    Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::flex(6, 1.0),
                        grid_template_rows: RepeatedGridTrack::flex(1, 1.0),
                        row_gap: Val::Px(6.0),
                        column_gap: Val::Px(6.0),
                        width: Val::Percent(100.0),
                        height: Val::Auto,
                        ..default()
                    },
                )).with_children(|grid| {
                    for i in 0..CHEST_SLOTS {
                        spawn_chest_slot(grid, i);
                    }
                });
            });
        }

        // Inventory panel - RIGHT side (or center if no chest)
        parent.spawn((
            Node {
                width: Val::Px(480.0),
                height: Val::Px(380.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(MENU_BACKGROUND),
            BorderColor::from(BUTTON_BORDER),
        )).with_children(|panel| {
            // Title
            panel.spawn((
                Text::new("INVENTORY"),
                TextFont {
                    font_size: 28.0,
                    ..default()
                },
                TextColor(ACCENT_COLOR),
                Node {
                    margin: UiRect::bottom(Val::Px(12.0)),
                    ..default()
                },
            ));

            // Grid container (6 columns x 4 rows = 24 slots)
            panel.spawn((
                Node {
                    display: Display::Grid,
                    grid_template_columns: RepeatedGridTrack::flex(6, 1.0),
                    grid_template_rows: RepeatedGridTrack::flex(4, 1.0),
                    row_gap: Val::Px(6.0),
                    column_gap: Val::Px(6.0),
                    width: Val::Percent(100.0),
                    height: Val::Auto,
                    ..default()
                },
            )).with_children(|grid| {
                for i in 0..INVENTORY_SLOTS {
                    spawn_slot(grid, i);
                }
            });

            // Instructions
            let hint = if chest_is_open {
                "Drag items between chest and inventory • Right-click to drop • Press E or ESC to close"
            } else {
                "Drag with left-click to move • Right-click to drop • Press I or ESC to close"
            };
            panel.spawn((
                Text::new(hint),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    margin: UiRect::top(Val::Px(12.0)),
                    ..default()
                },
            ));
        });
    });
}

/// Despawn inventory UI when closed
pub(super) fn despawn_inventory_ui(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    ui_query: Query<Entity, With<InventoryUI>>,
) {
    if inventory_open.0 {
        return;
    }

    for entity in ui_query.iter() {
        commands.entity(entity).despawn();
    }
}
