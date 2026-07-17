//! AI domain.
//!
//! Responsibilities:
//! - NPC spawn/bootstrap.
//! - Obstacle grid synchronization.
//! - NPC behavior tick/pathfinding.
//! - Dead-NPC cleanup.
//!
//! Dependency notes:
//! - May depend on `shared`, `world`, and collision query data.
//! - Should avoid depending on app wiring or UI-level concerns.

pub mod death_cleanup;
pub mod identity;
pub mod obstacles;
pub mod pathfinding;
pub mod ragdoll;
pub mod relevance;
pub mod spawn;
pub mod state;
pub mod tick;
