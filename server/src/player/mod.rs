//! Player domain.
//!
//! Responsibilities:
//! - Player spawn/setup and roster responses.
//! - Fixed-tick movement simulation.
//! - Death/respawn lifecycle.
//!
//! Dependency notes:
//! - May depend on `shared`, `net`, and persistence resources.
//! - Should avoid direct dependency on combat/collision internals.

pub mod commander;
pub mod hero;
pub mod index;
pub mod roster;
pub mod roster_cache;
pub mod spawn;
