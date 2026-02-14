//! drag drop systems.

use super::*;

pub(super) fn drag_icon_pos_from_cursor(window: &Window, ui_scale: f32) -> Option<Vec2> {
    let cursor = window.cursor_position()?;
    let scale = ui_scale.max(0.001);
    Some(Vec2::new(
        (cursor.x / scale) - DRAG_ICON_HALF,
        (cursor.y / scale) - DRAG_ICON_HALF,
    ))
}

/// Handle left-click drag & drop within the inventory UI
pub(super) fn handle_drag_and_drop(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    ui_root: Query<Entity, With<InventoryUI>>,
    slots: Query<(&InventorySlot, &Interaction), Without<ChestSlot>>,
    chest_slots: Query<(&ChestSlot, &Interaction), Without<InventorySlot>>,
    mut drag: ResMut<DragState>,
    mut local_player_inventory: Query<&mut Inventory, With<LocalPlayer>>,
    mut client_query: Query<
        &mut MessageSender<InventoryMoveRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut drag_icon: DragIconUi,
    preview_assets: Option<Res<ItemPreviewAssets>>,
) {
    if !inventory_open.0 {
        drag.dragging = false;
        drag.from_slot = None;
        drag.from_chest = false;
        drag.stack = None;
        drag.icon_entity = None;
        return;
    }

    let hovered_slot = slots
        .iter()
        .find(|(_, interaction)| {
            **interaction == Interaction::Hovered || **interaction == Interaction::Pressed
        })
        .map(|(slot, _)| slot.index);

    // Check if hovering over a chest slot - if so, don't start an inventory drag
    let hovering_chest = chest_slots.iter().any(|(_, interaction)| {
        *interaction == Interaction::Hovered || *interaction == Interaction::Pressed
    });

    // Start drag (only from inventory slots, not chest)
    if !drag.dragging && mouse.just_pressed(MouseButton::Left) && !hovering_chest {
        let Some(from) = hovered_slot else { return };

        let Ok(inventory) = local_player_inventory.single_mut() else {
            return;
        };
        if let Some(stack) = inventory.get_slot(from).copied() {
            drag.dragging = true;
            drag.from_slot = Some(from);
            drag.from_chest = false; // Dragging from inventory
            drag.stack = Some(stack);

            // Spawn floating icon under cursor (child of inventory root)
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

    // Update drag icon position (only when dragging from inventory, not from chest)
    if drag.dragging && !drag.from_chest {
        if let Ok(window) = windows.single() {
            if let Some(pos) = drag_icon_pos_from_cursor(window, ui_scale.0) {
                for mut node in drag_icon.nodes.iter_mut() {
                    node.left = Val::Px(pos.x.max(0.0));
                    node.top = Val::Px(pos.y.max(0.0));
                }
            }
        }
        // Keep icon color/text in sync (weapon/items)
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

    // End drag (only for inventory-to-inventory, chest drags are handled in handle_chest_slot_interactions)
    if drag.dragging && !drag.from_chest && mouse.just_released(MouseButton::Left) {
        let Some(from) = drag.from_slot else {
            drag.dragging = false;
            drag.from_chest = false;
            return;
        };

        // Check if dropping onto a chest slot (handled by chest system)
        let dropping_to_chest = chest_slots.iter().any(|(_, interaction)| {
            *interaction == Interaction::Hovered || *interaction == Interaction::Pressed
        });

        if !dropping_to_chest {
            // Inventory to inventory move
            if let Some(to) = hovered_slot {
                if to != from {
                    if let Ok(mut sender) = client_query.single_mut() {
                        sender.send::<ReliableChannel>(InventoryMoveRequest {
                            from: from as u8,
                            to: to as u8,
                        });
                    }

                    // Client-side prediction for snappy UI
                    if let Ok(mut inv) = local_player_inventory.single_mut() {
                        let _ = inv.move_or_stack_slot(from, to);
                    }
                }
            }

            // Despawn drag icon
            if let Some(icon) = drag.icon_entity.take() {
                commands.entity(icon).despawn();
            }

            drag.dragging = false;
            drag.from_slot = None;
            drag.from_chest = false;
            drag.stack = None;
        }
        // If dropping to chest, let the chest handler deal with cleanup
    }
}
