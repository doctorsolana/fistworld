//! Reusable private-business simulation.
//!
//! Physical trade work remains in the village `trades` and `commerce` modules.
//! This domain owns the facts which make that work an economy: exact business
//! accounts, seller receipts, purchasing rules, daily pricing, owner draws and
//! solvency. All expensive decisions happen at most once per business per
//! world day; per-tick work only appends compact events.

mod management;
mod transactions;

pub use management::review_business_management;
pub use transactions::{apply_business_events, BusinessEventQueue};
