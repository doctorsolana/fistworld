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

pub mod army;
pub mod boat;
pub mod business;
pub mod combat;
pub mod commander;
pub mod companies;
pub mod hero;
pub mod market;
pub mod permits;
pub mod roster;
pub mod spawn;
pub mod trade_routes;
