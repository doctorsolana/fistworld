//! Settlement branch policies and individual business-site cards.
//!
//! Cards are keyed by `BuildingId` / `SettlementId`; their numbers, states
//! and the absolute values baked into stepper payloads bind in place.

use super::binding::{bound_text, CompanyBound, CompanyView, ResourceField, RetainStep, SiteField};
use super::controls::{CompanyBranchPolicyButton, CompanyManagementButton, CompanySiteButton};
use super::model::{CompanyBranchRecord, CompanySiteRecord};
use super::widgets::detail_button;
use crate::ui::business_management::BusinessManagementSelection;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::ledger::{self, LedgerIllustration};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::protocol::HeroCompanyAction;

pub(super) fn spawn_branch_card(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    branch: &CompanyBranchRecord,
) {
    let company = view.company.id;
    let settlement = branch.settlement_id;
    let can_manage = view.can_manage();
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(11.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.64, 0.45, 0.20, 0.05)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            bound_text(
                card,
                view,
                CompanyBound::Branch(settlement),
                crate::ui::ledger::reading(13.0),
                EMBER,
            );
            for (good, _, _) in &branch.resources {
                let good = *good;
                let key = |field| CompanyBound::Resource(settlement, good, field);
                card.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::vertical(Val::Px(5.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|line| {
                        bound_text(
                            line,
                            view,
                            key(ResourceField::Line),
                            crate::ui::ledger::reading(12.5),
                            INK,
                        );
                        bound_text(
                            line,
                            view,
                            key(ResourceField::Policy),
                            crate::ui::ledger::reading(11.5),
                            INK_MUTED,
                        );
                    });
                    if can_manage {
                        let retained_percent = view
                            .value(key(ResourceField::Meter))
                            .and_then(|value| value.fill)
                            .unwrap_or(0.0);
                        row.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(5.0),
                                overflow: Overflow::clip_x(),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.20, 0.18, 0.15, 0.12)),
                        ))
                        .with_child((
                            key(ResourceField::Meter),
                            Node {
                                width: Val::Percent(retained_percent),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(EMBER),
                        ));
                        row.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(5.0),
                            row_gap: Val::Px(5.0),
                            ..default()
                        })
                        .with_children(|controls| {
                            // Step payloads are ABSOLUTE unit counts; the bind
                            // pass rewrites them from the live policy so a
                            // second press never sends a stale value.
                            for step in RetainStep::ALL {
                                let payload = key(ResourceField::Step(step));
                                let action = view
                                    .value(payload)
                                    .and_then(|value| value.action)
                                    .unwrap_or(HeroCompanyAction::SetRetainUnits {
                                        settlement,
                                        good,
                                        units: 0,
                                    });
                                branch_policy_button(
                                    controls,
                                    (CompanyBranchPolicyButton { company, action }, payload),
                                    step.label(),
                                    (),
                                );
                            }
                            let toggle = key(ResourceField::Toggle);
                            let action = view
                                .value(toggle)
                                .and_then(|value| value.action)
                                .unwrap_or(HeroCompanyAction::SetSellExcess {
                                    settlement,
                                    good,
                                    enabled: true,
                                });
                            let label = view
                                .value(key(ResourceField::ToggleLabel))
                                .and_then(|value| value.text)
                                .unwrap_or_default();
                            branch_policy_button(
                                controls,
                                (CompanyBranchPolicyButton { company, action }, toggle),
                                &label,
                                key(ResourceField::ToggleLabel),
                            );
                        });
                    }
                });
            }
        });
}

pub(super) fn branch_policy_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Bundle,
    label: &str,
    label_marker: impl Bundle,
) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                min_width: Val::Px(42.0),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(7.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            Text::new(label),
            UiButtonLabel,
            crate::ui::ledger::reading(11.5),
            TextColor(INK),
            Pickable::IGNORE,
            label_marker,
        ));
}

pub(super) fn spawn_site_ledger(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    site: &CompanySiteRecord,
) {
    let key = |field| CompanyBound::Site(site.id, field);
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.64, 0.45, 0.20, 0.03)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                line.spawn(ledger::illustration(
                    LedgerIllustration::building(site.kind),
                    Vec2::new(78.0, 58.0),
                ));
                bound_text(
                    line,
                    view,
                    key(SiteField::LedgerTitle),
                    crate::ui::ledger::reading(14.0),
                    INK,
                )
                .insert((
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        margin: UiRect::horizontal(Val::Px(10.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                bound_text(
                    line,
                    view,
                    key(SiteField::LedgerState),
                    crate::ui::ledger::reading(11.5),
                    INK_MUTED,
                )
                .insert(Pickable::IGNORE);
            });
            for field in [
                SiteField::LedgerSummary,
                SiteField::LedgerStock,
                SiteField::LedgerInput,
            ] {
                bound_text(
                    card,
                    view,
                    key(field),
                    crate::ui::ledger::reading(12.0),
                    INK_MUTED,
                )
                .insert(Pickable::IGNORE);
            }
            card.spawn(Node {
                justify_content: JustifyContent::FlexEnd,
                column_gap: Val::Px(6.0),
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|actions| spawn_site_actions(actions, view, site));
        });
}

/// VIEW DETAILS is keyed by the durable `BuildingId`; SITE SETTINGS carries
/// the replicated client `Entity`, which changes whenever the building leaves
/// and re-enters interest, so that payload is rebound each snapshot.
fn spawn_site_actions(
    actions: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    site: &CompanySiteRecord,
) {
    detail_button(actions, CompanySiteButton(site.id), "VIEW DETAILS");
    detail_button(
        actions,
        (
            CompanyManagementButton {
                target: BusinessManagementSelection::Site(site.entity),
                company: view.company.id,
            },
            CompanyBound::Site(site.id, SiteField::Settings),
        ),
        "SITE SETTINGS",
    );
}

/// The portfolio keeps its operating sites scannable; detailed flows remain below
/// in the site ledgers and the existing Places / management destinations.
pub(super) fn spawn_site_card(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    site: &CompanySiteRecord,
) {
    parent
        .spawn((
            Node {
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                padding: UiRect::vertical(Val::Px(5.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn(ledger::illustration(
                LedgerIllustration::building(site.kind),
                Vec2::new(64.0, 49.0),
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn(ledger::body(site.kind.label(), 16.0));
                bound_text(
                    copy,
                    view,
                    CompanyBound::Site(site.id, SiteField::State),
                    ledger::reading(12.0),
                    INK_MUTED,
                );
                crate::ui::encyclopedia::person_links::spawn_site_people(copy, site.id);
            });
            bound_text(
                row,
                view,
                CompanyBound::Site(site.id, SiteField::Staff),
                ledger::reading(14.0),
                INK,
            )
            .insert(Pickable::IGNORE);
            row.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::FlexEnd,
                column_gap: Val::Px(7.0),
                row_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|actions| spawn_site_actions(actions, view, site));
        });
}
