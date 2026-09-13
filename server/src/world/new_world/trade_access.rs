//! Founding's certified land groups constrain new overland trade commitments.
//! These server-only tags are a negative eligibility gate, not navigation
//! permission: same-group caravans still use the ordinary obstacle-aware route.

use bevy::prelude::*;
use shared::components::{SettlementId, TradeRouteStop};
use std::collections::HashMap;

/// Assigned to a founded Hall from the planner's actual overland links.
/// Different tags mean founding did not certify a connection, even when the
/// towns share a continent. Further route surveys or future bridges/shipping
/// need an explicit connectivity update; seeing a market's prices is not one.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FoundingLandNetwork(pub u64);

#[derive(Default)]
pub(crate) struct LandTradeAccess {
    groups: HashMap<SettlementId, FoundingLandNetwork>,
}

impl LandTradeAccess {
    /// Snapshot only the small Hall tag query during an admitted trade review.
    pub(crate) fn from_tags<'a>(
        tags: impl IntoIterator<Item = (&'a SettlementId, &'a FoundingLandNetwork)>,
    ) -> Self {
        Self {
            groups: tags.into_iter().map(|(id, group)| (*id, *group)).collect(),
        }
    }

    pub(crate) fn allows(&self, from: SettlementId, to: SettlementId) -> bool {
        match (self.groups.get(&from), self.groups.get(&to)) {
            (Some(a), Some(b)) => a == b,
            // Authored labs and later unclassified towns retain their ordinary
            // route validation. Absence of a tag supplies no founding restriction.
            _ => true,
        }
    }

    pub(crate) fn validate_schedule(&self, stops: &[TradeRouteStop]) -> Result<(), &'static str> {
        // An unclassified intermediate stop cannot supply a missing founding
        // certification between two groups in an overland circuit.
        let mut known = None;
        for stop in stops {
            if let Some(group) = self.groups.get(&stop.settlement) {
                if known.is_some_and(|previous| previous != group) {
                    return Err("These towns are not connected by a supported caravan route yet.");
                }
                known = Some(group);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::TradeRouteStopAction;

    #[test]
    fn known_separate_groups_are_denied_without_treating_untagged_labs_as_islands() {
        let tags = [
            (SettlementId(1), FoundingLandNetwork(7)),
            (SettlementId(2), FoundingLandNetwork(7)),
            (SettlementId(3), FoundingLandNetwork(9)),
        ];
        let access = LandTradeAccess::from_tags(tags.iter().map(|(id, group)| (id, group)));
        assert!(access.allows(SettlementId(1), SettlementId(2)));
        assert!(!access.allows(SettlementId(1), SettlementId(3)));
        assert!(!access.allows(SettlementId(3), SettlementId(2)));
        assert!(access.allows(SettlementId(1), SettlementId(4)));
        assert!(LandTradeAccess::default().allows(SettlementId(1), SettlementId(3)));
    }

    #[test]
    fn untagged_intermediate_stops_cannot_bridge_two_known_separate_networks() {
        let tags = [
            (SettlementId(1), FoundingLandNetwork(7)),
            (SettlementId(3), FoundingLandNetwork(9)),
        ];
        let access = LandTradeAccess::from_tags(tags.iter().map(|(id, group)| (id, group)));
        let stops = [1, 2, 3, 4].map(|id| TradeRouteStop {
            settlement: SettlementId(id),
            action: TradeRouteStopAction::Buy,
        });
        assert!(access.allows(stops[0].settlement, stops[1].settlement));
        assert!(access.allows(stops[1].settlement, stops[2].settlement));
        assert!(access.allows(stops[2].settlement, stops[3].settlement));
        assert!(access.validate_schedule(&stops[..2]).is_ok());
        assert!(access.validate_schedule(&stops).is_err());
    }
}
