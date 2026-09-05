//! Exact money units, opening balances, wallets and price arithmetic.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// One displayed coin is one hundred internal pennies.
///
/// Money never uses floating point. Prices may be derived with floating-point
/// tuning curves on the authoritative server, but every debit, credit and
/// conservation check is exact integer arithmetic.
pub const PENNIES_PER_COIN: u64 = 100;

pub const STARTING_VILLAGER_COINS: u64 = 10;

pub const STARTING_VILLAGER_MONEY: u64 = STARTING_VILLAGER_COINS * PENNIES_PER_COIN;

/// A newly created player hero gets a slightly larger testable foothold than
/// an ordinary immigrant. This is granted once with the body, never when the
/// same live hero is re-adopted after reconnecting.
pub const STARTING_HERO_COINS: u64 = 20;

pub const STARTING_HERO_MONEY: u64 = STARTING_HERO_COINS * PENNIES_PER_COIN;

pub const STARTING_TREASURY_MONEY: u64 = 20 * PENNIES_PER_COIN;

pub const BASIS_POINTS: u64 = 10_000;

/// Convert an observed per-unit cost into the asking price needed to retain a
/// target markup after the local market fee. Keeping this arithmetic beside the
/// shared money constants lets operating firms and prospective investors use
/// exactly the same definition of a sustainable price.
pub fn sustainable_unit_price(
    estimated_unit_cost: u64,
    market_fee_bps: u16,
    target_margin_bps: u16,
) -> u64 {
    if estimated_unit_cost == 0 {
        return 1;
    }
    let after_fee = BASIS_POINTS
        .saturating_sub(u64::from(market_fee_bps))
        .max(1);
    estimated_unit_cost
        .saturating_mul(BASIS_POINTS)
        .div_ceil(after_fee)
        .saturating_mul(BASIS_POINTS.saturating_add(u64::from(target_margin_bps)))
        .div_ceil(BASIS_POINTS)
        .max(1)
}

/// Format internal pennies for player-facing panels without losing pennies.
pub fn format_money(pennies: u64) -> String {
    format!(
        "{}.{:02}",
        pennies / PENNIES_PER_COIN,
        pennies % PENNIES_PER_COIN
    )
}

/// Personal liquid money. Coin is not a [`Good`] and has no physical bulk.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Wallet {
    pennies: u64,
}

impl Wallet {
    pub const fn new(pennies: u64) -> Self {
        Self { pennies }
    }

    pub const fn founding_villager() -> Self {
        Self::new(STARTING_VILLAGER_MONEY)
    }

    pub const fn founding_hero() -> Self {
        Self::new(STARTING_HERO_MONEY)
    }

    pub const fn balance(self) -> u64 {
        self.pennies
    }

    pub fn can_afford(self, pennies: u64) -> bool {
        self.pennies >= pennies
    }

    pub fn debit(&mut self, pennies: u64) -> bool {
        if self.pennies < pennies {
            return false;
        }
        self.pennies -= pennies;
        true
    }

    pub fn credit(&mut self, pennies: u64) {
        self.pennies = self.pennies.saturating_add(pennies);
    }
}

pub(super) fn signed_difference(income: u64, expense: u64) -> i64 {
    if income >= expense {
        income.saturating_sub(expense).min(i64::MAX as u64) as i64
    } else {
        -(expense.saturating_sub(income).min(i64::MAX as u64) as i64)
    }
}
