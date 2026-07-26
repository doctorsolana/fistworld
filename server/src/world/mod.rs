//! World domain.
//!
//! Responsibilities:
//! - Server world bootstrap.
//! - Replicated map-state resources.
//! - World-time progression and admin time updates.
//!
//! Dependency notes:
//! - May depend on `shared`.
//! - Should be consumed by other domains through explicit APIs/resources.

pub mod bootstrap;
pub mod map_state;
pub mod navgrid;
pub mod pathfinding;
pub mod time;
