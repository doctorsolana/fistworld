//! Weapon effects systems split by concern.

use super::*;

mod blood;
mod impacts;
mod muzzle;

pub use blood::{
    update_blood_bursts, update_blood_droplets, update_blood_ground_splats,
    update_blood_splash_rings,
};
pub use impacts::update_impact_markers;
pub use muzzle::{update_muzzle_flash, update_muzzle_smoke};

pub(crate) use blood::spawn_blood_splatter;
pub(crate) use muzzle::{spawn_muzzle_flash, spawn_muzzle_smoke};
