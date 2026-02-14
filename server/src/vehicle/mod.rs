//! Vehicle domain.
//!
//! Responsibilities:
//! - Vehicle interaction requests.
//! - Vehicle simulation and suspension state.
//!
//! Dependency notes:
//! - May depend on `shared` and player identity data.
//! - Should not depend on UI or client-specific concerns.

pub mod interaction;
pub mod simulation;
