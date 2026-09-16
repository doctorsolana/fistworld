//! Bounded observations of purchasing power, never reservations or promised sales.

use serde::{Deserialize, Serialize};

const DEMAND_BANDS: usize = 12;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct DemandBand {
    price: u64,
    units: u64,
}

/// Keeps distinct bids exact until the bounded observation fills. Overflow
/// coarsens canonical price buckets at their lower bid, so compression can only
/// understate purchasing power. No buyer list or allocation grows with population.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundedDemandCurve {
    bands: [DemandBand; DEMAND_BANDS],
    shift: u8,
}

impl Default for FundedDemandCurve {
    fn default() -> Self {
        Self::empty()
    }
}

impl FundedDemandCurve {
    pub const fn empty() -> Self {
        Self {
            bands: [DemandBand { price: 0, units: 0 }; DEMAND_BANDS],
            shift: 0,
        }
    }

    pub fn units_at(&self, price: u64) -> u64 {
        if price == 0 {
            return 0;
        }
        self.bands
            .iter()
            .filter(|band| band.price >= price)
            .fold(0_u64, |total, band| total.saturating_add(band.units))
    }

    pub fn total_units(&self) -> u64 {
        self.bands
            .iter()
            .fold(0_u64, |total, band| total.saturating_add(band.units))
    }

    pub fn price_levels(&self) -> impl Iterator<Item = u64> + '_ {
        self.bands
            .iter()
            .filter(|band| band.units > 0)
            .map(|band| band.price)
    }

    fn bucket(price: u64, shift: u8) -> u64 {
        ((price.max(1) >> shift) << shift).max(1)
    }

    pub fn add(&mut self, units: u64, price: u64) {
        if units == 0 {
            return;
        }
        let mut next = [DemandBand::default(); DEMAND_BANDS + 1];
        next[..DEMAND_BANDS].copy_from_slice(&self.bands);
        next[DEMAND_BANDS] = DemandBand {
            price: price.max(1),
            units,
        };
        self.fit(next);
    }

    pub fn merge(&mut self, other: &Self) {
        if other.shift > self.shift {
            self.shift = other.shift;
            let mut next = [DemandBand::default(); DEMAND_BANDS + 1];
            next[..DEMAND_BANDS].copy_from_slice(&self.bands);
            self.fit(next);
        }
        for band in other.bands.iter().filter(|band| band.units > 0) {
            self.add(band.units, band.price);
        }
    }

    /// Nested power-of-two buckets make overflow independent of claim arrival
    /// order. Unlike nearest-neighbour merging, early cheap bids cannot create
    /// an ever-widening interval that swallows the entire richer tail.
    fn fit(&mut self, mut pending: [DemandBand; DEMAND_BANDS + 1]) {
        loop {
            for band in &mut pending {
                if band.units > 0 {
                    band.price = Self::bucket(band.price, self.shift);
                }
            }
            pending.sort_unstable_by_key(|band| (band.units == 0, band.price));
            let mut combined = [DemandBand::default(); DEMAND_BANDS + 1];
            let mut len = 0;
            for band in pending.iter().copied().filter(|band| band.units > 0) {
                if len > 0 && combined[len - 1].price == band.price {
                    combined[len - 1].units = combined[len - 1].units.saturating_add(band.units);
                } else {
                    combined[len] = band;
                    len += 1;
                }
            }
            if len <= DEMAND_BANDS {
                self.bands.fill(DemandBand::default());
                self.bands[..len].copy_from_slice(&combined[..len]);
                return;
            }
            // At shift63 every u64 price fits in at most two buckets.
            self.shift += 1;
            pending = combined;
        }
    }

    /// Remove the original bid's canonical bucket, preserving other bands.
    /// Compressed prices remain conservative until the daily ledger expires.
    pub fn withdraw(&mut self, units: u64, price: u64) {
        if units == 0 {
            return;
        }
        let price = Self::bucket(price, self.shift);
        if let Some(band) = self
            .bands
            .iter_mut()
            .find(|band| band.units > 0 && band.price == price)
        {
            debug_assert!(band.units >= units);
            band.units = band.units.saturating_sub(units);
        } else {
            debug_assert!(false, "withdrawing a missing funded demand claim");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheap_claims_do_not_hide_richer_customers_or_survive_withdrawal() {
        let mut curve = FundedDemandCurve::default();
        curve.add(3, 20);
        curve.add(6, 50);
        assert_eq!(curve.units_at(20), 9);
        assert_eq!(curve.units_at(50), 6);
        assert_eq!(curve.units_at(51), 0);
        curve.withdraw(3, 20);
        assert_eq!(curve.price_levels().collect::<Vec<_>>(), vec![50]);
        assert_eq!(curve.units_at(50), 6);
    }

    #[test]
    fn overflow_is_bounded_conservative_and_lossless_in_quantity() {
        let mut curve = FundedDemandCurve::default();
        for price in 1..=200 {
            curve.add(1, price);
        }
        assert_eq!(curve.total_units(), 200);
        assert!(curve.price_levels().count() <= DEMAND_BANDS);
        for price in 1..=200 {
            assert!(curve.units_at(price) <= 201 - price);
        }
        for price in 1..=200 {
            curve.withdraw(1, price);
        }
        assert_eq!(curve.total_units(), 0);
    }

    #[test]
    fn overflow_prices_are_independent_of_claim_arrival_order() {
        let mut ascending = FundedDemandCurve::default();
        let mut descending = FundedDemandCurve::default();
        let mut permuted = FundedDemandCurve::default();
        for price in 1..=200 {
            ascending.add(1, price);
        }
        for price in (1..=200).rev() {
            descending.add(1, price);
        }
        for i in 0..200 {
            permuted.add(1, (i * 73) % 200 + 1);
        }
        assert_eq!(ascending, descending);
        assert_eq!(ascending, permuted);
        assert!(ascending.units_at(125) >= 70);
        ascending.add(2, u64::MAX);
        assert_eq!(ascending.total_units(), 202);
        ascending.withdraw(2, u64::MAX);
        assert_eq!(ascending.total_units(), 200);
    }

    #[test]
    fn merging_and_wire_roundtrip_preserve_distinct_bids() {
        let mut first = FundedDemandCurve::default();
        first.add(2, 100);
        let mut second = FundedDemandCurve::default();
        second.add(3, 150);
        first.merge(&second);
        assert_eq!(first.units_at(125), 3);
        assert_eq!(first.total_units(), 5);
        let bytes = bincode::serialize(&first).unwrap();
        assert_eq!(
            bincode::deserialize::<FundedDemandCurve>(&bytes).unwrap(),
            first
        );
    }
}
