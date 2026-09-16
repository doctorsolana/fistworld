use super::*;
use shared::components::{BuildingId, SettlementId};
use shared::economy::MarketSeller;

fn imported_wheat() -> MootMarket {
    let mut market = MootMarket::founding();
    market.consign(
        MarketSeller::Treasury(SettlementId(7)),
        Good::Wheat,
        100,
        20,
    );
    market.record_unmet_demand(Good::Flour, 20, 0, 20, 80);
    market
}

#[test]
fn imported_inputs_and_funded_customers_support_a_real_processor() {
    let market = imported_wheat();
    let reading = InvestmentMarket::new(&market);
    let plan = reading
        .restart_plan(SettlementBuildingKind::Windmill, 1.0, 100, 0, None, None)
        .unwrap();
    assert!(plan.output_units > 0 && plan.output_units <= 18);
    assert!(plan.daily_profit > 0);
    assert!(plan.working_cash >= 200);
    assert!(plan.asking_price <= 80);
    assert_eq!(
        market.listed_units(Good::Wheat),
        100,
        "reading may not buy inputs"
    );
}

#[test]
fn unpriced_hunger_and_an_unaffordable_batch_do_not_prove_a_restart() {
    let mut market = MootMarket::founding();
    market.consign(
        MarketSeller::Treasury(SettlementId(7)),
        Good::Wheat,
        100,
        120,
    );
    market.record_unmet_demand(Good::Flour, 20, 0, 20, 80);
    assert!(InvestmentMarket::new(&market)
        .restart_plan(SettlementBuildingKind::Windmill, 1.0, 100, 0, None, None)
        .is_none());
    let mut unfunded = MootMarket::founding();
    unfunded.consign(
        MarketSeller::Treasury(SettlementId(7)),
        Good::Wheat,
        100,
        20,
    );
    unfunded.record_unmet_demand(Good::Flour, 20, 0, 0, 80);
    assert!(InvestmentMarket::new(&unfunded)
        .restart_plan(SettlementBuildingKind::Windmill, 1.0, 100, 0, None, None)
        .is_none());
}

#[test]
fn one_stocked_batch_is_not_forecast_as_an_entire_shift() {
    let mut market = MootMarket::founding();
    market.consign(MarketSeller::Treasury(SettlementId(7)), Good::Flour, 2, 20);
    market.record_unmet_demand(Good::Bread, 100, 0, 100, 100);
    let plan = InvestmentMarket::new(&market)
        .restart_plan(SettlementBuildingKind::Bakery, 1.0, 100, 0, None, None)
        .unwrap();
    assert_eq!(plan.output_units, 4);
    assert!(plan.daily_profit <= 4 * 100 - 100 - 40);
}

#[test]
fn supply_cost_uses_the_whole_batch_price_ladder() {
    let mut market = MootMarket::founding();
    market.consign(MarketSeller::Business(BuildingId(1)), Good::Wheat, 1, 5);
    market.consign(MarketSeller::Business(BuildingId(2)), Good::Wheat, 10, 100);
    let reading = InvestmentMarket::new(&market);
    assert_eq!(reading.purchase_cost(Good::Wheat, 3), Some(205));
    assert_eq!(reading.purchase_cost(Good::Wheat, 12), None);
}

#[test]
fn accepted_restart_reserves_its_demand_for_the_rest_of_the_daily_review() {
    let mut market = imported_wheat();
    market.withdraw_unmet_demand(Good::Flour, 20, 0, 20, 80);
    market.record_unmet_demand(Good::Flour, 4, 0, 4, 80);
    let mut reading = InvestmentMarket::new(&market);
    let plan = reading
        .restart_plan(SettlementBuildingKind::Windmill, 1.0, 100, 0, None, None)
        .unwrap();
    assert_eq!(plan.output_units, 4);
    reading.reserve_restart(SettlementBuildingKind::Windmill, plan);
    assert!(reading
        .restart_plan(SettlementBuildingKind::Windmill, 1.0, 100, 0, None, None)
        .is_none());
}
