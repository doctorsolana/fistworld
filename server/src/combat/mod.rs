//! Combat domain.
//!
//! Responsibilities:
//! - Weapon fire/reload request handling.
//! - Projectile simulation.
//! - Character/world hit resolution.
//! - Combat cleanup.
//!
//! Dependency notes:
//! - May depend on `shared`, `collision`, `inventory`, and `player`.
//! - Should avoid depending on app wiring concerns.

pub mod bullet_sim;
pub mod cleanup;
pub mod fire;
pub mod geometry;
pub mod hit_characters;
pub mod hit_world;
pub mod melee;
pub mod reload;
pub mod target_index;
