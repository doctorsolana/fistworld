//! Append-only defense land reservations and paid civic construction.

mod construction;
mod geometry;
mod inspection;
mod planning;
pub use inspection::trace_defense_passages;

pub use construction::run_fortification_projects;
pub use planning::plan_settlement_defenses;

mod lab;
pub use lab::setup_defense_lab;
