//! items systems.

use super::*;

pub(super) fn item_model_scale(item_type: ItemType) -> f32 {
    match item_type {
        ItemType::RifleAmmo => 0.4,
        ItemType::SniperRounds => 0.45,
        ItemType::ShotgunShells => 0.5,
        ItemType::Stone => 0.6,
        ItemType::Wood => 0.65,
        ItemType::GoldCoin => 0.35,
        _ => 1.0,
    }
}
