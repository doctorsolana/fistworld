//! Opt-in snapshots of existing authoritative accounts and physical stock.
//! Stock is an observation, not a complete lifetime material-conservation ledger.
use bevy::prelude::*;
use serde_json::{Value, json};
use shared::{components::*, economy::*};
use std::collections::BTreeMap;

pub(super) fn snapshot(world: &mut World) -> Value {
    let money = crate::world::economic_accounting::money_breakdown(world);
    let mut stock = BTreeMap::<&str, u64>::new();
    for inventory in world.query::<&GoodsInventory>().iter(world) {
        for good in Good::ALL {
            *stock.entry(good.label()).or_default() += u64::from(inventory.amount(good));
        }
    }
    let mut businesses: Vec<_> = world.query::<(
        &BuildingId, &SettlementBuilding, &BuildingOf, &BusinessAccount,
        Option<&OperatedBy>, Option<&GoodsInventory>, Option<&BusinessCondition>,
        Option<&BusinessWagePolicy>, Option<&BusinessStaffingPolicy>,
    )>().iter(world).map(|(id, building, of, account, company, inventory, condition, wage, staffing)| {
        json!({"id":id.0,"kind":building.kind,"settlement":of.0.0,"company":company.map(|of|of.0.0),
            "account":account,"inventory":inventory,"condition":condition,"wage":wage,"staffing":staffing})
    }).collect();
    businesses.sort_by_key(|site| site["id"].as_u64());
    let mut companies: Vec<_> = world
        .query::<(
            &CompanyId,
            &Company,
            &CompanyAccount,
            Option<&CompanyManagementPolicy>,
            Option<&CompanyDecisionHistory>,
        )>()
        .iter(world)
        .map(|(id, company, account, policy, decisions)| {
            json!({"id":id.0,"name":company.name,"account":account,"policy":policy,
            "decisions":decisions.map(|history|history.entries())})
        })
        .collect();
    companies.sort_by_key(|company| company["id"].as_u64());
    json!({"money_total":money.total(),"new_villager_endowment":STARTING_VILLAGER_MONEY,
        "money_by_owner":{"wallets":money.wallets,"treasuries":money.treasuries,"companies":money.companies,
            "households":money.households,"construction_escrow":money.construction_escrow,
            "trade_escrow":money.trade_escrow,"regional_road_escrow":money.regional_road_escrow,
            "port_construction_escrow":money.port_construction_escrow,"port_haul_escrow":money.port_haul_escrow,"clearing":money.clearing},
        "physical_inventory_totals":stock,"businesses":businesses,"companies":companies})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_preserve_stock_and_count_accounts_without_treating_book_value_as_cash() {
        let mut world = World::new();
        let mut stock = GoodsInventory::new(100);
        stock.add(Good::Wood, 7);
        let original = stock.clone();
        let cargo = world.spawn(stock).id();
        world.spawn(Wallet::new(125));
        world.spawn(CompanyAccount {
            cash: 200,
            book_value: 900,
            ..default()
        });
        world.spawn(BusinessAccount {
            unposted_company_capital: 75,
            book_value: 900,
            ..default()
        });
        let count = world.entities().len();
        let first = snapshot(&mut world);
        let second = snapshot(&mut world);
        assert_eq!(first, second);
        assert_eq!(first["money_total"], 400);
        assert_eq!(first["physical_inventory_totals"]["Wood"], 7);
        assert_eq!(world.get::<GoodsInventory>(cargo), Some(&original));
        assert_eq!(world.entities().len(), count);
    }
}
