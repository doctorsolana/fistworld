//! Weapon debug resources.

/// Debug mode resource for visualizing bullet trajectories.
#[derive(bevy::prelude::Resource, Default)]
pub struct WeaponDebugMode(pub bool);
