//! Persistence domain.
//!
//! Responsibilities:
//! - In-process reconnect snapshots for session accounts.
//! - Session-scoped player account snapshots.
//!
//! Dependency notes:
//! - May depend on `shared`.
//! - Should expose account resources to other domains.

pub mod autosave;
pub mod profiles;
