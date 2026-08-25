//! Shared ECS components used by both server and client.

mod actors;
mod army;
mod health;
mod identity;
mod trade;
mod village_roads;
mod world;

pub use actors::*;
pub use army::*;
pub use health::*;
pub use identity::*;
pub use trade::*;
pub use village_roads::*;
pub use world::*;
