//! Client-side game systems
//!
//! Organized into submodules for maintainability.

mod connection;
mod debug_physics;
mod npc;
mod particles;
mod player;
mod rendering;
mod world;

// Re-export everything for easy access from main.rs
pub use connection::*;
pub use debug_physics::*;
pub use npc::*;
pub use particles::*;
pub use player::*;
pub use rendering::*;
pub use world::*;
