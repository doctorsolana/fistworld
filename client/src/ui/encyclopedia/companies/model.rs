//! Company directory snapshots, selection, policy feedback and route drafts.

use bevy::prelude::*;
use shared::components::{
    BuildingId, CompanyId, PersonId, SettlementBuildingKind, SettlementId, TradeRouteId,
    TradeRouteMode, TradeRouteStatus, TradeRouteStop, TradeRouteStopAction, TradeRouteTrip,
};
use shared::economy::{
    BusinessSourcingMode, BusinessState, CompanyAccount, CompanyDecisionRecord,
    CompanyManagementPolicy, CompanyResourcePolicy, Good,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyHolderRecord {
    pub person: PersonId,
    pub name: String,
    pub shares: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyOfferRecord {
    pub seller: PersonId,
    pub seller_name: String,
    pub shares: u16,
    pub unit_price: u64,
    pub listed_day: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanySiteRecord {
    pub entity: Entity,
    pub id: BuildingId,
    pub settlement: String,
    pub settlement_id: SettlementId,
    pub kind: SettlementBuildingKind,
    pub workers: usize,
    pub positions: u8,
    pub enabled_positions: u8,
    pub state: BusinessState,
    pub wage_arrears: u64,
    pub tax_arrears: u64,
    pub current_day: shared::economy::BusinessDayLedger,
    pub previous_day: shared::economy::BusinessDayLedger,
    pub output: Option<Good>,
    pub output_stock: u32,
    pub asking_price: Option<u64>,
    pub input: Option<Good>,
    pub input_stock: u32,
    pub input_target: u32,
    pub input_coverage_days: u8,
    pub sourcing: Option<BusinessSourcingMode>,
    pub preferred_supplier: Option<BuildingId>,
    pub goods: Vec<(Good, u32)>,
    pub used_bulk: u32,
    pub bulk_capacity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyBranchRecord {
    pub settlement: String,
    pub settlement_id: SettlementId,
    pub sites: usize,
    pub storage_halls: usize,
    pub used_bulk: u32,
    pub bulk_capacity: u32,
    pub resources: Vec<(Good, u32, CompanyResourcePolicy)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyRouteRecord {
    pub id: TradeRouteId,
    pub warehouse: BuildingId,
    pub warehouse_name: String,
    pub mode: TradeRouteMode,
    pub origin: String,
    pub destination: String,
    pub good: Good,
    pub cargo_target: u32,
    pub cargo_onboard: u32,
    pub maximum_purchase_price: u64,
    pub minimum_destination_price: u64,
    pub automatic: bool,
    pub autonomous_management: bool,
    pub expected_trip_profit: i64,
    pub decision_confidence: u8,
    pub assigned_caravaner: Option<String>,
    pub current_stop: u8,
    pub status: TradeRouteStatus,
    pub completed_trips: u32,
    pub lifetime_units: u32,
    pub lifetime_delivery_revenue: u64,
    pub lifetime_purchase_cost: u64,
    pub lifetime_consigned_value: u64,
    pub stops: Vec<CompanyRouteStopRecord>,
    pub trips: Vec<TradeRouteTrip>,
    pub latest_trip: Option<TradeRouteTrip>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyRouteStopRecord {
    pub settlement: SettlementId,
    pub settlement_name: String,
    pub action: TradeRouteStopAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanySettlementRecord {
    pub id: SettlementId,
    pub name: String,
    pub has_marketplace: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyRecord {
    pub id: CompanyId,
    pub name: String,
    pub founded_day: u32,
    pub master: PersonId,
    pub master_name: String,
    pub account: CompanyAccount,
    pub policy: CompanyManagementPolicy,
    pub holders: Vec<CompanyHolderRecord>,
    pub offers: Vec<CompanyOfferRecord>,
    pub decisions: Vec<CompanyDecisionRecord>,
    pub sites: Vec<CompanySiteRecord>,
    pub branches: Vec<CompanyBranchRecord>,
    pub routes: Vec<CompanyRouteRecord>,
}

impl CompanyRecord {
    pub fn shares_owned_by(&self, person: PersonId) -> u16 {
        self.holders
            .iter()
            .find(|holder| holder.person == person)
            .map_or(0, |holder| holder.shares)
    }

    pub fn accounting_equity(&self) -> u64 {
        self.account
            .cash
            .saturating_add(self.account.book_value)
            .saturating_sub(self.account.wage_arrears)
            .saturating_sub(self.account.tax_arrears)
    }

    pub fn holding_book_interest(&self, shares: u16) -> u64 {
        self.accounting_equity().saturating_mul(u64::from(shares))
            / u64::from(shared::components::COMPANY_TOTAL_SHARES)
    }

    pub(super) fn status(&self) -> &'static str {
        if self.sites.is_empty() {
            "NO SITES"
        } else if self.sites.iter().any(|site| {
            matches!(
                site.state,
                BusinessState::Insolvent | BusinessState::Liquidating | BusinessState::Closed
            )
        }) {
            "AT RISK"
        } else if self.account.wage_arrears > 0 || self.account.tax_arrears > 0 {
            "IN ARREARS"
        } else if self.account.current_day.profit() > 0 {
            "PROFITABLE"
        } else if self
            .sites
            .iter()
            .all(|site| matches!(site.state, BusinessState::New))
        {
            "NEW"
        } else {
            "TRADING"
        }
    }
}

#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct CompanyDirectory {
    pub records: Vec<CompanyRecord>,
    pub settlements: Vec<CompanySettlementRecord>,
    pub local_person: Option<PersonId>,
    pub local_wallet: Option<u64>,
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompanyFilter {
    #[default]
    All,
    MyHoldings,
    SharesForSale,
}

impl CompanyFilter {
    pub(super) const ALL: [Self; 3] = [Self::All, Self::MyHoldings, Self::SharesForSale];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::All => "ALL FIRMS",
            Self::MyHoldings => "MY HOLDINGS",
            Self::SharesForSale => "SHARES FOR SALE",
        }
    }
}

#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedCompany(pub Option<CompanyId>);

#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompanyDrilldownReturn(pub Option<CompanyId>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeRouteQuickAction {
    DispatchOnce,
    Mothball,
    Reopen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeRouteEditorAction {
    Cancel,
    Save,
    PreviousWarehouse,
    NextWarehouse,
    PreviousGood,
    NextGood,
    CargoDown(u32),
    CargoUp(u32),
    BuyPriceDown(u64),
    BuyPriceUp(u64),
    SellPriceDown(u64),
    SellPriceUp(u64),
    ToggleAutomatic,
    AddStop,
    RemoveStop(usize),
    MoveStopLeft(usize),
    MoveStopRight(usize),
    PreviousStopSettlement(usize),
    NextStopSettlement(usize),
    PreviousStopAction(usize),
    NextStopAction(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeRouteDraft {
    pub company: CompanyId,
    pub route: Option<TradeRouteId>,
    pub warehouse: BuildingId,
    pub good: Good,
    pub cargo_target: u32,
    pub maximum_purchase_price: u64,
    pub minimum_destination_price: u64,
    pub automatic: bool,
    pub stops: Vec<TradeRouteStop>,
    pub pending: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub struct TradeRouteEditorState {
    pub draft: Option<TradeRouteDraft>,
    pub message: String,
    pub success: bool,
}

#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct CompanyPolicyFeedback {
    pub message: String,
    pub success: bool,
}
