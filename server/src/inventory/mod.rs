//! Inventory domain.
//!
//! Responsibilities:
//! - Hotbar selection and slot movement.
//! - Ground item pickup/drop.
//! - Chest open/close/transfer flows.
//! - Drop-on-death handling.
//!
//! Dependency notes:
//! - May depend on `shared`, `player`, and persistence-related resources.
//! - Should avoid direct dependency on net transport details.

pub mod chest;
pub mod death_drop;
pub mod ground_items;
pub mod hotbar;
pub mod test_spawns;
