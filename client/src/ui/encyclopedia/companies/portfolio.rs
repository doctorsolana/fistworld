//! Player ownership totals, company list rows and their live ledger text.
//!
//! The portfolio strip is spawned once per `local_person` presence and its
//! four values bind under [`CompanyBound::Portfolio`]. A list row is keyed by
//! `CompanyId`; its ledger and status lines rewrite in place so a profit-sign
//! flip or a share purchase never tears the row down under the pointer.

use super::binding::{portfolio_value, CompanyBound, PortfolioField};
use super::controls::CompanyRow;
use super::model::{CompanyDirectory, CompanyRecord};
use crate::ui::foundation::{button_chrome, UiButtonVariant};
use crate::ui::ledger::{self, LedgerIllustration};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT};
use bevy::prelude::*;
use shared::components::{CompanyId, PersonId};
use shared::economy::format_money;

pub(super) fn spawn_portfolio(parent: &mut ChildSpawnerCommands<'_>, directory: &CompanyDirectory) {
    if directory.local_person.is_none() {
        parent.spawn((
            Text::new(
                "PORTFOLIO UNAVAILABLE  /  Spawn or select your Hero to identify personal holdings. The company directory remains usable.",
            ),
            crate::ui::ledger::reading(13.5),
            TextColor(INK_MUTED),
        ));
        return;
    }
    for (label, field, note) in [
        (
            "Hero wallet",
            PortfolioField::Wallet,
            "Spendable by your Hero",
        ),
        (
            "Holdings",
            PortfolioField::Holdings,
            "Direct share positions",
        ),
        (
            "Book interest",
            PortfolioField::Interest,
            "Accounting estimate, not cash",
        ),
        (
            "Company Master",
            PortfolioField::Master,
            "Executive authority",
        ),
    ] {
        portfolio_card(parent, directory, label, field, note);
    }
}

fn portfolio_card(
    parent: &mut ChildSpawnerCommands<'_>,
    directory: &CompanyDirectory,
    label: &str,
    field: PortfolioField,
    note: &str,
) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                border: UiRect::right(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(label),
                crate::ui::ledger::reading(12.0),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                CompanyBound::Portfolio(field),
                Text::new(portfolio_value(directory, field).text.unwrap_or_default()),
                ledger::reading_strong(17.0),
                TextColor(INK),
            ));
            card.spawn((
                Text::new(note),
                crate::ui::ledger::reading(11.0),
                TextColor(INK_MUTED),
            ));
        });
}

/// The one line of a company row that moves with every books snapshot. It is
/// rewritten in place by [`rebuild_company_view`]; everything else on the row
/// is covered by [`company_rows_signature`].
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct CompanyRowLedger(pub(super) CompanyId);

/// The status / shares / Master line of a row, also rewritten in place, so a
/// PROFITABLE <-> TRADING flip never respawns the row.
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct CompanyRowStatus(pub(super) CompanyId);

pub(super) fn company_ledger_line(company: &CompanyRecord) -> String {
    format!(
        "{} site{}  /  {} route{}  /  {} cash  /  today {}{}",
        company.sites.len(),
        if company.sites.len() == 1 { "" } else { "s" },
        company.routes.len(),
        if company.routes.len() == 1 { "" } else { "s" },
        format_money(company.account.cash),
        if company.account.current_day.profit() < 0 {
            "-"
        } else {
            "+"
        },
        format_money(company.account.current_day.profit().unsigned_abs()),
    )
}

pub(super) fn company_status_line(
    company: &CompanyRecord,
    local_person: Option<PersonId>,
) -> String {
    let shares = local_person.map_or(0, |person| company.shares_owned_by(person));
    if shares > 0 {
        format!(
            "{:.1}% yours · {}{}",
            f32::from(shares) / 10.0,
            company.status(),
            if local_person == Some(company.master) {
                " · Master"
            } else {
                ""
            }
        )
    } else {
        company.status().into()
    }
}

pub(super) fn spawn_company_row(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    local_person: Option<PersonId>,
) {
    parent
        .spawn((
            Button,
            CompanyRow(company.id),
            Node {
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Row),
        ))
        .with_children(|row| {
            row.spawn(ledger::illustration_medallion(
                LedgerIllustration::Company,
                68.0,
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn(ledger::heading(company.name.clone(), 17.0));
                copy.spawn((
                    CompanyRowLedger(company.id),
                    Text::new(company_ledger_line(company)),
                    ledger::reading(12.0),
                    TextColor(INK_MUTED),
                ));
                copy.spawn((
                    CompanyRowStatus(company.id),
                    Text::new(company_status_line(company, local_person)),
                    ledger::reading_strong(12.0),
                    TextColor(EMBER),
                ));
            });
        });
}
