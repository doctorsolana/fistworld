//! Replicated state for extending a home without evicting its household.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{BuildingId, HouseAppearance, PersonId};

pub const HOUSE_UPGRADE_WOOD_REQUIRED: u32 = 8;
pub const HOUSE_UPGRADE_WORK_SECONDS: f32 = 60.0;
pub const HOUSE_UPGRADE_BUILDER_FEE_PENNIES: u64 = 100;
/// Reservation ceiling, not a promise to pay more than the actual market quote.
pub const HOUSE_UPGRADE_ESCROW_PENNIES: u64 = HOUSE_UPGRADE_WOOD_REQUIRED as u64
    * crate::economy::Good::Wood.base_price()
    + HOUSE_UPGRADE_BUILDER_FEE_PENNIES;

/// Lives on a separate construction site. The original house remains usable
/// at its existing capacity until the authoritative construction completes.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct HouseUpgradeWorksite {
    pub house: BuildingId,
    pub owner: PersonId,
    pub target: HouseAppearance,
    pub wood_required: u32,
}
