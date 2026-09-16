//! Read-only authoritative cash census shared by connected acceptance journals
//! and economy laboratories. Book value, owed claims and sold goods are not cash;
//! earned-but-unpaid labour remains in its owning escrow until actually paid.
//! This explicit diagnostic traversal is never an ordinary per-frame game system.

use super::village::{self, InheritedBusinessCapital};
use bevy::prelude::*;
use shared::{
    components::Settlement,
    economy::{BusinessAccount, CompanyAccount, HouseholdEconomy, Wallet},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MoneyBreakdown {
    pub(crate) wallets: u64,
    pub(crate) treasuries: u64,
    pub(crate) companies: u64,
    pub(crate) households: u64,
    pub(crate) construction_escrow: u64,
    pub(crate) trade_escrow: u64,
    pub(crate) regional_road_escrow: u64,
    pub(crate) port_construction_escrow: u64,
    pub(crate) port_haul_escrow: u64,
    pub(crate) clearing: u64,
}

impl MoneyBreakdown {
    pub(crate) fn total(self) -> u64 {
        self.wallets
            .saturating_add(self.treasuries)
            .saturating_add(self.companies)
            .saturating_add(self.households)
            .saturating_add(self.construction_escrow)
            .saturating_add(self.trade_escrow)
            .saturating_add(self.regional_road_escrow)
            .saturating_add(self.port_construction_escrow)
            .saturating_add(self.port_haul_escrow)
            .saturating_add(self.clearing)
    }
}

/// Every penny must be in exactly one authoritative wallet, household purse,
/// company treasury, civic treasury or unfinished-business escrow after the
/// tick's event queue settles. `unposted_company_capital` is included only for the
/// brief construction/upgrade seam before it is posted to a company.
pub(crate) fn money_breakdown(world: &mut World) -> MoneyBreakdown {
    let wallets = world
        .query::<&Wallet>()
        .iter(world)
        .map(|wallet| wallet.balance())
        .sum::<u64>();
    let treasuries = world
        .query::<&Settlement>()
        .iter(world)
        .map(|settlement| settlement.treasury)
        .sum::<u64>();
    let company_cash = world
        .query::<&CompanyAccount>()
        .iter(world)
        .map(|account| account.cash)
        .sum::<u64>();
    let unposted_site_cash = world
        .query::<&BusinessAccount>()
        .iter(world)
        .map(|account| account.unposted_company_capital)
        .sum::<u64>();
    let household_cash = world
        .query::<&HouseholdEconomy>()
        .iter(world)
        .map(|household| household.pennies)
        .sum::<u64>();
    let construction_escrow = world
        .query::<&InheritedBusinessCapital>()
        .iter(world)
        .map(|capital| capital.0)
        .sum::<u64>()
        .saturating_add(
            world
                .get_resource::<crate::world::house_upgrades::HouseUpgradeProjects>()
                .map_or(0, |projects| projects.total_escrow_pennies()),
        );
    let trade_escrow = world
        .query::<&shared::components::CivicTradeContract>()
        .iter(world)
        .map(|contract| contract.escrow_cash)
        .sum::<u64>();
    let regional_road_escrow = world
        .query::<&crate::world::regional_roads::RegionalProject>()
        .iter(world)
        .map(|project| project.escrow_cash)
        .sum::<u64>();
    let port_construction_escrow = world
        .query::<&crate::world::ports::PortWorkProject>()
        .iter(world)
        .map(|project| project.labour_escrow)
        .sum::<u64>();
    let port_haul_escrow = world
        .query::<&crate::world::shipping::PortHaulJob>()
        .iter(world)
        .map(|job| job.fee_remaining)
        .sum::<u64>();
    let clearing = world
        .get_resource::<village::BusinessEventQueue>()
        .map_or(0, |queue| queue.pending_sale_gross());
    MoneyBreakdown {
        wallets,
        treasuries,
        companies: company_cash.saturating_add(unposted_site_cash),
        households: household_cash,
        construction_escrow,
        trade_escrow,
        regional_road_escrow,
        port_construction_escrow,
        port_haul_escrow,
        clearing,
    }
}

pub(crate) fn total_money(world: &mut World) -> u64 {
    money_breakdown(world).total()
}
