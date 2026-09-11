//! Building footprint and collider metadata.
//!
//! Defines building types, their resource costs, footprints, and terrain modification parameters.

mod defs;
mod field_claims;
mod footprint;
pub mod tavern;
mod zones;

pub use defs::*;
pub use field_claims::*;
pub use footprint::*;
pub use zones::*;

#[cfg(test)]
mod tests;
