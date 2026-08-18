//! Pure production-rate rules shared by tactical and strategic workers.

use super::{CHOP_SECONDS, PERFECT_FIELD_SECONDS_PER_WHEAT};
use shared::components::{SettlementBuildingKind, WorldTime};
use shared::economy::{
    BusinessProcurementPolicy, BusinessSalePolicy, Good, BASIS_POINTS, MAXIMUM_STOCK_COVERAGE_DAYS,
};

/// One cached, daily operating decision for a productive workplace.
///
/// Market reasoning happens once in the employment review. Tactical workers
/// and the strategic simulation only claim units from this component, keeping
/// high-speed worlds O(businesses) per day rather than O(NPC decisions) per
/// frame. Manual businesses receive an uncapped plan.
#[derive(bevy::prelude::Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BusinessOperatingPlan {
    pub day: u32,
    pub target_output_units: u32,
    pub produced_output_units: u32,
    pub optimal_positions: u8,
    pub marginal_daily_profit: i64,
}

impl BusinessOperatingPlan {
    pub const fn uncapped(day: u32, positions: u8) -> Self {
        Self {
            day,
            target_output_units: u32::MAX,
            produced_output_units: 0,
            optimal_positions: positions,
            marginal_daily_profit: 0,
        }
    }

    pub const fn remaining(self, day: u32) -> u32 {
        if self.day != day {
            0
        } else {
            self.target_output_units
                .saturating_sub(self.produced_output_units)
        }
    }

    pub fn record(&mut self, day: u32, units: u32) {
        if self.day == day {
            self.produced_output_units = self.produced_output_units.saturating_add(units);
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InputStockTargets {
    pub daily_units: u32,
    pub reorder_below: u32,
    pub target_units: u32,
}

/// Conservative autonomous opening roster. A new workshop proves that it can
/// obtain inputs and sell output with one position before its Company Master
/// exposes the rest of the building's vacancies. Manual operators may change
/// the target immediately through the same staffing policy.
pub(crate) fn automatic_opening_positions(kind: SettlementBuildingKind) -> u8 {
    if kind.positions() == 0 {
        0
    } else {
        1
    }
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

    if kind == SettlementBuildingKind::LumberjackHut {
        let worker_seconds = RATED_SHIFT_SECONDS * f32::from(kind.positions());
        let cycles = (worker_seconds / lumber_seconds_per_tree(site_quality)).floor() as u32;
        return Some(DailyProductionEstimate {
            output: Good::Wood,
            output_units: cycles.saturating_mul(lumber_tree_yield(site_quality)),
            input: None,
        });
    }

    if kind == SettlementBuildingKind::StoneQuarry {
        let worker_seconds = RATED_SHIFT_SECONDS * f32::from(kind.positions());
        let output_units =
            ((worker_seconds / quarry_seconds_per_stone(site_quality)).floor() as u32).max(1);
        return Some(DailyProductionEstimate {
            output: Good::Stone,
            output_units,
            input: None,
        });
    }

    if kind == SettlementBuildingKind::LivestockFarm {
        let worker_seconds = RATED_SHIFT_SECONDS * f32::from(kind.positions());
        let output_units =
            ((worker_seconds / livestock_seconds_per_meat(site_quality)).floor() as u32).max(1);
        return Some(DailyProductionEstimate {
            output: Good::Meat,
            output_units,
            input: None,
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

/// Estimate the cost of one saleable unit for a particular enacted roster.
///
/// This deliberately uses the site's real quality and the same proportional
/// capacity rule as the daily staffing planner. A new low-quality extractor
/// must therefore quote enough to fund its first worker instead of pricing as
/// though it occupied perfect land and then rationally hiring nobody.
pub(crate) fn estimated_staffed_unit_cost(
    kind: SettlementBuildingKind,
    site_quality: f32,
    staffed_positions: u8,
    daily_wage: u64,
    mut input_unit_price: impl FnMut(Good) -> u64,
) -> Option<u64> {
    let capacity = rated_daily_production(kind, site_quality)?;
    let positions = kind.positions().max(1);
    let staffed_positions = staffed_positions.clamp(1, positions);
    let mut output_units = capacity
        .output_units
        .saturating_mul(u32::from(staffed_positions))
        .div_ceil(u32::from(positions));
    let input_cost = if let Some(recipe) = processing_recipe(kind) {
        let cycles = output_units / recipe.output_units.max(1);
        output_units = cycles.saturating_mul(recipe.output_units);
        u64::from(cycles)
            .saturating_mul(u64::from(recipe.input_units))
            .saturating_mul(input_unit_price(recipe.input))
    } else {
        0
    };
    if output_units == 0 {
        return None;
    }
    let payroll = u64::from(staffed_positions).saturating_mul(daily_wage);
    Some(
        payroll
            .saturating_add(input_cost)
            .div_ceil(u64::from(output_units)),
    )
}

/// Productive seconds for one dressed Stone unit. Rocky highland sites expose
/// workable faces and fractured material; meadow quarries remain possible but
/// substantially less competitive rather than being prohibited by biome.
pub(crate) fn quarry_seconds_per_stone(site_quality: f32) -> f32 {
    300.0 - 120.0 * site_quality.clamp(0.0, 1.0)
}

/// Productive tending time for one Meat ration and its paired Wool by-product.
/// A fully staffed perfect pasture rates six Meat per ordinary day; poor land
/// remains viable at roughly four, while fish and grain keep distinct niches.
pub(crate) fn livestock_seconds_per_meat(site_quality: f32) -> f32 {
    540.0 - 180.0 * site_quality.clamp(0.0, 1.0)
}

/// Materialise paired livestock products atomically. A full store must never
/// create Meat while silently dropping its Wool by-product (or vice versa).
pub(crate) fn produce_livestock_cycles(
    inventory: &mut shared::economy::GoodsInventory,
    requested: u32,
) -> u32 {
    let cycle_bulk = Good::Meat.bulk_per_unit() + Good::Wool.bulk_per_unit();
    let cycles = requested.min(inventory.free_bulk() / cycle_bulk.max(1));
    let meat = inventory.add(Good::Meat, cycles);
    let wool = inventory.add(Good::Wool, meat);
    debug_assert_eq!(wool, meat);
    meat
}

fn staffed_daily_units(full_staffed_units: u32, workers: usize, positions: u8) -> u32 {
    let positions = u32::from(positions.max(1));
    // A new processor stocks for its first hire instead of remaining empty
    // until employment and procurement happen in exactly the right order.
    let planned_workers = u32::try_from(workers)
        .unwrap_or(u32::MAX)
        .clamp(1, positions);
    full_staffed_units
        .saturating_mul(planned_workers)
        .div_ceil(positions)
}

/// Translate the owner-facing days-of-supply setting into the unit targets
/// consumed by both tactical and strategic logistics. Recipe capacity and the
/// current roster are authoritative; the cached units are not a second owner
/// policy.
pub(crate) fn rated_input_stock_targets(
    kind: SettlementBuildingKind,
    good: Good,
    workers: usize,
    coverage_days: u8,
) -> Option<InputStockTargets> {
    let coverage_days = coverage_days.min(MAXIMUM_STOCK_COVERAGE_DAYS);
    if kind == SettlementBuildingKind::Tavern && Good::TAVERN_INPUTS.contains(&good) {
        let full_staffed_units = match good {
            Good::Bread => 4,
            Good::Meat | Good::Wheat => 2,
            _ => return None,
        };
        let daily_units = staffed_daily_units(full_staffed_units, workers, kind.positions()).max(1);
        let target_units = daily_units.saturating_mul(u32::from(coverage_days));
        return Some(InputStockTargets {
            daily_units,
            reorder_below: target_units.div_ceil(2).min(target_units),
            target_units,
        });
    }
    let capacity = rated_daily_production(kind, 1.0)?;
    let (input, full_staffed_units) = capacity.input?;
    if input != good {
        return None;
    }
    if coverage_days == 0 {
        return Some(InputStockTargets::default());
    }
    let daily_units = staffed_daily_units(full_staffed_units, workers, kind.positions()).max(1);
    let storage_limit =
        kind.storage_bulk_capacity().saturating_mul(2) / 3 / good.bulk_per_unit().max(1);
    let target_units = daily_units
        .saturating_mul(u32::from(coverage_days))
        .min(storage_limit)
        .max(1);
    let recipe_batch = processing_recipe(kind)
        .filter(|recipe| recipe.input == good)
        .map_or(1, |recipe| recipe.input_units);
    let reorder_below = target_units.div_ceil(2).max(recipe_batch).min(target_units);
    Some(InputStockTargets {
        daily_units,
        reorder_below,
        target_units,
    })
}

/// Keep processor input previews aligned with recipe capacity and staffing.
/// Output retention is no longer derived per-site from days: it is an
/// absolute company-branch policy enforced once across all local sites.
pub fn sync_business_stock_targets(
    world_time: bevy::prelude::Query<&WorldTime>,
    mut businesses: bevy::prelude::Query<(
        &shared::components::SettlementBuilding,
        &mut BusinessProcurementPolicy,
        &mut BusinessSalePolicy,
        Option<&BusinessOperatingPlan>,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (building, mut procurement, mut sale, operating_plan) in businesses.iter_mut() {
        for good in Good::ALL {
            let mut rule = procurement.rule(good);
            if !rule.enabled {
                continue;
            }
            rule.set_coverage_days(rule.coverage_days);
            let mut targets = rated_input_stock_targets(
                building.kind,
                good,
                building.workers.len(),
                rule.coverage_days,
            )
            .unwrap_or_default();
            if let (Some(plan), Some(recipe)) = (
                operating_plan
                    .filter(|plan| plan.day == day && plan.target_output_units != u32::MAX),
                processing_recipe(building.kind).filter(|recipe| recipe.input == good),
            ) {
                let cycles = plan
                    .target_output_units
                    .div_ceil(recipe.output_units.max(1));
                let daily_units = cycles.saturating_mul(recipe.input_units);
                let storage_limit = building.kind.storage_bulk_capacity().saturating_mul(2)
                    / 3
                    / good.bulk_per_unit().max(1);
                let target_units = daily_units
                    .saturating_mul(u32::from(rule.coverage_days))
                    .min(storage_limit);
                targets = InputStockTargets {
                    daily_units,
                    reorder_below: target_units
                        .div_ceil(2)
                        .max(u32::from(target_units > 0).saturating_mul(recipe.input_units))
                        .min(target_units),
                    target_units,
                };
            }
            if rule.reorder_below != targets.reorder_below
                || rule.target_units != targets.target_units
            {
                rule.reorder_below = targets.reorder_below;
                rule.target_units = targets.target_units;
                procurement.set_rule(good, rule);
            } else if procurement.rule(good).coverage_days != rule.coverage_days {
                procurement.set_rule(good, rule);
            }
        }
        if sale.company_reserve_days != 0 || sale.company_reserve_units != 0 {
            sale.company_reserve_days = 0;
            sale.company_reserve_units = 0;
        }
    }
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
/// leaves some personal cargo room, and has no daily ceiling.
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
        assert_eq!(CHOP_SECONDS, 40.0);
        assert_eq!(SELF_SUPPLY_TREE_YIELD, 2);
        assert_eq!(lumber_tree_yield(0.0), 3);
        assert_eq!(lumber_tree_yield(0.49), 3);
        assert_eq!(lumber_tree_yield(0.5), 3);
        assert_eq!(lumber_tree_yield(1.0), 3);
        for quality in [f32::NEG_INFINITY, 0.0, 0.25, 0.5, 0.75, 1.0, f32::INFINITY] {
            assert!(lumber_tree_yield(quality) > SELF_SUPPLY_TREE_YIELD);
        }
        assert!((lumber_seconds_per_tree(0.0) - (140.0 / 3.0)).abs() < 0.01);
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
        let lumber = rated_daily_production(SettlementBuildingKind::LumberjackHut, 1.0).unwrap();
        let mill = rated_daily_production(SettlementBuildingKind::Windmill, 0.1).unwrap();
        let bakery = rated_daily_production(SettlementBuildingKind::Bakery, 0.9).unwrap();
        let livestock = rated_daily_production(SettlementBuildingKind::LivestockFarm, 1.0).unwrap();

        assert_eq!(farm.output, Good::Wheat);
        assert_eq!(farm.output_units, 12);
        assert_eq!(farm.input, None);
        assert_eq!(lumber.output, Good::Wood);
        assert!(lumber.output_units > 0);
        assert_eq!(lumber.input, None);
        assert_eq!(mill.input, Some((Good::Wheat, 18)));
        assert_eq!(mill.output, Good::Flour);
        assert_eq!(mill.output_units, 18);
        assert_eq!(bakery.input, Some((Good::Flour, 30)));
        assert_eq!(bakery.output, Good::Bread);
        assert_eq!(bakery.output_units, 60);
        assert_eq!(bakery.output_per_input(), 2);
        assert_eq!(livestock.output, Good::Meat);
        assert_eq!(livestock.output_units, 6);
        assert_eq!(livestock.input, None);
        assert_eq!(
            rated_daily_production(SettlementBuildingKind::LivestockFarm, 0.0)
                .unwrap()
                .output_units,
            4,
        );
    }

    #[test]
    fn livestock_cycles_create_paired_meat_and_wool_without_overfilling() {
        let mut inventory = shared::economy::GoodsInventory::new(7);
        assert_eq!(produce_livestock_cycles(&mut inventory, 10), 2);
        assert_eq!(inventory.amount(Good::Meat), 2);
        assert_eq!(inventory.amount(Good::Wool), 2);
        assert_eq!(inventory.used_bulk(), 6);
    }

    #[test]
    fn first_worker_cost_uses_real_site_quality() {
        let perfect = estimated_staffed_unit_cost(
            SettlementBuildingKind::FishermansHut,
            1.0,
            1,
            shared::economy::FOUNDING_DAILY_WAGE,
            Good::base_price,
        )
        .unwrap();
        let poor = estimated_staffed_unit_cost(
            SettlementBuildingKind::FishermansHut,
            0.42,
            1,
            shared::economy::FOUNDING_DAILY_WAGE,
            Good::base_price,
        )
        .unwrap();

        assert!(poor > perfect);
        let poor_capacity = rated_daily_production(SettlementBuildingKind::FishermansHut, 0.42)
            .unwrap()
            .output_units
            .div_ceil(2);
        assert_eq!(
            poor,
            shared::economy::FOUNDING_DAILY_WAGE.div_ceil(u64::from(poor_capacity))
        );
    }

    #[test]
    fn coverage_days_derive_staffing_aware_targets_and_hysteresis() {
        let one_worker =
            rated_input_stock_targets(SettlementBuildingKind::Windmill, Good::Wheat, 1, 2).unwrap();
        let two_workers =
            rated_input_stock_targets(SettlementBuildingKind::Windmill, Good::Wheat, 2, 2).unwrap();
        assert_eq!(one_worker.daily_units, 9);
        assert_eq!(one_worker.target_units, 18);
        assert_eq!(one_worker.reorder_below, 9);
        assert_eq!(two_workers.daily_units, 18);
        assert_eq!(two_workers.target_units, 36);
        assert_eq!(two_workers.reorder_below, 18);

        let off =
            rated_input_stock_targets(SettlementBuildingKind::Windmill, Good::Wheat, 2, 0).unwrap();
        assert_eq!(off, InputStockTargets::default());
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
