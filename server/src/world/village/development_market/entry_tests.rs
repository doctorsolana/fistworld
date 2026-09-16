use super::*;

#[test]
fn viable_challenger_can_replace_a_failed_processor_but_a_pending_entrant_stops_duplicates() {
    let mut market = MootMarket::founding();
    market.consign(
        shared::economy::MarketSeller::Treasury(shared::components::SettlementId(1)),
        Good::Wheat,
        40,
        20,
    );
    market.record_unmet_demand(Good::Flour, 12, 0, 12, 80);
    let plan = investment::InvestmentMarket::new(&market)
        .restart_plan(SettlementBuildingKind::Windmill, 1.0, 100, 0, None, None)
        .unwrap();
    let failed = DevelopmentMarketSignals {
        residents: 12,
        housing_capacity: 12,
        wheat_stock: 40,
        recoverable_windmills: 1,
        mill_challenger_units: plan.output_units,
        ..Default::default()
    };
    let opportunity =
        private_opportunities(failed, None, Some(&market), &SettlementPolicies::default())
            .into_iter()
            .find(|entry| entry.kind == SettlementBuildingKind::Windmill)
            .unwrap();
    assert!(opportunity.requires_independent_owner);
    assert!(opportunity.score >= 60.0);
    let pending = DevelopmentMarketSignals {
        windmills: 1,
        ..failed
    };
    let blocked =
        private_opportunities(pending, None, Some(&market), &SettlementPolicies::default())
            .into_iter()
            .find(|entry| entry.kind == SettlementBuildingKind::Windmill)
            .unwrap();
    assert!(!blocked.requires_independent_owner);
    assert!(blocked.score <= 5.0);
}

#[test]
fn a_failed_incumbent_without_a_funded_viable_challenger_keeps_entry_cautious() {
    let signals = DevelopmentMarketSignals {
        residents: 12,
        housing_capacity: 12,
        wheat_stock: 40,
        recoverable_windmills: 1,
        ..Default::default()
    };
    let opportunity = private_opportunities(
        signals,
        None,
        Some(&MootMarket::founding()),
        &SettlementPolicies::default(),
    )
    .into_iter()
    .find(|entry| entry.kind == SettlementBuildingKind::Windmill)
    .unwrap();
    assert!(!opportunity.requires_independent_owner);
    assert!(opportunity.score <= 5.0);
}

#[test]
fn investors_cost_new_jobs_at_the_observed_local_wage() {
    let low = expected_daily_profit(SettlementBuildingKind::Windmill, 1.0, None, 100).unwrap();
    let high = expected_daily_profit(SettlementBuildingKind::Windmill, 1.0, None, 500).unwrap();
    assert_eq!(
        low - high,
        i64::from(SettlementBuildingKind::Windmill.positions()) * 400
    );
}
