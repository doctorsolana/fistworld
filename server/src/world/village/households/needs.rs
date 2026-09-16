//! Bounded household needs and exact, income-neutral contribution arithmetic.

use super::*;
use shared::components::PersonId;

/// A Wood bundle provides sixteen small hearth uses. A shared hearth uses four
/// per day, plus one per occupant: four residents consume half a bundle/day.
/// Fuel affects the inspectable warmth reading, not food or work eligibility.
pub(super) const HEARTH_UNITS_PER_WOOD: u32 = 16;
pub(super) const PERSONAL_RESERVE_RATION_DAYS: u64 = 2;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ProvisionNeeds {
    pub food: u32,
    pub fuel: u32,
    pub today_food: u32,
    pub today_fuel: u32,
}

impl ProvisionNeeds {
    pub fn for_home(
        residents: usize,
        economy: &HouseholdEconomy,
        pantry: &GoodsInventory,
        hearth: &HearthState,
    ) -> Self {
        let food = (residents as u32)
            .saturating_mul(u32::from(economy.pantry_target_days))
            .saturating_sub(pantry.edible_amount());
        let fuel = hearth.deficit(residents, economy.fuel_target_days, pantry);
        Self {
            food,
            fuel,
            today_food: (residents as u32)
                .saturating_sub(pantry.edible_amount())
                .min(food),
            today_fuel: hearth.deficit(residents, economy.fuel_target_days.min(1), pantry),
        }
    }
}

pub(super) fn daily_hearth_units(residents: usize) -> u32 {
    if residents == 0 {
        0
    } else {
        4u32.saturating_add(residents as u32)
    }
}

/// Partly burned fuel stays at the physical hearth when a household moves.
#[derive(Component, Debug)]
pub(crate) struct HearthState {
    pub credit: u32,
    /// Actual bundles removed at this hearth; retained for conservation journals.
    pub consumed_wood: u64,
    last_day: u32,
}

impl Default for HearthState {
    fn default() -> Self {
        Self {
            credit: 0,
            consumed_wood: 0,
            last_day: u32::MAX,
        }
    }
}

impl HearthState {
    pub fn vacant_until(&mut self, day: u32) {
        self.last_day = day;
    }

    pub fn advance(
        &mut self,
        day: u32,
        residents: usize,
        pantry: &mut GoodsInventory,
        economy: &mut HouseholdEconomy,
    ) {
        if self.last_day == u32::MAX {
            self.last_day = day;
            return;
        }
        let elapsed = day.saturating_sub(self.last_day);
        if elapsed == 0 {
            return;
        }
        self.last_day = day;
        let daily = daily_hearth_units(residents);
        if daily == 0 {
            return;
        }
        let wanted = u64::from(daily) * u64::from(elapsed);
        let missing = wanted.saturating_sub(u64::from(self.credit));
        let wood = missing
            .div_ceil(u64::from(HEARTH_UNITS_PER_WOOD))
            .min(u64::from(u32::MAX)) as u32;
        let burned = pantry.remove(Good::Wood, wood);
        self.consumed_wood = self.consumed_wood.saturating_add(u64::from(burned));
        let energy = u64::from(self.credit) + u64::from(burned) * u64::from(HEARTH_UNITS_PER_WOOD);
        let served = energy.min(wanted);
        self.credit = energy.saturating_sub(served) as u32;
        let last_day_served = served.saturating_sub(u64::from(elapsed - 1) * u64::from(daily));
        economy.fuel_satisfaction = (last_day_served * 100 / u64::from(daily)).min(100) as u8;
        if served == wanted {
            economy.fuel_shortage_days = 0;
        } else {
            let short_days = u64::from(elapsed).saturating_sub(served / u64::from(daily));
            let prior = if served >= u64::from(daily) {
                0
            } else {
                economy.fuel_shortage_days
            };
            economy.fuel_shortage_days =
                prior.saturating_add(short_days.min(u64::from(u16::MAX)) as u16);
        }
    }

    pub fn deficit(&self, residents: usize, days: u8, pantry: &GoodsInventory) -> u32 {
        let target = daily_hearth_units(residents).saturating_mul(u32::from(days));
        target
            .saturating_sub(self.credit)
            .div_ceil(HEARTH_UNITS_PER_WOOD)
            .saturating_sub(pantry.amount(Good::Wood))
    }
}

/// Proportional to spendable cash, with largest-remainder rounding. Reordering
/// members never changes contributions; equal penny ties rotate by day.
pub(super) fn fair_contributions(
    members: &[(PersonId, u64)],
    wanted: u64,
    day: u32,
) -> Vec<(PersonId, u64)> {
    let total: u128 = members.iter().map(|(_, cash)| u128::from(*cash)).sum();
    if total == 0 || wanted == 0 {
        return Vec::new();
    }
    let target = u128::from(wanted).min(total);
    let mut parts: Vec<_> = members
        .iter()
        .map(|(id, cash)| {
            let weighted = target * u128::from(*cash);
            (*id, (weighted / total) as u64, weighted % total, *cash)
        })
        .collect();
    parts.sort_by_key(|(id, _, remainder, _)| {
        (
            std::cmp::Reverse(*remainder),
            shared::worldgen::splitmix64(id.0 ^ u64::from(day)),
        )
    });
    let apportioned: u128 = parts.iter().map(|(_, cash, _, _)| u128::from(*cash)).sum();
    let mut pennies = target - apportioned;
    for (_, cash, _, available) in &mut parts {
        if pennies == 0 {
            break;
        }
        if *cash < *available {
            *cash += 1;
            pennies -= 1;
        }
    }
    debug_assert_eq!(pennies, 0);
    parts.into_iter().map(|(id, cash, ..)| (id, cash)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contributions_are_proportional_conserved_and_order_independent() {
        let members = [(PersonId(1), 100), (PersonId(2), 300), (PersonId(3), 0)];
        let mut parts = fair_contributions(&members, 101, 7);
        parts.sort_by_key(|part| part.0);
        assert_eq!(
            parts,
            vec![(PersonId(1), 25), (PersonId(2), 76), (PersonId(3), 0)]
        );
        let mut reversed =
            fair_contributions(&members.into_iter().rev().collect::<Vec<_>>(), 101, 7);
        reversed.sort_by_key(|part| part.0);
        assert_eq!(parts, reversed);
        assert_eq!(
            fair_contributions(&members, 500, 7)
                .iter()
                .map(|(_, amount)| amount)
                .sum::<u64>(),
            400
        );
    }
    #[test]
    fn contribution_products_do_not_overflow_large_wallets() {
        let parts = fair_contributions(
            &[(PersonId(1), u64::MAX), (PersonId(2), u64::MAX)],
            u64::MAX,
            0,
        );
        assert_eq!(
            parts
                .iter()
                .map(|(_, amount)| u128::from(*amount))
                .sum::<u128>(),
            u128::from(u64::MAX)
        );
    }
    #[test]
    fn hearth_burns_real_wood_once_and_preserves_partial_bundle() {
        let mut pantry = GoodsInventory::new(80);
        pantry.add(Good::Wood, 2);
        let mut hearth = HearthState::default();
        let mut economy = HouseholdEconomy::default();
        hearth.advance(0, 4, &mut pantry, &mut economy);
        hearth.advance(1, 4, &mut pantry, &mut economy);
        assert_eq!((pantry.amount(Good::Wood), hearth.credit), (1, 8));
        assert_eq!(hearth.consumed_wood, 1);
        hearth.advance(1, 4, &mut pantry, &mut economy);
        assert_eq!((pantry.amount(Good::Wood), hearth.credit), (1, 8));
        assert_eq!(hearth.consumed_wood, 1);
        hearth.advance(4, 4, &mut pantry, &mut economy);
        assert_eq!(
            (
                pantry.amount(Good::Wood),
                hearth.credit,
                economy.fuel_satisfaction
            ),
            (0, 0, 100)
        );
        assert_eq!(hearth.consumed_wood, 2);
        hearth.advance(5, 4, &mut pantry, &mut economy);
        assert_eq!(hearth.consumed_wood, 2);
        assert_eq!(
            (economy.fuel_satisfaction, economy.fuel_shortage_days),
            (0, 1)
        );
        pantry.add(Good::Wood, 1);
        hearth.advance(6, 4, &mut pantry, &mut economy);
        assert_eq!(hearth.consumed_wood, 3);
        assert_eq!(
            (economy.fuel_satisfaction, economy.fuel_shortage_days),
            (100, 0)
        );
    }
    #[test]
    fn empty_homes_do_not_burn_fuel() {
        let mut pantry = GoodsInventory::new(80);
        pantry.add(Good::Wood, 2);
        let mut hearth = HearthState::default();
        let mut economy = HouseholdEconomy::default();
        hearth.advance(0, 0, &mut pantry, &mut economy);
        hearth.advance(30, 0, &mut pantry, &mut economy);
        assert_eq!(pantry.amount(Good::Wood), 2);
        assert_eq!(hearth.consumed_wood, 0);
        assert_eq!(hearth.deficit(0, 4, &pantry), 0);
    }
}
