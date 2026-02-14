//! Curated ergonomic imports for gameplay/runtime code.
//!
//! Prefer explicit domain imports for new code. This prelude is a convenience layer.

pub use crate::building::{BuildingPosition, BuildingType, PlacedBuilding};
pub use crate::components::{
    EquippedWeapon, Health, LocalPlayer, Npc, NpcPosition, Player, PlayerPosition, PlayerRotation,
    PlayerVelocity,
};
pub use crate::items::{ChestStorage, HotbarSelection, Inventory, ItemStack, ItemType};
pub use crate::physics::{ground_clearance_center, step_character};
pub use crate::protocol::{
    NameRejectionReason, NameSubmissionResult, ProtocolPlugin, ReliableChannel,
};
pub use crate::terrain::{ChunkCoord, TerrainGenerator, WorldTerrain, CHUNK_SIZE, WORLD_SEED};
pub use crate::vehicle::{
    can_interact_with_vehicle, vehicle_def, InVehicle, Vehicle, VehicleDriver, VehicleState,
    VehicleType,
};
pub use crate::weapons::{WeaponStats, WeaponType};
