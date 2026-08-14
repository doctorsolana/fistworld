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
    pub windmills: usize,
    pub bakeries: usize,
    pub completed_windmills: usize,
    pub completed_bakeries: usize,
    pub unproven_windmill: bool,
    pub unproven_bakery: bool,
    pub lumber_huts: usize,
    pub houses: usize,
    pub wheat_stock: u32,
    pub flour_stock: u32,
    pub bread_stock: u32,
    pub wood_stock: u32,
    pub recent_wheat_output: u32,
    pub recent_flour_output: u32,
    pub recent_bread_output: u32,
    pub recent_windmill_input: u32,
    pub recent_windmill_sales: u32,
    pub recent_windmill_profit: i64,
    pub recent_bakery_input: u32,
    pub recent_bakery_sales: u32,
    pub recent_bakery_profit: i64,
    pub construction_wood_demand: u32,
}

impl DevelopmentMarketSignals {
    pub fn beds(self) -> usize {
        self.houses
            .saturating_mul(SettlementBuildingKind::House.housing_capacity() as usize)
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

const FOUNDING_PRIVATE_KINDS: [SettlementBuildingKind; 6] = [
    SettlementBuildingKind::House,
    SettlementBuildingKind::Farmstead,
    SettlementBuildingKind::FishermansHut,
    SettlementBuildingKind::Windmill,
    SettlementBuildingKind::Bakery,
    SettlementBuildingKind::LumberjackHut,
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
const PROCESSOR_MIN_UTILISATION_PERCENT: u32 = 60;
const PROCESSOR_MIN_SALES_PERCENT: u32 = 30;

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
    recent_input: u32,
    recent_sales: u32,
    recent_profit: i64,
) -> bool {
    if existing == 0 {
        return true;
    }
    let Some(capacity) = super::rated_daily_production(kind, 1.0) else {
        return false;
    };
    let window_input_capacity = (existing as u32)
        .saturating_mul(capacity.input_units())
        .saturating_mul(RECENT_OUTPUT_WINDOW_DAYS);
    let required_input = window_input_capacity
        .saturating_mul(PROCESSOR_MIN_UTILISATION_PERCENT)
        .div_ceil(100);
    let required_sales = window_input_capacity
        .saturating_mul(capacity.output_per_input())
        .saturating_mul(PROCESSOR_MIN_SALES_PERCENT)
        .div_ceil(100);
    recent_input >= required_input && recent_sales >= required_sales && recent_profit > 0
}

fn processor_market_facts(
    kind: SettlementBuildingKind,
    signals: DevelopmentMarketSignals,
) -> Option<(usize, usize, bool, u32, u32, i64)> {
    match kind {
        SettlementBuildingKind::Windmill => Some((
            signals.windmills,
            signals.completed_windmills,
            signals.unproven_windmill,
            signals.recent_windmill_sales,
            sustainable_daily_input(signals.recent_wheat_output, signals.wheat_stock),
            signals.recent_windmill_profit,
        )),
        SettlementBuildingKind::Bakery => Some((
            signals.bakeries,
            signals.completed_bakeries,
            signals.unproven_bakery,
            signals.recent_bakery_sales,
            sustainable_daily_input(signals.recent_flour_output, signals.flour_stock),
            signals.recent_bakery_profit,
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
    let Some((existing, completed, unproven, recent_sales, sustainable_input, recent_profit)) =
        processor_market_facts(kind, signals)
    else {
        return false;
    };
    // `existing` includes approved worksites. Only one challenger may be under
    // review, and a newly opened challenger gets several days to reveal its
    // effect before the same old monopoly evidence can approve a third plant.
    if existing == 0 || completed < existing || unproven || sustainable_input == 0 {
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
        return if signals.farms + signals.fishers == 0 {
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
            if signals.farms + signals.fishers == 0 {
                return 130.0;
            }
            let mill_capacity = signals.windmills.max(1) as u32
                * super::rated_daily_production(SettlementBuildingKind::Windmill, 1.0)
                    .map_or(1, |capacity| capacity.input_units());
            let raw_supply = signals
                .wheat_stock
                .saturating_add(signals.recent_wheat_output);
            let backlog = (raw_supply as f32 / mill_capacity as f32).clamp(0.0, 3.0);
            // Food pressure advertises extraction, but a growing pile of raw
            // Wheat tells investors that processing—not another field—is the
            // current bottleneck.
            (12.0 + pressure * 70.0 - backlog * 30.0 - signals.farms as f32 * 5.0).max(5.0)
        }
        SettlementBuildingKind::FishermansHut => {
            if signals.farms + signals.fishers == 0 {
                return 128.0;
            }
            // Fishing competes with farming as its own investment. It has no
            // Wheat-processing bottleneck, while repeated huts steadily make
            // the next shoreline claim less compelling. Geography makes the
            // final decision: inland applicants simply cannot secure a plot.
            (14.0 + pressure * 70.0 - signals.fishers as f32 * 8.0).max(5.0)
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
            if !processor_expansion_proven(
                SettlementBuildingKind::Windmill,
                signals.windmills,
                signals.recent_windmill_input,
                signals.recent_windmill_sales,
                signals.recent_windmill_profit,
            ) {
                return 5.0;
            }
            let capacity = signals.windmills as u32
                * super::rated_daily_production(SettlementBuildingKind::Windmill, 1.0)
                    .map_or(1, |capacity| capacity.input_units());
            let uncovered = daily_input.saturating_sub(capacity);
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
            if !processor_expansion_proven(
                SettlementBuildingKind::Bakery,
                signals.bakeries,
                signals.recent_bakery_input,
                signals.recent_bakery_sales,
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
        SettlementBuildingKind::Hall
        | SettlementBuildingKind::Market
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church => 0.0,
    };

    if kind == SettlementBuildingKind::House {
        return raw;
    }
    // The Hall still permits private speculation at full price, but it stops
    // subsidising rows of businesses while a large share of residents sleeps
    // rough. The first food extractor is exempt because shelter without any
    // food source is not a viable founding sequence.
    let first_food_source = signals.farms + signals.fishers == 0
        && matches!(
            kind,
            SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut
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
    for kind in [
        SettlementBuildingKind::House,
        SettlementBuildingKind::Farmstead,
        SettlementBuildingKind::FishermansHut,
        SettlementBuildingKind::Windmill,
        SettlementBuildingKind::Bakery,
        SettlementBuildingKind::LumberjackHut,
        SettlementBuildingKind::Market,
        SettlementBuildingKind::Tavern,
        SettlementBuildingKind::Church,
    ] {
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
        SettlementBuildingKind::Windmill => 3,
        SettlementBuildingKind::Bakery => 4,
        SettlementBuildingKind::LumberjackHut => 5,
        SettlementBuildingKind::Market => 6,
        SettlementBuildingKind::Tavern => 7,
        SettlementBuildingKind::Church => 8,
        SettlementBuildingKind::Hall => 9,
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

/// Cash escrowed with a processing permit so the finished firm can buy at
/// least one complete physical recipe batch and meet its first full-staffed
/// payroll. Extractors can begin with labour; processors cannot create their
/// first sale without a real purchased input.
pub fn minimum_startup_capital(kind: SettlementBuildingKind, market: Option<&MootMarket>) -> u64 {
    let Some(recipe) = super::processing_recipe(kind) else {
        return 0;
    };
    let input_price = market.map_or(recipe.input.base_price(), |market| {
        market.suggested_price(recipe.input)
    });
    let first_batch = u64::from(recipe.input_units).saturating_mul(input_price);
    let first_payroll = u64::from(kind.positions()).saturating_mul(FOUNDING_DAILY_WAGE);
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
    let profit = expected_daily_profit(opportunity.kind, site_quality, market).unwrap_or(0);
    let mut profit_signal =
        (profit as f32 / FOUNDING_DAILY_WAGE.max(1) as f32 * 12.0).clamp(-32.0, 32.0);
    if opportunity.score <= 5.0
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
            | SettlementBuildingKind::LumberjackHut
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
            // Sustained output, rather than a one-off stockpile, proves that
            // the existing mill's rated eighteen-input daily capacity is
            // insufficient and that the current firm is genuinely utilised.
            recent_wheat_output: 48,
            recent_windmill_input: 24,
            recent_windmill_sales: 12,
            recent_windmill_profit: 100,
            ..covered
        };
        let extra_mill = private_opportunities(backlog, None, None, &policies)
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
            Good::Wheat.base_price()
                + u64::from(SettlementBuildingKind::Windmill.positions()) * FOUNDING_DAILY_WAGE,
            "the approved challenger must open with its first input and payroll funded"
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
