//! Shared ECS components used by both server and client.

mod actors;
mod health;
mod village_roads;
mod world;

pub use actors::*;
pub use health::*;
pub use village_roads::*;
pub use world::*;
