//! One-time founding decisions, using the ordinary businesses' real rates.
//!
//! These are investment standards for an established starting community, not
//! new restrictions on player permits or multipliers on later production.

use crate::world::village::{processing_recipe, rated_daily_production};
use shared::components::SettlementBuildingKind as Kind;

pub(super) const fn minimum_quality(kind: Kind) -> f32 {
    match kind {
        Kind::Farmstead => 0.35,
        Kind::LivestockFarm => 0.65,
        Kind::LumberjackHut => 0.45,
        Kind::StoneQuarry => 0.30,
        Kind::FishermansHut => 0.45,
        _ => 0.0,
    }
}

pub(super) fn qualifies(kind: Kind, quality: f32) -> bool {
    quality.is_finite() && quality >= minimum_quality(kind)
}

#[derive(Default, Debug, Clone, Copy)]
pub(super) struct FoodCapacity {
    wheat: u32,
    mill_input: u32,
    flour: u32,
    bakery_input: u32,
    bread: u32,
    direct: u32,
}

impl FoodCapacity {
    pub(super) fn from_sites(sites: impl IntoIterator<Item = (Kind, f32)>) -> Self {
        let mut capacity = Self::default();
        for (kind, quality) in sites {
            let Some(rated) = rated_daily_production(kind, quality) else {
                continue;
            };
            match kind {
                Kind::Farmstead => capacity.wheat += rated.output_units,
                Kind::Windmill => {
                    capacity.mill_input += rated.input_units();
                    capacity.flour += rated.output_units;
                }
                Kind::Bakery => {
                    capacity.bakery_input += rated.input_units();
                    capacity.bread += rated.output_units;
                }
                Kind::FishermansHut | Kind::LivestockFarm => {
                    capacity.direct += rated.output_units;
                }
                _ => {}
            }
        }
        capacity
    }

    fn milled(self) -> u32 {
        let recipe = processing_recipe(Kind::Windmill).expect("grain mill recipe");
        (self.wheat.min(self.mill_input) / recipe.input_units * recipe.output_units).min(self.flour)
    }

    pub(super) fn rations(self) -> u32 {
        let flour = self.milled();
        // Real bakery batches consume two Flour and return four Bread. The
        // unused Flour remains edible through household baking; raw Wheat is
        // never a ration and installed mills with no Wheat create nothing.
        let recipe = processing_recipe(Kind::Bakery).expect("bread recipe");
        let cycles = (flour.min(self.bakery_input) / recipe.input_units)
            .min(self.bread / recipe.output_units);
        self.direct + cycles * recipe.output_units + flour - cycles * recipe.input_units
    }

    pub(super) fn sustains(self, population: usize) -> bool {
        // Rated shifts omit journeys, temporary vacancies and collection.
        // Leave a modest 20% capacity margin; finite opening stock is not
        // counted. The aggregate soak checks the resulting actual economy.
        u64::from(self.rations()) * 5 >= population as u64 * 6
    }

    pub(super) fn next_grain_workplace(self) -> Kind {
        if self.wheat > self.mill_input {
            Kind::Windmill
        } else if self.milled() > self.bakery_input {
            Kind::Bakery
        } else {
            Kind::Farmstead
        }
    }
}

/// Seeded household/business history affects which viable secondary trades
/// have opened already. It never fabricates the resource or its legal plot.
pub(super) fn opportunity_roll(salt: u64, kind: Kind) -> f32 {
    let discriminator = match kind {
        Kind::LivestockFarm => 0x5041_5354_5552_45,
        Kind::LumberjackHut => 0x5449_4D42_4552,
        Kind::StoneQuarry => 0x5354_4F4E_45,
        Kind::StorageHall => 0x5354_4F52_45,
        Kind::Tavern => 0x5441_5645_524E,
        _ => 0,
    };
    let mut rng = shared::rng::XorShift64::new(salt ^ discriminator);
    (rng.next_u64() & 65535) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_capacity_requires_real_inputs_and_honours_processing_bottlenecks() {
        let capacity = |sites: &[(Kind, f32)]| FoodCapacity::from_sites(sites.iter().copied());
        assert_eq!(capacity(&[(Kind::Farmstead, 1.0)]).rations(), 0);
        assert_eq!(
            capacity(&[(Kind::Windmill, 0.5), (Kind::Bakery, 0.5)]).rations(),
            0
        );
        let one = capacity(&[
            (Kind::Farmstead, 1.0),
            (Kind::Windmill, 0.5),
            (Kind::Bakery, 0.5),
        ]);
        let poor = capacity(&[
            (Kind::Farmstead, 0.35),
            (Kind::Windmill, 0.5),
            (Kind::Bakery, 0.5),
        ]);
        assert!(one.rations() > poor.rations());
        assert!(one.sustains(12));
        assert!(!poor.sustains(12));
        let mut sites = vec![(Kind::Farmstead, 1.0); 5];
        sites.extend([(Kind::Windmill, 0.5), (Kind::Bakery, 0.5)]);
        let limited = capacity(&sites);
        let mill = rated_daily_production(Kind::Windmill, 0.5).unwrap();
        assert_eq!(limited.rations(), mill.output_units * 2);
        assert_eq!(limited.next_grain_workplace(), Kind::Windmill);
    }

    #[test]
    fn coastal_and_pasture_food_use_actual_rates_without_a_cosmetic_grain_chain() {
        let sites = [(Kind::FishermansHut, 0.9), (Kind::LivestockFarm, 0.8)];
        let expected: u32 = sites
            .into_iter()
            .map(|(kind, quality)| rated_daily_production(kind, quality).unwrap().output_units)
            .sum();
        let capacity = FoodCapacity::from_sites(sites);
        assert_eq!(capacity.rations(), expected);
        assert_eq!(capacity.next_grain_workplace(), Kind::Farmstead);
    }

    #[test]
    fn founding_investors_do_not_confuse_meadow_stone_with_a_workable_quarry() {
        use shared::worldgen::ResourceProfile;
        let meadow = ResourceProfile {
            wood: 0.25,
            stone: 0.10,
            iron: 0.02,
            farmland: 0.9,
        };
        assert!(!qualifies(
            Kind::StoneQuarry,
            Kind::StoneQuarry.yield_quality(&meadow)
        ));
        assert!(!qualifies(
            Kind::LumberjackHut,
            Kind::LumberjackHut.yield_quality(&meadow)
        ));
        assert!(qualifies(
            Kind::Farmstead,
            Kind::Farmstead.yield_quality(&meadow)
        ));
        assert!(qualifies(Kind::StoneQuarry, 0.48));
        assert!(qualifies(Kind::LumberjackHut, 0.8));
        assert!(!qualifies(Kind::StoneQuarry, f32::NAN));
        // A mill is a processor, so neither stone nor farmland changes its
        // yield. Its ordinary recipe still explicitly consumes Wheat.
        assert_eq!(Kind::Windmill.yield_quality(&meadow), 0.5);
        assert_eq!(
            rated_daily_production(Kind::Windmill, 0.5)
                .unwrap()
                .input
                .unwrap()
                .0,
            shared::economy::Good::Wheat
        );
    }

    #[test]
    fn secondary_opportunities_are_seeded_and_vary_independently() {
        let mut pastoral = 0;
        let mut timber = 0;
        for seed in 0..64 {
            pastoral += usize::from(opportunity_roll(seed, Kind::LivestockFarm) < 0.55);
            timber += usize::from(opportunity_roll(seed, Kind::LumberjackHut) < 0.8);
            assert_eq!(
                opportunity_roll(seed, Kind::LivestockFarm),
                opportunity_roll(seed, Kind::LivestockFarm)
            );
        }
        assert!((5..60).contains(&pastoral));
        assert!((5..64).contains(&timber));
        assert_ne!(
            opportunity_roll(u64::MAX, Kind::LivestockFarm),
            opportunity_roll(u64::MAX, Kind::StoneQuarry)
        );
    }
}
