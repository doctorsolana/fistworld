//! Bounded observations of delivered inter-settlement cargo, never promised demand.

use bevy::prelude::*;
use shared::components::{SettlementId, TradeRouteId};
use std::collections::{BTreeMap, VecDeque};

pub(super) const EVIDENCE_DAYS: u32 = 14;
const MAX_PAIRS: usize = 128;
const MAX_ACTIVE_LEGS: usize = 64;
const MAX_EVENTS_PER_PAIR: usize = 64;
const MAX_CORRIDOR_POINTS: usize = 8_192;
const SAMPLE_SPACING: f32 = 2.0;
const MAX_OBSERVED_STEP: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SettlementPair(pub SettlementId, pub SettlementId);
impl SettlementPair {
    pub(crate) fn new(a: SettlementId, b: SettlementId) -> Option<Self> {
        (a.is_assigned() && b.is_assigned() && a != b).then_some(if a < b {
            Self(a, b)
        } else {
            Self(b, a)
        })
    }
}

/// One exact freight journey/stop. Partial unloading adds units to that same
/// observation; it cannot masquerade as several repeated trading journeys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TradeLeg {
    pub(crate) route: TradeRouteId,
    pub(crate) cycle: u32,
    pub(crate) stop: u8,
}

struct Trace {
    pair: SettlementPair,
    from: SettlementId,
    updated_day: u32,
    points: Vec<Vec2>,
    invalid: bool,
}

#[derive(Clone, Copy)]
struct Delivery {
    leg: TradeLeg,
    day: u32,
    units: u32,
}

#[derive(Default)]
pub(super) struct PairTraffic {
    deliveries: VecDeque<Delivery>,
    /// Actual traversed centerline, canonical low-ID town first. This is
    /// evidence for a road survey, not permission to ignore ribbon collision.
    pub(super) corridor: Vec<Vec2>,
}
impl PairTraffic {
    pub(super) fn trips(&self) -> usize {
        self.deliveries.len()
    }
    pub(super) fn units(&self) -> u64 {
        self.deliveries
            .iter()
            .map(|delivery| u64::from(delivery.units))
            .sum()
    }
    pub(super) fn observed_days(&self, day: u32) -> u32 {
        self.deliveries
            .front()
            .map_or(1, |delivery| {
                day.saturating_sub(delivery.day).saturating_add(1)
            })
            .min(EVIDENCE_DAYS)
            .max(1)
    }
    fn recent_day(&self) -> u32 {
        self.deliveries.back().map_or(0, |delivery| delivery.day)
    }
}

#[derive(Resource, Default)]
pub(crate) struct RegionalRoadTraffic {
    traces: BTreeMap<TradeLeg, Trace>,
    pub(super) pairs: BTreeMap<SettlementPair, PairTraffic>,
}
impl RegionalRoadTraffic {
    /// Called in the existing active-trader loop. No additional actor/world scan.
    pub(crate) fn observe(
        &mut self,
        leg: TradeLeg,
        from: SettlementId,
        to: SettlementId,
        position: Vec2,
        day: u32,
    ) {
        let Some(pair) = SettlementPair::new(from, to) else {
            return;
        };
        if !position.is_finite() {
            return;
        }
        if !self.traces.contains_key(&leg) {
            // One carrier route cannot have two current embodied legs. A
            // new cycle also retires an earlier aborted trace without a
            // per-frame global cleanup scan.
            self.traces
                .retain(|previous, _| previous.route != leg.route);
            if self.traces.len() >= MAX_ACTIVE_LEGS {
                return;
            }
        }
        let trace = self.traces.entry(leg).or_insert_with(|| Trace {
            pair,
            from,
            updated_day: day,
            points: Vec::new(),
            invalid: false,
        });
        if trace.pair != pair || trace.from != from {
            trace.invalid = true;
            return;
        }
        trace.updated_day = day;
        if trace.invalid {
            return;
        }
        if let Some(previous) = trace.points.last() {
            let distance = previous.distance(position);
            if distance > MAX_OBSERVED_STEP {
                trace.invalid = true;
                return;
            }
            if distance < SAMPLE_SPACING {
                return;
            }
        }
        if trace.points.len() >= MAX_CORRIDOR_POINTS {
            trace.invalid = true;
            return;
        }
        trace.points.push(position);
    }

    /// Call only after real positive cargo transfer at its authored destination.
    /// An offered price, empty visit or aborted loading cycle is not traffic.
    pub(crate) fn delivered(
        &mut self,
        leg: TradeLeg,
        from: SettlementId,
        to: SettlementId,
        position: Vec2,
        units: u32,
        day: u32,
    ) {
        if units == 0 {
            return;
        }
        let Some(pair) = SettlementPair::new(from, to) else {
            return;
        };
        self.observe(leg, from, to, position, day);
        if !self.pairs.contains_key(&pair) && self.pairs.len() >= MAX_PAIRS {
            if let Some(oldest) = self
                .pairs
                .iter()
                .min_by_key(|(pair, flow)| (flow.recent_day(), **pair))
                .map(|(pair, _)| *pair)
            {
                self.pairs.remove(&oldest);
            }
        }
        let flow = self.pairs.entry(pair).or_default();
        if let Some(delivery) = flow
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.leg == leg)
        {
            delivery.units = delivery.units.saturating_add(units);
        } else {
            if flow.deliveries.len() == MAX_EVENTS_PER_PAIR {
                flow.deliveries.pop_front();
            }
            flow.deliveries.push_back(Delivery { leg, day, units });
        }
        if let Some(trace) = self
            .traces
            .get(&leg)
            .filter(|trace| !trace.invalid && trace.points.len() >= 2)
        {
            let mut points = trace.points.clone();
            if points
                .last()
                .is_some_and(|last| last.distance_squared(position) > 0.01)
            {
                points.push(position);
            }
            if from != pair.0 {
                points.reverse();
            }
            // Use a recent successful journey, so changed routes can replace a
            // formerly short but now blocked corridor on later review.
            flow.corridor = points;
        }
    }

    /// A completed/aborted timetable stop no longer consumes a live trace slot.
    pub(crate) fn finish_leg(&mut self, leg: TradeLeg) {
        self.traces.remove(&leg);
    }

    pub(super) fn expire(&mut self, day: u32) {
        self.traces
            .retain(|_, trace| day.saturating_sub(trace.updated_day) < EVIDENCE_DAYS);
        self.pairs.retain(|_, flow| {
            flow.deliveries
                .retain(|delivery| day.saturating_sub(delivery.day) < EVIDENCE_DAYS);
            !flow.deliveries.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn leg(cycle: u32) -> TradeLeg {
        TradeLeg {
            route: TradeRouteId(1),
            cycle,
            stop: 1,
        }
    }

    #[test]
    fn only_delivered_cargo_commits_a_corridor_and_partial_unloads_are_one_trip() {
        let mut traffic = RegionalRoadTraffic::default();
        let a = SettlementId(2);
        let b = SettlementId(1);
        for step in 0..5 {
            traffic.observe(leg(0), a, b, Vec2::X * step as f32 * 2.0, 1);
        }
        assert!(traffic.pairs.is_empty());
        traffic.delivered(leg(0), a, b, Vec2::X * 8.0, 0, 1);
        assert!(traffic.pairs.is_empty());
        traffic.delivered(leg(0), a, b, Vec2::X * 8.0, 3, 1);
        traffic.delivered(leg(0), a, b, Vec2::X * 8.0, 2, 1);
        let flow = &traffic.pairs[&SettlementPair(b, a)];
        assert_eq!(flow.trips(), 1);
        assert_eq!(flow.units(), 5);
        assert_eq!(flow.corridor.first(), Some(&(Vec2::X * 8.0)));
        assert_eq!(flow.corridor.last(), Some(&Vec2::ZERO));
        traffic.expire(15);
        assert!(traffic.pairs.is_empty());
        assert!(traffic.traces.is_empty());
    }

    #[test]
    fn separate_timetable_legs_do_not_create_a_fictitious_direct_connection() {
        let mut traffic = RegionalRoadTraffic::default();
        traffic.observe(leg(0), SettlementId(1), SettlementId(2), Vec2::ZERO, 0);
        traffic.observe(leg(0), SettlementId(1), SettlementId(2), Vec2::X * 200.0, 0);
        traffic.delivered(
            leg(0),
            SettlementId(1),
            SettlementId(2),
            Vec2::X * 200.0,
            5,
            0,
        );
        assert!(
            traffic.pairs[&SettlementPair(SettlementId(1), SettlementId(2))]
                .corridor
                .is_empty(),
            "a teleport is not a certified walking corridor"
        );
        let next = TradeLeg { stop: 2, ..leg(0) };
        traffic.observe(next, SettlementId(2), SettlementId(3), Vec2::ZERO, 0);
        traffic.observe(next, SettlementId(2), SettlementId(3), Vec2::X * 2.0, 0);
        traffic.delivered(next, SettlementId(2), SettlementId(3), Vec2::X * 2.0, 2, 0);
        assert!(
            !traffic
                .pairs
                .contains_key(&SettlementPair(SettlementId(1), SettlementId(3)))
        );
        assert!(
            !traffic.pairs[&SettlementPair(SettlementId(2), SettlementId(3))]
                .corridor
                .is_empty()
        );
    }
}
