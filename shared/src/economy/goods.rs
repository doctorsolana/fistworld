//! Good identities, food categories, bulk units and public trade tiers.

use serde::{Deserialize, Serialize};

/// Physical bulk goods in the settlement economy.
///
/// Coin is intentionally absent. It has no cargo bulk, is not consumed by a
/// building, and belongs in a wallet/ledger rather than in the same arithmetic
/// as logs and grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Good {
    Food,
    Wheat,
    Wood,
    Stone,
    Iron,
    /// Wheat milled into a household-ready baking ingredient. One Flour can
    /// become one basic ration in a cabin, but is not served directly at the
    /// Moot commons.
    Flour,
    /// Prepared bakery bread. Two Flour become four Bread, making this the
    /// first higher-efficiency (tier-two) food rather than a cosmetic rename.
    Bread,
    /// Ready-to-cook livestock food. One unit is one household ration; taverns
    /// may later turn it into a higher-value prepared meal.
    Meat,
    /// Raw fleece from livestock. It is deliberately not edible and is the
    /// first input reserved for the future spinner/weaver clothing chain.
    Wool,
}

/// Civic infrastructure required before a good may enter a settlement's
/// public order book. Physical ownership and private company transfers remain
/// legal at every tier; this controls only Hall/Marketplace trade.
///
/// The empty level-one rung is intentional with today's resource set. Future
/// crafted goods can opt into the ordinary Marketplace without changing save
/// data or scattering building checks throughout the economy.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[repr(u8)]
pub enum MarketTradeTier {
    #[default]
    Moot,
    Marketplace,
    PavedMarketplace,
}

impl MarketTradeTier {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Moot => "MOOT EXCHANGE",
            Self::Marketplace => "MARKETPLACE LEVEL 1",
            Self::PavedMarketplace => "MARKETPLACE LEVEL 2",
        }
    }

    pub const fn requirement_label(self) -> &'static str {
        match self {
            Self::Moot => "the Moot",
            Self::Marketplace => "a level 1 Marketplace",
            Self::PavedMarketplace => "a level 2 paved Marketplace",
        }
    }
}

impl Good {
    /// Existing discriminants stay in their original order for replicated and
    /// saved data; new goods are appended.
    pub const ALL: [Self; 9] = [
        Self::Food,
        Self::Wheat,
        Self::Wood,
        Self::Stone,
        Self::Iron,
        Self::Flour,
        Self::Bread,
        Self::Meat,
        Self::Wool,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// Household pantries consume better prepared food first. Flour is last:
    /// it becomes an ordinary ration only through home baking.
    pub const HOUSEHOLD_FOOD_PRIORITY: [Self; 4] =
        [Self::Bread, Self::Meat, Self::Food, Self::Flour];

    /// Food which can be handed to an unhoused resident and eaten in the Moot
    /// commons. Flour deliberately is not on this list.
    pub const READY_TO_EAT_PRIORITY: [Self; 3] = [Self::Bread, Self::Meat, Self::Food];

    /// Physical inputs a future Tavern may procure. Wheat remains the founding
    /// ale grain even though raw Wheat is not a household ration.
    pub const TAVERN_INPUTS: [Self; 3] = [Self::Meat, Self::Bread, Self::Wheat];

    /// Minimum public exchange able to list and clear this good. All current
    /// founding resources remain Moot-tradeable; Iron is the first specialist
    /// commodity reserved for a level-two Marketplace.
    pub const fn minimum_market_tier(self) -> MarketTradeTier {
        match self {
            Self::Iron => MarketTradeTier::PavedMarketplace,
            Self::Food
            | Self::Wheat
            | Self::Wood
            | Self::Stone
            | Self::Flour
            | Self::Bread
            | Self::Meat
            | Self::Wool => MarketTradeTier::Moot,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            // The original discriminant remains `Food` for save/network
            // stability, but its only physical producer is fishing.
            Self::Food => "Fish",
            Self::Wheat => "Wheat",
            Self::Wood => "Wood",
            Self::Stone => "Stone",
            Self::Iron => "Iron",
            Self::Flour => "Flour",
            Self::Bread => "Bread",
            Self::Meat => "Meat",
            Self::Wool => "Wool",
        }
    }

    /// Whether one unit can satisfy one resident's daily food ration.
    ///
    /// Flour is edible only through a household pantry; direct meal services
    /// must additionally require [`Self::is_ready_to_eat`]. Raw Wheat is never
    /// food after the milling chain was introduced.
    pub const fn is_edible(self) -> bool {
        matches!(self, Self::Food | Self::Flour | Self::Bread | Self::Meat)
    }

    pub const fn is_ready_to_eat(self) -> bool {
        matches!(self, Self::Food | Self::Bread | Self::Meat)
    }

    /// Bread is the first tier-two food. The tier is inspectable now and can
    /// later feed preferences, health and migration without identifying foods
    /// by display string.
    pub const fn food_tier(self) -> u8 {
        match self {
            Self::Food | Self::Flour | Self::Meat => 1,
            Self::Bread => 2,
            Self::Wheat | Self::Wood | Self::Stone | Self::Iron | Self::Wool => 0,
        }
    }

    /// How much physical capacity one unit occupies.
    ///
    /// Quantities are gameplay units (a ration, a tied wood bundle, a dressed
    /// stone block, an iron billet), not kilograms. The relative bulk is what
    /// makes a person able to carry many meals but only a few bundles of wood.
    pub const fn bulk_per_unit(self) -> u32 {
        match self {
            Self::Food => 1,
            Self::Wheat => 2,
            Self::Wood => 4,
            Self::Stone => 6,
            Self::Iron => 3,
            Self::Flour | Self::Bread => 1,
            Self::Meat => 1,
            Self::Wool => 2,
        }
    }

    /// Reference value before local stock pressure and liquidity risk.
    pub const fn base_price(self) -> u64 {
        match self {
            Self::Food => 100,
            Self::Wheat => 80,
            // A founding villager's ten coins must be enough to buy the ten
            // bundles for a modest cabin on a reasonably stocked market. The
            // scarcity curve can still make timber dear in a true shortage.
            Self::Wood => 50,
            Self::Stone => 250,
            Self::Iron => 400,
            Self::Flour => 120,
            Self::Bread => 180,
            Self::Meat => 140,
            Self::Wool => 90,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Food => 0,
            Self::Wheat => 1,
            Self::Wood => 2,
            Self::Stone => 3,
            Self::Iron => 4,
            Self::Flour => 5,
            Self::Bread => 6,
            Self::Meat => 7,
            Self::Wool => 8,
        }
    }
}
