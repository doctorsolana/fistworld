//! Vehicle physics system - PURE PHYSICS approach.
//!
//! No artificial limits. Physics handles everything:
//! - Can't climb walls because: steep slope = less normal force = less traction + gravity pulls back
//! - Gets air off crests because: when ground drops away, you're airborne
//! - Slides on steep slopes because: gravity component along slope > available traction

mod components;
mod physics;
mod tuning;

pub use components::*;
pub use physics::*;
pub use tuning::*;
