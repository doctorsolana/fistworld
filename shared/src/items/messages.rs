use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Client -> Server: Request to pick up the nearest ground item.
/// The server will find the closest item within pickup range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickupRequest;

/// Client -> Server: Request to drop an item from inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropRequest {
    /// Inventory slot index to drop from
    pub slot_index: usize,
}

/// Client -> Server: Select active hotbar slot (0..HOTBAR_SLOTS-1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectHotbarSlot {
    pub index: u8,
}

/// Client -> Server: Request to move an item stack within the inventory (server authoritative).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryMoveRequest {
    pub from: u8,
    pub to: u8,
}

/// Replicated: which hotbar slot is currently active.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct HotbarSelection {
    pub index: u8,
}

/// Client -> Server: Request to open the nearest chest.
/// Server will find closest chest within range and track it as open for this client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenChestRequest;

/// Client -> Server: Request to close the currently open chest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseChestRequest;

/// Client -> Server: Request to transfer an item between player inventory and chest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChestTransferRequest {
    /// true = chest -> player inventory, false = player inventory -> chest
    pub from_chest: bool,
    /// Source slot index
    pub from_slot: u8,
    /// Destination slot index
    pub to_slot: u8,
}
