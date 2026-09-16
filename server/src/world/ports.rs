//! Public waterfront infrastructure and company-funded hull construction.
//!
//! A finished port is an access point to its town's existing MootMarket, never
//! a second exchange. Only construction uses reserved, physically hauled piles.

mod construction;
mod funding;
pub(crate) mod lab;
mod lab_sites;
mod orders;
mod recovery;
mod siting;

pub(crate) use construction::{PortBuilder, PortWorkProject, advance_port_projects};
pub(crate) use orders::{advance_ship_orders, cancel_ship_order, order_ship};
pub(crate) use siting::{PortDevelopment, review_public_ports};

pub(crate) const PORT_MATERIALS: [(shared::economy::Good, u32); 2] = [
    (shared::economy::Good::Wood, 48),
    (shared::economy::Good::Stone, 16),
];
pub(crate) const PORT_BUILD_SECONDS: f32 = 180.0;
