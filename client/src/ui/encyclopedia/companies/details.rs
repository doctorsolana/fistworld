//! Selected-company ownership, governance and current accounting details.

use super::controls::{
    CompanyBranchPolicyButton, CompanyManagementButton, CompanyPersonButton, NewTradeRouteButton,
};
use super::model::{CompanyDirectory, CompanyPolicyFeedback, CompanyRecord, TradeRouteEditorState};
use super::routes::spawn_route_card;
use super::sites::{spawn_branch_card, spawn_site_card};
use super::widgets::{
    detail_button, detail_stat, key_value, signed_money, spawn_note, spawn_section_title,
};
use crate::ui::foundation::{button_chrome, UiButtonVariant};
use crate::ui::styles::{EMBER, INK, INK_MUTED};
use bevy::prelude::*;
use shared::components::SettlementBuildingKind;
use shared::economy::{format_money, CompanyDayLedger};
use shared::protocol::HeroCompanyAction;

pub(super) fn spawn_company_detail(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    directory: &CompanyDirectory,
    feedback: &CompanyPolicyFeedback,
    route_feedback: &TradeRouteEditorState,
) {
    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(company.name.clone()),
                        TextFont {
                            font_size: FontSize::Px(26.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    copy.spawn((
                        Text::new(format!(
                            "COMPANY #{}  /  FOUNDED DAY {}  /  {}",
                            company.id.0,
                            company.founded_day,
                            company.status(),
                        )),
                        TextFont {
                            font_size: FontSize::Px(12.5),
                            ..default()
                        },
                        TextColor(EMBER),
                    ));
                });
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|actions| {
                    detail_button(
                        actions,
                        crate::ui::history::CompanyHistoryButton {
                            company: company.id,
                            name: company.name.clone(),
                        },
                        "FULL LEDGER",
                    );
                    if let Some(site) = company.sites.first() {
                        detail_button(
                            actions,
                            CompanyManagementButton {
                                site: site.entity,
                                company: company.id,
                            },
                            "COMPANY CONTROLS",
                        );
                    }
                });
        });

    if !feedback.message.is_empty() {
        spawn_note(
            parent,
            &format!(
                "{}: {}",
                if feedback.success {
                    "UPDATED"
                } else {
                    "NOT CHANGED"
                },
                feedback.message
            ),
        );
    }
    if !route_feedback.message.is_empty() {
        spawn_note(
            parent,
            &format!(
                "{}: {}",
                if route_feedback.success {
                    "ROUTE UPDATED"
                } else {
                    "ROUTE NOT CHANGED"
                },
                route_feedback.message
            ),
        );
    }

    let liabilities = company
        .account
        .wage_arrears
        .saturating_add(company.account.tax_arrears);
    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|stats| {
            detail_stat(
                stats,
                "COMPANY CASH",
                format!("{} coin", format_money(company.account.cash)),
            );
            detail_stat(
                stats,
                "TODAY'S REVENUE",
                format!(
                    "{} coin",
                    format_money(company.account.current_day.external_revenue)
                ),
            );
            detail_stat(
                stats,
                "TODAY'S PROFIT",
                signed_money(company.account.current_day.profit()),
            );
            detail_stat(
                stats,
                "LIABILITIES",
                format!("{} coin", format_money(liabilities)),
            );
            detail_stat(
                stats,
                "CAPITAL ASSETS",
                format!("{} coin", format_money(company.account.book_value)),
            );
            detail_stat(
                stats,
                "BOOK EQUITY",
                format!("{} coin", format_money(company.accounting_equity())),
            );
        });

    spawn_section_title(
        parent,
        "YOUR POSITION",
        "wallet money and company money stay separate",
    );
    if let Some(person) = directory.local_person {
        let shares = company.shares_owned_by(person);
        if shares == 0 {
            spawn_note(
                parent,
                if company.offers.is_empty() {
                    "You own no shares. No shareholder is currently offering stock."
                } else {
                    "You own no shares. Public offers are listed below; open CONTROLS & SHARES to trade."
                },
            );
        } else {
            key_value(
                parent,
                "OWNERSHIP",
                format!(
                    "{} / 1,000 shares ({:.1}%)  /  estimated book interest {} coin",
                    shares,
                    f32::from(shares) / 10.0,
                    format_money(company.holding_book_interest(shares)),
                ),
            );
            key_value(
                parent,
                "AUTHORITY",
                if company.master == person {
                    "You are Company Master and control ordinary operating decisions".to_string()
                } else if shares > shared::components::COMPANY_TOTAL_SHARES / 2 {
                    "Majority holder; may appoint the Company Master".to_string()
                } else {
                    "Shareholder; economic ownership without executive authority".to_string()
                },
            );
            if company.master == person && shares == shared::components::COMPANY_TOTAL_SHARES {
                parent
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(7.0),
                        row_gap: Val::Px(7.0),
                        ..default()
                    })
                    .with_children(|actions| {
                        actions.spawn((
                            Text::new(format!(
                                "HERO WALLET  {} coin",
                                format_money(directory.local_wallet.unwrap_or(0))
                            )),
                            TextFont {
                                font_size: FontSize::Px(12.5),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                        detail_button(
                            actions,
                            CompanyBranchPolicyButton {
                                company: company.id,
                                action: HeroCompanyAction::ContributeCapital {
                                    amount: shared::economy::PENNIES_PER_COIN,
                                },
                            },
                            "ADD 1 COIN",
                        );
                        detail_button(
                            actions,
                            CompanyBranchPolicyButton {
                                company: company.id,
                                action: HeroCompanyAction::ContributeCapital {
                                    amount: 5 * shared::economy::PENNIES_PER_COIN,
                                },
                            },
                            "ADD 5 COIN",
                        );
                    });
                spawn_note(
                    parent,
                    "Capital contributions move personal coin into the company treasury; they are not revenue or profit.",
                );
            }
        }
    } else {
        spawn_note(
            parent,
            "Spawn or select your Hero to resolve personal holdings.",
        );
    }

    spawn_section_title(
        parent,
        "CONSOLIDATED LEDGER",
        "wages post to the completed shift at dawn; internal transfers are memorandum only",
    );
    spawn_day_ledger(parent, "TODAY", company.account.current_day);
    if company.account.previous_day.day != u32::MAX {
        spawn_day_ledger(parent, "PREVIOUS DAY", company.account.previous_day);
    }
    key_value(
        parent,
        "LIFETIME CAPITAL",
        format!(
            "{} contributed  /  {} capital spending  /  {} distributed",
            format_money(company.account.contributed_capital),
            format_money(company.account.capital_expenditures),
            format_money(company.account.owner_withdrawals),
        ),
    );

    spawn_section_title(
        parent,
        "LOCAL GOODS & STORAGE",
        "stock is physical and never shared between settlements",
    );
    if company.branches.is_empty() {
        spawn_note(parent, "This company has no local operating branch.");
    } else {
        let can_manage = directory.local_person == Some(company.master);
        for branch in &company.branches {
            spawn_branch_card(parent, company.id, branch, can_manage);
        }
    }

    spawn_section_title(
        parent,
        "TRADE ROUTES",
        "mobile company assets with physical cargo and ordered town stops",
    );
    let can_manage_routes = directory.local_person == Some(company.master);
    let ready_warehouse = company
        .sites
        .iter()
        .any(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0);
    if can_manage_routes {
        parent
            .spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn((
                    Text::new(if ready_warehouse {
                        "Choose up to eight towns and tell the caravan what to do at each stop."
                    } else {
                        "A staffed Storage Hall is required before this company can open a route."
                    }),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
                if ready_warehouse && directory.settlements.len() >= 2 {
                    detail_button(bar, NewTradeRouteButton(company.id), "NEW CARAVAN ROUTE");
                }
            });
    }
    if company.routes.is_empty() {
        spawn_note(
            parent,
            "This company operates no caravan route. Contract routes appear automatically when the company accepts funded public freight.",
        );
    } else {
        for route in &company.routes {
            spawn_route_card(parent, company.id, route, can_manage_routes);
        }
    }

    spawn_section_title(
        parent,
        "OPERATING SITES",
        "Each site is its own building page in PLACES; VIEW DETAILS opens it",
    );
    if company.sites.is_empty() {
        spawn_note(parent, "This company has no operating site.");
    } else {
        for site in &company.sites {
            spawn_site_card(parent, company.id, site);
        }
    }

    spawn_section_title(parent, "OWNERSHIP", "1,000 ordinary shares in total");
    for holder in &company.holders {
        parent
            .spawn((
                Button,
                CompanyPersonButton(holder.person),
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                button_chrome(UiButtonVariant::Row),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(if holder.person == company.master {
                        format!("{}  /  COMPANY MASTER", holder.name)
                    } else {
                        holder.name.clone()
                    }),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
                row.spawn((
                    Text::new(format!(
                        "{} shares  /  {:.1}%",
                        holder.shares,
                        f32::from(holder.shares) / 10.0
                    )),
                    TextFont {
                        font_size: FontSize::Px(13.5),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                    Pickable::IGNORE,
                ));
            });
    }

    spawn_section_title(
        parent,
        "PUBLIC SHARE OFFERS",
        "seller chooses price; ownership moves only on purchase",
    );
    if company.offers.is_empty() {
        spawn_note(parent, "No shares are currently offered.");
    } else {
        for offer in &company.offers {
            key_value(
                parent,
                &offer.seller_name.to_uppercase(),
                format!(
                    "{} shares at {} coin each  /  listed day {}  /  total {} coin",
                    offer.shares,
                    format_money(offer.unit_price),
                    offer.listed_day,
                    format_money(offer.unit_price.saturating_mul(u64::from(offer.shares))),
                ),
            );
        }
    }

    spawn_section_title(
        parent,
        "GOVERNANCE",
        "company-wide policy set by the Company Master",
    );
    key_value(
        parent,
        "COMPANY MASTER",
        format!("{}  /  Person #{}", company.master_name, company.master.0),
    );
    key_value(
        parent,
        "OPERATING POLICY",
        format!(
            "{}  /  {}  /  {} payroll reserve days",
            company.policy.strategy.label(),
            if company.policy.autopilot {
                "autopilot"
            } else {
                "manual"
            },
            company.policy.payroll_reserve_days,
        ),
    );
    key_value(
        parent,
        "DIVIDENDS",
        if company.policy.automatic_dividends {
            format!(
                "automatic after reserves  /  up to {} coin per day",
                format_money(company.policy.max_daily_dividend)
            )
        } else {
            "retained until the Company Master distributes available profit".to_string()
        },
    );

    spawn_section_title(
        parent,
        "RECENT MASTER DECISIONS",
        "bounded executive audit trail",
    );
    if company.decisions.is_empty() {
        spawn_note(parent, "No strategy change has been recorded yet.");
    } else {
        for decision in company.decisions.iter().rev().take(8) {
            key_value(
                parent,
                &format!("DAY {}", decision.day),
                format!(
                    "{} -> {}  /  {}",
                    decision.from.label(),
                    decision.to.label(),
                    decision.reason.label(),
                ),
            );
        }
    }
}

pub(super) fn spawn_day_ledger(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    day: CompanyDayLedger,
) {
    if day.day == u32::MAX {
        key_value(parent, label, "No completed trading record".to_string());
        return;
    }
    key_value(
        parent,
        &format!("{label} / DAY {}", day.day),
        format!(
            "revenue {}  -  wages {}  -  outside inputs {}  -  market/delivery {}  -  tax {}  =  {}  /  dividends {}  /  capex {}",
            format_money(day.external_revenue),
            format_money(day.wage_expense),
            format_money(day.external_input_expense),
            format_money(day.market_fees.saturating_add(day.delivery_fees)),
            format_money(day.profit_taxes),
            signed_money(day.profit()),
            format_money(day.owner_withdrawals),
            format_money(day.capital_expenditures),
        ),
    );
    if day.internal_revenue > 0 || day.internal_input_expense > 0 {
        key_value(
            parent,
            "INTERNAL FLOW MEMO",
            format!(
                "{} supplier credits / {} buyer charges; eliminated from company profit",
                format_money(day.internal_revenue),
                format_money(day.internal_input_expense),
            ),
        );
    }
}
