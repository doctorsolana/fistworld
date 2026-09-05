//! Company directory, player portfolio and consolidated current ledgers.
//!
//! A workplace panel answers "how do I run this site?". This page answers the
//! wider questions: what firms exist, which ones do I own, who controls them,
//! where they operate, and whether the whole company is actually healthy.
//!
//! Replicated snapshots, input actions and retained views have separate owners.
//! The encyclopedia plugin registers the public systems re-exported here.

mod controls;
mod details;
mod directory;
mod model;
mod portfolio;
mod route_actions;
mod route_editor;
mod routes;
mod sites;
mod view;
mod widgets;

pub(super) use controls::{
    handle_company_branch_policy_buttons, handle_company_filter_buttons,
    handle_company_management_buttons, handle_company_person_buttons, handle_company_rows,
    handle_company_site_buttons, handle_new_company_button, receive_company_policy_results,
    style_company_controls,
};
pub use controls::{
    CompanyBranchPolicyButton, CompanyCountText, CompanyDetailContent, CompanyDetailViewport,
    CompanyFilterButton, CompanyListContent, CompanyListViewport, CompanyManagementButton,
    CompanyPersonButton, CompanyPortfolioContent, CompanyRow, CompanySiteButton,
    EditTradeRouteButton, NewCompanyPageButton, NewTradeRouteButton, TradeRouteEditorButton,
    TradeRouteQuickActionButton,
};
pub(super) use directory::{company_tab_active, refresh_company_directory};
pub use model::{
    CompanyBranchRecord, CompanyDirectory, CompanyDrilldownReturn, CompanyFilter,
    CompanyHolderRecord, CompanyOfferRecord, CompanyPolicyFeedback, CompanyRecord,
    CompanyRouteRecord, CompanyRouteStopRecord, CompanySettlementRecord, CompanySiteRecord,
    SelectedCompany, TradeRouteDraft, TradeRouteEditorAction, TradeRouteEditorState,
    TradeRouteQuickAction,
};
pub(super) use route_actions::{
    handle_trade_route_editor_buttons, handle_trade_route_open_buttons,
    handle_trade_route_quick_actions, receive_trade_route_results,
};
pub(super) use view::{rebuild_company_view, spawn_companies_tab};

#[cfg(test)]
mod tests;
