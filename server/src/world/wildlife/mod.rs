//! Server-owned wildlife. The bounded persistent population keeps the same
//! behavior regardless of observation. See WILDLIFE.md.
mod behavior;
mod placement;
mod population;
#[cfg(test)]
mod tests;

pub use behavior::tick;
pub use placement::spawn_checked;
pub(crate) use placement::{WildHorse, allocate_id, clear, grounded, now, set_activity};
pub use population::populate;
