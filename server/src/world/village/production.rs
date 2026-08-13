//! Pure production-rate rules shared by tactical and strategic workers.

use super::{CHOP_SECONDS, PERFECT_FIELD_SECONDS_PER_WHEAT};
use shared::components::{SettlementBuildingKind, WorldTime};
use shared::economy::{Good, BASIS_POINTS};

/// Rated output used by investors and development planning.
///
/// This is derived from the same worker slots, recipe durations and ordinary
/// shift used by the physical simulation. It is a capacity estimate, not a
/// daily grant: walking, missing inputs, vacancies and full stores still make
/// observed output lower.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DailyProductionEstimate {
    pub output: Good,
    pub output_units: u32,
    pub input: Option<(Good, u32)>,
}

impl DailyProductionEstimate {
    pub const fn input_units(self) -> u32 {
        match self.input {
            Some((_, units)) => units,
            None => 0,
        }
    }

    pub const fn output_per_input(self) -> u32 {
        let input = self.input_units();
        if input == 0 {
            0
        } else {
            self.output_units / input
        }
    }
}

const RATED_SHIFT_SECONDS: f32 = WorldTime::DEFAULT_DAY_DURATION * WorldTime::WORKDAY_END_DAY_T;

/// One embodied processing cycle. Extractors create their output at a field,
/// shore or tree; processors instead consume and create stock inside their
/// bounded workplace inventory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ProcessingRecipe {
    pub input: Good,
    pub input_units: u32,
    pub output: Good,
    pub output_units: u32,
    pub work_seconds: f32,
}

impl ProcessingRecipe {
    /// Net new resident-rations created by one cycle. Converting two existing
    /// Flour rations into four Bread adds two rations, not four, which keeps
    /// settlement production history honest.
    pub const fn net_food_units(self) -> u32 {
        let input = if self.input.is_edible() {
            self.input_units
        } else {
            0
        };
        let output = if self.output.is_edible() {
            self.output_units
        } else {
            0
        };
        output.saturating_sub(input)
    }
}

pub(crate) const fn processing_recipe(kind: SettlementBuildingKind) -> Option<ProcessingRecipe> {
    match kind {
        SettlementBuildingKind::Windmill => Some(ProcessingRecipe {
            input: Good::Wheat,
            input_units: 1,
            output: Good::Flour,
            output_units: 1,
            work_seconds: 120.0,
        }),
        SettlementBuildingKind::Bakery => Some(ProcessingRecipe {
            input: Good::Flour,
            input_units: 2,
            output: Good::Bread,
            output_units: 4,
            work_seconds: 140.0,
        }),
        _ => None,
    }
}

/// Full-staffed founding-business capacity under uninterrupted ordinary work.
/// Adding future building levels or skill multipliers belongs here so the
/// physical simulation and investment planner cannot drift into separate
/// ideas of how large a business is.
pub(crate) fn rated_daily_production(
    kind: SettlementBuildingKind,
    site_quality: f32,
) -> Option<DailyProductionEstimate> {
    if let Some(recipe) = processing_recipe(kind) {
        let worker_seconds = RATED_SHIFT_SECONDS * f32::from(kind.positions());
        let cycles = (worker_seconds / recipe.work_seconds).floor() as u32;
        return Some(DailyProductionEstimate {
            output: recipe.output,
            output_units: cycles.saturating_mul(recipe.output_units),
            input: Some((recipe.input, cycles.saturating_mul(recipe.input_units))),
        });
    }

    let seconds_per_unit = match kind {
        SettlementBuildingKind::Farmstead => farmer_seconds_per_wheat(site_quality),
        SettlementBuildingKind::FishermansHut => fisher_seconds_per_food(site_quality),
        _ => return None,
    };
    let worker_seconds = RATED_SHIFT_SECONDS * f32::from(kind.positions());
    let output_units = ((worker_seconds / seconds_per_unit).floor() as u32).max(1);
    Some(DailyProductionEstimate {
        output: if kind == SettlementBuildingKind::Farmstead {
            Good::Wheat
        } else {
            Good::Food
        },
        output_units,
        input: None,
    })
}

/// Highest input bid at which a fully staffed processor can still pay its
/// current wage offer and retain the owner's chosen markup at the current
/// output ask. Autopilot recalculates this instead of using an eternal multiple
/// of the input's base price.
pub(crate) fn maximum_viable_input_unit_price(
    kind: SettlementBuildingKind,
    output_unit_price: u64,
    daily_wage: u64,
    market_fee_bps: u16,
    target_margin_bps: u16,
) -> Option<u64> {
    let capacity = rated_daily_production(kind, 1.0)?;
    let (_, input_units) = capacity.input?;
    if input_units == 0 || capacity.output_units == 0 {
        return None;
    }
    let net_revenue = u64::from(capacity.output_units)
        .saturating_mul(output_unit_price)
        .saturating_mul(BASIS_POINTS.saturating_sub(u64::from(market_fee_bps)))
        / BASIS_POINTS;
    let cost_budget = net_revenue.saturating_mul(BASIS_POINTS)
        / BASIS_POINTS.saturating_add(u64::from(target_margin_bps));
    let payroll = u64::from(kind.positions()).saturating_mul(daily_wage);
    Some(
        cost_budget
            .saturating_sub(payroll)
            .checked_div(u64::from(input_units))
            .unwrap_or(0)
            .max(1),
    )
}

/// Perform as many complete cycles as both labour and physical stock permit.
/// Returns `(cycles, output_units)`. Inputs are removed only after enough
/// post-consumption bulk exists for the complete output batch.
pub(crate) fn process_available_cycles(
    inventory: &mut shared::economy::GoodsInventory,
    recipe: ProcessingRecipe,
    requested_cycles: u32,
) -> (u32, u32) {
    let mut cycles = 0u32;
    for _ in 0..requested_cycles {
        if inventory.amount(recipe.input) < recipe.input_units {
            break;
        }
        let reclaimed_bulk = recipe
            .input_units
            .saturating_mul(recipe.input.bulk_per_unit());
        let output_bulk = recipe
            .output_units
            .saturating_mul(recipe.output.bulk_per_unit());
        if inventory.free_bulk().saturating_add(reclaimed_bulk) < output_bulk {
            break;
        }
        debug_assert_eq!(
            inventory.remove(recipe.input, recipe.input_units),
            recipe.input_units
        );
        let produced = inventory.add(recipe.output, recipe.output_units);
        debug_assert_eq!(produced, recipe.output_units);
        cycles = cycles.saturating_add(1);
    }
    (cycles, cycles.saturating_mul(recipe.output_units))
}

/// Restrict an affordable market preview to input that completes whole recipe
/// batches. Buying one Flour when a bakery needs two strands scarce cash in an
/// unusable half-batch; a sensible automatic owner waits until it can buy the
/// pair. One-unit recipes and future non-processor procurement remain unchanged.
pub(crate) fn viable_processing_input_purchase(
    kind: SettlementBuildingKind,
    input: Good,
    held: u32,
    affordable_units: u32,
) -> u32 {
    let Some(recipe) = processing_recipe(kind).filter(|recipe| recipe.input == input) else {
        return affordable_units;
    };
    let complete_total = held
        .saturating_add(affordable_units)
        .checked_div(recipe.input_units.max(1))
        .unwrap_or(0)
        .saturating_mul(recipe.input_units.max(1));
    complete_total.saturating_sub(held).min(affordable_units)
}

/// Actual field labour required for one inventory unit. Progress is retained
/// between shifts, so short working days reduce current output without erasing
/// work or requiring a special per-day cap. Almost barren fields can still
/// yield eventually, but are proportionally unattractive places to build.
pub(crate) fn farmer_seconds_per_wheat(quality: f32) -> f32 {
    let quality = if quality.is_finite() {
        quality.clamp(0.01, 1.0)
    } else {
        0.01
    };
    PERFECT_FIELD_SECONDS_PER_WHEAT / quality
}

/// Pier quality uses the same readable scale as farmland: ideal water can
/// approach six catches per ordinary shift, while a two-thirds-quality shore
/// approaches four. Travel along the pier is real labour overhead rather than
/// being hidden inside a daily output table.
pub(crate) fn fisher_seconds_per_food(quality: f32) -> f32 {
    farmer_seconds_per_wheat(quality)
}

/// Emergency self-supply is deliberately inefficient. It prevents a founding
/// settlement with no timber firm from deadlocking, but one ordinary resident
/// with an improvised axe should not compete with a staffed Lumberjack Hut.
pub(crate) const SELF_SUPPLY_TREE_YIELD: u32 = 2;

/// A professional woodcutter turns each completed real-tree interaction into
/// three physical bundles. This is always greater than emergency self-supply,
/// exactly fits a villager's carrying capacity, and has no daily ceiling.
pub(crate) const fn lumber_tree_yield(_quality: f32) -> u32 {
    3
}

/// Sparse or awkward woodland takes longer for a professional to turn into a
/// full three-bundle load. Even the poorest valid plot remains more productive
/// per hour than two-bundle emergency work; good forest approaches the base
/// interaction time.
pub(crate) fn lumber_seconds_per_tree(quality: f32) -> f32 {
    let quality = if quality.is_finite() {
        quality.clamp(0.0, 1.0)
    } else {
        0.0
    };
    CHOP_SECONDS * (7.0 / 6.0 - quality / 6.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn professional_tree_work_always_beats_emergency_self_supply() {
        assert_eq!(SELF_SUPPLY_TREE_YIELD, 2);
        assert_eq!(lumber_tree_yield(0.0), 3);
        assert_eq!(lumber_tree_yield(0.49), 3);
        assert_eq!(lumber_tree_yield(0.5), 3);
        assert_eq!(lumber_tree_yield(1.0), 3);
        for quality in [f32::NEG_INFINITY, 0.0, 0.25, 0.5, 0.75, 1.0, f32::INFINITY] {
            assert!(lumber_tree_yield(quality) > SELF_SUPPLY_TREE_YIELD);
        }
        assert!((lumber_seconds_per_tree(0.0) - 105.0).abs() < 0.01);
        assert!((lumber_seconds_per_tree(1.0) - CHOP_SECONDS).abs() < 0.01);
        assert!(3.0 / lumber_seconds_per_tree(0.0) > SELF_SUPPLY_TREE_YIELD as f32 / CHOP_SECONDS);
    }

    #[test]
    fn wheat_is_milled_one_for_one_and_bread_doubles_food_output() {
        let mill = processing_recipe(SettlementBuildingKind::Windmill).unwrap();
        let bakery = processing_recipe(SettlementBuildingKind::Bakery).unwrap();
        assert_eq!(mill.net_food_units(), 1);
        assert_eq!(bakery.net_food_units(), 2);

        let mut stock = shared::economy::GoodsInventory::new(20);
        stock.add(Good::Wheat, 2);
        assert_eq!(process_available_cycles(&mut stock, mill, 3), (2, 2));
        assert_eq!(stock.amount(Good::Wheat), 0);
        assert_eq!(stock.amount(Good::Flour), 2);
        assert_eq!(process_available_cycles(&mut stock, bakery, 1), (1, 4));
        assert_eq!(stock.amount(Good::Flour), 0);
        assert_eq!(stock.amount(Good::Bread), 4);
    }

    #[test]
    fn planning_capacity_is_derived_from_physical_work_rates() {
        let farm = rated_daily_production(SettlementBuildingKind::Farmstead, 1.0).unwrap();
        let mill = rated_daily_production(SettlementBuildingKind::Windmill, 0.1).unwrap();
        let bakery = rated_daily_production(SettlementBuildingKind::Bakery, 0.9).unwrap();

        assert_eq!(farm.output, Good::Wheat);
        assert_eq!(farm.output_units, 12);
        assert_eq!(farm.input, None);
        assert_eq!(mill.input, Some((Good::Wheat, 18)));
        assert_eq!(mill.output, Good::Flour);
        assert_eq!(mill.output_units, 18);
        assert_eq!(bakery.input, Some((Good::Flour, 30)));
        assert_eq!(bakery.output, Good::Bread);
        assert_eq!(bakery.output_units, 60);
        assert_eq!(bakery.output_per_input(), 2);
    }

    #[test]
    fn processor_input_bid_follows_output_revenue_and_real_recipe() {
        let ordinary = maximum_viable_input_unit_price(
            SettlementBuildingKind::Bakery,
            Good::Bread.base_price(),
            shared::economy::FOUNDING_DAILY_WAGE,
            500,
            1_500,
        )
        .unwrap();
        let expensive_bread = maximum_viable_input_unit_price(
            SettlementBuildingKind::Bakery,
            Good::Bread.base_price() * 2,
            shared::economy::FOUNDING_DAILY_WAGE,
            500,
            1_500,
        )
        .unwrap();
        assert!(ordinary > Good::Flour.base_price());
        assert!(expensive_bread > ordinary);
    }

    #[test]
    fn a_bakery_waits_for_an_affordable_complete_flour_batch() {
        assert_eq!(
            viable_processing_input_purchase(SettlementBuildingKind::Bakery, Good::Flour, 0, 1,),
            0
        );
        assert_eq!(
            viable_processing_input_purchase(SettlementBuildingKind::Bakery, Good::Flour, 0, 3,),
            2
        );
        assert_eq!(
            viable_processing_input_purchase(SettlementBuildingKind::Bakery, Good::Flour, 1, 1,),
            1
        );
        assert_eq!(
            viable_processing_input_purchase(SettlementBuildingKind::Windmill, Good::Wheat, 0, 3,),
            3
        );
    }
}
