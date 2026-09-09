//! Business evidence used by settlement permit demand.
//!
//! Physical inventory survives insolvency; productive capacity counts only
//! funded active positions. These rules collect evidence without approving a
//! permit or changing any account.

use crate::world::village::development_market::DevelopmentMarketSignals;
use shared::components::{SettlementBuilding, SettlementBuildingKind};
use shared::economy::{
    BusinessAccount, BusinessCondition, BusinessStaffingPolicy, BusinessState, Good,
    GoodsInventory, MootMarket,
};

/// Add one site's physical stock, capacity and recent operating evidence.
/// The caller selects the settlement and visits each site exactly once.
pub(super) fn accumulate_business_signals(
    signals: &mut DevelopmentMarketSignals,
    building: &SettlementBuilding,
    condition: Option<&BusinessCondition>,
    inventory: Option<&GoodsInventory>,
    account: Option<&BusinessAccount>,
    staffing: Option<&BusinessStaffingPolicy>,
    market: Option<&MootMarket>,
) {
    // Physical stock remains market information after a firm stops
    // operating. Hiding a liquidator's full store makes the permit
    // system construct a replacement into the same unresolved glut.
    if let Some(inventory) = inventory {
        if building.kind != SettlementBuildingKind::House {
            signals.uncollected_food = signals
                .uncollected_food
                .saturating_add(inventory.edible_amount());
        }
        signals.wheat_stock = signals
            .wheat_stock
            .saturating_add(inventory.amount(Good::Wheat));
        signals.flour_stock = signals
            .flour_stock
            .saturating_add(inventory.amount(Good::Flour));
        signals.bread_stock = signals
            .bread_stock
            .saturating_add(inventory.amount(Good::Bread));
        signals.meat_stock = signals
            .meat_stock
            .saturating_add(inventory.amount(Good::Meat));
        signals.wood_stock = signals
            .wood_stock
            .saturating_add(inventory.amount(Good::Wood));
        signals.stone_stock = signals
            .stone_stock
            .saturating_add(inventory.amount(Good::Stone));
        if building.kind == SettlementBuildingKind::StorageHall {
            if condition.is_none_or(|condition| condition.state.can_operate()) {
                signals.active_storage_free_bulk = signals
                    .active_storage_free_bulk
                    .saturating_add(inventory.free_bulk());
            }
        } else {
            for &output in crate::world::village::commerce::business_outputs(building.kind) {
                let units = inventory.amount(output);
                let bulk = units.saturating_mul(output.bulk_per_unit());
                signals.stranded_output_bulk = signals.stranded_output_bulk.saturating_add(bulk);
                let quote =
                    market.map_or(output.base_price(), |market| market.suggested_price(output));
                signals.stranded_output_value = signals
                    .stranded_output_value
                    .saturating_add(u64::from(units).saturating_mul(quote));
            }
        }
    }
    if condition.is_some_and(|condition| !condition.state.counts_as_active_capacity()) {
        if condition.is_some_and(|condition| condition.state.counts_as_recoverable_capacity()) {
            match building.kind {
                SettlementBuildingKind::Farmstead => signals.recoverable_farms += 1,
                SettlementBuildingKind::FishermansHut => signals.recoverable_fishers += 1,
                SettlementBuildingKind::LivestockFarm => signals.recoverable_livestock_farms += 1,
                SettlementBuildingKind::Windmill => signals.recoverable_windmills += 1,
                SettlementBuildingKind::Bakery => signals.recoverable_bakeries += 1,
                SettlementBuildingKind::LumberjackHut => signals.recoverable_lumber_huts += 1,
                SettlementBuildingKind::StoneQuarry => signals.recoverable_stone_quarries += 1,
                SettlementBuildingKind::StorageHall => signals.recoverable_storage_halls += 1,
                _ => {}
            }
        }
        return;
    }
    if let Some(capacity) =
        crate::world::village::rated_daily_production(building.kind, building.quality)
    {
        let enabled = staffing
            .copied()
            .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
            .target_for(building.kind);
        let staffed = capacity.output_units.saturating_mul(u32::from(enabled))
            / u32::from(building.kind.positions().max(1));
        let idle = capacity.output_units.saturating_sub(staffed);
        match building.kind {
            SettlementBuildingKind::Windmill => {
                signals.active_windmill_output_capacity = signals
                    .active_windmill_output_capacity
                    .saturating_add(staffed);
                signals.idle_windmill_output_capacity =
                    signals.idle_windmill_output_capacity.saturating_add(idle);
            }
            SettlementBuildingKind::Bakery => {
                signals.active_bakery_output_capacity = signals
                    .active_bakery_output_capacity
                    .saturating_add(staffed);
                signals.idle_bakery_output_capacity =
                    signals.idle_bakery_output_capacity.saturating_add(idle);
            }
            _ => {}
        }
    }
    if matches!(
        building.kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::LivestockFarm
    ) {
        // Completed shells are not food supply. Only the roster the
        // owner is actually willing to fund counts as anticipated
        // extractor capacity; otherwise several zero-worker farms can
        // make a starving town believe it already has enough food.
        let anticipated = crate::world::village::rated_daily_production(
            building.kind,
            building.quality,
        )
        .map_or(0, |capacity| {
            let enabled = staffing
                .copied()
                .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
                .target_for(building.kind);
            capacity
                .output_units
                .saturating_mul(u32::from(enabled))
                .div_ceil(u32::from(building.kind.positions().max(1)))
        });
        match building.kind {
            SettlementBuildingKind::Farmstead => {
                signals.anticipated_wheat_output =
                    signals.anticipated_wheat_output.saturating_add(anticipated);
            }
            SettlementBuildingKind::FishermansHut => {
                signals.anticipated_fish_output =
                    signals.anticipated_fish_output.saturating_add(anticipated);
            }
            SettlementBuildingKind::LivestockFarm => {
                signals.anticipated_meat_output =
                    signals.anticipated_meat_output.saturating_add(anticipated);
            }
            _ => {}
        }
    }
    if let Some(account) = account {
        if let Some(output) = crate::world::village::business_output(building.kind) {
            let dispatched = account
                .current_day
                .sold_units
                .saturating_add(account.previous_day.sold_units)
                .div_ceil(2)
                .saturating_mul(output.bulk_per_unit());
            signals.recent_logistics_bulk =
                signals.recent_logistics_bulk.saturating_add(dispatched);
        }
        if building.kind == SettlementBuildingKind::StorageHall {
            signals.recent_storage_cost = signals
                .recent_storage_cost
                .saturating_add(account.current_day.wage_expense)
                .saturating_add(account.previous_day.wage_expense);
        }
        let recent = account
            .current_day
            .produced_units
            .saturating_add(account.previous_day.produced_units);
        match building.kind {
            SettlementBuildingKind::Farmstead => {
                signals.recent_wheat_output = signals.recent_wheat_output.saturating_add(recent);
            }
            SettlementBuildingKind::FishermansHut => {
                signals.recent_fish_output = signals.recent_fish_output.saturating_add(recent);
            }
            SettlementBuildingKind::LivestockFarm => {
                signals.recent_meat_output = signals.recent_meat_output.saturating_add(recent);
            }
            SettlementBuildingKind::Windmill => {
                // A management label may leave `New` during the
                // opening day. Capacity is not proven until one full
                // trading day has actually closed; otherwise several
                // permit reviews can approve successors before the
                // first mill has an auditable result.
                signals.unproven_windmill |= condition
                    .is_some_and(|condition| condition.state == BusinessState::New)
                    || account.previous_day.day == u32::MAX;
                signals.recent_flour_output = signals.recent_flour_output.saturating_add(recent);
                signals.recent_windmill_input = signals.recent_windmill_input.saturating_add(
                    account
                        .current_day
                        .purchased_input_units
                        .saturating_add(account.previous_day.purchased_input_units),
                );
                signals.recent_windmill_sales = signals.recent_windmill_sales.saturating_add(
                    account
                        .current_day
                        .sold_units
                        .saturating_add(account.previous_day.sold_units),
                );
                signals.recent_windmill_profit = signals
                    .recent_windmill_profit
                    .saturating_add(account.current_day.profit())
                    .saturating_add(account.previous_day.profit());
                if !condition.is_some_and(|condition| condition.state == BusinessState::New)
                    && account.current_day.day != u32::MAX
                    && account.previous_day.day != u32::MAX
                    && account
                        .current_day
                        .profit()
                        .saturating_add(account.previous_day.profit())
                        <= 0
                {
                    signals.lossmaking_windmills += 1;
                }
            }
            SettlementBuildingKind::Bakery => {
                signals.unproven_bakery |= condition
                    .is_some_and(|condition| condition.state == BusinessState::New)
                    || account.previous_day.day == u32::MAX;
                signals.recent_bread_output = signals.recent_bread_output.saturating_add(recent);
                signals.recent_bakery_input = signals.recent_bakery_input.saturating_add(
                    account
                        .current_day
                        .purchased_input_units
                        .saturating_add(account.previous_day.purchased_input_units),
                );
                signals.recent_bakery_sales = signals.recent_bakery_sales.saturating_add(
                    account
                        .current_day
                        .sold_units
                        .saturating_add(account.previous_day.sold_units),
                );
                signals.recent_bakery_profit = signals
                    .recent_bakery_profit
                    .saturating_add(account.current_day.profit())
                    .saturating_add(account.previous_day.profit());
                if !condition.is_some_and(|condition| condition.state == BusinessState::New)
                    && account.current_day.day != u32::MAX
                    && account.previous_day.day != u32::MAX
                    && account
                        .current_day
                        .profit()
                        .saturating_add(account.previous_day.profit())
                        <= 0
                {
                    signals.lossmaking_bakeries += 1;
                }
            }
            _ => {}
        }
    }
}
