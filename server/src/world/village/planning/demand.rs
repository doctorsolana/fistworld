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

/// Processing permits depend on an operating upstream building, not merely an
/// approved plot. Pending sites still count for duplicate suppression in
/// `have`, but a mill cannot process a Farmstead's promise and a bakery cannot
/// bake with an unfinished windmill.
pub(super) fn processing_upstream_is_complete(
    kind: SettlementBuildingKind,
    completed: &HashMap<SettlementBuildingKind, usize>,
) -> bool {
    let has = |upstream| completed.get(&upstream).copied().unwrap_or(0) > 0;
    match kind {
        SettlementBuildingKind::Windmill => has(SettlementBuildingKind::Farmstead),
        SettlementBuildingKind::Bakery => has(SettlementBuildingKind::Windmill),
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
