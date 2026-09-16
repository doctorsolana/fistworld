//! Current route status, stops and trip-history cards.
//!
//! A card is keyed by `TradeRouteId`. Cargo, status, caravaner, trip rows and
//! the current-stop highlight bind in place; only the gating bits hashed in
//! `binding::company_structure_key` (mode, at-home, automatic, ship, stop and
//! trip counts) decide which controls exist.

use super::binding::{bound_text, CompanyBound, CompanyView, RouteField, StopField};
use super::controls::{EditTradeRouteButton, TradeRouteQuickActionButton};
use super::model::{CompanyRouteRecord, TradeRouteQuickAction};
use super::widgets::detail_button;
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::components::{TradeRouteMode, TradeRouteStatus};

pub(super) fn spawn_route_card(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    route: &CompanyRouteRecord,
) {
    let company = view.company.id;
    let can_manage = view.can_manage();
    let key = |field| CompanyBound::Route(route.id, field);
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
                        bound_text(
                            copy,
                            view,
                            key(RouteField::Title),
                            crate::ui::ledger::reading(15.0),
                            INK,
                        );
                        bound_text(
                            copy,
                            view,
                            key(RouteField::Subtitle),
                            crate::ui::ledger::reading(11.5),
                            INK_MUTED,
                        );
                    });
                bound_text(
                    header,
                    view,
                    key(RouteField::Status),
                    crate::ui::ledger::reading(12.0),
                    INK_MUTED,
                );
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
                for index in 0..route.stops.len() {
                    let slot = index as u8;
                    if index > 0 {
                        timeline.spawn((
                            Text::new(">"),
                            crate::ui::ledger::reading(17.0),
                            TextColor(INK_MUTED),
                        ));
                    }
                    let card_key = CompanyBound::RouteStop(route.id, slot, StopField::Card);
                    let (background, border) = view
                        .value(card_key)
                        .and_then(|value| value.highlight)
                        .unwrap_or((Color::srgba(0.96, 0.95, 0.91, 0.70), PLATE_RULE_SOFT));
                    timeline
                        .spawn((
                            card_key,
                            Node {
                                min_width: Val::Px(112.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                ..default()
                            },
                            BackgroundColor(background),
                            BorderColor::all(border),
                        ))
                        .with_children(|stop_card| {
                            bound_text(
                                stop_card,
                                view,
                                CompanyBound::RouteStop(route.id, slot, StopField::Name),
                                crate::ui::ledger::reading(11.5),
                                INK,
                            );
                            bound_text(
                                stop_card,
                                view,
                                CompanyBound::RouteStop(route.id, slot, StopField::Action),
                                crate::ui::ledger::reading(11.0),
                                EMBER,
                            );
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
                for field in [
                    RouteField::Cargo,
                    RouteField::Caravaner,
                    RouteField::Trips,
                    RouteField::Units,
                    RouteField::Service,
                ] {
                    bound_text(
                        facts,
                        view,
                        key(field),
                        crate::ui::ledger::reading(11.5),
                        INK_MUTED,
                    );
                }
            });

            bound_text(
                card,
                view,
                key(RouteField::Summary),
                crate::ui::ledger::reading(12.0),
                INK_MUTED,
            );

            if route.trips.is_empty() {
                card.spawn((
                    Text::new("No completed circuit yet."),
                    crate::ui::ledger::reading(11.5),
                    TextColor(INK_MUTED),
                ));
            } else {
                for slot in 0..route.trips.len().min(3) {
                    bound_text(
                        card,
                        view,
                        CompanyBound::RouteTrip(route.id, slot as u8),
                        crate::ui::ledger::reading(11.0),
                        INK_MUTED,
                    );
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
                    } else if route.ship.is_some() && route.status != TradeRouteStatus::Mothballed {
                        detail_button(
                            actions,
                            TradeRouteQuickActionButton {
                                company,
                                route: route.id,
                                action: TradeRouteQuickAction::Mothball,
                            },
                            "STOP AFTER VOYAGE",
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
