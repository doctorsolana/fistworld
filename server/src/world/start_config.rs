//! Validated, server-owned settings for a new world. The immutable terrain
//! recipe is sent to clients; these settings only initialize ordinary society.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use shared::economy::{capacity, Good, GoodsInventory};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum OpeningProfile {
    #[default]
    Mature,
    Frontier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpeningStock {
    pub bread: u32,
    pub wheat: u32,
    pub wood: u32,
}

impl OpeningStock {
    pub fn goods(self) -> [(Good, u32); 3] {
        [
            (Good::Bread, self.bread),
            (Good::Wheat, self.wheat),
            (Good::Wood, self.wood),
        ]
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct WorldStartConfig {
    /// Fraction of the original world's area, not its edge length.
    pub world_area_scale: f32,
    pub opening: OpeningProfile,
    pub settlement_count: usize,
    /// Used only by Frontier; Mature derives population from each viable site.
    pub founders_per_settlement: usize,
    /// Total world-wide recurring arrival rate; None keeps the natural default.
    pub immigrants_per_day: Option<f32>,
    pub world_npc_cap: usize,
    /// None chooses a random seed. The explicit seed environment variable wins.
    pub seed: Option<u64>,
    /// None preserves the original population-scaled, finite Hall provisions.
    pub hall_stock: Option<OpeningStock>,
    pub hall_treasury_pennies: Option<u64>,
}

impl Default for WorldStartConfig {
    fn default() -> Self {
        Self {
            world_area_scale: 1.0,
            opening: OpeningProfile::Mature,
            settlement_count: 10,
            founders_per_settlement: 6,
            immigrants_per_day: None,
            world_npc_cap: 5_000,
            seed: None,
            hall_stock: None,
            hall_treasury_pennies: None,
        }
    }
}

impl WorldStartConfig {
    pub fn from_ron(source: &str) -> Result<Self, String> {
        let config: Self = ron::from_str(source).map_err(|error| error.to_string())?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_environment() -> Result<Self, String> {
        match std::env::var_os("FISTWORLD_WORLD_CONFIG") {
            Some(path) => {
                let path = std::path::PathBuf::from(path);
                let source = std::fs::read_to_string(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                Self::from_ron(&source).map_err(|error| format!("{}: {error}", path.display()))
            }
            None => Ok(Self::default()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let half_extent = self.half_extent();
        if !self.world_area_scale.is_finite()
            || self.world_area_scale <= 0.0
            || self.world_area_scale > 1.0
            || !(64.0..=shared::map::SESSION_HALF_EXTENT).contains(&half_extent)
        {
            return Err("world_area_scale must be finite and between 0.000244140625 and 1 (64–4096 m half extent)".into());
        }
        if !(1..=32).contains(&self.settlement_count) {
            return Err("settlement_count must be between 1 and 32".into());
        }
        if !(1..=128).contains(&self.founders_per_settlement) {
            return Err("founders_per_settlement must be between 1 and 128".into());
        }
        if self
            .immigrants_per_day
            .is_some_and(|rate| !rate.is_finite() || !(0.0..=20.0).contains(&rate))
        {
            return Err(
                "immigrants_per_day must be None or a finite world-wide rate between 0 and 20"
                    .into(),
            );
        }
        if !(1..=50_000).contains(&self.world_npc_cap)
            || (self.opening == OpeningProfile::Frontier
                && self.world_npc_cap < self.settlement_count * self.founders_per_settlement)
        {
            return Err(
                "world_npc_cap must be between 1 and 50000 and accommodate the Frontier founders"
                    .into(),
            );
        }
        if self
            .hall_treasury_pennies
            .is_some_and(|cash| cash > 1_000_000_000)
        {
            return Err("hall_treasury_pennies must not exceed 1000000000".into());
        }
        if let Some(stock) = self.hall_stock {
            let mut inventory = GoodsInventory::new_partitioned(capacity::HALL);
            for (good, amount) in stock.goods() {
                if inventory.add(good, amount) != amount {
                    return Err(format!(
                        "hall_stock exceeds the Hall's finite {good:?} storage"
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn half_extent(&self) -> f32 {
        shared::map::SESSION_HALF_EXTENT * self.world_area_scale.sqrt()
    }

    pub fn recipe(&self, seed: u64) -> shared::worldgen::GeneratedWorld {
        let mut recipe = shared::map::new_world_recipe(seed);
        recipe.half_extent = self.half_extent();
        recipe
    }

    pub fn minimum_settlements(&self) -> usize {
        match self.opening {
            // Preserve the previous mature ten-target/eight-minimum behavior.
            OpeningProfile::Mature => self.settlement_count.min(8),
            OpeningProfile::Frontier => self.settlement_count,
        }
    }

    pub fn opening_stock(&self, population: usize) -> OpeningStock {
        self.hall_stock.unwrap_or(OpeningStock {
            bread: population as u32 * 2,
            wheat: population as u32,
            wood: 12,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintained_presets_preserve_mature_and_define_an_exact_small_frontier() {
        let mature =
            WorldStartConfig::from_ron(include_str!("../../../config/worlds/mature.ron")).unwrap();
        assert_eq!(mature, WorldStartConfig::default());
        assert_eq!(mature.minimum_settlements(), 8);
        let small =
            WorldStartConfig::from_ron(include_str!("../../../config/worlds/small-frontier.ron"))
                .unwrap();
        assert_eq!(small.minimum_settlements(), 4);
        assert_eq!(small.settlement_count * small.founders_per_settlement, 24);
        assert_eq!(small.opening, OpeningProfile::Frontier);
        assert_eq!(small.immigrants_per_day, Some(3.0));
        let relative_area = (small.half_extent() / mature.half_extent()).powi(2);
        assert!((relative_area - 0.2).abs() < 0.000001);
        assert!(small.seed.is_none());
        let a = small.recipe(71);
        let b = small.recipe(71);
        assert_eq!(ron::to_string(&a).unwrap(), ron::to_string(&b).unwrap());
    }

    #[test]
    fn invalid_or_misspelled_settings_fail_instead_of_silently_changing_the_world() {
        for source in [
            "(world_area_scale:0.0)",
            "(world_area_scale:-0.5)",
            "(world_area_scale:1.1)",
            "(settlement_count:0)",
            "(settlement_count:33)",
            "(founders_per_settlement:129)",
            "(opening:Frontier,settlement_count:4,founders_per_settlement:6,world_npc_cap:23)",
            "(immigrants_per_day:Some(-1.0))",
            "(immigrants_per_day:Some(21.0))",
            "(world_area_size:0.2)",
            "(hall_stock:Some((bread:4294967295,wheat:0,wood:0)))",
        ] {
            assert!(
                WorldStartConfig::from_ron(source).is_err(),
                "accepted {source}"
            );
        }
        for scale in [f32::NAN, f32::INFINITY] {
            assert!(WorldStartConfig {
                world_area_scale: scale,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(WorldStartConfig::from_ron("(immigrants_per_day:Some(0.0))").is_ok());
    }
}
