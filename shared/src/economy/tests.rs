//! Cross-domain economic invariants.

use super::*;
use bevy::prelude::*;

#[test]
fn new_heroes_begin_with_twenty_coins_without_changing_villager_money() {
    assert_eq!(Wallet::founding_hero().balance(), 20 * PENNIES_PER_COIN);
    assert_eq!(Wallet::founding_villager().balance(), 10 * PENNIES_PER_COIN);
}

#[test]
fn site_consolidation_never_erases_direct_company_capital() {
    let mut account = CompanyAccount {
        cash: 1_000,
        contributed_capital: 1_000,
        ..default()
    };
    account.refresh_from_sites(
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        CompanyDayLedger::empty(2),
        Some(CompanyDayLedger::empty(1)),
    );
    assert_eq!(account.contributed_capital, 1_000);
    account.refresh_from_sites(
        2,
        0,
        0,
        1_500,
        0,
        0,
        0,
        CompanyDayLedger::empty(2),
        Some(CompanyDayLedger::empty(1)),
    );
    assert_eq!(account.contributed_capital, 1_500);
}

#[test]
fn personal_inventory_holds_four_wood_bundles() {
    assert_eq!(capacity::VILLAGER, Good::Wood.bulk_per_unit() * 4);
}

#[test]
fn porter_cart_capacity_is_large_and_inventory_resizing_is_lossless() {
    assert_eq!(capacity::PORTER, capacity::VILLAGER * 6);
    assert_eq!(BusinessSalePolicy::default().max_units_per_collection, 64);
    assert_eq!(PorterCartState::for_used_bulk(0).load_slots, 0);
    assert_eq!(PorterCartState::for_used_bulk(1).load_slots, 1);
    assert_eq!(PorterCartState::for_used_bulk(48).load_slots, 1);
    assert_eq!(PorterCartState::for_used_bulk(49).load_slots, 2);
    assert_eq!(PorterCartState::for_used_bulk(96).load_slots, 2);

    let mut inventory = GoodsInventory::new(capacity::VILLAGER);
    inventory.resize_bulk_capacity(capacity::PORTER);
    assert_eq!(inventory.add(Good::Wood, 24), 24);
    inventory.resize_bulk_capacity(capacity::VILLAGER);
    assert_eq!(inventory.amount(Good::Wood), 24);
    assert_eq!(inventory.bulk_capacity(), capacity::PORTER);
}

#[test]
fn unlike_goods_compete_for_the_same_physical_space() {
    let mut inventory = GoodsInventory::new(12);

    assert_eq!(inventory.add(Good::Food, 4), 4);
    assert_eq!(inventory.add(Good::Wood, 9), 2);
    assert_eq!(inventory.amount(Good::Food), 4);
    assert_eq!(inventory.amount(Good::Wood), 2);
    assert_eq!(inventory.used_bulk(), 12);
    assert_eq!(inventory.free_bulk(), 0);
}

#[test]
fn partitioned_public_storage_keeps_each_goods_space_independent() {
    let mut inventory = GoodsInventory::new_partitioned(12);

    assert_eq!(inventory.add(Good::Wood, 4), 3);
    assert_eq!(inventory.free_units(Good::Wood), 0);
    assert_eq!(inventory.add(Good::Bread, 12), 12);
    assert_eq!(inventory.amount(Good::Wood), 3);
    assert_eq!(inventory.amount(Good::Bread), 12);
    assert_eq!(inventory.free_units(Good::Bread), 0);
    assert_eq!(inventory.free_units(Good::Flour), 12);
    assert_eq!(inventory.used_bulk(), 24);
    assert_eq!(
        inventory.bulk_capacity(),
        12 * u32::try_from(Good::COUNT).unwrap()
    );
}

#[test]
fn converting_a_full_legacy_store_preserves_every_stack() {
    let mut inventory = GoodsInventory::new(12);
    assert_eq!(inventory.add(Good::Wood, 3), 3);

    inventory.resize_partitioned_bulk_capacity(8);

    assert_eq!(inventory.amount(Good::Wood), 3);
    assert_eq!(inventory.partition_bulk_capacity(), Some(12));
    assert_eq!(inventory.add(Good::Bread, 12), 12);
}

#[test]
fn company_resource_rules_are_independent_between_settlements() {
    let north = crate::components::SettlementId(7);
    let south = crate::components::SettlementId(8);
    let mut policies = CompanyBranchPolicies::default();
    policies.set_resource(
        north,
        Good::Wheat,
        CompanyResourcePolicy {
            retain_units: 40,
            sell_excess: false,
        },
    );
    policies.set_resource(
        south,
        Good::Wheat,
        CompanyResourcePolicy {
            retain_units: 5,
            sell_excess: true,
        },
    );

    assert_eq!(policies.resource(north, Good::Wheat).retain_units, 40);
    assert!(!policies.resource(north, Good::Wheat).sell_excess);
    assert_eq!(policies.resource(south, Good::Wheat).retain_units, 5);
    assert!(policies.resource(south, Good::Wheat).sell_excess);
    assert_eq!(
        policies.resource(north, Good::Bread),
        CompanyResourcePolicy::default()
    );
    assert_eq!(policies.branches().len(), 2);
    let public = CompanyResourcePolicy {
        retain_units: 20,
        sell_excess: true,
    };
    assert_eq!(public.public_surplus(100, 10), 70);
    assert_eq!(public.public_surplus(15, 10), 0);
    assert_eq!(
        policies.resource(north, Good::Wheat).public_surplus(100, 0),
        0
    );
}

#[test]
fn a_partial_transfer_is_lossless() {
    let mut carrier = GoodsInventory::new(40);
    let mut nearly_full_hut = GoodsInventory::new(12);
    assert_eq!(carrier.add(Good::Wood, 8), 8);
    assert_eq!(nearly_full_hut.add(Good::Food, 8), 8);

    let before = carrier.amount(Good::Wood) + nearly_full_hut.amount(Good::Wood);
    let moved = carrier.transfer_to(&mut nearly_full_hut, Good::Wood, 8);
    let after = carrier.amount(Good::Wood) + nearly_full_hut.amount(Good::Wood);

    assert_eq!(moved, 1, "only one wood bundle fits in four bulk");
    assert_eq!(
        before, after,
        "a full destination must not eat the overflow"
    );
    assert_eq!(carrier.amount(Good::Wood), 7);
    assert_eq!(nearly_full_hut.amount(Good::Wood), 1);
}

#[test]
fn removal_never_underflows() {
    let mut inventory = GoodsInventory::new(12);
    inventory.add(Good::Iron, 2);
    assert_eq!(inventory.remove(Good::Iron, 99), 2);
    assert!(inventory.is_empty());
}

#[test]
fn meat_flour_and_bread_are_food_but_wheat_and_wool_are_not() {
    let mut inventory = GoodsInventory::new(40);
    inventory.add(Good::Food, 2);
    inventory.add(Good::Wheat, 3);
    inventory.add(Good::Flour, 2);
    inventory.add(Good::Bread, 1);
    inventory.add(Good::Meat, 2);
    inventory.add(Good::Wool, 2);
    inventory.add(Good::Wood, 2);

    assert_eq!(inventory.edible_amount(), 7);
    assert_eq!(inventory.remove_edible(4), 4);
    assert_eq!(inventory.amount(Good::Bread), 0);
    assert_eq!(inventory.amount(Good::Meat), 0);
    assert_eq!(inventory.amount(Good::Food), 1);
    assert_eq!(inventory.amount(Good::Flour), 2);
    assert_eq!(inventory.amount(Good::Wheat), 3);
    assert_eq!(inventory.amount(Good::Wood), 2);
    assert!(!Good::Wheat.is_edible());
    assert!(!Good::Wool.is_edible());
    assert!(Good::Meat.is_ready_to_eat());
    assert!(Good::Flour.is_edible());
    assert!(!Good::Flour.is_ready_to_eat());
    assert_eq!(Good::Bread.food_tier(), 2);
    assert_eq!(Good::TAVERN_INPUTS, [Good::Meat, Good::Bread, Good::Wheat]);
}

#[test]
fn accounting_good_and_carried_appearance_are_separate() {
    let mut inventory = GoodsInventory::new(12);
    inventory.add(Good::Food, 3);
    let load = CarriedLoad::from_inventory(&inventory);

    assert_eq!(load.good, Some(Good::Food));
    assert_eq!(
        load.visible_appearance(),
        Some(CarriedAppearance::FishBasket)
    );

    let presentation_override = CarriedLoad {
        appearance: Some(CarriedAppearance::StoneBundle),
        ..load
    };
    assert_eq!(
        presentation_override.good,
        Some(Good::Food),
        "changing presentation must not change accounting"
    );
    assert_eq!(
        presentation_override.visible_appearance(),
        Some(CarriedAppearance::StoneBundle),
        "an explicit producer-specific appearance must override the fallback"
    );
    assert_eq!(
        CarriedAppearance::default_for(Good::Wool),
        CarriedAppearance::WoolFleece
    );
    assert_eq!(
        CarriedAppearance::default_for(Good::Meat),
        CarriedAppearance::MeatHaunch
    );
    assert_eq!(CarriedAppearance::WoolFleece.label(), "Wool bale");
    assert_eq!(CarriedAppearance::MeatHaunch.label(), "Haunch of meat");
}

#[test]
fn market_consignment_pays_only_after_a_real_purchase() {
    let mut market = MootMarket::founding();
    let seller = MarketSeller::Business(crate::components::BuildingId(7));
    market.consign(seller, Good::Wheat, 6, 80);
    assert_eq!(market.seller_listed_units(seller, Good::Wheat), 6);

    let purchase = market.purchase(Good::Wheat, 3, 240, None, None);
    assert_eq!(
        purchase.trade,
        MarketTrade {
            units: 3,
            pennies: 240
        }
    );
    assert_eq!(purchase.fills.len(), 1);
    assert_eq!(purchase.fills[0].seller, seller);
    assert_eq!(purchase.fills[0].gross, 240);
    assert_eq!(purchase.fills[0].market_fee, 12);
    assert_eq!(market.seller_listed_units(seller, Good::Wheat), 3);
}

#[test]
fn contracted_purchase_never_substitutes_a_different_seller() {
    let mut market = MootMarket::founding();
    let contracted = MarketSeller::Business(crate::components::BuildingId(21));
    let cheaper_rival = MarketSeller::Business(crate::components::BuildingId(22));
    market.consign(contracted, Good::Stone, 8, 250);
    market.consign(cheaper_rival, Good::Stone, 8, 100);

    let purchase = market.purchase_from_seller(contracted, Good::Stone, 8, 2_000, Some(250));

    assert_eq!(purchase.trade.units, 8);
    assert_eq!(purchase.trade.pennies, 2_000);
    assert!(purchase.fills.iter().all(|fill| fill.seller == contracted));
    assert_eq!(market.seller_listed_units(contracted, Good::Stone), 0);
    assert_eq!(market.seller_listed_units(cheaper_rival, Good::Stone), 8);
}

#[test]
fn daily_market_flow_keeps_executed_prices_separate_and_resets() {
    let mut market = MootMarket::founding();
    let seller = MarketSeller::Business(crate::components::BuildingId(8));
    market.consign(seller, Good::Wheat, 4, 80);
    let consumer_purchase = market.purchase(Good::Wheat, 2, u64::MAX, None, None);
    let pool = market.pool(Good::Wheat);
    assert_eq!(
        pool.day.producer_units,
        u64::from(consumer_purchase.trade.units)
    );
    let seller_net = consumer_purchase.fills[0]
        .gross
        .saturating_sub(consumer_purchase.fills[0].market_fee);
    assert_eq!(pool.day.producer_coin, seller_net);
    assert_eq!(
        pool.day.consumer_units,
        u64::from(consumer_purchase.trade.units)
    );
    assert_eq!(pool.day.consumer_coin, consumer_purchase.trade.pennies);
    assert!(pool.day.high_bid >= pool.day.low_bid);
    assert!(pool.day.high_ask >= pool.day.low_ask);

    let closing_bid = pool.bid;
    let closing_ask = pool.ask;
    market.begin_new_day();
    let next_day = market.pool(Good::Wheat).day;
    assert_eq!(next_day.opening_bid, closing_bid);
    assert_eq!(next_day.opening_ask, closing_ask);
    assert_eq!(next_day.producer_units, 0);
    assert_eq!(next_day.consumer_units, 0);
    assert_eq!(market.pool(Good::Wheat).previous_day.consumer_units, 2);
}

#[test]
fn wholesale_collection_pays_producer_without_inventing_local_consumption() {
    let mut market = MootMarket::founding();
    let seller = MarketSeller::Business(crate::components::BuildingId(18));
    market.consign(seller, Good::Bread, 6, 10);

    let purchase = market.purchase_for_resale(Good::Bread, 4, 40, Some(10), None);
    assert_eq!(purchase.trade.units, 4);
    let flow = market.pool(Good::Bread).day;
    assert_eq!(flow.producer_units, 4);
    assert_eq!(flow.consumer_units, 0);
    assert_eq!(flow.consumer_coin, 0);
}

#[test]
fn market_distinguishes_missing_stock_from_rejected_prices() {
    let mut market = MootMarket::founding();
    let seller = MarketSeller::Business(crate::components::BuildingId(81));
    market.consign(seller, Good::Flour, 4, 600);

    let purchase = market.purchase_recording_demand(Good::Flour, 6, 600, None, None);
    assert_eq!(purchase.trade.units, 1);
    let flow = market.pool(Good::Flour).day;
    assert_eq!(flow.consumer_units, 1);
    assert_eq!(flow.unavailable_units, 2);
    assert_eq!(flow.unaffordable_units, 3);
    assert_eq!(flow.funded_unmet_units, 4);
    assert_eq!(flow.requested_units(), 6);
}

#[test]
fn best_offer_and_last_sale_are_distinct_market_quotes() {
    let mut market = MootMarket::founding();
    let expensive = MarketSeller::Business(crate::components::BuildingId(9));
    let cheap = MarketSeller::Business(crate::components::BuildingId(10));
    market.consign(expensive, Good::Food, 2, 140);
    market.consign(cheap, Good::Food, 1, 90);
    assert_eq!(market.pool(Good::Food).ask, 90);
    assert_eq!(market.pool(Good::Food).bid, 0);

    let purchase = market.purchase(Good::Food, 1, 90, None, None);
    assert_eq!(purchase.trade.units, 1);
    assert_eq!(market.pool(Good::Food).bid, 90);
    assert_eq!(market.pool(Good::Food).ask, 140);
}

#[test]
fn profit_is_revenue_less_real_expenses_and_never_opening_capital() {
    let mut account = BusinessAccount::with_capital(1_000);
    account.record_sale(1, 500, 25, 5);
    account.incur_wages(1, 200);
    account.record_input_purchase(1, 100, 2);

    assert_eq!(account.lifetime_profit(), 175);
    assert_eq!(account.retained_profit(), 175);
    assert_eq!(account.current_day.profit(), 175);
    assert_eq!(account.unposted_company_capital, 1_000);
    assert_eq!(account.contributed_capital, 1_000);
    assert_eq!(account.wage_arrears, 200);
}

#[test]
fn defaulted_wages_remove_the_claim_without_burning_firm_cash() {
    let mut account = BusinessAccount::with_capital(58);
    account.incur_wages(1, 100);

    assert_eq!(account.write_off_wage_claim(100), 100);
    assert_eq!(account.unposted_company_capital, 58);
    assert_eq!(account.wage_arrears, 0);
    assert_eq!(account.defaulted_wages, 100);
}

#[test]
fn completed_shift_wages_do_not_roll_an_open_ledger_backwards() {
    let mut account = BusinessAccount::default();
    account.record_sale(4, 500, 0, 5);
    account.roll_to_day(5);
    account.record_sale(5, 200, 0, 2);

    account.incur_completed_day_wages(4, 100);

    assert_eq!(account.current_day.day, 5);
    assert_eq!(account.current_day.gross_revenue, 200);
    assert_eq!(account.current_day.wage_expense, 0);
    assert_eq!(account.previous_day.day, 4);
    assert_eq!(account.previous_day.gross_revenue, 500);
    assert_eq!(account.previous_day.wage_expense, 100);
    assert_eq!(account.wage_arrears, 100);
}

#[test]
fn company_dividend_capacity_protects_payroll_inputs_and_liabilities() {
    let mut market = MootMarket::founding();
    market.consign(
        MarketSeller::Business(crate::components::BuildingId(91)),
        Good::Wheat,
        20,
        80,
    );
    let wage = BusinessWagePolicy {
        daily_wage: 100,
        ..default()
    };
    let management = BusinessManagementPolicy {
        payroll_reserve_days: 2,
        ..default()
    };
    let procurement = BusinessProcurementPolicy::none().with_rule(
        Good::Wheat,
        BusinessInputRule {
            enabled: true,
            coverage_days: 2,
            reorder_below: 2,
            target_units: 10,
            maximum_unit_price: 100,
        },
    );
    let reserve =
        business_working_capital(2, &wage, &management, &procurement, None, Some(&market));
    assert_eq!(reserve.payroll, 400);
    assert_eq!(reserve.inputs, 800);
    assert_eq!(reserve.operating_buffer, 200);

    let mut held_stock = [0; Good::COUNT];
    held_stock[Good::Wheat.index()] = 6;
    let partially_stocked = business_working_capital(
        2,
        &wage,
        &management,
        &procurement,
        Some(&held_stock),
        Some(&market),
    );
    assert_eq!(
        partially_stocked.inputs, 320,
        "cash protection covers only the missing input target"
    );

    let mut account = BusinessAccount::with_capital(2_000);
    account.record_sale(1, 3_000, 0, 30);
    account.incur_wages(1, 300);
    account.incur_profit_tax(1, 100);
    let mut company = CompanyAccount {
        cash: 5_000,
        wage_arrears: account.wage_arrears,
        tax_arrears: account.tax_arrears,
        ..default()
    };
    let protected = reserve.total_with_liabilities(&account);
    let draw = account
        .retained_profit()
        .min(company.cash.saturating_sub(protected));
    assert_eq!(draw, 2_600, "opening capital is not distributable profit");
    assert!(company.debit(draw));
    account.record_company_dividend(1, draw);
    assert!(company.cash >= reserve.total_with_liabilities(&account));
    assert_eq!(account.retained_profit(), 0);
}

#[test]
fn liquidation_markdown_keeps_food_listed_and_can_clear_below_the_reference_price() {
    let mut market = MootMarket::founding();
    let seller = MarketSeller::Business(crate::components::BuildingId(92));
    market.consign(seller, Good::Bread, 5, 200);
    for _ in 0..20 {
        market.markdown_seller(seller, 1_500);
    }
    assert_eq!(market.seller_listed_units(seller, Good::Bread), 5);
    assert_eq!(market.listed_edible_units(), 5);
    assert!(market.suggested_price(Good::Bread) < Good::Bread.base_price() / 4);
    assert!(market.suggested_price(Good::Bread) > 0);
}

#[test]
fn competing_price_excludes_the_reviewing_seller() {
    let mut market = MootMarket::founding();
    let first = MarketSeller::Business(crate::components::BuildingId(1));
    let second = MarketSeller::Business(crate::components::BuildingId(2));
    market.consign(first, Good::Bread, 4, 90);
    market.consign(second, Good::Bread, 4, 75);

    assert_eq!(market.best_competing_price(first, Good::Bread), Some(75));
    assert_eq!(market.best_competing_price(second, Good::Bread), Some(90));
}

#[test]
fn public_target_shortfall_is_a_signal_not_a_consignment_cap() {
    let mut market = MootMarket::founding();
    let seller = MarketSeller::Business(crate::components::BuildingId(190));
    assert_eq!(market.target_shortfall(Good::Wheat), 4);
    market.consign(seller, Good::Wheat, 3, Good::Wheat.base_price());
    assert_eq!(market.target_shortfall(Good::Wheat), 1);

    market.set_targets(12, 0);
    assert_eq!(market.target_shortfall(Good::Wheat), 29);
    assert_eq!(market.target_shortfall(Good::Stone), 4);
    market.consign(seller, Good::Wheat, 40, Good::Wheat.base_price() - 1);
    assert_eq!(market.target_shortfall(Good::Wheat), 0);
    assert_eq!(market.listed_units(Good::Wheat), 43);
}

#[test]
fn marketplace_deepens_wheat_planning_without_capping_offers() {
    let mut market = MootMarket::founding();
    market.set_targets_with_marketplace(16, 0, false);
    assert_eq!(market.pool(Good::Wheat).target_stock, 32);
    market.set_targets_with_marketplace(16, 0, true);
    assert_eq!(market.pool(Good::Wheat).target_stock, 96);
    assert_eq!(market.pool(Good::Food).target_stock, 96);
}

#[test]
fn public_goods_unlock_once_at_their_declared_market_tier() {
    let seller = MarketSeller::Business(crate::components::BuildingId(195));
    let mut market = MootMarket::founding();

    for good in Good::ALL {
        assert_eq!(
            market.can_trade(good),
            good != Good::Iron,
            "every current founding good except Iron should trade at the Moot",
        );
    }
    market.consign(seller, Good::Iron, 2, Good::Iron.base_price());
    assert_eq!(market.listed_units(Good::Iron), 0);
    assert_eq!(
        market.purchase_recording_demand(Good::Iron, 2, u64::MAX, None, None),
        MarketPurchase::default(),
    );
    assert_eq!(
        market.pool(Good::Iron).day.unmet_units(),
        0,
        "a legally locked good must not look like an economic shortage",
    );

    market.unlock_trade_tier(MarketTradeTier::Marketplace);
    assert!(!market.can_trade(Good::Iron));
    market.unlock_trade_tier(MarketTradeTier::PavedMarketplace);
    assert!(market.can_trade(Good::Iron));
    market.consign(seller, Good::Iron, 2, Good::Iron.base_price());
    assert_eq!(market.listed_units(Good::Iron), 2);

    market.unlock_trade_tier(MarketTradeTier::Moot);
    assert!(
        market.can_trade(Good::Iron),
        "an exchange unlock cannot regress and strand existing listings",
    );
}

#[test]
fn estate_transfer_preserves_the_goods_price_and_future_proceeds() {
    let person = MarketSeller::Person(crate::components::PersonId(93));
    let treasury = MarketSeller::Treasury(crate::components::SettlementId(94));
    let mut market = MootMarket::founding();
    market.consign(person, Good::Flour, 7, 61);

    assert_eq!(market.transfer_seller(person, treasury), 7);
    assert_eq!(market.seller_total_listed_units(person), 0);
    assert_eq!(market.seller_listed_units(treasury, Good::Flour), 7);
    let purchase = market.purchase(Good::Flour, 7, u64::MAX, None, None);
    assert_eq!(purchase.trade.pennies, 7 * 61);
    assert!(purchase
        .fills
        .iter()
        .all(|fill| fill.seller == treasury && fill.unit_price == 61));
}

#[test]
fn purchase_preview_is_exact_and_can_exclude_the_buyers_own_stock() {
    let mut market = MootMarket::founding();
    let buyer = MarketSeller::Business(crate::components::BuildingId(1));
    let rival = MarketSeller::Business(crate::components::BuildingId(2));
    market.consign(buyer, Good::Wheat, 5, 50);
    market.consign(rival, Good::Wheat, 5, 80);

    let preview = market.preview_purchase(Good::Wheat, 3, 240, Some(80), Some(buyer));
    assert_eq!(
        preview,
        MarketTrade {
            units: 3,
            pennies: 240
        }
    );
    let purchase = market.purchase(Good::Wheat, 3, 240, Some(80), Some(buyer));
    assert_eq!(purchase.trade, preview);
    assert_eq!(purchase.fills[0].seller, rival);
    assert_eq!(market.seller_listed_units(buyer, Good::Wheat), 5);
}

#[test]
fn inventory_reconciliation_never_leaves_phantom_market_offers() {
    let settlement = crate::components::SettlementId(3);
    let seller = MarketSeller::Business(crate::components::BuildingId(11));
    let mut market = MootMarket::founding();
    let mut inventory = GoodsInventory::new(40);
    inventory.add(Good::Wheat, 2);
    market.consign(MarketSeller::Treasury(settlement), Good::Wheat, 2, 70);
    market.consign(seller, Good::Wheat, 3, 80);

    market.reconcile_inventory(settlement, &inventory);

    assert_eq!(market.listed_units(Good::Wheat), 2);
    assert_eq!(
        market.seller_listed_units(seller, Good::Wheat),
        2,
        "the migration-created Treasury claim should be discarded first"
    );
    assert_eq!(
        market
            .purchase(Good::Wheat, 99, u64::MAX, None, None)
            .trade
            .units,
        2
    );
}

#[test]
fn only_needed_housing_is_free_while_businesses_have_a_floor() {
    use crate::components::SettlementBuildingKind;
    assert_eq!(permit_price(SettlementBuildingKind::House, 4, true), 0);
    assert!(permit_price(SettlementBuildingKind::Farmstead, 0, true) >= PENNIES_PER_COIN);
    assert_eq!(permit_price(SettlementBuildingKind::Windmill, 0, true), 0);
    assert!(permit_price(SettlementBuildingKind::Windmill, 0, false) > 0);
    assert!(
        permit_price(SettlementBuildingKind::Farmstead, 2, false)
            > permit_price(SettlementBuildingKind::Farmstead, 0, true)
    );
    assert!(
        permit_price_with_subsidy(SettlementBuildingKind::Farmstead, 0, true, 0)
            > permit_price_with_subsidy(
                SettlementBuildingKind::Farmstead,
                0,
                true,
                DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
            ),
        "the enacted subsidy must reduce requested business permit prices"
    );
    assert_eq!(
        permit_price_with_subsidy(SettlementBuildingKind::Farmstead, 0, false, 0),
        permit_price_with_subsidy(
            SettlementBuildingKind::Farmstead,
            0,
            false,
            MAXIMUM_BUSINESS_PERMIT_SUBSIDY_BPS,
        ),
        "speculative businesses must not receive a demand subsidy"
    );
}

#[test]
fn player_owned_amenity_permits_cost_real_money() {
    use crate::components::SettlementBuildingKind;
    assert_eq!(
        player_permit_price_with_subsidy(
            SettlementBuildingKind::Market,
            0,
            false,
            DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
        ),
        500
    );
    assert_eq!(
        player_permit_price_with_subsidy(
            SettlementBuildingKind::Tavern,
            0,
            false,
            DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
        ),
        400
    );
    assert_eq!(
        player_permit_price_with_subsidy(
            SettlementBuildingKind::Church,
            0,
            false,
            DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS,
        ),
        600
    );
}

#[test]
fn capital_spending_changes_book_value_without_reducing_operating_profit() {
    let mut account = BusinessAccount::with_project_funding(500, 0, 300, 2);
    account.record_sale(2, 200, 10, 1);
    account.incur_wages(2, 50);
    assert_eq!(account.book_value, 300);
    assert_eq!(account.capital_expenditures, 300);
    assert_eq!(account.current_day.capital_expenditures, 300);
    assert_eq!(account.current_day.profit(), 140);
}
