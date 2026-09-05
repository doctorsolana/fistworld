//! Shared automatic and player permit pricing under civic subsidies.

use super::{
    BASIS_POINTS, DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS, MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS,
    PENNIES_PER_COIN,
};

/// Automatic permit price. Housing is civic approval and remains free;
/// every business permit has a positive floor.
pub fn permit_price(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    needed_by_settlement: bool,
) -> u64 {
    permit_price_with_subsidy(
        kind,
        applicant_holdings,
        needed_by_settlement,
        DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
    )
}

/// Automatic permit price under the settlement's enacted growth subsidy.
/// Only requested private businesses receive the discount. Housing remains
/// free, civic projects remain public and speculative firms pay full price.
pub fn permit_price_with_subsidy(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    needed_by_settlement: bool,
    subsidy_bps: u16,
) -> u64 {
    use crate::components::SettlementBuildingKind;
    if kind == SettlementBuildingKind::House {
        return 0;
    }
    if kind == SettlementBuildingKind::Hall {
        return u64::MAX;
    }
    // Turning raw Wheat into the settlement's first edible grain supply is
    // emergency infrastructure. When the opportunity board explicitly asks
    // for a Windmill, the Hall waives the land-use fee rather than taking the
    // processor's scarce opening input cash. Speculative mills still pay.
    if kind == SettlementBuildingKind::Windmill && needed_by_settlement {
        return 0;
    }
    let base: u64 = match kind {
        SettlementBuildingKind::Farmstead
        | SettlementBuildingKind::FishermansHut
        | SettlementBuildingKind::LivestockFarm => 300,
        SettlementBuildingKind::LumberjackHut => 250,
        SettlementBuildingKind::StoneQuarry => 350,
        SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => 250,
        SettlementBuildingKind::StorageHall => 400,
        // Civic blockouts are settlement-requested progression infrastructure.
        // Their physical wood still has to be supplied; pricing can be revisited
        // with the wider ownership model without creating a progression lock.
        SettlementBuildingKind::Market
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church => return 0,
        SettlementBuildingKind::House | SettlementBuildingKind::Hall => unreachable!(),
    };
    let holdings_multiplier_bps =
        BASIS_POINTS.saturating_add((applicant_holdings as u64).saturating_mul(BASIS_POINTS / 2));
    let need_multiplier_bps = if needed_by_settlement {
        BASIS_POINTS.saturating_sub(u64::from(
            subsidy_bps.min(MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS),
        ))
    } else {
        BASIS_POINTS
    };
    base.saturating_mul(holdings_multiplier_bps)
        .saturating_mul(need_multiplier_bps)
        .div_ceil(BASIS_POINTS.saturating_mul(BASIS_POINTS))
        .max(PENNIES_PER_COIN)
}

/// Player-facing permit price, including privately commissioned amenities.
///
/// Automatic civic projects still use [`permit_price_with_subsidy`] and cost
/// their Reeve nothing. A player who chooses to own the same unlocked service
/// building pays a real land-use fee; demand may discount it but can never
/// decide whether the permit is legal.
pub fn player_permit_price_with_subsidy(
    kind: crate::components::SettlementBuildingKind,
    applicant_holdings: usize,
    requested_by_settlement: bool,
    subsidy_bps: u16,
) -> u64 {
    use crate::components::SettlementBuildingKind;
    let base: u64 = match kind {
        SettlementBuildingKind::Market => 500,
        SettlementBuildingKind::Tavern => 400,
        SettlementBuildingKind::Church => 600,
        _ => {
            return permit_price_with_subsidy(
                kind,
                applicant_holdings,
                requested_by_settlement,
                subsidy_bps,
            );
        }
    };
    let holdings_multiplier_bps =
        BASIS_POINTS.saturating_add((applicant_holdings as u64).saturating_mul(BASIS_POINTS / 2));
    let demand_multiplier_bps = if requested_by_settlement {
        BASIS_POINTS.saturating_sub(u64::from(
            subsidy_bps.min(MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS),
        ))
    } else {
        BASIS_POINTS
    };
    base.saturating_mul(holdings_multiplier_bps)
        .saturating_mul(demand_multiplier_bps)
        .div_ceil(BASIS_POINTS.saturating_mul(BASIS_POINTS))
        .max(PENNIES_PER_COIN)
}
