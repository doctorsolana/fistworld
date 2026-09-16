//! Funded regional connections justified by completed physical trade journeys.
//! Traffic evidence never paints a road; an accountable worker builds each section.

pub(crate) mod bridge;
mod evidence;
mod investment;
pub(crate) mod lab;
mod projects;

pub(crate) use evidence::{RegionalRoadTraffic, SettlementPair, TradeLeg};
pub(crate) use investment::{RegionalInfrastructure, review_regional_investment};
pub(crate) use projects::{RegionalProject, advance_regional_projects};

use bevy::prelude::*;
use shared::components::RoadBridge;

/// Finite surveyed sections keep regional roads region-scoped on the wire.
#[derive(Clone, Debug)]
pub(crate) enum RegionalStep {
    Dirt(Vec<Vec2>),
    Bridge(RoadBridge),
}
impl RegionalStep {
    fn length(&self) -> f32 {
        match self {
            Self::Dirt(points) => points
                .windows(2)
                .map(|pair| pair[0].distance(pair[1]))
                .sum(),
            Self::Bridge(bridge) => bridge.length(),
        }
    }
    fn work_units(&self) -> u64 {
        match self {
            Self::Dirt(points) => points.len().saturating_sub(1).max(1) as u64,
            Self::Bridge(bridge) => bridge::required_work(bridge),
        }
    }
}

/// Remains attached across section boundaries and personal errands. Only this
/// contract may release the worker; no second employer can acquire that gap.
#[derive(Component, Clone, Copy)]
pub(crate) struct RegionalRoadWorker {
    pub(crate) project: Entity,
}

/// A failed regional section retains its completed prefix instead of deleting
/// paid infrastructure or being mistaken for a local building connector.
#[derive(Component, Clone, Copy)]
pub(crate) struct RegionalRoadSection {
    pub(crate) project: Entity,
}
