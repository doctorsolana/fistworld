//! Bounded negative pickup results, owned by the porter rather than one route.
use bevy::prelude::*;

const RECENT_PICKUPS: usize = 8;
const RETRY_SECONDS: f64 = 20.0;

#[derive(Clone, Copy)]
struct FailedPickup {
    business: Entity,
    entrance: Vec3,
    retry_after: f64,
}

/// Server-only work selection memory. The route planner remembers one goal;
/// alternating between two failed sellers must not erase both negative results.
/// Fixed storage follows the actor's lifecycle and allocates nothing per tick.
#[derive(Component, Clone, Default)]
pub(crate) struct MarketPickupFailures([Option<FailedPickup>; RECENT_PICKUPS]);

impl MarketPickupFailures {
    pub(super) fn record(&mut self, business: Entity, entrance: Vec3, now: f64, porter: Entity) {
        let slot = self
            .0
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.business == business))
            .or_else(|| {
                self.0
                    .iter()
                    .position(|entry| entry.is_none_or(|entry| entry.retry_after <= now))
            })
            .unwrap_or_else(|| {
                self.0
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.unwrap().retry_after.total_cmp(&b.unwrap().retry_after)
                    })
                    .unwrap()
                    .0
            });
        // Use unwarped simulation time, and spread retries after a shared
        // obstruction so a group of porters does not all wake in one tick.
        let hash = porter.to_bits().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let jitter = (hash & 1023) as f64 / 1023.0 * 5.0;
        self.0[slot] = Some(FailedPickup {
            business,
            entrance,
            retry_after: now + RETRY_SECONDS + jitter,
        });
    }

    pub(super) fn blocks(&self, business: Entity, entrance: Vec3, now: f64) -> bool {
        self.0.iter().flatten().any(|entry| {
            entry.business == business
                && now < entry.retry_after
                && entry.entrance.distance_squared(entrance) <= 0.01
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_pickups_are_bounded_expire_and_do_not_block_a_moved_entrance() {
        let mut failures = MarketPickupFailures::default();
        let porter = Entity::from_bits(8);
        for index in 1..=10 {
            failures.record(Entity::from_bits(index), Vec3::ZERO, index as f64, porter);
        }
        assert_eq!(failures.0.iter().flatten().count(), RECENT_PICKUPS);
        let latest = Entity::from_bits(10);
        assert!(failures.blocks(latest, Vec3::ZERO, 10.0));
        assert!(!failures.blocks(latest, Vec3::X, 10.0));
        assert!(!failures.blocks(latest, Vec3::ZERO, 36.0));
        assert!(!failures.blocks(Entity::from_bits(1), Vec3::ZERO, 10.0));
    }
}
