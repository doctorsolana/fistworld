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
pub mod bridges;
pub mod dev;
pub(crate) mod economic_accounting;
pub mod fortifications;
pub mod household_yards;
pub mod house_upgrades;
pub mod identity;
pub mod immigration;
pub mod map_state;
pub mod navgrid;
pub(crate) mod new_world;
pub mod pathfinding;
pub mod regions;
pub(crate) mod regional_roads;
pub(crate) mod ports;
pub(crate) mod shipping;
pub mod settlement_development;
pub mod settlement_directory;
pub mod simulation_time;
pub(crate) mod start_config;
pub mod time;
pub mod village;
#[cfg(test)]
pub mod village_lab;
pub(crate) mod village_lab_scenario;
pub mod village_roads;

pub mod army_lab;

pub mod wildlife;

pub(crate) mod farm_boundaries;
