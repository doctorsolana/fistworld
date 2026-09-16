//! Permit admission limits, upstream completion and civic/food priorities.

use crate::world::village::*;

/// A settlement can approve distinct permits concurrently, but it cannot turn
/// every newly arrived resident into an independent construction crew on the
/// same morning. Capacity grows with population and stays bounded while this
/// region is tactically embodied; completed sites immediately free a slot.
pub(super) const CONSTRUCTION_CREW_RESIDENTS_PER_SITE: u32 = 12;

pub(super) const MIN_CONCURRENT_SETTLEMENT_WORKSITES: usize = 3;

pub(super) const MAX_CONCURRENT_SETTLEMENT_WORKSITES: usize = 12;

pub(super) fn has_planned_food_extractor(have: &HashMap<SettlementBuildingKind, usize>) -> bool {
    [
        SettlementBuildingKind::Farmstead,
        SettlementBuildingKind::FishermansHut,
        SettlementBuildingKind::LivestockFarm,
    ]
    .into_iter()
    .any(|kind| have.get(&kind).copied().unwrap_or(0) > 0)
}

pub(in crate::world::village) fn concurrent_worksite_capacity(residents: u32) -> usize {
    (residents.div_ceil(CONSTRUCTION_CREW_RESIDENTS_PER_SITE) as usize).clamp(
        MIN_CONCURRENT_SETTLEMENT_WORKSITES,
        MAX_CONCURRENT_SETTLEMENT_WORKSITES,
    )
}

pub(crate) fn development_pipeline_has_capacity(
    residents: u32,
    worksites: usize,
    connectors: usize,
) -> bool {
    worksites.saturating_add(connectors) < concurrent_worksite_capacity(residents)
}

/// A local completed upstream workplace may support one opening trial. A
/// town importing its inputs can instead establish a processor against actual
/// affordable stock and funded output demand; it need not duplicate the
/// exporting town's farms or mills. Neither an unfinished plot nor a future
/// trade promise supplies a startup batch.
pub(super) fn processing_inputs_are_available(
    kind: SettlementBuildingKind,
    completed: &HashMap<SettlementBuildingKind, usize>,
    market: Option<&crate::world::village::development_market::investment::InvestmentMarket>,
) -> bool {
    let has = |upstream| completed.get(&upstream).copied().unwrap_or(0) > 0;
    match kind {
        SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => {
            let local = match kind {
                SettlementBuildingKind::Windmill => has(SettlementBuildingKind::Farmstead),
                _ => has(SettlementBuildingKind::Windmill),
            };
            local
                || market.is_some_and(|market| {
                    market
                        .restart_plan(kind, 1.0, market.hiring_wage(), 0, None, None)
                        .is_some()
                })
        }
        _ => true,
    }
}

/// Tier infrastructure is requested only after survival/housing shortages are
/// satisfied. The charter influences where it goes, not whether demand and the
/// current tier justify it.
pub(super) fn next_civic_need(
    tier: shared::components::SettlementTier,
    existing: &HashMap<SettlementBuildingKind, usize>,
) -> Option<SettlementBuildingKind> {
    let has = |kind| existing.get(&kind).copied().unwrap_or(0) > 0;
    match tier {
        shared::components::SettlementTier::Village => {
            if !has(SettlementBuildingKind::Market) {
                Some(SettlementBuildingKind::Market)
            } else {
                None
            }
        }
        shared::components::SettlementTier::Town => {
            (!has(SettlementBuildingKind::Church)).then_some(SettlementBuildingKind::Church)
        }
        _ => None,
    }
}

/// A coastal settlement should diversify its second food workplace instead of
/// building Farmsteads forever merely because the shortage model continues to
/// return `Farmstead`. The shoreline search remains authoritative: inland
/// settlements fall straight back to the requested farm.
pub(in crate::world::village) fn should_try_complementary_fishing(
    requested: Option<SettlementBuildingKind>,
    planned_farms: usize,
    planned_fishers: usize,
) -> bool {
    // Keep advancing the bounded shoreline search while houses or another
    // urgent permit temporarily leads the shortage model. Waiting until that
    // model asks for Food again lets a migration wave fill the only usable
    // waterfront before the one-ring-at-a-time search ever reaches it. A
    // successful coast claim pauses just one ordinary permit and then this
    // condition switches off permanently.
    requested == Some(SettlementBuildingKind::Farmstead)
        && planned_farms > 0
        && planned_fishers == 0
}

#[cfg(test)]
mod processor_input_tests {
    use super::*;
    use crate::world::village::development_market::investment::InvestmentMarket;

    #[test]
    fn an_importing_town_can_open_a_processor_without_local_upstream_buildings() {
        let mut market = MootMarket::founding();
        market.consign(
            shared::economy::MarketSeller::Treasury(shared::components::SettlementId(1)),
            Good::Wheat,
            40,
            20,
        );
        market.record_unmet_demand(Good::Flour, 12, 0, 12, 80);
        assert!(processing_inputs_are_available(
            SettlementBuildingKind::Windmill,
            &HashMap::new(),
            Some(&InvestmentMarket::new(&market))
        ));
    }

    #[test]
    fn imports_must_be_physical_and_affordable_before_the_upstream_gate_opens() {
        let mut market = MootMarket::founding();
        market.record_unmet_demand(Good::Bread, 20, 0, 20, 80);
        assert!(!processing_inputs_are_available(
            SettlementBuildingKind::Bakery,
            &HashMap::new(),
            Some(&InvestmentMarket::new(&market))
        ));
        market.consign(
            shared::economy::MarketSeller::Treasury(shared::components::SettlementId(1)),
            Good::Flour,
            40,
            300,
        );
        assert!(!processing_inputs_are_available(
            SettlementBuildingKind::Bakery,
            &HashMap::new(),
            Some(&InvestmentMarket::new(&market))
        ));
    }
}
