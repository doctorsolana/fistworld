//! Server-owned wildlife. Identity survives observation changes for the running
//! world's lifetime; only the bounded observed subset wanders. See WILDLIFE.md.
mod behavior;
mod placement;
mod population;
#[cfg(test)]
mod tests;

pub use behavior::{tick, update_observation};
pub use placement::spawn_checked;
pub(crate) use placement::{
    allocate_id, clear, grounded, now, set_activity, ActiveWildHorse, WildHorse, MAX_ACTIVE_HORSES,
    OBSERVATION_RADIUS,
};
pub use population::populate;
