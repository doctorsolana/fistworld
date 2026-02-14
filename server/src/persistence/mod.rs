//! Persistence domain.
//!
//! Responsibilities:
//! - Player profile storage/load/save.
//! - Periodic autosave orchestration.
//!
//! Dependency notes:
//! - May depend on `shared`.
//! - Should expose storage APIs/resources to other domains, not direct file calls.

pub mod autosave;
pub mod io_queue;
pub mod profiles;
