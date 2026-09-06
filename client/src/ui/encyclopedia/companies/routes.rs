//! Current route status, stops and trip-history cards.

use super::controls::{EditTradeRouteButton, TradeRouteQuickActionButton};
use super::model::{CompanyRouteRecord, TradeRouteQuickAction};
use super::widgets::detail_button;
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::components::{CompanyId, TradeRouteMode, TradeRouteStatus};
use shared::economy::format_money;

pub(super) fn spawn_route_card(
    parent: &mut ChildSpawnerCommands<'_>,
    company: CompanyId,
    route: &CompanyRouteRecord,
    can_manage: bool,
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.88, 0.86, 0.81, 0.60)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|header| {
                header
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|copy| {
                        copy.spawn((
                            Text::new(format!(
                                "CARAVAN ROUTE #{}  /  {}",
                                route.id.0,
                                route.good.label().to_uppercase()
                            )),
                            crate::ui::typography::text(15.0),
                            TextColor(INK),
                        ));
                        copy.spawn((
                            Text::new(format!(
                                "{}  /  HOME {}",
                                route.mode.label().to_uppercase(),
                                route.warehouse_name.to_uppercase()
                            )),
                            crate::ui::typography::text(11.5),
                            TextColor(INK_MUTED),
                        ));
                    });
                header.spawn((
                    Text::new(route.status.label().to_uppercase()),
                    crate::ui::typography::text(12.0),
                    TextColor(if route.status == TradeRouteStatus::Mothballed {
                        EMBER
                    } else {
                        INK_MUTED
                    }),
                ));
            });

            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                row_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|timeline| {
                for (index, stop) in route.stops.iter().enumerate() {
                    if index > 0 {
                        timeline.spawn((
                            Text::new(">"),
                            crate::ui::typography::text(17.0),
                            TextColor(INK_MUTED),
                        ));
                    }
                    timeline
                        .spawn((
                            Node {
                                min_width: Val::Px(112.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                ..default()
                            },
                            BackgroundColor(if usize::from(route.current_stop) == index
                                && route.assigned_caravaner.is_some()
                            {
                                Color::srgba(0.78, 0.42, 0.22, 0.14)
                            } else {
                                Color::srgba(0.96, 0.95, 0.91, 0.70)
                            }),
                            BorderColor::from(if usize::from(route.current_stop) == index
                                && route.assigned_caravaner.is_some()
                            {
                                EMBER
                            } else {
                                PLATE_RULE_SOFT
                            }),
                        ))
                        .with_children(|stop_card| {
                            stop_card.spawn((
                                Text::new(format!(
                                    "STOP {}  /  {}",
                                    index + 1,
                                    stop.settlement_name.to_uppercase()
                                )),
                                crate::ui::typography::text(11.5),
                                TextColor(INK),
                            ));
                            stop_card.spawn((
                                Text::new(stop.action.label().to_uppercase()),
                                crate::ui::typography::text(11.0),
                                TextColor(EMBER),
                            ));
                        });
                }
            });

            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(14.0),
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|facts| {
                for text in [
                    format!("CARGO  {} / {}", route.cargo_onboard, route.cargo_target),
                    format!(
                        "CARAVANER  {}",
                        route
                            .assigned_caravaner
                            .as_deref()
                            .unwrap_or("not assigned")
                    ),
                    format!("TRIPS  {}", route.completed_trips),
                    format!("UNITS MOVED  {}", route.lifetime_units),
                    if route.autonomous_management {
                        "SERVICE  MASTER-REVIEWED TRIAL".to_string()
                    } else if route.automatic {
                        "SERVICE  REPEAT".to_string()
                    } else {
                        "SERVICE  ONE CIRCUIT".to_string()
                    },
                ] {
                    facts.spawn((
                        Text::new(text),
                        crate::ui::typography::text(11.5),
                        TextColor(INK_MUTED),
                    ));
                }
            });

            match route.mode {
                TradeRouteMode::ContractCarrier => card.spawn((
                    Text::new(format!(
                        "Buyer-funded cargo  /  {} coin freight earned  /  purchase ceiling {} coin. Stops are fixed by the public contract.",
                        format_money(route.lifetime_delivery_revenue),
                        format_money(route.maximum_purchase_price),
                    )),
                    crate::ui::typography::text(12.0),
                    TextColor(INK_MUTED),
                )),
                TradeRouteMode::Merchant => card.spawn((
                    Text::new(format!(
                        "Buy at or below {} coin  /  list sales at or above {} coin  /  {} coin spent  /  {} coin consigned at asking value.{} Consignment becomes revenue only when a real buyer purchases it.",
                        format_money(route.maximum_purchase_price),
                        format_money(route.minimum_destination_price),
                        format_money(route.lifetime_purchase_cost),
                        format_money(route.lifetime_consigned_value),
                        if route.autonomous_management {
                            format!(
                                " Company Master forecast: {} coin/trip at {}% confidence; the route pauses when cargo repeatedly remains unsold.",
                                if route.expected_trip_profit >= 0 {
                                    format_money(route.expected_trip_profit as u64)
                                } else {
                                    format!("-{}", format_money(route.expected_trip_profit.unsigned_abs()))
                                },
                                route.decision_confidence,
                            )
                        } else {
                            String::new()
                        },
                    )),
                    crate::ui::typography::text(12.0),
                    TextColor(INK_MUTED),
                )),
            };

            if route.trips.is_empty() {
                card.spawn((
                    Text::new("No completed circuit yet."),
                    crate::ui::typography::text(11.5),
                    TextColor(INK_MUTED),
                ));
            } else {
                for trip in route.trips.iter().rev().take(3) {
                    card.spawn((
                        Text::new(format!(
                            "DAY {}  /  {} stops  /  {} units  /  bought {}  /  freight {}  /  consigned {}  /  {:.1} world min",
                            trip.completed_day,
                            trip.stops_visited,
                            trip.units,
                            format_money(trip.source_purchase_cost),
                            format_money(trip.delivery_revenue),
                            format_money(trip.consigned_value),
                            trip.travel_world_seconds as f32 / 60.0,
                        )),
                        crate::ui::typography::text(11.0),
                        TextColor(INK_MUTED),
                    ));
                }
            }

            if can_manage && route.mode == TradeRouteMode::Merchant {
                card.spawn(Node {
                    justify_content: JustifyContent::FlexEnd,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|actions| {
                    let at_home = route.assigned_caravaner.is_none()
                        && matches!(
                            route.status,
                            TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                        );
                    if at_home {
                        detail_button(
                            actions,
                            EditTradeRouteButton {
                                company,
                                route: route.id,
                            },
                            "EDIT TIMETABLE",
                        );
                    }
                    if route.status == TradeRouteStatus::Idle && !route.automatic {
                        detail_button(
                            actions,
                            TradeRouteQuickActionButton {
                                company,
                                route: route.id,
                                action: TradeRouteQuickAction::DispatchOnce,
                            },
                            "RUN ONE CIRCUIT",
                        );
                    }
                    if route.status == TradeRouteStatus::Idle {
                        detail_button(
                            actions,
                            TradeRouteQuickActionButton {
                                company,
                                route: route.id,
                                action: TradeRouteQuickAction::Mothball,
                            },
                            "MOTHBALL",
                        );
                    } else if route.status == TradeRouteStatus::Mothballed {
                        detail_button(
                            actions,
                            TradeRouteQuickActionButton {
                                company,
                                route: route.id,
                                action: TradeRouteQuickAction::Reopen,
                            },
                            "REOPEN & DISPATCH",
                        );
                    }
                });
            }
        });
}
