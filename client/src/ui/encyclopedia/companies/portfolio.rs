//! Player ownership totals, company list rows and their live ledger text.

use super::controls::CompanyRow;
use super::model::{CompanyDirectory, CompanyRecord};
use crate::ui::foundation::{button_chrome, UiButtonVariant};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::components::{CompanyId, PersonId};
use shared::economy::format_money;

pub(super) fn spawn_portfolio(parent: &mut ChildSpawnerCommands<'_>, directory: &CompanyDirectory) {
    let Some(person) = directory.local_person else {
        parent.spawn((
            Text::new(
                "PORTFOLIO UNAVAILABLE  /  Spawn or select your Hero to identify personal holdings. The company directory remains usable.",
            ),
            crate::ui::typography::text(13.5),
            TextColor(INK_MUTED),
        ));
        return;
    };
    let holdings: Vec<_> = directory
        .records
        .iter()
        .filter_map(|company| {
            let shares = company.shares_owned_by(person);
            (shares > 0).then_some((company, shares))
        })
        .collect();
    let estimated_interest = holdings.iter().fold(0u64, |total, (company, shares)| {
        total.saturating_add(company.holding_book_interest(*shares))
    });
    let mastered = directory
        .records
        .iter()
        .filter(|company| company.master == person)
        .count();
    portfolio_card(
        parent,
        "HERO WALLET",
        directory.local_wallet.map_or_else(
            || "Not in range".to_string(),
            |wallet| format!("{} coin", format_money(wallet)),
        ),
        "Spendable by your Hero",
    );
    portfolio_card(
        parent,
        "COMPANY HOLDINGS",
        format!(
            "{} firm{}",
            holdings.len(),
            if holdings.len() == 1 { "" } else { "s" }
        ),
        "Direct share positions",
    );
    portfolio_card(
        parent,
        "BOOK INTEREST",
        format!("{} coin", format_money(estimated_interest)),
        "Accounting estimate, not cash",
    );
    portfolio_card(
        parent,
        "COMPANY MASTER",
        format!("{} firm{}", mastered, if mastered == 1 { "" } else { "s" }),
        "Executive authority",
    );
}

pub(super) fn portfolio_card(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    value: String,
    note: &str,
) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.9, 0.88, 0.84, 0.58)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(label),
                crate::ui::typography::text(12.0),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                Text::new(value),
                crate::ui::typography::text(17.0),
                TextColor(INK),
            ));
            card.spawn((
                Text::new(note),
                crate::ui::typography::text(11.0),
                TextColor(INK_MUTED),
            ));
        });
}

/// The one line of a company row that moves with every books snapshot. It is
/// rewritten in place by [`rebuild_company_view`]; everything else on the row
/// is covered by [`company_rows_signature`].
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct CompanyRowLedger(pub(super) CompanyId);

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

pub(super) fn spawn_company_row(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    local_person: Option<PersonId>,
) {
    let shares = local_person.map_or(0, |person| company.shares_owned_by(person));
    parent
        .spawn((
            Button,
            CompanyRow(company.id),
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(4.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Row),
        ))
        .with_children(|row| {
            row.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                line.spawn((
                    Text::new(company.name.clone()),
                    crate::ui::typography::text(15.5),
                    TextColor(INK),
                ));
                line.spawn((
                    Text::new(company.status()),
                    crate::ui::typography::text(11.5),
                    TextColor(if company.status() == "AT RISK" {
                        EMBER
                    } else {
                        INK_MUTED
                    }),
                ));
            });
            row.spawn((
                CompanyRowLedger(company.id),
                Text::new(company_ledger_line(company)),
                crate::ui::typography::text(12.0),
                TextColor(INK_MUTED),
            ));
            if shares > 0 {
                row.spawn((
                    Text::new(format!(
                        "YOUR HOLDING  {} / 1,000 ({:.1}%){}",
                        shares,
                        f32::from(shares) / 10.0,
                        if local_person == Some(company.master) {
                            "  /  COMPANY MASTER"
                        } else {
                            ""
                        }
                    )),
                    crate::ui::typography::text(12.0),
                    TextColor(EMBER),
                ));
            }
        });
}
