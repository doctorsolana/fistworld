//! Shared physics constants and ground-clearance helpers.
//!
//! The character controller (`step_character`) died with the FPS embodiment; what
//! survives is terrain/ground math still used by spawn placement and unit pathfinding.

mod constants;

pub use constants::*;
