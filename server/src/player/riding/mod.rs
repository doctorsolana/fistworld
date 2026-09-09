//! Server-owned mounted soldiers and riding; ambient wildlife belongs to world.
use crate::{
    player::hero::OfflineHero,
    world::wildlife::{clear, grounded, now, set_activity, WildHorse},
};
use bevy::prelude::*;
use shared::components::*;

mod cavalry;
mod commands;
pub use cavalry::equip_cavalry;
pub(crate) use cavalry::remove_equipment;
mod lifecycle;
pub use commands::order;
use commands::stop;
pub use lifecycle::tick;
#[derive(Component)]
struct DismountLanding(Vec3);
#[cfg(test)]
mod tests;
