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

pub mod building_geometry;
pub mod building_index;
pub mod geometry;
pub mod library;
pub mod raycast;
pub mod resolve_npc;
pub mod resolve_player;
pub mod resolve_vehicle;
pub mod streaming;
