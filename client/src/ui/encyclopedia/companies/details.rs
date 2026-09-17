//! Selected-company ownership, governance and current accounting details.
//!
//! Everything here is spawned once per structural change and then bound in
//! place: each volatile node carries a [`CompanyBound`] key and takes its
//! text from [`CompanyView::value`]. A branch that decides whether a node
//! exists must be mirrored in `binding::company_structure_key`.

use super::binding::{
    bound_text, ownership_class, CompanyBound, CompanyField, CompanyView, DayLedger,
    OwnershipClass, PairField,
};
use super::controls::{CompanyBranchPolicyButton, CompanyManagementButton, NewTradeRouteButton};
use super::routes::spawn_route_card;
use super::sites::{spawn_branch_card, spawn_site_card, spawn_site_ledger};
use super::widgets::{
    bound_note, detail_button, detail_stat, key_value, spawn_note, spawn_section_title, Label,
};
use crate::ui::business_management::BusinessManagementSelection;
use crate::ui::encyclopedia::person_links::spawn_person_link_with;
use crate::ui::ledger::{self, LedgerIllustration};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT};
use bevy::prelude::*;
use shared::protocol::HeroCompanyAction;

pub(super) fn spawn_company_detail(parent: &mut ChildSpawnerCommands<'_>, view: &CompanyView<'_>) {
    let company = view.company;
    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|header| {
            header.spawn(ledger::illustration_medallion(
                LedgerIllustration::Company,
                112.0,
            ));
            header
                .spawn(Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    bound_text(
                        copy,
                        view,
                        CompanyBound::Company(CompanyField::Name),
                        crate::ui::typography::heading(30.0),
                        INK,
                    );
                    bound_text(
                        copy,
                        view,
                        CompanyBound::Company(CompanyField::Status),
                        crate::ui::ledger::reading(12.5),
                        EMBER,
                    );
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
                        (
                            crate::ui::history::CompanyHistoryButton {
                                company: company.id,
                                name: company.name.clone(),
                            },
                            CompanyBound::Company(CompanyField::HistoryName),
                        ),
                        "FULL LEDGER",
                    );
                    detail_button(
                        actions,
                        CompanyManagementButton {
                            target: BusinessManagementSelection::Company(company.id),
                            company: company.id,
                        },
                        "COMPANY SETTINGS",
                    );
                });
        });

    // Both notes always exist; an empty message hides its node. A result
    // receipt therefore binds instead of respawning the pane under the
    // button that was just pressed.
    bound_note(
        parent,
        view,
        CompanyBound::Company(CompanyField::NoteFeedback),
    );
    bound_note(parent, view, CompanyBound::Company(CompanyField::NoteRoute));

    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|stats| {
            for (label, field) in [
                ("COMPANY CASH", CompanyField::Cash),
                ("TODAY'S REVENUE", CompanyField::Revenue),
                ("TODAY'S PROFIT", CompanyField::Profit),
                ("LIABILITIES", CompanyField::Liabilities),
                ("CAPITAL ASSETS", CompanyField::Assets),
                ("BOOK EQUITY", CompanyField::Equity),
            ] {
                detail_stat(stats, view, label, CompanyBound::Company(field));
            }
        });

    parent.spawn(ledger::ornament_rule());
    parent
        .spawn(Node {
            column_gap: Val::Px(24.0),
            align_items: AlignItems::Stretch,
            ..default()
        })
        .with_children(|columns| {
            columns
                .spawn(Node {
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(7.0),
                    ..default()
                })
                .with_children(|position| spawn_position(position, view));
            columns
                .spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        border: UiRect::left(Val::Px(1.0)),
                        padding: UiRect::left(Val::Px(22.0)),
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|ledger| spawn_today_ledger(ledger, view));
        });

    spawn_section_title(parent, "Operating Sites", "");
    if company.sites.is_empty() {
        spawn_note(
            parent,
            "No workplaces are currently observed. Company settings are still available.",
        );
    } else {
        for site in &company.sites {
            spawn_site_card(parent, view, site);
        }
    }

    super::fleet::spawn_fleet(parent, view);

    spawn_section_title(parent, "Trade Routes", "");
    if view.can_manage() {
        parent
            .spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|bar| {
                bound_text(
                    bar,
                    view,
                    CompanyBound::Company(CompanyField::RoutesHint),
                    crate::ui::ledger::reading(12.0),
                    INK_MUTED,
                );
                if view.ready_warehouse() && view.directory.settlements.len() >= 2 {
                    detail_button(bar, NewTradeRouteButton(company.id), "NEW CARAVAN ROUTE");
                }
            });
    }
    if company.routes.is_empty() {
        spawn_note(
            parent,
            "This company operates no trade route. Contract routes appear automatically when the company accepts funded public freight.",
        );
    } else {
        for route in &company.routes {
            spawn_route_card(parent, view, route);
        }
    }

    spawn_section_title(
        parent,
        "CONSOLIDATED LEDGER",
        "wages post to the completed shift at dawn; internal transfers are memorandum only",
    );
    spawn_day_ledger(parent, view, DayLedger::Today);
    if company.account.previous_day.day != u32::MAX {
        spawn_day_ledger(parent, view, DayLedger::Previous);
    }
    key_value(
        parent,
        view,
        None,
        Label::Fixed("LIFETIME CAPITAL"),
        CompanyBound::Company(CompanyField::LifetimeCapital),
    );

    spawn_section_title(
        parent,
        "LOCAL GOODS & STORAGE",
        "stock is physical and never shared between settlements",
    );
    if company.branches.is_empty() {
        spawn_note(parent, "No local branch stock is currently observed.");
    } else {
        for branch in &company.branches {
            spawn_branch_card(parent, view, branch);
        }
    }

    if !company.sites.is_empty() {
        spawn_section_title(
            parent,
            "Site Ledgers",
            "Production, supply, staffing and current liabilities",
        );
        for site in &company.sites {
            spawn_site_ledger(parent, view, site);
        }
    }

    spawn_section_title(parent, "OWNERSHIP", "1,000 ordinary shares in total");
    // Holders are slots in display order: a share sale re-sorts the cap
    // table, and the slots rebind their link, name and detail in place.
    for (index, holder) in company.holders.iter().enumerate() {
        let slot = index as u8;
        let detail = view
            .value(CompanyBound::Holder(
                slot,
                super::binding::HolderField::Detail,
            ))
            .and_then(|value| value.text)
            .unwrap_or_default();
        spawn_person_link_with(
            parent,
            holder.person,
            &holder.name,
            &detail,
            CompanyBound::Holder(slot, super::binding::HolderField::Link),
            CompanyBound::Holder(slot, super::binding::HolderField::Name),
            CompanyBound::Holder(slot, super::binding::HolderField::Detail),
        );
    }

    spawn_section_title(
        parent,
        "PUBLIC SHARE OFFERS",
        "seller chooses price; ownership moves only on purchase",
    );
    if company.offers.is_empty() {
        spawn_note(parent, "No shares are currently offered.");
    } else {
        for index in 0..company.offers.len() {
            let slot = index as u8;
            key_value(
                parent,
                view,
                None,
                Label::Bound(CompanyBound::Offer(slot, PairField::Label)),
                CompanyBound::Offer(slot, PairField::Value),
            );
        }
    }

    spawn_section_title(
        parent,
        "GOVERNANCE",
        "company-wide policy set by the Company Master",
    );
    spawn_person_link_with(
        parent,
        company.master,
        &company.master_name,
        "Company master  ›",
        (),
        CompanyBound::Company(CompanyField::MasterName),
        (),
    );
    key_value(
        parent,
        view,
        None,
        Label::Fixed("OPERATING POLICY"),
        CompanyBound::Company(CompanyField::OperatingPolicy),
    );
    key_value(
        parent,
        view,
        None,
        Label::Fixed("DIVIDENDS"),
        CompanyBound::Company(CompanyField::Dividends),
    );
    // The replicated headroom snapshot; the amount picker is in COMPANY SETTINGS.
    key_value(
        parent,
        view,
        None,
        Label::Fixed("DISTRIBUTABLE"),
        CompanyBound::Company(CompanyField::DividendCapacity),
    );

    spawn_section_title(
        parent,
        "RECENT MASTER DECISIONS",
        "bounded executive audit trail",
    );
    if company.decisions.is_empty() {
        spawn_note(parent, "No strategy change has been recorded yet.");
    } else {
        for index in 0..company.decisions.len().min(8) {
            let slot = index as u8;
            key_value(
                parent,
                view,
                None,
                Label::Bound(CompanyBound::Decision(slot, PairField::Label)),
                CompanyBound::Decision(slot, PairField::Value),
            );
        }
    }
}

/// One consolidated day line plus its always-present, display-bound memo row.
pub(super) fn spawn_day_ledger(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    which: DayLedger,
) {
    key_value(
        parent,
        view,
        None,
        Label::Bound(CompanyBound::Company(CompanyField::DayLabel(which))),
        CompanyBound::Company(CompanyField::DayLine(which)),
    );
    key_value(
        parent,
        view,
        Some(CompanyBound::Company(CompanyField::DayMemoRow(which))),
        Label::Fixed("INTERNAL FLOW MEMO"),
        CompanyBound::Company(CompanyField::DayMemo(which)),
    );
}

/// Keep personal ownership distinct from the company treasury and profit.
/// The ownership class is structural; the numbers inside each class bind.
fn spawn_position(parent: &mut ChildSpawnerCommands<'_>, view: &CompanyView<'_>) {
    let company = view.company;
    spawn_section_title(parent, "Your Position", "");
    let class = ownership_class(company, view.directory);
    if matches!(class, OwnershipClass::NoHero | OwnershipClass::NoShares) {
        bound_note(
            parent,
            view,
            CompanyBound::Company(CompanyField::PositionNote),
        );
        return;
    }
    bound_text(
        parent,
        view,
        CompanyBound::Company(CompanyField::PositionShares),
        ledger::reading_strong(21.0),
        INK,
    )
    .insert(Pickable::IGNORE);
    bound_note(
        parent,
        view,
        CompanyBound::Company(CompanyField::PositionInterest),
    );
    bound_text(
        parent,
        view,
        CompanyBound::Company(CompanyField::PositionRole),
        ledger::reading(15.0),
        INK,
    )
    .insert(Pickable::IGNORE);
    // Always spawned; hidden until the server publishes a dividend snapshot.
    bound_note(
        parent,
        view,
        CompanyBound::Company(CompanyField::PositionDividend),
    );
    // Every remaining class holds shares: any shareholder may donate.
    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(8.0),
            row_gap: Val::Px(6.0),
            margin: UiRect::vertical(Val::Px(4.0)),
            ..default()
        })
        .with_children(|actions| {
            for (amount, label) in [(1, "CONTRIBUTE 1 COIN"), (5, "CONTRIBUTE 5 COIN")] {
                detail_button(
                    actions,
                    CompanyBranchPolicyButton {
                        company: company.id,
                        action: HeroCompanyAction::ContributeCapital {
                            amount: amount * shared::economy::PENNIES_PER_COIN,
                        },
                    },
                    label,
                );
            }
        });
    bound_note(
        parent,
        view,
        CompanyBound::Company(CompanyField::PositionNote),
    );
}

fn spawn_today_ledger(parent: &mut ChildSpawnerCommands<'_>, view: &CompanyView<'_>) {
    spawn_section_title(parent, "Today's Ledger", "");
    if view.company.account.current_day.day == u32::MAX {
        spawn_note(parent, "No trading record yet.");
        return;
    }
    for (label, field) in [
        ("Revenue", CompanyField::TodayRevenue),
        ("Wages", CompanyField::TodayWages),
        ("Outside inputs", CompanyField::TodayInputs),
        ("Market & delivery fees", CompanyField::TodayFees),
        ("Profit tax", CompanyField::TodayTax),
    ] {
        parent
            .spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(ledger::body(label, 15.0));
                bound_text(
                    row,
                    view,
                    CompanyBound::Company(field),
                    ledger::reading_strong(15.0),
                    INK,
                )
                .insert(Pickable::IGNORE);
            });
    }
    parent.spawn(ledger::ornament_rule());
    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|row| {
            row.spawn(ledger::body_strong("Profit", 17.0));
            bound_text(
                row,
                view,
                CompanyBound::Company(CompanyField::TodayProfit),
                ledger::reading_strong(17.0),
                INK,
            );
        });
}
