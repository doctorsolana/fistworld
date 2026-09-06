//! Settlement branch policies and individual business-site cards.

use super::controls::{CompanyBranchPolicyButton, CompanyManagementButton, CompanySiteButton};
use super::model::{CompanyBranchRecord, CompanySiteRecord};
use super::widgets::{detail_button, signed_money};
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::styles::{BUTTON_NORMAL, EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::components::CompanyId;
use shared::economy::{format_money, BusinessSourcingMode};
use shared::protocol::HeroCompanyAction;

pub(super) fn spawn_branch_card(
    parent: &mut ChildSpawnerCommands<'_>,
    company: CompanyId,
    branch: &CompanyBranchRecord,
    can_manage: bool,
) {
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
            BackgroundColor(Color::srgba(0.88, 0.86, 0.81, 0.52)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(format!(
                    "{}  /  {} SITE{}  /  {} STORAGE HALL{}  /  {} OF {} BULK USED",
                    branch.settlement.to_uppercase(),
                    branch.sites,
                    if branch.sites == 1 { "" } else { "S" },
                    branch.storage_halls,
                    if branch.storage_halls == 1 { "" } else { "S" },
                    branch.used_bulk,
                    branch.bulk_capacity,
                )),
                crate::ui::typography::text(13.0),
                TextColor(EMBER),
            ));
            for (good, held, policy) in &branch.resources {
                let unit_capacity = branch.bulk_capacity / good.bulk_per_unit().max(1);
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
                        line.spawn((
                            Text::new(format!(
                                "{}  /  {} HELD  /  RETAIN {}",
                                good.label().to_uppercase(),
                                held,
                                policy.retain_units,
                            )),
                            crate::ui::typography::text(12.5),
                            TextColor(INK),
                        ));
                        line.spawn((
                            Text::new(if policy.sell_excess {
                                "SELL EXCESS"
                            } else {
                                "HOLD ALL"
                            }),
                            crate::ui::typography::text(11.5),
                            TextColor(INK_MUTED),
                        ));
                    });
                    if can_manage {
                        let retained_percent = if unit_capacity == 0 {
                            0.0
                        } else {
                            policy.retain_units.min(unit_capacity) as f32 * 100.0
                                / unit_capacity as f32
                        };
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
                            for (label, units) in [
                                ("CLEAR", 0),
                                ("-10", policy.retain_units.saturating_sub(10)),
                                ("-1", policy.retain_units.saturating_sub(1)),
                                (
                                    "+1",
                                    policy.retain_units.saturating_add(1).min(unit_capacity),
                                ),
                                (
                                    "+10",
                                    policy.retain_units.saturating_add(10).min(unit_capacity),
                                ),
                                ("MAX", unit_capacity),
                            ] {
                                branch_policy_button(
                                    controls,
                                    CompanyBranchPolicyButton {
                                        company,
                                        action: HeroCompanyAction::SetRetainUnits {
                                            settlement: branch.settlement_id,
                                            good: *good,
                                            units,
                                        },
                                    },
                                    label.to_string(),
                                );
                            }
                            branch_policy_button(
                                controls,
                                CompanyBranchPolicyButton {
                                    company,
                                    action: HeroCompanyAction::SetSellExcess {
                                        settlement: branch.settlement_id,
                                        good: *good,
                                        enabled: !policy.sell_excess,
                                    },
                                },
                                if policy.sell_excess {
                                    "HOLD ALL".to_string()
                                } else {
                                    "SELL EXCESS".to_string()
                                },
                            );
                        });
                    }
                });
            }
        });
}

pub(super) fn branch_policy_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: CompanyBranchPolicyButton,
    label: String,
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
            crate::ui::typography::text(11.5),
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

pub(super) fn spawn_site_card(
    parent: &mut ChildSpawnerCommands<'_>,
    company: CompanyId,
    site: &CompanySiteRecord,
) {
    let flow = match (site.input, site.output) {
        (Some(input), Some(output)) => format!("{} -> {}", input.label(), output.label()),
        (None, Some(output)) => format!("produces {}", output.label()),
        _ => "service site".to_string(),
    };
    let source = site.sourcing.map_or_else(
        || "public/local sourcing".to_string(),
        |sourcing| {
            let label = match sourcing {
                BusinessSourcingMode::PreferOwned => "company first",
                BusinessSourcingMode::CheapestAvailable => "best value",
                BusinessSourcingMode::OwnedOnly => "company only",
            };
            format!(
                "{}{}",
                label,
                site.preferred_supplier
                    .map_or_else(String::new, |supplier| {
                        format!(" from site #{}", supplier.0)
                    })
            )
        },
    );
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
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                line.spawn((
                    Text::new(format!(
                        "{} #{}  /  {}",
                        site.kind.label().to_uppercase(),
                        site.id.0,
                        site.settlement.to_uppercase()
                    )),
                    crate::ui::typography::text(14.0),
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
                line.spawn((
                    Text::new(site.state.label().to_uppercase()),
                    crate::ui::typography::text(11.5),
                    TextColor(INK_MUTED),
                    Pickable::IGNORE,
                ));
            });
            card.spawn((
                Text::new(format!(
                    "{}  /  {}  /  staff {} / {} open / {} max  /  today {}",
                    flow,
                    source,
                    site.workers,
                    site.enabled_positions,
                    site.positions,
                    signed_money(site.current_day.profit()),
                )),
                crate::ui::typography::text(12.0),
                TextColor(INK_MUTED),
                Pickable::IGNORE,
            ));
            if let (Some(output), Some(price)) = (site.output, site.asking_price) {
                card.spawn((
                    Text::new(format!(
                        "{} site stock {} / ask {} coin  /  wage-tax debt {} coin; public excess is set for the whole local branch above",
                        output.label(),
                        site.output_stock,
                        format_money(price),
                        format_money(site.wage_arrears.saturating_add(site.tax_arrears)),
                    )),
                    crate::ui::typography::text(12.0),
                    TextColor(INK_MUTED),
                    Pickable::IGNORE,
                ));
            }
            if let Some(input) = site.input {
                card.spawn((
                    Text::new(format!(
                        "{} input / {} day{} cover / {} held / {} target",
                        input.label(),
                        site.input_coverage_days,
                        if site.input_coverage_days == 1 {
                            ""
                        } else {
                            "s"
                        },
                        site.input_stock,
                        site.input_target,
                    )),
                    crate::ui::typography::text(12.0),
                    TextColor(INK_MUTED),
                    Pickable::IGNORE,
                ));
            }
            card.spawn(Node {
                justify_content: JustifyContent::FlexEnd,
                column_gap: Val::Px(6.0),
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|actions| {
                detail_button(actions, CompanySiteButton(site.id), "VIEW DETAILS");
                detail_button(
                    actions,
                    CompanyManagementButton {
                        site: site.entity,
                        company,
                    },
                    "MANAGE SITE",
                );
            });
        });
}
