//! Permit opportunity boards, player approvals and property listings.

use super::SettlementBuildingKind;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Current permit-market signals published by the settlement hall.
///
/// These are invitations, not construction orders. A subsidized opportunity
/// receives the enacted permit discount; residents may still choose a lower
/// signal at full price or decline every offer. Scores are quantized so normal
/// stock movement does not create noisy high-frequency replication.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermitMarketOpportunity {
    pub kind: SettlementBuildingKind,
    pub score: u8,
    pub subsidized: bool,
    /// Competition signals are meant to admit a new owner, rather than let
    /// the incumbent use a public discount to deepen the same monopoly.
    #[serde(default)]
    pub requires_independent_owner: bool,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SettlementOpportunityBoard {
    /// Highest signal first; bounded by the settlement's tier-unlocked uses.
    pub opportunities: Vec<PermitMarketOpportunity>,
}

/// One unspent land-use right purchased by a player-controlled person.
///
/// The paid permit fee is refundable until a plot is chosen. Business working
/// capital is deliberately *not* escrowed here: it remains ordinary company
/// cash and is spent on materials, inputs, wages or later expansion when those
/// costs actually occur.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlayerPermit {
    pub id: super::PermitId,
    pub settlement: super::SettlementId,
    pub kind: SettlementBuildingKind,
    pub fee_escrow: u64,
    pub purchased_day: u32,
    /// Productive and private-service permits must name their legal company.
    /// Housing remains personal and therefore carries `None`.
    #[serde(default)]
    pub company: Option<super::CompanyId>,
}

/// Small replicated permit wallet on a player's live hero.
///
/// This is deliberately separate from cargo: a stamped land right has no
/// physical bulk. The server caps it and remains the only authority allowed to
/// append, consume or refund an entry.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerPermitLedger {
    pub permits: Vec<PlayerPermit>,
}

impl PlayerPermitLedger {
    /// A replication/UI anti-spam bound on simultaneously *unused* stamps.
    /// It never restricts permit kinds or lifetime ownership: placing or
    /// surrendering any stamp immediately frees its slot.
    pub const MAX_ACTIVE: usize = 8;

    pub fn get(&self, id: super::PermitId) -> Option<&PlayerPermit> {
        self.permits.iter().find(|permit| permit.id == id)
    }
}

/// Whether a property listing is a finished workplace or a permitted site
/// whose materials and construction duty transfer with the purchase.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyListingStage {
    CompletedBusiness,
    UnfinishedWorksite,
}

impl PropertyListingStage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CompletedBusiness => "Completed business",
            Self::UnfinishedWorksite => "Unfinished worksite",
        }
    }
}

/// One small, globally useful property-market record published by the hall.
///
/// Buildings themselves remain detailed world entities. This summary lets a
/// settlement menu stay complete even when a large town's outer workplace is
/// beyond the client's detailed replication radius.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PropertyMarketListing {
    pub kind: SettlementBuildingKind,
    pub stage: PropertyListingStage,
    pub asking_price: u64,
    pub listed_day: u32,
    pub reason: crate::economy::BusinessSaleReason,
    pub position: Vec3,
}

/// Current private buildings and unfinished business permits offered for
/// takeover in this settlement. Empty is a real market state, not missing data.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SettlementPropertyBoard {
    pub listings: Vec<PropertyMarketListing>,
}
