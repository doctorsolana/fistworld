//! Market-led settlement development decisions.
//!
//! The Moot advertises shortages and changes permit prices; it does not order a
//! resident to found a particular firm.  This module converts the settlement's
//! small, cached economic readings into opportunities which individual
//! applicants can judge through their own strategy.  It is deliberately pure:
//! geography and the authoritative permit transaction remain in `planning`.

use shared::components::{
    PermitMarketOpportunity, SettlementBuildingKind, SettlementOpportunityBoard,
    SettlementPolicies, SettlementTier,
};
use shared::economy::{
    sustainable_unit_price, BusinessStrategy, Good, MootMarket, SettlementEconomy, BASIS_POINTS,
    DEFAULT_TARGET_MARGIN_BPS, FOUNDING_DAILY_WAGE,
};

/// Current physical and commercial facts used by one permit review.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DevelopmentMarketSignals {
    pub residents: u32,
    pub farms: usize,
    pub fishers: usize,
    pub livestock_farms: usize,
    pub windmills: usize,
    pub bakeries: usize,
    pub storage_halls: usize,
    pub stone_quarries: usize,
    pub taverns: usize,
    /// Completed capacity which is temporarily idle, liquidating or offered
    /// for takeover. It suppresses duplicate construction while remaining
    /// distinct from currently productive capacity.
    pub recoverable_farms: usize,
    pub recoverable_fishers: usize,
    pub recoverable_livestock_farms: usize,
    pub recoverable_windmills: usize,
    pub recoverable_bakeries: usize,
    pub recoverable_lumber_huts: usize,
    pub recoverable_storage_halls: usize,
    pub recoverable_stone_quarries: usize,
    pub completed_windmills: usize,
    pub completed_bakeries: usize,
    pub unproven_windmill: bool,
    pub unproven_bakery: bool,
    pub lumber_huts: usize,
    pub houses: usize,
    pub wheat_stock: u32,
    pub flour_stock: u32,
    pub bread_stock: u32,
    pub meat_stock: u32,
    pub wood_stock: u32,
    pub stone_stock: u32,
    /// Physical Stone the civic centre still needs for its Town Hall project.
    pub town_hall_stone_demand: u32,
    pub recent_wheat_output: u32,
    pub recent_fish_output: u32,
    pub recent_meat_output: u32,
    /// Rated daily output from active and already-approved farms. Unlike
    /// `recent_wheat_output`, this closes the permit signal during startup,
    /// before the first field has completed a physical harvest.
    pub anticipated_wheat_output: u32,
    /// Rated daily output from active and already-approved fishing huts.
    pub anticipated_fish_output: u32,
    pub anticipated_meat_output: u32,
    /// Rated output from not-yet-completed extractors. Pending capacity is
    /// counted conservatively so one permit has time to become embodied, but
    /// it cannot masquerade as a proven full shift.
    pub pending_wheat_output: u32,
    pub pending_fish_output: u32,
    pub pending_meat_output: u32,
    /// Demonstrated daily Wheat demand committed outside this settlement.
    /// This remains zero until physical caravan contracts are introduced; it
    /// is deliberately separate from local mill purchases so a future grain-
    /// exporting town can expand without weakening local oversupply control.
    pub recent_wheat_export_demand: u32,
    /// Funded demand advertised by other completed Marketplaces, indexed by
    /// good. This remains opportunity rather than guaranteed sales; active
    /// and incoming capacity are already deducted by the route evaluator.
    pub merchant_export_units: [u32; Good::COUNT],
    pub recent_flour_output: u32,
    pub recent_bread_output: u32,
    pub recent_windmill_input: u32,
    pub recent_windmill_sales: u32,
    pub recent_windmill_profit: i64,
    pub recent_bakery_input: u32,
    pub recent_bakery_sales: u32,
    pub recent_bakery_profit: i64,
    /// Rated output exposed by current staffing, plus already-built capacity
    /// which could be exposed by hiring before another plant is justified.
    pub active_windmill_output_capacity: u32,
    pub active_bakery_output_capacity: u32,
    pub idle_windmill_output_capacity: u32,
    pub idle_bakery_output_capacity: u32,
    pub lossmaking_windmills: usize,
    pub lossmaking_bakeries: usize,
    /// Physical logistics evidence. Stock waiting at productive sites is work
    /// for carts; recent dispatched bulk and free depot space are capacity
    /// already available to clear it.
    pub stranded_output_bulk: u32,
    /// Edible stock still at workplaces, before public collection. Hunger with
    /// a full day's food here is a circulation problem, not another field.
    pub uncollected_food: u32,
    pub stranded_output_value: u64,
    pub recent_logistics_bulk: u32,
    pub active_storage_free_bulk: u32,
    pub recent_storage_cost: u64,
    /// Buyer-funded inter-settlement cargo currently offered from this
    /// settlement. A live contract can justify branch warehouse capacity even
    /// before local output becomes stranded.
    pub export_contract_bulk: u32,
    /// Publicly observed, funded merchant demand in other Marketplace towns.
    /// This is an opportunity rather than a reservation: speculative cargo
    /// remains at the company's risk until a real buyer clears it.
    pub merchant_export_bulk: u32,
    pub construction_wood_demand: u32,
}

impl DevelopmentMarketSignals {
    pub fn beds(self) -> usize {
        self.houses
            .saturating_mul(SettlementBuildingKind::House.housing_capacity() as usize)
    }

    pub const fn recoverable(self, kind: SettlementBuildingKind) -> usize {
        match kind {
            SettlementBuildingKind::Farmstead => self.recoverable_farms,
            SettlementBuildingKind::FishermansHut => self.recoverable_fishers,
            SettlementBuildingKind::LivestockFarm => self.recoverable_livestock_farms,
            SettlementBuildingKind::Windmill => self.recoverable_windmills,
            SettlementBuildingKind::Bakery => self.recoverable_bakeries,
            SettlementBuildingKind::LumberjackHut => self.recoverable_lumber_huts,
            SettlementBuildingKind::StorageHall => self.recoverable_storage_halls,
            SettlementBuildingKind::StoneQuarry => self.recoverable_stone_quarries,
            _ => 0,
        }
    }

    pub const fn merchant_export_demand(self, good: Good) -> u32 {
        self.merchant_export_units[good.index()]
    }
}

/// One visible market signal. `civic_priority` earns the enacted permit
/// discount; a lower-scoring opportunity remains legal at full price.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevelopmentOpportunity {
    pub kind: SettlementBuildingKind,
    pub score: f32,
    pub civic_priority: bool,
    /// A high-price incumbent created this opportunity. Entry must change the
    /// seller set, rather than giving the incumbent another plant.
    pub requires_independent_owner: bool,
}

const FOUNDING_PRIVATE_KINDS: [SettlementBuildingKind; 10] = [
    SettlementBuildingKind::House,
    SettlementBuildingKind::Farmstead,
    SettlementBuildingKind::FishermansHut,
    SettlementBuildingKind::LivestockFarm,
    SettlementBuildingKind::Windmill,
    SettlementBuildingKind::Bakery,
    SettlementBuildingKind::LumberjackHut,
    SettlementBuildingKind::StorageHall,
    SettlementBuildingKind::StoneQuarry,
    SettlementBuildingKind::Tavern,
];

/// A processor may reasonably invest against an existing stockpile, but that
/// stock is finite.  Amortising it over a working week prevents one unusually
/// large delivery from being mistaken for the same amount of new input every
/// day until a row of mills or bakeries has been approved.
const PROCESSOR_BACKLOG_CLEAR_DAYS: u32 = 7;
/// `planning` deliberately combines the current and previous business day so
/// a permit review does not react to one quiet instant. Convert that window
/// back into a conservative daily flow before comparing it with daily plant
/// capacity.
const RECENT_OUTPUT_WINDOW_DAYS: u32 = 2;

fn clamp_score(score: f32) -> f32 {
    if score.is_finite() {
        score.clamp(0.0, 140.0)
    } else {
        0.0
    }
}

fn sustainable_daily_input(recent_output: u32, stock: u32) -> u32 {
    recent_output
        .div_ceil(RECENT_OUTPUT_WINDOW_DAYS)
        .saturating_add(stock.div_ceil(PROCESSOR_BACKLOG_CLEAR_DAYS))
}

fn processor_expansion_proven(
    kind: SettlementBuildingKind,
    existing: usize,
    active_output_capacity: u32,
    idle_output_capacity: u32,
    recoverable_sites: usize,
    lossmaking_sites: usize,
    supported_output: u32,
    recent_profit: i64,
) -> bool {
    if existing == 0 {
        return true;
    }
    let _ = kind;
    recoverable_sites == 0
        && idle_output_capacity == 0
        && lossmaking_sites == 0
        && recent_profit > 0
        && supported_output > active_output_capacity
}

fn processor_market_facts(
    kind: SettlementBuildingKind,
    signals: DevelopmentMarketSignals,
) -> Option<(usize, usize, bool, u32, u32, i64, usize, usize)> {
    match kind {
        SettlementBuildingKind::Windmill => Some((
            signals.windmills,
            signals.completed_windmills,
            signals.unproven_windmill,
            signals.recent_windmill_sales,
            sustainable_daily_input(signals.recent_wheat_output, signals.wheat_stock),
            signals.recent_windmill_profit,
            signals.recoverable_windmills,
            signals.lossmaking_windmills,
        )),
        SettlementBuildingKind::Bakery => Some((
            signals.bakeries,
            signals.completed_bakeries,
            signals.unproven_bakery,
            signals.recent_bakery_sales,
            sustainable_daily_input(signals.recent_flour_output, signals.flour_stock),
            signals.recent_bakery_profit,
            signals.recoverable_bakeries,
            signals.lossmaking_bakeries,
        )),
        _ => None,
    }
}

fn competitive_output_reference(kind: SettlementBuildingKind, market: &MootMarket) -> Option<u64> {
    let capacity = super::rated_daily_production(kind, 1.0)?;
    let (input, input_units) = capacity.input?;
    let daily_cost = u64::from(input_units)
        .saturating_mul(market.suggested_price(input))
        .saturating_add(u64::from(kind.positions()).saturating_mul(FOUNDING_DAILY_WAGE));
    let unit_cost = daily_cost.div_ceil(u64::from(capacity.output_units.max(1)));
    Some(sustainable_unit_price(
        unit_cost,
        market.market_fee_bps(),
        DEFAULT_TARGET_MARGIN_BPS,
    ))
}

/// Capacity pressure and competitive pressure are deliberately independent.
/// An overpriced or poorly managed monopoly may leave nominal plant capacity
/// idle; requiring that same incumbent to reach 60% utilisation before anybody
/// may challenge it would make market power permanent.
fn processor_competition_proven(
    kind: SettlementBuildingKind,
    signals: DevelopmentMarketSignals,
    market: Option<&MootMarket>,
) -> bool {
    let Some(market) = market else {
        return false;
    };
    let Some((
        existing,
        completed,
        unproven,
        recent_sales,
        sustainable_input,
        recent_profit,
        recoverable,
        lossmaking,
    )) = processor_market_facts(kind, signals)
    else {
        return false;
    };
    // `existing` includes approved worksites. Only one challenger may be under
    // review, and a newly opened challenger gets several days to reveal its
    // effect before the same old monopoly evidence can approve a third plant.
    if existing == 0
        || completed < existing
        || unproven
        || sustainable_input == 0
        || recoverable > 0
        || lossmaking > 0
    {
        return false;
    }
    let Some(capacity) = super::rated_daily_production(kind, 1.0) else {
        return false;
    };
    let output = capacity.output;
    let pool = market.pool(output);
    let current = pool.day;
    let previous = pool.previous_day;
    let observed_days =
        usize::from(current.requested_units() > 0) + usize::from(previous.requested_units() > 0);
    if observed_days < 2 || recent_sales == 0 || recent_profit <= 0 {
        return false;
    }
    let recent_unmet = current.unmet_units().saturating_add(previous.unmet_units());
    let thin_market = market.listed_units(output) < pool.target_stock.max(2) / 2;
    if recent_unmet == 0 && !thin_market {
        return false;
    }
    let recent_high_ask = pool.ask.max(current.high_ask).max(previous.high_ask);
    let Some(reference) = competitive_output_reference(kind, market) else {
        return false;
    };
    recent_high_ask.saturating_mul(100) >= reference.saturating_mul(150)
}

fn competition_score(kind: SettlementBuildingKind, market: &MootMarket) -> f32 {
    let capacity = super::rated_daily_production(kind, 1.0)
        .expect("processor competition requires a production estimate");
    let pool = market.pool(capacity.output);
    let reference = competitive_output_reference(kind, market).unwrap_or(1);
    let high_ask = pool
        .ask
        .max(pool.day.high_ask)
        .max(pool.previous_day.high_ask);
    let premium = high_ask.saturating_sub(reference) as f32 / reference.max(1) as f32;
    let unmet = pool
        .day
        .unmet_units()
        .saturating_add(pool.previous_day.unmet_units()) as f32;
    68.0 + (premium * 18.0).min(30.0) + (unmet * 1.5).min(24.0)
}

fn housing_pressure(signals: DevelopmentMarketSignals) -> f32 {
    let residents = signals.residents.max(1) as usize;
    residents.saturating_sub(signals.beds()) as f32 / residents as f32
}

fn food_pressure(
    signals: DevelopmentMarketSignals,
    economy: Option<&SettlementEconomy>,
    policies: &SettlementPolicies,
) -> f32 {
    let Some(economy) = economy.filter(|economy| economy.observed_days > 0) else {
        return if signals.farms + signals.fishers + signals.livestock_farms == 0 {
            1.0
        } else {
            // One completed extractor is enough evidence for a founding day.
            // Further fields remain legal speculation, but the hall does not
            // advertise them urgently until a real food reading exists.
            0.15
        };
    };
    let residents = signals.residents.max(1) as f32;
    let production_gap = (1.0 - economy.recent_food_production / residents).clamp(0.0, 1.0);
    let reserve_target = f32::from(policies.food_reserve_target_days.max(1));
    let reserve_gap = (1.0 - economy.reserve_days / reserve_target).clamp(0.0, 1.0);
    production_gap.max(reserve_gap)
}

/// Competition is density in the customer base, not a lifetime site quota.
/// Preserve the founding-town caution while letting a ten-times larger town
/// support ten times the extractors when its measured shortage is the same.
fn extractor_crowding_penalty(count: usize, residents: u32, per_site: f32) -> f32 {
    let customer_scale = (residents as f32 / 24.0).max(1.0);
    count as f32 * per_site / customer_scale
}

/// Capacity pressure for one kind of food extractor.
///
/// The other extractor may satisfy local residents, while committed exports
/// belong only to the good actually ordered. Fertile land therefore closes a
/// local Farmstead shortage faster, but real caravan demand can reopen it.
fn extractor_capacity_pressure(
    kind: SettlementBuildingKind,
    signals: DevelopmentMarketSignals,
) -> f32 {
    let conservative_output = |rated: u32, pending: u32, recent: u32| {
        let active_rated = rated.saturating_sub(pending);
        let recent_daily = recent.div_ceil(RECENT_OUTPUT_WINDOW_DAYS);
        // Commutes, carrying and market hand-offs are physical work too. A
        // third of nameplate output is the safe promise until the settlement
        // has observed this extractor doing better; pending sites receive the
        // same discount so a construction queue cannot suppress all entry.
        recent_daily
            .max(active_rated.div_ceil(3))
            .saturating_add(pending.div_ceil(3))
    };
    let wheat = conservative_output(
        signals.anticipated_wheat_output,
        signals.pending_wheat_output,
        signals.recent_wheat_output,
    );
    let fish = conservative_output(
        signals.anticipated_fish_output,
        signals.pending_fish_output,
        signals.recent_fish_output,
    );
    let meat = conservative_output(
        signals.anticipated_meat_output,
        signals.pending_meat_output,
        signals.recent_meat_output,
    );
    // Raw Wheat is not a ration. For fishing investment, only the grain which
    // the local chain recently turned into Flour/Bread can displace Fish.
    // Bakery output contributes only its net gain over the Flour it consumed.
    let grain_food = signals
        .recent_flour_output
        .saturating_add(signals.recent_bread_output / 2)
        .div_ceil(RECENT_OUTPUT_WINDOW_DAYS);
    let (anticipated, competing_local, export_demand) = match kind {
        SettlementBuildingKind::Farmstead => (
            wheat,
            fish.saturating_add(meat),
            signals
                .recent_wheat_export_demand
                .max(signals.merchant_export_demand(Good::Wheat)),
        ),
        SettlementBuildingKind::FishermansHut => (
            fish,
            grain_food.saturating_add(meat),
            signals.merchant_export_demand(Good::Food),
        ),
        SettlementBuildingKind::LivestockFarm => (
            meat,
            grain_food.saturating_add(fish),
            signals
                .merchant_export_demand(Good::Meat)
                .saturating_add(signals.merchant_export_demand(Good::Wool).div_ceil(2)),
        ),
        _ => return 0.0,
    };
    let target = signals
        .residents
        .saturating_sub(competing_local)
        .saturating_add(export_demand);
    if target == 0 {
        return 0.0;
    }
    (1.0 - anticipated as f32 / target as f32).clamp(0.0, 1.0)
}

fn opportunity_score(
    kind: SettlementBuildingKind,
    signals: DevelopmentMarketSignals,
    economy: Option<&SettlementEconomy>,
    market: Option<&MootMarket>,
    policies: &SettlementPolicies,
) -> f32 {
    let residents = signals.residents.max(1) as usize;
    let pressure = food_pressure(signals, economy, policies);
    let shelter_pressure = housing_pressure(signals);
    let extractor_export = match kind {
        SettlementBuildingKind::Farmstead => signals
            .recent_wheat_export_demand
            .max(signals.merchant_export_demand(Good::Wheat)),
        SettlementBuildingKind::FishermansHut => signals.merchant_export_demand(Good::Food),
        SettlementBuildingKind::LivestockFarm => signals
            .merchant_export_demand(Good::Meat)
            .saturating_add(signals.merchant_export_demand(Good::Wool)),
        _ => 0,
    };
    if matches!(
        kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::LivestockFarm
    ) && signals.uncollected_food >= signals.residents.max(1)
        && extractor_export == 0
    {
        // More empty shells cannot make existing private food reach the Hall.
        // Reopen investment as soon as collections clear the physical backlog;
        // funded external orders remain independent of local distribution.
        return 5.0;
    }
    let emergency_food_entry = matches!(
        kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::LivestockFarm
    ) && pressure >= 0.5;
    if kind != SettlementBuildingKind::House
        && signals.recoverable(kind) > 0
        && !emergency_food_entry
    {
        // Reopen or buy the existing structure before consuming land, Wood
        // and builder time on an economically identical duplicate. Acute food
        // failure is the exception: a mothballed or liquidating extractor has
        // already shown that it cannot answer current demand, so competitors
        // may seek a permit rather than waiting indefinitely.
        return 4.0;
    }
    let raw = match kind {
        SettlementBuildingKind::House => {
            let missing_beds = residents.saturating_sub(signals.beds());
            if missing_beds == 0 {
                return 0.0;
            }
            // A founding food extractor may still win the town's first permit,
            // but sustained homelessness must advertise shelter more strongly
            // than a merely profitable downstream processor.
            82.0 + 38.0 * shelter_pressure
        }
        SettlementBuildingKind::Farmstead => {
            if signals.farms + signals.fishers + signals.livestock_farms == 0
                && pressure >= 0.5
                && signals
                    .wheat_stock
                    .saturating_add(signals.flour_stock)
                    .saturating_add(signals.bread_stock)
                    == 0
            {
                return 130.0;
            }
            let capacity_pressure =
                extractor_capacity_pressure(SettlementBuildingKind::Farmstead, signals);
            let mill_capacity = signals.windmills.max(1) as u32
                * super::rated_daily_production(SettlementBuildingKind::Windmill, 1.0)
                    .map_or(1, |capacity| capacity.input_units());
            let raw_supply = signals
                .wheat_stock
                .saturating_add(signals.recent_wheat_output)
                .saturating_sub(
                    signals
                        .recent_wheat_export_demand
                        .saturating_mul(RECENT_OUTPUT_WINDOW_DAYS),
                );
            let backlog = (raw_supply as f32 / mill_capacity as f32).clamp(0.0, 3.0);
            // Food pressure advertises extraction, but a growing pile of raw
            // Wheat tells investors that processing—not another field—is the
            // current bottleneck.
            (12.0 + pressure.min(capacity_pressure) * 70.0
                - backlog * 30.0
                - extractor_crowding_penalty(signals.farms, signals.residents, 5.0))
            .max(5.0)
        }
        SettlementBuildingKind::FishermansHut => {
            if signals.farms + signals.fishers + signals.livestock_farms == 0 && pressure >= 0.5 {
                return 128.0;
            }
            let capacity_pressure =
                extractor_capacity_pressure(SettlementBuildingKind::FishermansHut, signals);
            // Fishing competes with farming as its own investment. It has no
            // Wheat-processing bottleneck, while repeated huts steadily make
            // the next shoreline claim less compelling. Geography makes the
            // final decision: inland applicants simply cannot secure a plot.
            (14.0 + pressure.min(capacity_pressure) * 70.0
                - extractor_crowding_penalty(signals.fishers, signals.residents, 8.0))
            .max(5.0)
        }
        SettlementBuildingKind::LivestockFarm => {
            if signals.farms + signals.fishers + signals.livestock_farms == 0 && pressure >= 0.5 {
                return 126.0;
            }
            let capacity_pressure =
                extractor_capacity_pressure(SettlementBuildingKind::LivestockFarm, signals);
            let wool_value = market.map_or(Good::Wool.base_price(), |market| {
                market.suggested_price(Good::Wool)
            });
            let byproduct_bonus =
                ((wool_value as f32 / Good::Wool.base_price() as f32) - 1.0).clamp(0.0, 1.0) * 12.0;
            (12.0 + pressure.min(capacity_pressure) * 66.0 + byproduct_bonus
                - extractor_crowding_penalty(signals.livestock_farms, signals.residents, 7.0))
            .max(5.0)
        }
        SettlementBuildingKind::Windmill => {
            let upstream_exists = signals.farms > 0 || signals.wheat_stock > 0;
            if !upstream_exists {
                return 8.0;
            }
            let daily_input =
                sustainable_daily_input(signals.recent_wheat_output, signals.wheat_stock);
            if daily_input == 0 {
                // A completed farm is a plausible upstream prospect, not proof
                // of Wheat. This low speculative signal lets a bold owner build
                // ahead without making every hamlet force a mill shell.
                return if signals.windmills == 0 { 38.0 } else { 8.0 };
            }
            let competitive = processor_competition_proven(kind, signals, market);
            if competitive {
                return competition_score(kind, market.expect("competition has market"));
            }
            if signals.completed_windmills < signals.windmills || signals.unproven_windmill {
                return 5.0;
            }
            let capacity = super::rated_daily_production(kind, 1.0)
                .expect("windmill opportunity requires rated capacity");
            let output_demand = market
                .map_or(0, |market| {
                    let pool = market.pool(capacity.output);
                    pool.day
                        .requested_units()
                        .max(pool.previous_day.requested_units())
                        .min(u64::from(u32::MAX)) as u32
                })
                .saturating_add(signals.merchant_export_demand(Good::Flour));
            let sales_demand = signals
                .recent_windmill_sales
                .div_ceil(RECENT_OUTPUT_WINDOW_DAYS);
            let supported_output = daily_input
                .saturating_mul(capacity.output_per_input())
                .min(output_demand.max(sales_demand));
            if !processor_expansion_proven(
                SettlementBuildingKind::Windmill,
                signals.windmills,
                signals.active_windmill_output_capacity,
                signals.idle_windmill_output_capacity,
                signals.recoverable_windmills,
                signals.lossmaking_windmills,
                supported_output,
                signals.recent_windmill_profit,
            ) {
                return 5.0;
            }
            let uncovered =
                supported_output.saturating_sub(signals.active_windmill_output_capacity);
            if signals.windmills > 0 && uncovered == 0 {
                return 5.0;
            }
            let founding = if signals.windmills == 0 { 45.0 } else { 0.0 };
            let flour_glut = signals.flour_stock.saturating_sub(signals.residents) as f32;
            30.0 + founding + (uncovered as f32 * 6.0).min(70.0) - (flour_glut * 1.5).min(35.0)
        }
        SettlementBuildingKind::Bakery => {
            let daily_flour =
                sustainable_daily_input(signals.recent_flour_output, signals.flour_stock);
            if daily_flour == 0 {
                // A bold owner may establish one bakery in anticipation of a
                // completed mill. Bread scarcity alone must not approve a row
                // of empty ovens before any Flour has actually moved.
                return if signals.windmills > 0 && signals.bakeries == 0 {
                    30.0
                } else {
                    5.0
                };
            }
            let competitive = processor_competition_proven(kind, signals, market);
            if competitive {
                return competition_score(kind, market.expect("competition has market"));
            }
            if signals.completed_bakeries < signals.bakeries || signals.unproven_bakery {
                return 5.0;
            }
            let capacity = super::rated_daily_production(kind, 1.0)
                .expect("bakery opportunity requires rated capacity");
            let output_demand = market
                .map_or(u64::from(signals.residents), |market| {
                    let pool = market.pool(capacity.output);
                    pool.day
                        .requested_units()
                        .max(pool.previous_day.requested_units())
                        .max(u64::from(signals.residents))
                })
                .saturating_add(u64::from(signals.merchant_export_demand(Good::Bread)));
            let supported_output = daily_flour
                .saturating_mul(capacity.output_per_input())
                .min(output_demand.min(u64::from(u32::MAX)) as u32);
            if !processor_expansion_proven(
                SettlementBuildingKind::Bakery,
                signals.bakeries,
                signals.active_bakery_output_capacity,
                signals.idle_bakery_output_capacity,
                signals.recoverable_bakeries,
                signals.lossmaking_bakeries,
                supported_output,
                signals.recent_bakery_profit,
            ) {
                return 5.0;
            }
            let bread_sales = u64::from(signals.recent_bakery_sales)
                .max(market.map_or(0, |market| market.pool(Good::Bread).day.consumer_units));
            // Two Bread come from one Flour. Sales prove demand, but cannot
            // make more input exist, so the supported input rate is bounded by
            // the larger of observed upstream flow and sales-backed Flour.
            let sales_backed_flour = bread_sales.div_ceil(2).min(u64::from(u32::MAX)) as u32;
            let supported_input = daily_flour.max(sales_backed_flour);
            let bakery_capacity = signals.bakeries as u32
                * super::rated_daily_production(SettlementBuildingKind::Bakery, 1.0)
                    .map_or(1, |capacity| capacity.input_units());
            let uncovered = supported_input.saturating_sub(bakery_capacity);
            if signals.bakeries > 0 && uncovered == 0 {
                return 5.0;
            }
            let bread_scarcity = signals
                .residents
                .saturating_sub(signals.bread_stock)
                .min(24) as f32;
            18.0 + (uncovered as f32 * 6.0).min(60.0) + bread_scarcity
        }
        SettlementBuildingKind::LumberjackHut => {
            let shortage = signals
                .construction_wood_demand
                .saturating_add(signals.merchant_export_demand(Good::Wood))
                .saturating_sub(signals.wood_stock);
            if shortage == 0 {
                return 5.0;
            }
            // A worksite backlog is finite demand, not permission for one
            // woodcutter per unfinished cabin. Existing and approved huts are
            // anticipated supply. Roughly one hut per eighty outstanding Wood
            // is useful, bounded by the town's labour base; market prices may
            // still entice an occasional speculative entrant above this plan.
            let labour_ceiling = signals.residents.max(1).div_ceil(50) as usize;
            let desired = (shortage.div_ceil(80) as usize)
                .max(1)
                .min(labour_ceiling.max(1));
            if signals.lumber_huts >= desired {
                return 5.0;
            }
            let missing_capacity = desired.saturating_sub(signals.lumber_huts) as f32;
            48.0 + missing_capacity * 13.0 + (shortage as f32 * 0.08).min(18.0)
        }
        SettlementBuildingKind::StoneQuarry => {
            let shortage = signals
                .town_hall_stone_demand
                .saturating_add(signals.merchant_export_demand(Good::Stone))
                .saturating_sub(signals.stone_stock);
            if shortage == 0 {
                return 5.0;
            }
            let rated = super::rated_daily_production(kind, 1.0)
                .map_or(1, |capacity| capacity.output_units.max(1));
            let desired = shortage.div_ceil(rated) as usize;
            if signals.stone_quarries >= desired.max(1) {
                return 5.0;
            }
            58.0 + (shortage as f32 * 4.0).min(42.0)
        }
        SettlementBuildingKind::StorageHall => {
            let regional_bulk = signals
                .export_contract_bulk
                .saturating_add(signals.merchant_export_bulk);
            if regional_bulk > 0 && signals.storage_halls == 0 {
                return 78.0 + (regional_bulk as f32 * 0.15).min(18.0);
            }
            let unhandled_bulk = signals
                .stranded_output_bulk
                .saturating_sub(signals.recent_logistics_bulk)
                .saturating_sub(signals.active_storage_free_bulk);
            if unhandled_bulk == 0 || signals.stranded_output_bulk == 0 {
                return 4.0;
            }
            let value_at_risk = signals
                .stranded_output_value
                .saturating_mul(u64::from(unhandled_bulk))
                / u64::from(signals.stranded_output_bulk.max(1));
            let expected_cost = signals
                .recent_storage_cost
                .div_ceil(RECENT_OUTPUT_WINDOW_DAYS as u64)
                .max(FOUNDING_DAILY_WAGE);
            if value_at_risk <= expected_cost {
                return 4.0;
            }
            let return_multiple = value_at_risk as f32 / expected_cost.max(1) as f32;
            48.0 + (return_multiple.ln_1p() * 18.0).min(50.0)
        }
        SettlementBuildingKind::Tavern => {
            let meal_supply = signals
                .bread_stock
                .saturating_add(signals.meat_stock)
                .saturating_add(signals.recent_bread_output)
                .saturating_add(signals.recent_meat_output);
            if meal_supply == 0 {
                // A private player may still speculate at full permit price,
                // but the Hall must not subsidise an empty dining room before
                // any meal supply exists for it to purchase.
                return 5.0;
            }
            let useful_capacity = signals
                .taverns
                .saturating_mul(shared::economy::TAVERN_MEALS_PER_INNKEEPER_DAY as usize);
            let affluent_demand = (signals.residents as usize).div_ceil(5);
            if signals.residents < 12 || useful_capacity >= affluent_demand.max(1) {
                5.0
            } else if signals.taverns == 0 {
                74.0 + (signals.residents as f32 * 0.35).min(16.0)
            } else {
                46.0 + ((affluent_demand - useful_capacity) as f32 * 2.0).min(32.0)
            }
        }
        SettlementBuildingKind::Hall
        | SettlementBuildingKind::Market
        | SettlementBuildingKind::Church => 0.0,
    };

    if kind == SettlementBuildingKind::House {
        return raw;
    }
    // The Hall still permits private speculation at full price, but it stops
    // subsidising rows of businesses while a large share of residents sleeps
    // rough. The first food extractor is exempt because shelter without any
    // food source is not a viable founding sequence.
    let first_food_source = signals.farms + signals.fishers + signals.livestock_farms == 0
        && matches!(
            kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::FishermansHut
                | SettlementBuildingKind::LivestockFarm
        );
    let shelter_penalty = if first_food_source {
        0.0
    } else {
        shelter_pressure * 55.0
    };
    (raw - shelter_penalty).max(5.0)
}

/// Sorted private opportunities. Every legal founding trade is represented,
/// even at a low score, so an opportunistic owner may speculate without a Moot
/// subsidy. Zero-score housing is omitted because there is no unmet household
/// need to approve.
pub fn private_opportunities(
    signals: DevelopmentMarketSignals,
    economy: Option<&SettlementEconomy>,
    market: Option<&MootMarket>,
    policies: &SettlementPolicies,
) -> Vec<DevelopmentOpportunity> {
    let mut opportunities: Vec<_> = FOUNDING_PRIVATE_KINDS
        .into_iter()
        .filter_map(|kind| {
            let score = clamp_score(opportunity_score(kind, signals, economy, market, policies));
            let requires_independent_owner = processor_competition_proven(kind, signals, market);
            (score > 0.0).then_some(DevelopmentOpportunity {
                kind,
                score,
                civic_priority: score >= 60.0,
                requires_independent_owner,
            })
        })
        .collect();
    opportunities.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| kind_order(a.kind).cmp(&kind_order(b.kind)))
    });
    opportunities
}

pub fn replicated_opportunity_board(
    opportunities: &[DevelopmentOpportunity],
    civic: Option<DevelopmentOpportunity>,
    tier: SettlementTier,
) -> SettlementOpportunityBoard {
    let mut entries: Vec<_> = opportunities
        .iter()
        .copied()
        .chain(civic)
        .map(|opportunity| PermitMarketOpportunity {
            kind: opportunity.kind,
            score: opportunity.score.round().clamp(0.0, 100.0) as u8,
            subsidized: opportunity.civic_priority,
            requires_independent_owner: opportunity.requires_independent_owner,
        })
        .collect();
    entries.retain(|entry| entry.kind.is_player_permit_available_at(tier));
    // Demand controls the score and subsidy, not whether a private player may
    // apply. Keep every tier-unlocked permit visible at full price even when
    // the Hall is not encouraging it. Only the settlement rung, payment and
    // the bounded unused-permit ledger constrain a valid quote.
    for kind in SettlementBuildingKind::PLAYER_PERMIT_KINDS {
        if !kind.is_player_permit_available_at(tier) {
            continue;
        }
        if !entries.iter().any(|entry| entry.kind == kind) {
            entries.push(PermitMarketOpportunity {
                kind,
                score: 0,
                subsidized: false,
                requires_independent_owner: false,
            });
        }
    }
    entries.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| kind_order(a.kind).cmp(&kind_order(b.kind)))
    });
    entries.dedup_by_key(|opportunity| opportunity.kind);
    // Every tier-unlocked use remains visible. Civic demand can add a signal or
    // discount, but it may not push a legal private permit off the board.
    SettlementOpportunityBoard {
        opportunities: entries,
    }
}

fn kind_order(kind: SettlementBuildingKind) -> u8 {
    match kind {
        SettlementBuildingKind::House => 0,
        SettlementBuildingKind::Farmstead => 1,
        SettlementBuildingKind::FishermansHut => 2,
        SettlementBuildingKind::LivestockFarm => 3,
        SettlementBuildingKind::Windmill => 4,
        SettlementBuildingKind::Bakery => 5,
        SettlementBuildingKind::LumberjackHut => 6,
        SettlementBuildingKind::StoneQuarry => 7,
        SettlementBuildingKind::StorageHall => 8,
        SettlementBuildingKind::Market => 9,
        SettlementBuildingKind::Tavern => 10,
        SettlementBuildingKind::Church => 11,
        SettlementBuildingKind::Hall => 12,
    }
}

fn expected_daily_business(
    kind: SettlementBuildingKind,
    site_quality: f32,
) -> Option<(Good, u32, Option<(Good, u32)>)> {
    if let Some(capacity) = super::rated_daily_production(kind, site_quality) {
        return Some((capacity.output, capacity.output_units, capacity.input));
    }
    match kind {
        SettlementBuildingKind::LumberjackHut => {
            let quality = site_quality.clamp(0.01, 1.0);
            Some((Good::Wood, (quality * 8.0).round().max(1.0) as u32, None))
        }
        _ => None,
    }
}

/// Recommended processor working cash: enough for one complete physical recipe
/// batch and prudent one-person opening payroll. It remains ordinary spendable
/// company treasury cash; it is neither a second permit charge nor escrow.
/// Extractors can begin with labour, while processors cannot make their first
/// sale without real purchased input. With no quote, the owner budgets for a
/// 2.6x supply shock rather than assuming base price; this is risk assessment,
/// not a market price cap.
pub fn minimum_startup_capital(kind: SettlementBuildingKind, market: Option<&MootMarket>) -> u64 {
    if kind == SettlementBuildingKind::StorageHall {
        // A depot cannot earn before its porter completes a first trip. Fund
        // a Balanced three-day payroll runway plus one coin of real risk
        // capital for a cash-sized trial cargo. This is ordinary founder or
        // retained company money, never escrow or a civic grant.
        return 4 * FOUNDING_DAILY_WAGE;
    }
    if kind == SettlementBuildingKind::Tavern {
        // A private inn must be able to open with one worker and one ordinary
        // service day's pantry. This is spendable company cash, not a public
        // grant or a second fee. Customer meal/ale recipes may refine these
        // quantities later without changing the ownership/procurement seam.
        let pantry = [(Good::Bread, 2u32), (Good::Meat, 1), (Good::Wheat, 1)]
            .into_iter()
            .map(|(good, units)| {
                let quoted =
                    market.map_or(good.base_price(), |market| market.suggested_price(good));
                u64::from(units).saturating_mul(quoted.max(good.base_price()))
            })
            .fold(0u64, u64::saturating_add);
        return pantry.saturating_add(FOUNDING_DAILY_WAGE);
    }
    let Some(recipe) = super::processing_recipe(kind) else {
        return 0;
    };
    let prudent_unquoted_price = recipe.input.base_price().saturating_mul(260) / 100;
    let input_price = market
        .map_or(recipe.input.base_price(), |market| {
            market.suggested_price(recipe.input)
        })
        .max(prudent_unquoted_price);
    let first_batch = u64::from(recipe.input_units).saturating_mul(input_price);
    let first_payroll =
        u64::from(super::automatic_opening_positions(kind)).saturating_mul(FOUNDING_DAILY_WAGE);
    first_batch
        .saturating_add(first_payroll)
        .max(shared::economy::PENNIES_PER_COIN)
}

/// Conservative current-day profit estimate in pennies. It is intentionally
/// not prophecy: prices, staffing, travel and future supply can change after a
/// permit is granted, which is how apparently good firms can still fail.
pub fn expected_daily_profit(
    kind: SettlementBuildingKind,
    site_quality: f32,
    market: Option<&MootMarket>,
) -> Option<i64> {
    let (output, output_units, input) = expected_daily_business(kind, site_quality)?;
    let output_price = market.map_or(output.base_price(), |market| market.suggested_price(output));
    let fee_bps = market.map_or(0, MootMarket::market_fee_bps);
    let gross = u64::from(output_units).saturating_mul(output_price);
    let revenue =
        gross.saturating_mul(BASIS_POINTS.saturating_sub(u64::from(fee_bps))) / BASIS_POINTS;
    let input_cost = input.map_or(0, |(good, units)| {
        u64::from(units)
            .saturating_mul(market.map_or(good.base_price(), |market| market.suggested_price(good)))
    });
    let wages = u64::from(kind.positions()).saturating_mul(FOUNDING_DAILY_WAGE);
    let costs = input_cost.saturating_add(wages);
    Some(if revenue >= costs {
        revenue.saturating_sub(costs).min(i64::MAX as u64) as i64
    } else {
        -(costs.saturating_sub(revenue).min(i64::MAX as u64) as i64)
    })
}

pub fn investor_threshold(strategy: BusinessStrategy) -> f32 {
    match strategy {
        BusinessStrategy::Opportunistic => 35.0,
        BusinessStrategy::Growth => 43.0,
        BusinessStrategy::Balanced => 49.0,
        BusinessStrategy::HighMargin => 52.0,
        BusinessStrategy::Cautious => 58.0,
    }
}

/// One resident's subjective reading of an advertised opportunity.
pub fn investor_score(
    opportunity: DevelopmentOpportunity,
    strategy: BusinessStrategy,
    site_quality: f32,
    market: Option<&MootMarket>,
    holdings: usize,
    person_seed: u64,
) -> f32 {
    if opportunity.score <= 15.0
        && matches!(
            opportunity.kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::FishermansHut
                | SettlementBuildingKind::LivestockFarm
        )
    {
        // Good terrain is valuable only when somebody can use the output.
        // Keep the permit legal on the public board, but do not let an NPC's
        // quality/profit estimate override a town whose active and pending
        // extractor capacity already covers demonstrated local/export demand.
        return f32::NEG_INFINITY;
    }
    let profit = expected_daily_profit(opportunity.kind, site_quality, market).unwrap_or(0);
    let mut profit_signal =
        (profit as f32 / FOUNDING_DAILY_WAGE.max(1) as f32 * 12.0).clamp(-32.0, 32.0);
    if opportunity.score <= 15.0
        && matches!(
            opportunity.kind,
            SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery
        )
    {
        // A theoretical full-throughput margin cannot manufacture missing
        // inputs. One founding processor may speculate; once anticipated
        // capacity covers observed flow, a high output price alone cannot
        // queue empty duplicate shells.
        profit_signal = profit_signal.min(8.0);
    }
    let strategy_signal = match strategy {
        BusinessStrategy::Opportunistic => 10.0,
        BusinessStrategy::Growth => 6.0,
        BusinessStrategy::Balanced => 0.0,
        BusinessStrategy::HighMargin => profit_signal.max(0.0) * 0.25,
        BusinessStrategy::Cautious => {
            if profit < 0 {
                -10.0
            } else {
                2.0
            }
        }
    };
    let quality_signal = if matches!(
        opportunity.kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::LivestockFarm
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::StoneQuarry
    ) {
        (site_quality.clamp(0.0, 1.0) - 0.5) * 60.0
    } else {
        0.0
    };
    // Stable individual variation avoids a town full of equally informed
    // residents changing their minds on the same tick.
    let mixed = person_seed
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .rotate_left(u32::from(kind_order(opportunity.kind)));
    let personal = (mixed % 13) as f32 - 6.0;
    opportunity.score + profit_signal + strategy_signal + quality_signal + personal
        - holdings as f32 * 5.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::economy::TOWN_HALL_STONE_REQUIRED;

    #[test]
    fn a_large_towns_food_shortage_keeps_the_same_per_capita_investment_signal() {
        let opportunity = |scale: u32, supplied: bool| {
            let residents = 24 * scale;
            let output = if supplied { residents } else { residents / 2 };
            let signals = DevelopmentMarketSignals {
                residents,
                houses: (6 * scale) as usize,
                livestock_farms: (2 * scale) as usize,
                anticipated_meat_output: output * 3,
                recent_meat_output: output * 2,
                ..Default::default()
            };
            let economy = SettlementEconomy {
                observed_days: 5,
                recent_food_production: output as f32,
                reserve_days: if supplied { 3.0 } else { 0.0 },
                ..Default::default()
            };
            private_opportunities(
                signals,
                Some(&economy),
                None,
                &SettlementPolicies::default(),
            )
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::LivestockFarm)
            .unwrap()
        };
        let village = opportunity(1, false);
        let city = opportunity(10, false);
        assert!(
            (village.score - city.score).abs() < 0.001,
            "equal per-capita shortages must not become a hidden city-scale farm ceiling"
        );
        assert!(
            city.score > 15.0,
            "a starving city's food investment must remain open"
        );
        let supplied = opportunity(10, true);
        assert!(
            supplied.score <= 15.0,
            "demonstrated capacity still closes oversupply"
        );
        assert!(
            investor_score(supplied, BusinessStrategy::Opportunistic, 1.0, None, 0, 23)
                .is_infinite()
        );
    }

    #[test]
    fn hungry_towns_collect_a_days_existing_food_before_adding_extractors() {
        let economy = SettlementEconomy {
            observed_days: 5,
            unmet_food: 40,
            recent_food_production: 20.0,
            reserve_days: 0.0,
            ..Default::default()
        };
        let demand = DevelopmentMarketSignals {
            residents: 100,
            houses: 25,
            farms: 4,
            livestock_farms: 5,
            ..Default::default()
        };
        let score = |signals| {
            private_opportunities(
                signals,
                Some(&economy),
                None,
                &SettlementPolicies::default(),
            )
        };
        let before = score(demand);
        let stranded = score(DevelopmentMarketSignals {
            uncollected_food: 100,
            ..demand
        });
        for kind in [
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::FishermansHut,
            SettlementBuildingKind::LivestockFarm,
        ] {
            assert!(
                before
                    .iter()
                    .find(|entry| entry.kind == kind)
                    .unwrap()
                    .score
                    > 15.0
            );
            assert!(
                stranded
                    .iter()
                    .find(|entry| entry.kind == kind)
                    .unwrap()
                    .score
                    <= 15.0,
                "food waiting for carts must not trigger duplicate {kind:?} shells"
            );
        }
        let exporting = score(DevelopmentMarketSignals {
            uncollected_food: 100,
            recent_wheat_export_demand: 50,
            ..demand
        });
        assert!(
            exporting
                .iter()
                .find(|entry| entry.kind == SettlementBuildingKind::Farmstead)
                .unwrap()
                .score
                > 15.0
        );
        assert_eq!(
            score(demand),
            before,
            "cleared backlog reopens normal investment"
        );
    }

    #[test]
    fn town_hall_stone_shortage_advertises_exactly_one_initial_quarry() {
        let shortage = DevelopmentMarketSignals {
            residents: 30,
            houses: 8,
            town_hall_stone_demand: TOWN_HALL_STONE_REQUIRED,
            ..Default::default()
        };
        let opportunity =
            private_opportunities(shortage, None, None, &SettlementPolicies::default())
                .into_iter()
                .find(|opportunity| opportunity.kind == SettlementBuildingKind::StoneQuarry)
                .unwrap();
        assert!(opportunity.score >= 80.0);
        assert!(opportunity.civic_priority);

        let approved = DevelopmentMarketSignals {
            stone_quarries: 1,
            ..shortage
        };
        let duplicate = private_opportunities(approved, None, None, &SettlementPolicies::default())
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::StoneQuarry)
            .unwrap();
        assert_eq!(duplicate.score, 5.0);

        let supplied = DevelopmentMarketSignals {
            stone_stock: TOWN_HALL_STONE_REQUIRED,
            stone_quarries: 0,
            ..shortage
        };
        let no_longer_needed =
            private_opportunities(supplied, None, None, &SettlementPolicies::default())
                .into_iter()
                .find(|opportunity| opportunity.kind == SettlementBuildingKind::StoneQuarry)
                .unwrap();
        assert_eq!(no_longer_needed.score, 5.0);
    }

    #[test]
    fn raw_wheat_backlog_does_not_bypass_existing_mill_viability() {
        let signals = DevelopmentMarketSignals {
            residents: 40,
            farms: 6,
            windmills: 1,
            houses: 10,
            wheat_stock: 80,
            recent_wheat_output: 24,
            ..Default::default()
        };
        let economy = SettlementEconomy {
            observed_days: 2,
            reserve_days: 0.2,
            recent_food_production: 5.0,
            ..Default::default()
        };
        let opportunities = private_opportunities(
            signals,
            Some(&economy),
            None,
            &SettlementPolicies::default(),
        );
        let mill = opportunities
            .iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
            .unwrap();
        assert_eq!(mill.score, 5.0);
        assert_ne!(opportunities[0].kind, SettlementBuildingKind::Windmill);
    }

    #[test]
    fn housing_remains_open_during_a_food_emergency() {
        let signals = DevelopmentMarketSignals {
            residents: 40,
            farms: 2,
            windmills: 1,
            houses: 2,
            ..Default::default()
        };
        let economy = SettlementEconomy {
            observed_days: 2,
            reserve_days: 0.0,
            ..Default::default()
        };
        let opportunities = private_opportunities(
            signals,
            Some(&economy),
            None,
            &SettlementPolicies::default(),
        );
        assert!(opportunities
            .iter()
            .any(|opportunity| opportunity.kind == SettlementBuildingKind::House));
    }

    #[test]
    fn livestock_is_a_real_food_opportunity_and_existing_capacity_closes_the_signal() {
        let policies = SettlementPolicies::default();
        let economy = SettlementEconomy {
            observed_days: 2,
            reserve_days: 0.0,
            recent_food_production: 0.0,
            ..Default::default()
        };
        let hungry = DevelopmentMarketSignals {
            residents: 12,
            houses: 3,
            ..Default::default()
        };
        let initial = private_opportunities(hungry, Some(&economy), None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::LivestockFarm)
            .unwrap();
        assert!(initial.score >= 100.0);

        let supplied = DevelopmentMarketSignals {
            livestock_farms: 2,
            anticipated_meat_output: 12,
            recent_meat_output: 24,
            ..hungry
        };
        let covered = private_opportunities(supplied, Some(&economy), None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::LivestockFarm)
            .unwrap();
        assert!(covered.score <= 15.0);
    }

    #[test]
    fn anticipated_lumber_capacity_closes_a_construction_boom_signal() {
        let mut signals = DevelopmentMarketSignals {
            residents: 200,
            lumber_huts: 0,
            construction_wood_demand: 640,
            ..Default::default()
        };
        let policies = SettlementPolicies::default();
        let initial = private_opportunities(signals, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::LumberjackHut)
            .unwrap();
        assert!(initial.score >= 60.0);

        // Two hundred residents can sensibly staff four huts. Pending huts
        // count as anticipated supply, so the same finite worksite bill cannot
        // approve forty more firms before the first ones open.
        signals.lumber_huts = 4;
        let supplied = private_opportunities(signals, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::LumberjackHut)
            .unwrap();
        assert_eq!(supplied.score, 5.0);
    }

    #[test]
    fn anticipated_extractors_close_startup_hunger_but_exports_reopen_farms() {
        let policies = SettlementPolicies::default();
        let economy = SettlementEconomy {
            observed_days: 2,
            reserve_days: 0.0,
            recent_food_production: 0.0,
            ..Default::default()
        };
        let covered = DevelopmentMarketSignals {
            residents: 15,
            farms: 2,
            anticipated_wheat_output: 16,
            recent_wheat_output: 32,
            houses: 4,
            ..Default::default()
        };
        let local = private_opportunities(covered, Some(&economy), None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Farmstead)
            .unwrap();
        assert!(local.score <= 15.0);
        assert!((0..100).all(|seed| {
            investor_score(local, BusinessStrategy::Opportunistic, 1.0, None, 0, seed)
                < investor_threshold(BusinessStrategy::Opportunistic)
        }));

        let exporting = DevelopmentMarketSignals {
            recent_wheat_export_demand: 12,
            ..covered
        };
        let export = private_opportunities(exporting, Some(&economy), None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Farmstead)
            .unwrap();
        assert!(
            export.score > local.score,
            "real external demand must reopen a fertile town's farm opportunity"
        );
    }

    #[test]
    fn bread_scarcity_cannot_queue_repeated_empty_bakeries() {
        let policies = SettlementPolicies::default();
        let prospective = DevelopmentMarketSignals {
            residents: 200,
            farms: 2,
            windmills: 1,
            bakeries: 0,
            ..Default::default()
        };
        let first = private_opportunities(prospective, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Bakery)
            .unwrap();
        assert_eq!(first.score, 30.0);

        let already_approved = DevelopmentMarketSignals {
            bakeries: 1,
            ..prospective
        };
        let next = private_opportunities(already_approved, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Bakery)
            .unwrap();
        assert_eq!(next.score, 5.0);
        assert!((0..100).all(|seed| {
            investor_score(next, BusinessStrategy::Opportunistic, 0.5, None, 0, seed)
                < investor_threshold(BusinessStrategy::Opportunistic)
        }));
    }

    #[test]
    fn pending_mill_without_wheat_flow_cannot_sell_hypothetical_full_throughput() {
        let signals = DevelopmentMarketSignals {
            residents: 20,
            farms: 1,
            windmills: 1,
            houses: 5,
            ..Default::default()
        };
        let mill = private_opportunities(
            signals,
            None,
            Some(&MootMarket::founding()),
            &SettlementPolicies::default(),
        )
        .into_iter()
        .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
        .unwrap();
        assert_eq!(mill.score, 8.0);
        assert!((0..100).all(|seed| {
            investor_score(mill, BusinessStrategy::Opportunistic, 0.5, None, 0, seed)
                < investor_threshold(BusinessStrategy::Opportunistic)
        }));
    }

    #[test]
    fn finite_flour_stock_is_amortised_instead_of_reapproved_every_round() {
        let policies = SettlementPolicies::default();
        let mut signals = DevelopmentMarketSignals {
            residents: 80,
            farms: 4,
            windmills: 2,
            flour_stock: 160,
            recent_flour_output: 16,
            houses: 20,
            ..Default::default()
        };

        // A stockpile alone does not prove that three existing bakeries can
        // obtain inputs, operate profitably and sell output. It therefore
        // cannot approve a fourth speculative processor.
        signals.bakeries = 3;
        let still_open = private_opportunities(signals, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Bakery)
            .unwrap();
        assert_eq!(still_open.score, 5.0);

        signals.bakeries = 4;
        let covered = private_opportunities(signals, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Bakery)
            .unwrap();
        assert_eq!(covered.score, 5.0);
    }

    #[test]
    fn severe_homelessness_advertises_houses_before_downstream_processors() {
        let signals = DevelopmentMarketSignals {
            residents: 80,
            farms: 4,
            windmills: 2,
            bakeries: 1,
            flour_stock: 80,
            recent_flour_output: 16,
            houses: 4,
            ..Default::default()
        };
        let opportunities =
            private_opportunities(signals, None, None, &SettlementPolicies::default());
        assert_eq!(opportunities[0].kind, SettlementBuildingKind::House);
        let bakery = opportunities
            .iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Bakery)
            .unwrap();
        assert!(
            bakery.score > 0.0,
            "private speculation should remain legal at full permit price"
        );
        assert!(!bakery.civic_priority);
    }

    #[test]
    fn observed_input_flow_adds_processors_only_beyond_anticipated_capacity() {
        let policies = SettlementPolicies::default();
        let covered = DevelopmentMarketSignals {
            residents: 40,
            farms: 2,
            windmills: 1,
            wheat_stock: 8,
            houses: 10,
            ..Default::default()
        };
        let covered_mill = private_opportunities(covered, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
            .unwrap();
        assert_eq!(covered_mill.score, 5.0);

        let backlog = DevelopmentMarketSignals {
            // Sustained upstream flow alone is not enough: requested Flour
            // must exceed the current mill's exposed output capacity too.
            recent_wheat_output: 50,
            recent_windmill_input: 36,
            recent_windmill_sales: 18,
            recent_windmill_profit: 100,
            completed_windmills: 1,
            active_windmill_output_capacity: 18,
            ..covered
        };
        let mut market = MootMarket::founding();
        market.purchase_recording_demand(Good::Flour, 25, u64::MAX, None, None);
        market.begin_new_day();
        market.purchase_recording_demand(Good::Flour, 25, u64::MAX, None, None);
        let extra_mill = private_opportunities(backlog, None, Some(&market), &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
            .unwrap();
        assert!(extra_mill.score >= 60.0);
    }

    #[test]
    fn sustained_expensive_flour_invites_one_independent_competitor() {
        let mut market = MootMarket::founding();
        market.set_targets(40, 0);
        market.consign(
            shared::economy::MarketSeller::Business(shared::components::BuildingId(701)),
            Good::Wheat,
            20,
            Good::Wheat.base_price(),
        );
        market.consign(
            shared::economy::MarketSeller::Business(shared::components::BuildingId(702)),
            Good::Flour,
            6,
            6 * shared::economy::PENNIES_PER_COIN,
        );
        market.purchase_recording_demand(
            Good::Flour,
            4,
            6 * shared::economy::PENNIES_PER_COIN,
            None,
            None,
        );
        market.begin_new_day();
        market.purchase_recording_demand(
            Good::Flour,
            4,
            6 * shared::economy::PENNIES_PER_COIN,
            None,
            None,
        );

        let signals = DevelopmentMarketSignals {
            residents: 40,
            farms: 3,
            windmills: 1,
            completed_windmills: 1,
            houses: 10,
            wheat_stock: 20,
            recent_wheat_output: 12,
            recent_windmill_sales: 2,
            recent_windmill_profit: 800,
            ..Default::default()
        };
        let opportunity =
            private_opportunities(signals, None, Some(&market), &SettlementPolicies::default())
                .into_iter()
                .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
                .unwrap();
        assert!(opportunity.score >= 60.0);
        assert!(opportunity.requires_independent_owner);
        let board = replicated_opportunity_board(&[opportunity], None, SettlementTier::Hamlet);
        let advertised = board
            .opportunities
            .iter()
            .find(|entry| entry.kind == SettlementBuildingKind::Windmill)
            .unwrap();
        assert!(
            advertised.subsidized,
            "the Hall must subsidize the competitive Windmill permit"
        );
        assert!(
            advertised.requires_independent_owner,
            "the player-facing permit must preserve the independent-entrant rule"
        );
        assert_eq!(
            minimum_startup_capital(SettlementBuildingKind::Windmill, Some(&market)),
            Good::Wheat.base_price().saturating_mul(260) / 100 + FOUNDING_DAILY_WAGE,
            "the approved challenger must fund one input batch and a prudent opening payroll even when the market is briefly unquoted"
        );

        let pending_challenger = DevelopmentMarketSignals {
            windmills: 2,
            ..signals
        };
        let blocked = private_opportunities(
            pending_challenger,
            None,
            Some(&market),
            &SettlementPolicies::default(),
        )
        .into_iter()
        .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
        .unwrap();
        assert_eq!(blocked.score, 5.0);
        assert!(!blocked.requires_independent_owner);

        let high_prices_survived_challenge = DevelopmentMarketSignals {
            windmills: 2,
            completed_windmills: 2,
            unproven_windmill: false,
            ..signals
        };
        let next_competitor = private_opportunities(
            high_prices_survived_challenge,
            None,
            Some(&market),
            &SettlementPolicies::default(),
        )
        .into_iter()
        .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
        .unwrap();
        assert!(
            next_competitor.score >= 60.0 && next_competitor.requires_independent_owner,
            "if independent owners keep Flour expensive, the Hall may incentivize another entrant"
        );
    }

    #[test]
    fn lossmaking_processor_blocks_another_competitive_copy() {
        let mut market = MootMarket::founding();
        market.consign(
            shared::economy::MarketSeller::Business(shared::components::BuildingId(701)),
            Good::Flour,
            1,
            Good::Flour.base_price().saturating_mul(3),
        );
        market.purchase_recording_demand(Good::Flour, 8, u64::MAX, None, None);
        market.begin_new_day();
        market.purchase_recording_demand(Good::Flour, 8, u64::MAX, None, None);
        let signals = DevelopmentMarketSignals {
            residents: 20,
            farms: 3,
            windmills: 2,
            completed_windmills: 2,
            recent_wheat_output: 24,
            recent_windmill_sales: 2,
            recent_windmill_profit: 100,
            lossmaking_windmills: 1,
            houses: 5,
            ..Default::default()
        };

        let mill =
            private_opportunities(signals, None, Some(&market), &SettlementPolicies::default())
                .into_iter()
                .find(|opportunity| opportunity.kind == SettlementBuildingKind::Windmill)
                .unwrap();
        assert_eq!(mill.score, 5.0);
    }

    #[test]
    fn player_board_keeps_every_private_permit_visible_without_inventing_subsidies() {
        let board = replicated_opportunity_board(
            &[],
            Some(DevelopmentOpportunity {
                kind: SettlementBuildingKind::Market,
                score: 95.0,
                civic_priority: true,
                requires_independent_owner: false,
            }),
            SettlementTier::Village,
        );
        for kind in FOUNDING_PRIVATE_KINDS {
            let offer = board
                .opportunities
                .iter()
                .find(|offer| offer.kind == kind)
                .unwrap_or_else(|| panic!("missing player permit {kind:?}"));
            assert_eq!(offer.score, 0);
            assert!(!offer.subsidized);
        }
        assert!(board
            .opportunities
            .iter()
            .any(|offer| offer.kind == SettlementBuildingKind::Market));
        assert!(board
            .opportunities
            .iter()
            .any(|offer| offer.kind == SettlementBuildingKind::Tavern));
        assert!(!board
            .opportunities
            .iter()
            .any(|offer| offer.kind == SettlementBuildingKind::Church));

        let hamlet = replicated_opportunity_board(
            &[],
            Some(DevelopmentOpportunity {
                kind: SettlementBuildingKind::Market,
                score: 95.0,
                civic_priority: true,
                requires_independent_owner: false,
            }),
            SettlementTier::Hamlet,
        );
        assert!(!hamlet.opportunities.iter().any(|offer| matches!(
            offer.kind,
            SettlementBuildingKind::Market
                | SettlementBuildingKind::Tavern
                | SettlementBuildingKind::Church
        )));

        let town = replicated_opportunity_board(&[], None, SettlementTier::Town);
        for kind in SettlementBuildingKind::PLAYER_PERMIT_KINDS {
            assert!(
                town.opportunities.iter().any(|offer| offer.kind == kind),
                "the Town permit board omitted {kind:?}"
            );
        }
    }

    #[test]
    fn a_private_tavern_opens_with_a_real_pantry_budget() {
        let capital = minimum_startup_capital(SettlementBuildingKind::Tavern, None);
        let expected = FOUNDING_DAILY_WAGE
            + Good::Bread.base_price() * 2
            + Good::Meat.base_price()
            + Good::Wheat.base_price();
        assert_eq!(capital, expected);
        assert!(capital > FOUNDING_DAILY_WAGE);
    }

    #[test]
    fn a_storage_hall_opens_with_porter_runway_and_trial_cargo_cash() {
        assert_eq!(
            minimum_startup_capital(SettlementBuildingKind::StorageHall, None),
            4 * FOUNDING_DAILY_WAGE
        );
    }

    #[test]
    fn a_tavern_is_not_subsidised_before_meal_supply_exists() {
        let policies = SettlementPolicies::default();
        let empty = DevelopmentMarketSignals {
            residents: 40,
            houses: 10,
            farms: 2,
            windmills: 1,
            ..Default::default()
        };
        let premature = private_opportunities(empty, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Tavern)
            .unwrap();
        assert_eq!(premature.score, 5.0);
        assert!(!premature.civic_priority);

        let supplied = DevelopmentMarketSignals {
            bread_stock: 8,
            ..empty
        };
        let useful = private_opportunities(supplied, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::Tavern)
            .unwrap();
        assert!(useful.score >= 60.0);
        assert!(useful.civic_priority);
    }

    #[test]
    fn storage_is_branch_infrastructure_not_a_founding_trade() {
        let policies = SettlementPolicies::default();
        let tiny = DevelopmentMarketSignals {
            residents: 8,
            farms: 1,
            windmills: 1,
            houses: 2,
            wheat_stock: 30,
            ..Default::default()
        };
        let premature = private_opportunities(tiny, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::StorageHall)
            .unwrap();
        assert_eq!(premature.score, 4.0);

        let established = DevelopmentMarketSignals {
            farms: 2,
            windmills: 1,
            stranded_output_bulk: 300,
            stranded_output_value: 30 * shared::economy::PENNIES_PER_COIN,
            ..tiny
        };
        let useful = private_opportunities(established, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::StorageHall)
            .unwrap();
        assert!(useful.score > premature.score);

        let already_supplied = DevelopmentMarketSignals {
            storage_halls: 1,
            active_storage_free_bulk: established.stranded_output_bulk,
            ..established
        };
        let duplicate = private_opportunities(already_supplied, None, None, &policies)
            .into_iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::StorageHall)
            .unwrap();
        assert_eq!(duplicate.score, 4.0);
    }

    #[test]
    fn fishing_is_a_repeatable_competing_food_investment() {
        let signals = DevelopmentMarketSignals {
            residents: 80,
            farms: 3,
            fishers: 1,
            houses: 20,
            ..Default::default()
        };
        let economy = SettlementEconomy {
            observed_days: 2,
            reserve_days: 0.0,
            recent_food_production: 5.0,
            ..Default::default()
        };
        let opportunities = private_opportunities(
            signals,
            Some(&economy),
            None,
            &SettlementPolicies::default(),
        );
        let fishing = opportunities
            .iter()
            .find(|opportunity| opportunity.kind == SettlementBuildingKind::FishermansHut)
            .unwrap();
        assert!(fishing.score >= 60.0);
    }

    #[test]
    fn mothballed_food_site_does_not_block_emergency_competition() {
        let signals = DevelopmentMarketSignals {
            residents: 30,
            houses: 8,
            recoverable_fishers: 1,
            ..Default::default()
        };
        let economy = SettlementEconomy {
            observed_days: 3,
            reserve_days: 0.0,
            recent_food_production: 0.0,
            unmet_food: 30,
            ..Default::default()
        };
        let fishing = private_opportunities(
            signals,
            Some(&economy),
            None,
            &SettlementPolicies::default(),
        )
        .into_iter()
        .find(|opportunity| opportunity.kind == SettlementBuildingKind::FishermansHut)
        .unwrap();

        assert!(fishing.score >= 100.0, "score was {}", fishing.score);
    }

    #[test]
    fn poor_sites_deter_cautious_owners_without_becoming_illegal() {
        let opportunity = DevelopmentOpportunity {
            kind: SettlementBuildingKind::Farmstead,
            score: 52.0,
            civic_priority: false,
            requires_independent_owner: false,
        };
        let cautious = investor_score(opportunity, BusinessStrategy::Cautious, 0.05, None, 0, 7);
        let opportunistic = investor_score(
            opportunity,
            BusinessStrategy::Opportunistic,
            0.05,
            None,
            0,
            7,
        );
        assert!(cautious < investor_threshold(BusinessStrategy::Cautious));
        assert!(opportunistic > cautious);
    }
}
