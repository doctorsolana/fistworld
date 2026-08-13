//! Persistence domain.
//!
//! Responsibilities:
//! - In-process reconnect snapshots for session accounts.
//! - Legacy profile migration helpers kept outside the live hot path.
//!
//! Dependency notes:
//! - May depend on `shared`.
//! - Should expose storage APIs/resources to other domains, not direct file calls.

pub mod autosave;
pub mod profiles;
