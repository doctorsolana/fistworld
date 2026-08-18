use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SettlementTier;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(
            Component,
            Serialize,
            Deserialize,
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
        )]
        pub struct $name(pub u64);

        impl $name {
            pub const UNASSIGNED: Self = Self(0);

            pub const fn is_assigned(self) -> bool {
                self.0 != 0
            }
        }
    };
}

stable_id!(PersonId);
stable_id!(SettlementId);
stable_id!(BuildingId);
stable_id!(PermitId);
stable_id!(CompanyId);
stable_id!(TradeContractId);
stable_id!(TradeRouteId);

/// One person's durable voting/economic interest in a company. Every share is
/// an ordinary equal unit; percentages are derived for display only.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompanyShare {
    pub shareholder: PersonId,
    pub shares: u16,
}

pub const COMPANY_TOTAL_SHARES: u16 = 1_000;

/// One public offer of already-issued ordinary shares. Listing shares does not
/// issue new equity: the seller keeps voting/dividend rights until an atomic
/// purchase moves whole shares and coin together.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompanyShareOffer {
    pub seller: PersonId,
    pub shares: u16,
    pub unit_price: u64,
    #[serde(default)]
    pub listed_day: u32,
}

/// Replicated, bounded order board for one company. A shareholder may post one
/// offer at a time, which makes reservations exact without a separate escrow
/// entity or an unbounded per-company order book.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CompanyShareMarket {
    offers: Vec<CompanyShareOffer>,
}

impl CompanyShareMarket {
    pub fn offers(&self) -> &[CompanyShareOffer] {
        &self.offers
    }

    pub fn offer_from(&self, seller: PersonId) -> Option<CompanyShareOffer> {
        self.offers
            .iter()
            .find(|offer| offer.seller == seller)
            .copied()
    }

    /// Replace the seller's one active offer. The cap table remains unchanged
    /// until purchase, but no holder can offer more than they own.
    pub fn list(
        &mut self,
        ownership: &CompanyOwnership,
        seller: PersonId,
        shares: u16,
        unit_price: u64,
        day: u32,
    ) -> bool {
        if shares == 0 || unit_price == 0 || ownership.share_count(seller) < shares {
            return false;
        }
        self.cancel(seller);
        self.offers.push(CompanyShareOffer {
            seller,
            shares,
            unit_price,
            listed_day: day,
        });
        self.offers.sort_unstable_by_key(|offer| offer.seller);
        true
    }

    pub fn cancel(&mut self, seller: PersonId) -> bool {
        let before = self.offers.len();
        self.offers.retain(|offer| offer.seller != seller);
        self.offers.len() != before
    }

    /// Complete a validated whole-share transfer and consume exactly that many
    /// offered shares. Payment is handled by the authoritative server beside
    /// this mutation so a failed wallet transfer never reaches this method.
    pub fn fill(
        &mut self,
        ownership: &mut CompanyOwnership,
        seller: PersonId,
        buyer: PersonId,
        shares: u16,
    ) -> bool {
        let Some(index) = self
            .offers
            .iter()
            .position(|offer| offer.seller == seller && offer.shares >= shares && shares > 0)
        else {
            return false;
        };
        if !ownership.transfer(seller, buyer, shares) {
            return false;
        }
        self.offers[index].shares -= shares;
        if self.offers[index].shares == 0 {
            self.offers.remove(index);
        }
        true
    }

    /// Repair offers after inheritance, death or another authoritative cap
    /// table edit. This never changes ownership; it only removes impossible
    /// sell quantities.
    pub fn reconcile(&mut self, ownership: &CompanyOwnership) {
        for offer in &mut self.offers {
            offer.shares = offer.shares.min(ownership.share_count(offer.seller));
        }
        self.offers.retain(|offer| offer.shares > 0);
    }
}

/// Durable productive enterprise. A clan or person owns shares in this
/// entity; productive buildings point at it with [`OperatedBy`]. Household
/// property deliberately remains outside this identity.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Company {
    pub name: String,
    #[serde(default)]
    pub founded_day: u32,
}

/// The executive office responsible for ordinary company decisions. Shares
/// remain ownership and governance; this office is authority. “Company
/// Master” is deliberately broad enough for farms, workshops and merchant
/// concerns without incorrectly calling every firm a guild.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompanyLeadership {
    pub master: PersonId,
}

impl CompanyLeadership {
    pub const TITLE: &'static str = "Company Master";

    pub fn can_manage(self, person: PersonId) -> bool {
        self.master == person
    }
}

/// Replicated cap table. Share trading mutates this table atomically so no
/// intermediate packet can create more or less than one whole company.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CompanyOwnership {
    shares: Vec<CompanyShare>,
}

impl CompanyOwnership {
    pub fn sole(owner: PersonId) -> Self {
        Self {
            shares: vec![CompanyShare {
                shareholder: owner,
                shares: COMPANY_TOTAL_SHARES,
            }],
        }
    }

    pub fn from_shares(mut shares: Vec<CompanyShare>) -> Option<Self> {
        shares.retain(|share| share.shares > 0);
        shares.sort_unstable_by_key(|share| share.shareholder);
        let mut merged = Vec::<CompanyShare>::with_capacity(shares.len());
        for share in shares {
            if let Some(previous) = merged
                .last_mut()
                .filter(|previous| previous.shareholder == share.shareholder)
            {
                previous.shares = previous.shares.checked_add(share.shares)?;
            } else {
                merged.push(share);
            }
        }
        (merged
            .iter()
            .map(|share| u32::from(share.shares))
            .sum::<u32>()
            == u32::from(COMPANY_TOTAL_SHARES))
        .then_some(Self { shares: merged })
    }

    pub fn shares(&self) -> &[CompanyShare] {
        &self.shares
    }

    pub fn share_count(&self, shareholder: PersonId) -> u16 {
        self.shares
            .iter()
            .find(|share| share.shareholder == shareholder)
            .map_or(0, |share| share.shares)
    }

    pub fn controlling_shareholder(&self) -> Option<PersonId> {
        self.shares
            .iter()
            .max_by_key(|share| (share.shares, std::cmp::Reverse(share.shareholder)))
            .map(|share| share.shareholder)
    }

    /// Majority governance may appoint the Company Master and approve later
    /// structural actions such as issuing or transferring treasury shares.
    pub fn can_appoint_master(&self, shareholder: PersonId) -> bool {
        self.share_count(shareholder) > COMPANY_TOTAL_SHARES / 2
    }

    /// Transfer an exact interest without changing total ownership. Management
    /// authority follows the resulting cap table rather than display names.
    pub fn transfer(&mut self, from: PersonId, to: PersonId, shares_to_transfer: u16) -> bool {
        if shares_to_transfer == 0 || from == to || self.share_count(from) < shares_to_transfer {
            return false;
        }
        let mut shares = self.shares.clone();
        let Some(from_share) = shares.iter_mut().find(|share| share.shareholder == from) else {
            return false;
        };
        from_share.shares -= shares_to_transfer;
        if let Some(to_share) = shares.iter_mut().find(|share| share.shareholder == to) {
            let Some(next) = to_share.shares.checked_add(shares_to_transfer) else {
                return false;
            };
            to_share.shares = next;
        } else {
            shares.push(CompanyShare {
                shareholder: to,
                shares: shares_to_transfer,
            });
        }
        let Some(next) = Self::from_shares(shares) else {
            return false;
        };
        *self = next;
        true
    }
}

/// Durable operating relationship. `OwnedBy` remains the property-title
/// compatibility component; economic pooling and supply chains use this id.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OperatedBy(pub CompanyId);

/// Lightweight world-directory record. This is the only settlement data that
/// must be globally visible; markets, inventories, buildings and residents are
/// region-scoped detail.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementSummary {
    pub id: SettlementId,
    pub name: String,
    pub tier: SettlementTier,
    pub residents: u32,
    pub treasury: u64,
    pub prosperity: f32,
    pub reserve_days: f32,
    #[serde(default)]
    pub recent_food_production: f32,
    #[serde(default)]
    pub recent_food_consumption: f32,
    #[serde(default)]
    pub hungry: u32,
    #[serde(default)]
    pub housing_capacity: u32,
    #[serde(default)]
    pub homeless: u32,
    #[serde(default)]
    pub job_seekers: u16,
    #[serde(default)]
    pub unpaid_workers: u16,
    #[serde(default)]
    pub unrest: f32,
    #[serde(default)]
    pub unrest_change: f32,
    #[serde(default)]
    pub unrest_target: f32,
    #[serde(default)]
    pub unrest_hunger_pressure: f32,
    #[serde(default)]
    pub unrest_housing_pressure: f32,
    #[serde(default)]
    pub unrest_wage_pressure: f32,
    pub houses: u16,
    pub farmsteads: u16,
    pub fishing_huts: u16,
    pub lumber_huts: u16,
    #[serde(default)]
    pub windmills: u16,
    #[serde(default)]
    pub bakeries: u16,
    /// A completed physical Marketplace connects this settlement to formal
    /// caravan trade. Its founding Moot exchange remains local when false.
    #[serde(default)]
    pub has_marketplace: bool,
}

/// Durable settlement membership. Runtime AI may still hold a session-local
/// Entity for fast ECS access, but saves, histories and cross-region systems
/// join through this identifier.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResidentOf(pub SettlementId);

/// Durable building-to-settlement relationship.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BuildingOf(pub SettlementId);

/// Durable road-to-settlement relationship. `VillageRoad::settlement` remains
/// a readable label for panels and logs only.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoadOf(pub SettlementId);

/// Durable parent link for a building's authored adjuncts such as wheat fields
/// and fishing piers.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AttachedTo(pub BuildingId);

/// Durable private ownership. Display names remain on SettlementBuilding for
/// UI compatibility; this component is authoritative when money or rights move.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OwnedBy(pub PersonId);

/// Durable workplace assignment stored on the person rather than inferred
/// from a display-name roster.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EmployedAt(pub BuildingId);

/// Founding and municipal jobs live at the settlement rather than in a
/// private building roster, but still need the same durable identity as a
/// business assignment.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CivicRole {
    Reeve,
    CityWorker,
    Guard,
    /// A founding public-works job which operates a Moot goods cart, audits
    /// the road network and builds or adopts missing connectors. A solvent
    /// Hamlet may staff two people in this same role.
    MootSteward,
}

impl CivicRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reeve => "Reeve",
            Self::CityWorker => "City Worker",
            Self::Guard => "Guard",
            Self::MootSteward => "Moot Steward",
        }
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CivicEmployment {
    pub settlement: SettlementId,
    pub role: CivicRole,
}

/// Durable home assignment. The live server also keeps the house Entity for
/// fast door access, while this survives serialization and entity remapping.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LivesAt(pub BuildingId);

#[cfg(test)]
mod company_tests {
    use super::*;

    #[test]
    fn company_shares_transfer_without_changing_total_ownership() {
        let founder = PersonId(7);
        let partner = PersonId(8);
        let mut ownership = CompanyOwnership::sole(founder);

        assert!(ownership.transfer(founder, partner, 400));
        assert_eq!(ownership.share_count(founder), 600);
        assert_eq!(ownership.share_count(partner), 400);
        assert!(ownership.can_appoint_master(founder));
        assert!(!ownership.can_appoint_master(partner));
        assert_eq!(ownership.controlling_shareholder(), Some(founder));
        assert_eq!(
            ownership
                .shares()
                .iter()
                .map(|share| u32::from(share.shares))
                .sum::<u32>(),
            u32::from(COMPANY_TOTAL_SHARES)
        );
    }

    #[test]
    fn invalid_cap_tables_and_overdrawn_transfers_are_rejected() {
        assert!(CompanyOwnership::from_shares(vec![CompanyShare {
            shareholder: PersonId(1),
            shares: 999,
        }])
        .is_none());
        let mut ownership = CompanyOwnership::sole(PersonId(1));
        assert!(!ownership.transfer(PersonId(1), PersonId(2), 1_001));
        assert_eq!(ownership.share_count(PersonId(1)), 1_000);
    }

    #[test]
    fn share_offer_fill_preserves_exactly_one_thousand_issued_shares() {
        let seller = PersonId(4);
        let buyer = PersonId(9);
        let mut ownership = CompanyOwnership::sole(seller);
        let mut market = CompanyShareMarket::default();
        assert!(market.list(&ownership, seller, 250, 175, 3));
        assert!(market.fill(&mut ownership, seller, buyer, 80));
        assert_eq!(ownership.share_count(seller), 920);
        assert_eq!(ownership.share_count(buyer), 80);
        assert_eq!(market.offer_from(seller).unwrap().shares, 170);
        assert_eq!(
            ownership
                .shares()
                .iter()
                .map(|holding| u32::from(holding.shares))
                .sum::<u32>(),
            u32::from(COMPANY_TOTAL_SHARES),
        );
    }
}
