//! Inventory UI - Valheim-style inventory grid
//!
//! Press I to open/close inventory.
//! Right-click slots to drop items.

pub mod chest;
pub mod drag_drop;
pub mod layout;
pub mod slots;

use chest::{handle_chest_slot_interactions, spawn_chest_slot, update_chest_slots};
use drag_drop::{drag_icon_pos_from_cursor, handle_drag_and_drop};
use layout::{
    despawn_inventory_ui, icon_bg_color, preview_icon_handle, setup_item_preview_assets,
    spawn_inventory_ui, toggle_inventory,
};
use slots::{handle_slot_interactions, spawn_slot, update_inventory_slots};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use lightyear::prelude::client::Connected;
use lightyear::prelude::*;
use shared::components::LocalPlayer;
use shared::items::{
    ChestStorage, ChestTransferRequest, DropRequest, HotbarSelection, Inventory,
    InventoryMoveRequest, ItemStack, ItemType, CHEST_SLOTS, HOTBAR_SLOTS, INVENTORY_SLOTS,
};
use shared::protocol::ReliableChannel;
use std::collections::HashMap;

use super::modal::sync_modal_cursor;
use super::styles::*;
use crate::chest::OpenChest;
use crate::input::InputState;
use crate::states::GameState;

/// Plugin for inventory UI
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryOpen>();
        app.init_resource::<DragState>();
        app.add_systems(Startup, setup_item_preview_assets);
        app.add_systems(
            Update,
            (
                toggle_inventory,
                spawn_inventory_ui,
                despawn_inventory_ui,
                // Chest interactions must run BEFORE inventory drag so chest clicks have priority
                handle_chest_slot_interactions,
                handle_drag_and_drop,
                update_inventory_slots,
                update_chest_slots,
                handle_slot_interactions,
            )
                .chain(),
        );
    }
}

/// Resource tracking if inventory is open
#[derive(Resource, Default)]
pub struct InventoryOpen(pub bool);

/// Marker for the inventory UI root
#[derive(Component)]
pub struct InventoryUI;

/// Item preview image handles for inventory UI
#[derive(Resource, Clone)]
pub struct ItemPreviewAssets {
    pub item_icons: HashMap<ItemType, Handle<Image>>,
}

/// Marker for an inventory slot
#[derive(Component)]
pub struct InventorySlot {
    pub index: usize,
}

/// Marker for slot item icon
#[derive(Component)]
pub struct SlotIcon {
    pub index: usize,
}

/// Marker for slot item icon image
#[derive(Component)]
pub struct SlotIconImage {
    pub index: usize,
}

/// Marker for slot quantity text
#[derive(Component)]
pub struct SlotQuantity {
    pub index: usize,
}

/// Marker for a chest slot (as opposed to player inventory slot)
#[derive(Component)]
pub struct ChestSlot {
    pub index: usize,
}

/// Marker for chest slot icon
#[derive(Component)]
pub struct ChestSlotIcon {
    pub index: usize,
}

/// Marker for chest slot icon image
#[derive(Component)]
pub struct ChestSlotIconImage {
    pub index: usize,
}

/// Marker for chest slot quantity text
#[derive(Component)]
pub struct ChestSlotQuantity {
    pub index: usize,
}

/// Marker for the chest panel (so we can despawn it separately)
#[derive(Component)]
pub struct ChestPanel;

/// While dragging: which slot we started from + a floating icon under the cursor
#[derive(Resource, Default)]
pub struct DragState {
    pub dragging: bool,
    pub from_slot: Option<usize>,
    pub from_chest: bool, // true if dragging from chest, false if from inventory
    pub stack: Option<ItemStack>,
    pub icon_entity: Option<Entity>,
}

/// Marker for the floating drag icon UI
#[derive(Component)]
pub struct DragIcon;

/// Marker for drag icon image
#[derive(Component)]
pub struct DragIconImage;

#[derive(SystemParam)]
struct DragIconUi<'w, 's> {
    nodes: Query<'w, 's, &'static mut Node, With<DragIcon>>,
    bg: Query<'w, 's, &'static mut BackgroundColor, With<DragIcon>>,
    text: Query<'w, 's, &'static mut Text, With<DragIcon>>,
    images: Query<'w, 's, (&'static mut ImageNode, &'static mut Visibility), With<DragIconImage>>,
}

/// Inventory slot colors
const SLOT_NORMAL: Color = Color::srgba(0.15, 0.12, 0.10, 0.9);
const SLOT_HOVERED: Color = Color::srgba(0.25, 0.20, 0.15, 0.95);
const SLOT_EMPTY: Color = Color::srgba(0.10, 0.08, 0.06, 0.7);
const SLOT_BORDER: Color = Color::srgba(0.4, 0.3, 0.2, 0.8);
const HOTBAR_BORDER: Color = Color::srgba(0.55, 0.45, 0.25, 0.9);
const DRAG_ICON_SIZE: f32 = 46.0;
const DRAG_ICON_HALF: f32 = DRAG_ICON_SIZE * 0.5;
