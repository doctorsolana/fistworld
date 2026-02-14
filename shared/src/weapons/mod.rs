//! Weapon system - types, stats, and registry.
//!
//! Extensible weapon framework with PUBG-style ballistics.

pub mod ballistics;
pub mod constants;
pub mod damage;
pub mod debug;
pub mod offsets;
pub mod types;

pub use constants::*;
pub use debug::*;
pub use offsets::*;
pub use types::*;
