//! Player domain.
//!
//! Responsibilities:
//! - Account command authority, commander views, heroes, boats and rosters.
//! - Authoritative movement, battalion formations and melee orders.
//! - Player-directed trade, companies, permits and construction.
//!
//! Dependency notes:
//! - Uses shared contracts, connection identity and session account resources.
//! - Movement and attacks use the world's navigation/collision proofs; economic
//!   commands use village ownership and accounting rules.

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
