//! Client-side game systems
//!
//! Organized into submodules for maintainability.

mod connection;
mod particles;
mod rendering;
mod world;

// Re-export everything for easy access from main.rs
pub use connection::*;
pub use particles::*;
pub use rendering::*;
pub use world::*;
