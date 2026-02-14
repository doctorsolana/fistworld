//! chest systems.

use super::*;

/// Spawn a single chest slot
pub(super) fn spawn_chest_slot(parent: &mut ChildSpawnerCommands, index: usize) {
    parent
        .spawn((
            ChestSlot { index },
            Button,
            Node {
                width: Val::Px(48.0),
                height: Val::Px(48.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(SLOT_EMPTY),
            BorderColor::from(Color::srgb(0.5, 0.35, 0.2)), // Wood-ish border
        ))
        .with_children(|slot| {
            // Item icon (colored box)
            slot.spawn((
                ChestSlotIcon { index },
                Node {
                    width: Val::Px(36.0),
                    height: Val::Px(36.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|icon| {
                icon.spawn((
                    ChestSlotIconImage { index },
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        ..default()
                    },
                    ImageNode::new(Handle::default()),
                    Visibility::Hidden,
                ));
            });

            // Quantity text
            slot.spawn((
                ChestSlotQuantity { index },
                Text::new(""),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(TEXT_COLOR),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(2.0),
                    bottom: Val::Px(0.0),
                    ..default()
                },
            ));
        });
}

/// Update chest slot visuals based on chest contents
pub(super) fn update_chest_slots(
    inventory_open: Res<InventoryOpen>,
    open_chest: Res<OpenChest>,
    chests: Query<&ChestStorage>,
    drag: Res<DragState>,
    mut slots: Query<(
        &ChestSlot,
        &mut BackgroundColor,
        &mut BorderColor,
        &Interaction,
    )>,
    mut icons: Query<(&ChestSlotIcon, &mut BackgroundColor), Without<ChestSlot>>,
    mut icon_images: Query<
        (&ChestSlotIconImage, &mut ImageNode, &mut Visibility),
        Without<ChestSlot>,
    >,
    mut quantities: Query<(&ChestSlotQuantity, &mut Text)>,
    preview_assets: Option<Res<ItemPreviewAssets>>,
) {
    if !inventory_open.0 {
        return;
    }

    let Some(chest_entity) = open_chest.entity else {
        return;
    };

    let Ok(chest) = chests.get(chest_entity) else {
        return;
    };

    // Update slot backgrounds
    for (slot, mut bg, mut border, interaction) in slots.iter_mut() {
        let has_item = chest.get_slot(slot.index).is_some();
        let is_drag_from = drag.dragging && drag.from_chest && drag.from_slot == Some(slot.index);

        *bg = match interaction {
            Interaction::Hovered | Interaction::Pressed => BackgroundColor(SLOT_HOVERED),
            Interaction::None => {
                if has_item {
                    BackgroundColor(SLOT_NORMAL)
                } else {
                    BackgroundColor(SLOT_EMPTY)
                }
            }
        };

        // Border highlight when dragging from this slot
        let border_color = if is_drag_from {
            Color::srgba(1.0, 1.0, 1.0, 0.8)
        } else {
            Color::srgb(0.5, 0.35, 0.2)
        };
        *border = BorderColor::from(border_color);
    }

    // Update icons
    for (icon, mut bg) in icons.iter_mut() {
        if let Some(stack) = chest.get_slot(icon.index) {
            *bg = BackgroundColor(icon_bg_color(stack.item_type));
        } else {
            *bg = BackgroundColor(Color::NONE);
        }
    }

    // Update icon images (weapons only)
    if let Some(previews) = preview_assets.as_ref() {
        for (icon, mut image, mut vis) in icon_images.iter_mut() {
            let handle = chest
                .get_slot(icon.index)
                .and_then(|stack| preview_icon_handle(stack.item_type, previews));
            if let Some(handle) = handle {
                *image = ImageNode::new(handle);
                *vis = Visibility::Visible;
            } else {
                *vis = Visibility::Hidden;
            }
        }
    } else {
        for (_, _, mut vis) in icon_images.iter_mut() {
            *vis = Visibility::Hidden;
        }
    }

    // Update quantities
    for (qty, mut text) in quantities.iter_mut() {
        if let Some(stack) = chest.get_slot(qty.index) {
            **text = format!("{}", stack.quantity);
        } else {
            **text = String::new();
        }
    }
}

/// Handle chest slot interactions (drag to/from chest)
pub(super) fn handle_chest_slot_interactions(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    open_chest: Res<OpenChest>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    ui_root: Query<Entity, With<InventoryUI>>,
    inv_slots: Query<(&InventorySlot, &Interaction), Without<ChestSlot>>,
    chest_slots: Query<(&ChestSlot, &Interaction), Without<InventorySlot>>,
    mut drag: ResMut<DragState>,
    chests: Query<&ChestStorage>,
    local_player: Query<&Inventory, With<LocalPlayer>>,
    mut client_query: Query<
        &mut MessageSender<ChestTransferRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut drag_icon: DragIconUi,
    preview_assets: Option<Res<ItemPreviewAssets>>,
) {
    if !inventory_open.0 || open_chest.entity.is_none() {
        return;
    }

    let Some(chest_entity) = open_chest.entity else {
        return;
    };
    let Ok(chest) = chests.get(chest_entity) else {
        return;
    };
    let Ok(_inventory) = local_player.single() else {
        return;
    };

    // Find hovered slots
    let hovered_chest_slot = chest_slots
        .iter()
        .find(|(_, i)| **i == Interaction::Hovered || **i == Interaction::Pressed)
        .map(|(s, _)| s.index);

    let hovered_inv_slot = inv_slots
        .iter()
        .find(|(_, i)| **i == Interaction::Hovered || **i == Interaction::Pressed)
        .map(|(s, _)| s.index);

    // Start drag from chest slot
    if !drag.dragging && mouse.just_pressed(MouseButton::Left) {
        if let Some(from) = hovered_chest_slot {
            if let Some(stack) = chest.get_slot(from).copied() {
                drag.dragging = true;
                drag.from_slot = Some(from);
                drag.from_chest = true;
                drag.stack = Some(stack);

                // Spawn floating icon
                if let Ok(root) = ui_root.single() {
                    commands.entity(root).with_children(|parent| {
                        let mut ec = parent.spawn((
                            DragIcon,
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Px(DRAG_ICON_SIZE),
                                height: Val::Px(DRAG_ICON_SIZE),
                                ..default()
                            },
                            BackgroundColor(icon_bg_color(stack.item_type)),
                            BorderColor::from(ACCENT_COLOR),
                            Text::new(if stack.quantity > 1 {
                                format!("{}", stack.quantity)
                            } else {
                                String::new()
                            }),
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(TEXT_COLOR),
                        ));
                        ec.with_children(|icon| {
                            icon.spawn((
                                DragIconImage,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(0.0),
                                    top: Val::Px(0.0),
                                    ..default()
                                },
                                ImageNode::new(Handle::default()),
                                Visibility::Hidden,
                            ));
                        });
                        drag.icon_entity = Some(ec.id());
                    });
                }
            }
        }
    }

    // Update drag icon position while dragging from chest
    if drag.dragging && drag.from_chest {
        if let Ok(window) = windows.single() {
            if let Some(pos) = drag_icon_pos_from_cursor(window, ui_scale.0) {
                for mut node in drag_icon.nodes.iter_mut() {
                    node.left = Val::Px(pos.x.max(0.0));
                    node.top = Val::Px(pos.y.max(0.0));
                }
            }
        }
        if let Some(stack) = drag.stack {
            for mut bg in drag_icon.bg.iter_mut() {
                *bg = BackgroundColor(icon_bg_color(stack.item_type));
            }
            for mut text in drag_icon.text.iter_mut() {
                **text = if stack.quantity > 1 {
                    format!("{}", stack.quantity)
                } else {
                    String::new()
                };
            }
            if let Some(previews) = preview_assets.as_ref() {
                let handle = preview_icon_handle(stack.item_type, previews);
                for (mut image, mut vis) in drag_icon.images.iter_mut() {
                    if let Some(handle) = handle.clone() {
                        *image = ImageNode::new(handle);
                        *vis = Visibility::Visible;
                    } else {
                        *vis = Visibility::Hidden;
                    }
                }
            } else {
                for (_, mut vis) in drag_icon.images.iter_mut() {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }

    // End drag - only handle chest-related transfers
    // (inventory -> inventory is handled by handle_drag_and_drop)
    if drag.dragging && mouse.just_released(MouseButton::Left) {
        let from_chest = drag.from_chest;
        let from_slot = drag.from_slot;

        // Only handle if:
        // 1. Dragging FROM chest (chest -> anywhere), or
        // 2. Dragging TO chest (inventory -> chest)
        let should_handle = from_chest || hovered_chest_slot.is_some();

        if !should_handle {
            // Not a chest-related transfer, let handle_drag_and_drop deal with it
            return;
        }

        // Clean up drag state
        if let Some(icon) = drag.icon_entity.take() {
            commands.entity(icon).despawn();
        }
        drag.dragging = false;
        drag.from_slot = None;
        drag.from_chest = false;
        drag.stack = None;

        let Some(from_slot) = from_slot else { return };

        // Determine target and send transfer request
        if from_chest {
            // Dragging FROM chest
            if let Some(to_slot) = hovered_inv_slot {
                // Chest -> Inventory
                if let Ok(mut sender) = client_query.single_mut() {
                    sender.send::<ReliableChannel>(ChestTransferRequest {
                        from_chest: true,
                        from_slot: from_slot as u8,
                        to_slot: to_slot as u8,
                    });
                }
            } else if let Some(to_slot) = hovered_chest_slot {
                // Chest -> Chest (reordering within chest)
                if to_slot != from_slot {
                    // For now, just ignore - can't reorder within chest
                }
            }
        } else {
            // Dragging FROM inventory TO chest
            if let Some(to_slot) = hovered_chest_slot {
                if let Ok(mut sender) = client_query.single_mut() {
                    sender.send::<ReliableChannel>(ChestTransferRequest {
                        from_chest: false,
                        from_slot: from_slot as u8,
                        to_slot: to_slot as u8,
                    });
                }
            }
        }
    }
}
