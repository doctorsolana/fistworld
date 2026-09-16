//! Reusable private-business simulation.
//!
//! Physical trade work remains in the village `trades` and `commerce` modules.
//! This domain owns the facts which make that work an economy: exact business
//! accounts, seller receipts, purchasing rules, daily pricing, owner draws and
//! solvency. All expensive decisions happen at most once per business per
//! world day; per-tick work only appends compact events.

/// Give a newly opened site two complete days after its possibly partial first day.
pub(super) const NEW_BUSINESS_DAYS: u32 = 3;

mod management;
pub(super) mod market_response;
mod transactions;

pub use management::review_business_management;
pub use transactions::{BusinessEventQueue, apply_business_events};
