//! Bounded physical storage, lossless transfers and carried-load presentation.

use super::Good;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The authored object shown in a character's arms.
///
/// This is deliberately separate from [`Good`]. Accounting can keep one Food
/// category while a fisherman carries a fish basket and a baker carries bread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CarriedAppearance {
    FishBasket,
    WheatSheaf,
    WoodBundle,
    StoneBundle,
    IronBundle,
    FlourSack,
    BreadBasket,
    WoolFleece,
    MeatHaunch,
}

impl CarriedAppearance {
    /// Every authored carried-resource appearance. Keep runtime asset tests
    /// exhaustive by iterating this list instead of maintaining a second one.
    pub const ALL: [Self; 9] = [
        Self::FishBasket,
        Self::WheatSheaf,
        Self::WoodBundle,
        Self::StoneBundle,
        Self::IronBundle,
        Self::FlourSack,
        Self::BreadBasket,
        Self::WoolFleece,
        Self::MeatHaunch,
    ];

    /// Default appearance when the producing routine has no more specific
    /// presentation. Food currently comes only from fishing, so its honest
    /// first appearance is a fish basket rather than a generic crate.
    pub const fn default_for(good: Good) -> Self {
        match good {
            Good::Food => Self::FishBasket,
            Good::Wheat => Self::WheatSheaf,
            Good::Wood => Self::WoodBundle,
            Good::Stone => Self::StoneBundle,
            Good::Iron => Self::IronBundle,
            Good::Flour => Self::FlourSack,
            Good::Bread => Self::BreadBasket,
            Good::Meat => Self::MeatHaunch,
            Good::Wool => Self::WoolFleece,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::FishBasket => "Fish basket",
            Self::WheatSheaf => "Wheat sheaf",
            Self::WoodBundle => "Wood bundle",
            Self::StoneBundle => "Stone bundle",
            Self::IronBundle => "Iron bundle",
            Self::FlourSack => "Flour sack",
            Self::BreadBasket => "Bread basket",
            Self::WoolFleece => "Wool bale",
            Self::MeatHaunch => "Haunch of meat",
        }
    }
}

/// Small replicated summary of what is visibly in a character's arms.
///
/// The authoritative quantities remain in [`GoodsInventory`]. This component
/// exists so a client can select a carry animation and prop without receiving
/// every private inventory slot whenever one amount changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CarriedLoad {
    pub good: Option<Good>,
    pub amount: u32,
    /// Visual presentation is not the accounting category. `None` remains
    /// valid for old saves/packets and falls back through `default_for`.
    #[serde(default)]
    pub appearance: Option<CarriedAppearance>,
}

impl CarriedLoad {
    pub fn from_inventory(inventory: &GoodsInventory) -> Self {
        let good = Good::ALL
            .into_iter()
            .find(|good| inventory.amount(*good) > 0);
        Self {
            good,
            amount: good.map(|good| inventory.amount(good)).unwrap_or(0),
            appearance: good.map(CarriedAppearance::default_for),
        }
    }

    pub const fn is_empty(self) -> bool {
        self.amount == 0 || self.good.is_none()
    }

    pub const fn visible_appearance(self) -> Option<CarriedAppearance> {
        if self.is_empty() {
            None
        } else if let Some(appearance) = self.appearance {
            Some(appearance)
        } else if let Some(good) = self.good {
            Some(CarriedAppearance::default_for(good))
        } else {
            None
        }
    }
}

/// Replicated presentation state for an embodied porter trip.
///
/// The server's [`GoodsInventory`] remains the cargo authority. Presence says
/// the character is currently hauling the handcart (including an empty
/// outbound leg); `load_slots` is the bounded 0..=2 visual summary consumed by
/// the two authored `Anchor_Load.*` nodes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PorterCartState {
    pub load_slots: u8,
}

impl PorterCartState {
    pub const fn for_used_bulk(used_bulk: u32) -> Self {
        Self {
            load_slots: if used_bulk == 0 {
                0
            } else if used_bulk <= capacity::PORTER / 2 {
                1
            } else {
                2
            },
        }
    }
}

/// Server-owned bounded storage for bulk goods.
///
/// Carried loads, workplaces and houses share one physical bulk allowance.
/// Public market stores can instead opt into equal independent compartments:
/// a full Wood bay then cannot consume the space reserved for Bread or Flour.
/// The fixed arrays keep the strategic tick cheap and serialization stable.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoodsInventory {
    amounts: [u32; Good::COUNT],
    bulk_capacity: u32,
    #[serde(default)]
    partition_bulk_capacity: Option<u32>,
}

impl GoodsInventory {
    pub const fn new(bulk_capacity: u32) -> Self {
        Self {
            amounts: [0; Good::COUNT],
            bulk_capacity,
            partition_bulk_capacity: None,
        }
    }

    /// Build storage with the same independent bulk allowance for every good.
    /// `partition_bulk_capacity` is therefore the capacity of each named bay,
    /// not a total shared between unlike resources.
    pub const fn new_partitioned(partition_bulk_capacity: u32) -> Self {
        Self {
            amounts: [0; Good::COUNT],
            bulk_capacity: partition_bulk_capacity.saturating_mul(Good::COUNT as u32),
            partition_bulk_capacity: Some(partition_bulk_capacity),
        }
    }

    pub const fn bulk_capacity(&self) -> u32 {
        self.bulk_capacity
    }

    pub const fn partition_bulk_capacity(&self) -> Option<u32> {
        self.partition_bulk_capacity
    }

    pub fn bulk_capacity_for(&self, good: Good) -> u32 {
        self.partition_bulk_capacity.unwrap_or_else(|| {
            self.amount(good).saturating_mul(good.bulk_per_unit()) + self.free_bulk()
        })
    }

    /// Resize physical storage without ever deleting goods. A requested
    /// shrink stops at the currently occupied bulk; callers can retry after
    /// the inventory is unloaded.
    pub fn resize_bulk_capacity(&mut self, requested: u32) {
        self.bulk_capacity = requested.max(self.used_bulk());
        self.partition_bulk_capacity = None;
    }

    /// Convert to, or resize, equal per-good compartments without deleting an
    /// over-capacity legacy stack. A later resize can finish shrinking once
    /// that particular good has been removed.
    pub fn resize_partitioned_bulk_capacity(&mut self, requested_per_good: u32) {
        let occupied_high_water = Good::ALL
            .iter()
            .map(|good| self.amount(*good).saturating_mul(good.bulk_per_unit()))
            .max()
            .unwrap_or(0);
        let per_good = requested_per_good.max(occupied_high_water);
        self.partition_bulk_capacity = Some(per_good);
        self.bulk_capacity = per_good.saturating_mul(Good::COUNT as u32);
    }

    pub fn amount(&self, good: Good) -> u32 {
        self.amounts[good.index()]
    }

    pub fn used_bulk(&self) -> u32 {
        Good::ALL
            .iter()
            .map(|good| self.amount(*good).saturating_mul(good.bulk_per_unit()))
            .fold(0, u32::saturating_add)
    }

    pub fn free_bulk(&self) -> u32 {
        if self.partition_bulk_capacity.is_some() {
            Good::ALL
                .iter()
                .map(|good| self.free_bulk_for(*good))
                .fold(0, u32::saturating_add)
        } else {
            self.bulk_capacity.saturating_sub(self.used_bulk())
        }
    }

    /// Remaining bulk which can accept this specific good. For ordinary
    /// inventories this is the shared remainder; for a public market it is
    /// only the named resource's compartment.
    pub fn free_bulk_for(&self, good: Good) -> u32 {
        self.partition_bulk_capacity.map_or_else(
            || self.bulk_capacity.saturating_sub(self.used_bulk()),
            |capacity| {
                capacity.saturating_sub(self.amount(good).saturating_mul(good.bulk_per_unit()))
            },
        )
    }

    pub fn free_units(&self, good: Good) -> u32 {
        self.free_bulk_for(good) / good.bulk_per_unit()
    }

    pub fn is_empty(&self) -> bool {
        self.amounts.iter().all(|amount| *amount == 0)
    }

    /// Household-edible portions currently stored here. Flour represents one
    /// basic ration baked at home; raw Wheat is intentionally excluded.
    pub fn edible_amount(&self) -> u32 {
        Good::HOUSEHOLD_FOOD_PRIORITY
            .iter()
            .map(|good| self.amount(*good))
            .fold(0, u32::saturating_add)
    }

    /// Ready meals which can be eaten without a household kitchen.
    pub fn ready_to_eat_amount(&self) -> u32 {
        Good::READY_TO_EAT_PRIORITY
            .iter()
            .map(|good| self.amount(*good))
            .fold(0, u32::saturating_add)
    }

    /// Consume up to `requested` household portions, Bread before fish and
    /// home-baked Flour.
    pub fn remove_edible(&mut self, requested: u32) -> u32 {
        let mut removed = 0u32;
        for good in Good::HOUSEHOLD_FOOD_PRIORITY {
            removed = removed.saturating_add(self.remove(good, requested.saturating_sub(removed)));
            if removed == requested {
                break;
            }
        }
        removed
    }

    /// Add up to `requested` units and return the amount accepted.
    pub fn add(&mut self, good: Good, requested: u32) -> u32 {
        let accepted = requested.min(self.free_units(good));
        self.amounts[good.index()] = self.amount(good).saturating_add(accepted);
        accepted
    }

    /// Remove up to `requested` units and return the amount removed.
    pub fn remove(&mut self, good: Good, requested: u32) -> u32 {
        let removed = requested.min(self.amount(good));
        self.amounts[good.index()] -= removed;
        removed
    }

    /// Move goods into another inventory, partially if the destination fills.
    ///
    /// Returns the amount moved. Goods are removed only after the destination
    /// accepts them, so a full store can never destroy a worker's carried load.
    pub fn transfer_to(&mut self, destination: &mut Self, good: Good, requested: u32) -> u32 {
        let available = requested.min(self.amount(good));
        let accepted = destination.add(good, available);
        let removed = self.remove(good, accepted);
        debug_assert_eq!(accepted, removed);
        accepted
    }
}

impl Default for GoodsInventory {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Initial physical capacities, measured in [`Good::bulk_per_unit`] units.
///
/// These are tuning, not economic values. They say how much can physically be
/// present before somebody must haul it elsewhere; they say nothing about who
/// owns the contents or what they are worth. `HALL` and `MARKET` are per-good
/// compartment sizes; the remaining constants are ordinary combined stores.
pub mod capacity {
    /// Personal cargo carried by both villagers and player heroes. Sixteen
    /// bulk fits four Wood bundles (up from three) while remaining far below
    /// even the smallest workplace store.
    pub const VILLAGER: u32 = 16;
    /// Temporary work capacity for Moot Stewards and private company porters.
    /// This is the future hand-cart allowance, not a larger personal backpack.
    pub const PORTER: u32 = 96;
    pub const HOUSE: u32 = 80;
    pub const FARMSTEAD: u32 = 240;
    pub const LIVESTOCK_FARM: u32 = 300;
    pub const LUMBERJACK_HUT: u32 = 240;
    pub const FISHERMANS_HUT: u32 = 240;
    pub const MARKET: u32 = 600;
    pub const TAVERN: u32 = 180;
    pub const CHURCH: u32 = 120;
    pub const WINDMILL: u32 = 240;
    pub const BAKERY: u32 = 240;
    /// A dedicated private store is deliberately much larger than a workshop,
    /// but remains finite so logistics and additional buildings still matter.
    pub const STORAGE_HALL: u32 = 2_400;
    pub const STONE_QUARRY: u32 = 360;
    pub const HALL: u32 = 1_200;
}
