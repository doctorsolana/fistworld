//! slots systems.

use super::*;

/// Spawn a single inventory slot
pub(super) fn spawn_slot(parent: &mut ChildSpawnerCommands, index: usize) {
    parent
        .spawn((
            InventorySlot { index },
            Button,
            Node {
                width: Val::Px(64.0),
                height: Val::Px(64.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(SLOT_EMPTY),
            BorderColor::from(SLOT_BORDER),
        ))
        .with_children(|slot| {
            // Item icon (colored square)
            slot.spawn((
                SlotIcon { index },
                Node {
                    width: Val::Px(46.0),
                    height: Val::Px(46.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|icon| {
                icon.spawn((
                    SlotIconImage { index },
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

            // Quantity text (bottom-right corner)
            slot.spawn((
                SlotQuantity { index },
                Text::new(""),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(TEXT_COLOR),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(2.0),
                    right: Val::Px(4.0),
                    ..default()
                },
            ));
        });
}

/// Update slot visuals based on inventory contents
pub(super) fn update_inventory_slots(
    inventory_open: Res<InventoryOpen>,
    local_player: Query<&Inventory, With<LocalPlayer>>,
    hotbar: Query<&HotbarSelection, With<LocalPlayer>>,
    drag: Res<DragState>,
    mut slots: Query<(
        &InventorySlot,
        &mut BackgroundColor,
        &mut BorderColor,
        &Interaction,
    )>,
    mut icons: Query<(&SlotIcon, &mut BackgroundColor), Without<InventorySlot>>,
    mut icon_images: Query<(&SlotIconImage, &mut ImageNode, &mut Visibility), Without<SlotIcon>>,
    mut quantities: Query<(&SlotQuantity, &mut Text)>,
    preview_assets: Option<Res<ItemPreviewAssets>>,
) {
    if !inventory_open.0 {
        return;
    }

    let Ok(inventory) = local_player.single() else {
        return;
    };
    let active_hotbar = hotbar.single().ok().map(|h| h.index as usize);

    // Update slot backgrounds based on interaction
    for (slot, mut bg, mut border, interaction) in slots.iter_mut() {
        let has_item = inventory.get_slot(slot.index).is_some();
        let is_hotbar = slot.index < HOTBAR_SLOTS;
        let is_active = active_hotbar == Some(slot.index);
        let is_drag_from = drag.dragging && drag.from_slot == Some(slot.index);

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

        // Border highlights: hotbar row + active slot
        let border_color = if is_active {
            ACCENT_COLOR
        } else if is_drag_from {
            Color::srgba(1.0, 1.0, 1.0, 0.8)
        } else if is_hotbar {
            HOTBAR_BORDER
        } else {
            SLOT_BORDER
        };
        *border = BorderColor::from(border_color);
    }

    // Update icons
    for (icon, mut bg) in icons.iter_mut() {
        if let Some(stack) = inventory.get_slot(icon.index) {
            *bg = BackgroundColor(icon_bg_color(stack.item_type));
        } else {
            *bg = BackgroundColor(Color::NONE);
        }
    }

    // Update icon images (weapons only)
    if let Some(previews) = preview_assets.as_ref() {
        for (icon, mut image, mut vis) in icon_images.iter_mut() {
            let handle = inventory
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
        if let Some(stack) = inventory.get_slot(qty.index) {
            **text = format!("{}", stack.quantity);
        } else {
            **text = String::new();
        }
    }
}

/// Handle slot interactions (right-click to drop)
pub(super) fn handle_slot_interactions(
    inventory_open: Res<InventoryOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    slots: Query<(&InventorySlot, &Interaction)>,
    local_player: Query<&Inventory, With<LocalPlayer>>,
    mut client_query: Query<
        &mut MessageSender<DropRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !inventory_open.0 {
        return;
    }

    // Check for right-click on slots
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    let Ok(inventory) = local_player.single() else {
        return;
    };

    for (slot, interaction) in slots.iter() {
        if *interaction == Interaction::Hovered || *interaction == Interaction::Pressed {
            // Check if slot has an item
            if inventory.get_slot(slot.index).is_some() {
                // Send drop request
                if let Ok(mut sender) = client_query.single_mut() {
                    sender.send::<ReliableChannel>(DropRequest {
                        slot_index: slot.index,
                    });
                    info!("Requesting drop from slot {}", slot.index);
                }
            }
        }
    }
}
