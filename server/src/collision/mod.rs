//! Collision domain.
//!
//! Responsibilities:
//! - Static/derived collider resource libraries.
//! - Collider streaming near active entities.
//! - Player/NPC/vehicle vs world collision resolution.
//!
//! Dependency notes:
//! - May depend on `shared` and world/static geometry.
//! - Should not depend on protocol ingress systems.

pub mod building_index;
pub mod library;
pub mod raycast;
pub mod streaming;
