//! Paid, in-place home extensions. Household identity and the old dwelling
//! remain usable while a separate project buys, hauls and constructs the work.

pub mod decision;
mod finance;
pub mod lab;
mod labor;
mod lifecycle;
mod placement;
mod project;

#[cfg(test)]
use lifecycle::cancel_upgrade;
pub use lifecycle::{request_upgrade, run_house_upgrade_projects};
pub use project::{required_escrow_pennies, HouseUpgradeBuilderRoutine, HouseUpgradeProjects};

#[cfg(test)]
mod tests;
