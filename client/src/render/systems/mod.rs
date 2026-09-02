//! Client-side game systems
//!
//! Organized into submodules for maintainability.

mod connection;
mod rendering;
pub use rendering::CloudCover;
mod world;

// Re-export everything for easy access from main.rs
pub use connection::*;
pub use rendering::*;
pub use world::*;
