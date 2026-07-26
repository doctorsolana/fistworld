//! Pickup system - detect nearby ground items and allow pickup with E key
//!
//! Shows a prompt when near items and sends PickupRequest to server.
//! Also handles 3D visuals for ground items with bobbing animation.
//! Additionally handles vehicle interaction prompts ("Press E to get on bike").

pub mod assets;
pub mod items;
pub mod prompts;
pub mod visuals;

use assets::setup_item_model_assets;
use items::item_model_scale;
use prompts::{
    cleanup_pickup_ui, detect_nearby_items, detect_nearby_vehicles, handle_pickup_input,
    show_pickup_prompt, show_vehicle_prompt,
};
use visuals::{
    animate_ground_items, cleanup_item_visuals, despawn_ground_item_visuals,
    spawn_ground_item_visuals,
};

use bevy::prelude::*;
use lightyear::prelude::client::Connected;
use lightyear::prelude::*;
use shared::components::{LocalPlayer, PlayerPosition};
use shared::items::{
    GroundItem, GroundItemPosition, ItemType, PickupRequest, PICKUP_RANGE,
    VEHICLE_INTERACTION_RANGE,
};
use shared::protocol::ReliableChannel;
use shared::vehicle::{Vehicle, VehicleDriver, VehicleState, VehicleType};

use crate::input::InputState;
use crate::states::GameState;
use std::collections::HashMap;

/// Plugin for the pickup system
pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NearbyItem>();
        app.init_resource::<NearbyVehicle>();
        app.add_systems(Startup, setup_item_model_assets);

        // Visual systems run in both Playing and Paused states (so items don't disappear when pausing)
        app.add_systems(
            Update,
            (
                spawn_ground_item_visuals,
                animate_ground_items,
                despawn_ground_item_visuals,
            )
                .chain()
                .run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))),
        );

        // Pickup interaction only in Playing state
        app.add_systems(
            Update,
            (detect_nearby_items, show_pickup_prompt, handle_pickup_input)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );

        // Vehicle interaction prompt only in Playing state
        app.add_systems(
            Update,
            (detect_nearby_vehicles, show_vehicle_prompt)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );

        // Cleanup prompt when leaving Playing (but NOT item visuals - they persist through pause)
        app.add_systems(OnExit(GameState::Playing), cleanup_pickup_ui);

        // Only cleanup item visuals when returning to main menu (actual disconnect)
        app.add_systems(OnEnter(GameState::MainMenu), cleanup_item_visuals);
    }
}

/// Resource tracking the nearest item to the player
#[derive(Resource, Default)]
pub struct NearbyItem {
    pub entity: Option<Entity>,
    pub item_type: Option<ItemType>,
    pub quantity: Option<u32>,
}

/// Marker for the pickup prompt UI
#[derive(Component)]
pub struct PickupPrompt;

// =============================================================================
// VEHICLE INTERACTION PROMPT
// =============================================================================

// VEHICLE_INTERACTION_RANGE is now imported from shared

/// Resource tracking the nearest vehicle to the player
#[derive(Resource, Default)]
pub struct NearbyVehicle {
    pub vehicle_type: Option<VehicleType>,
}

/// Marker for the vehicle interaction prompt UI
#[derive(Component)]
pub struct VehiclePrompt;

// =============================================================================
// ITEM PICKUP DETECTION
// =============================================================================

// =============================================================================
// GROUND ITEM VISUALS
// =============================================================================

/// Marker for ground item 3D visuals
#[derive(Component)]
pub struct GroundItemVisual {
    /// The GroundItem entity this visual belongs to
    pub source_entity: Entity,
    /// Animation timer for bobbing
    pub bob_timer: f32,
    /// Base Y position
    pub base_y: f32,
}

/// Scene handles for ground item models
#[derive(Resource, Clone)]
pub struct ItemModelAssets {
    pub scenes: HashMap<ItemType, Handle<Scene>>,
}
