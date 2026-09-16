//! Company water freight and finite harbour construction transport.
//!
//! Completed ports are access points to their town's existing market. Only
//! construction supplies have dedicated, physically hauled reservations.

pub(crate) mod crew;
mod directory;
mod logistics;
pub(crate) mod routes;
pub(crate) mod traffic;

pub(crate) use crew::advance_crew;
pub(crate) use directory::sync_maritime_directory;
pub(crate) use logistics::{
    PortHaulJob, PortHaulRequest, PortHaulRoutine, advance_port_hauls,
    cancel_port_haul, quote_haul_fee, spawn_port_haul,
};
pub(crate) use routes::advance_shipping;
