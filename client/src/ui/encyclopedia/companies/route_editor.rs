//! Trade-route editor layout using the current local draft.

use super::controls::TradeRouteEditorButton;
use super::model::{
    CompanyDirectory, CompanyRecord, TradeRouteDraft, TradeRouteEditorAction, TradeRouteEditorState,
};
use super::widgets::{detail_button, spawn_note, spawn_section_title};
use crate::ui::styles::{BUTTON_NORMAL, EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::components::{
    SettlementBuildingKind, SettlementId, TradeRouteStopAction, MAX_TRADE_ROUTE_STOPS,
};
use shared::economy::format_money;

pub(super) fn editor_button(
    parent: &mut ChildSpawnerCommands<'_>,
    action: TradeRouteEditorAction,
    label: &str,
) {
    detail_button(parent, TradeRouteEditorButton(action), label);
}

pub(super) fn spawn_trade_route_editor(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    directory: &CompanyDirectory,
    draft: &TradeRouteDraft,
    editor: &TradeRouteEditorState,
) {
    let warehouses: Vec<_> = company
        .sites
        .iter()
        .filter(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0)
        .collect();
    let warehouse = warehouses
        .iter()
        .find(|site| site.id == draft.warehouse)
        .copied();
    let settlement_name = |id: SettlementId| {
        directory
            .settlements
            .iter()
            .find(|settlement| settlement.id == id)
            .map_or_else(
                || format!("Settlement #{}", id.0),
                |settlement| settlement.name.clone(),
            )
    };

    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(if let Some(route) = draft.route {
                            format!("EDIT CARAVAN ROUTE #{}", route.0)
                        } else {
                            "NEW CARAVAN ROUTE".to_string()
                        }),
                        crate::ui::typography::text(24.0),
                        TextColor(INK),
                    ));
                    copy.spawn((
                        Text::new(format!(
                            "{}  /  ORDERED MERCHANT TIMETABLE",
                            company.name.to_uppercase()
                        )),
                        crate::ui::typography::text(12.0),
                        TextColor(EMBER),
                    ));
                });
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|actions| {
                    editor_button(actions, TradeRouteEditorAction::Cancel, "BACK");
                    editor_button(
                        actions,
                        TradeRouteEditorAction::Save,
                        if draft.pending {
                            "SAVING..."
                        } else {
                            "SAVE ROUTE"
                        },
                    );
                });
        });
    spawn_note(
        parent,
        "The caravan follows these stops from left to right, then loops back to stop one. Buy/Sell use public markets and real company cash. Load/Unload move owned stock through company Storage Halls without a sale.",
    );
    if !editor.message.is_empty() {
        spawn_note(parent, &editor.message);
    }

    spawn_section_title(
        parent,
        "CARAVAN & CARGO",
        "one porter cart and one good per route",
    );
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(8.0),
            row_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|settings| {
            settings
                .spawn((
                    Node {
                        width: Val::Percent(48.0),
                        min_width: Val::Px(220.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new("HOME STORAGE HALL"),
                        crate::ui::typography::text(11.5),
                        TextColor(INK_MUTED),
                    ));
                    card.spawn((
                        Text::new(warehouse.map_or_else(
                            || format!("Storage Hall #{}", draft.warehouse.0),
                            |site| {
                                format!(
                                    "Storage Hall #{} / {} / {} porter{}",
                                    site.id.0,
                                    site.settlement,
                                    site.workers,
                                    if site.workers == 1 { "" } else { "s" }
                                )
                            },
                        )),
                        crate::ui::typography::text(13.5),
                        TextColor(INK),
                    ));
                    if draft.route.is_none() && warehouses.len() > 1 {
                        card.spawn(Node {
                            column_gap: Val::Px(5.0),
                            ..default()
                        })
                        .with_children(|buttons| {
                            editor_button(
                                buttons,
                                TradeRouteEditorAction::PreviousWarehouse,
                                "‹ PREVIOUS",
                            );
                            editor_button(buttons, TradeRouteEditorAction::NextWarehouse, "NEXT ›");
                        });
                    }
                });
            settings
                .spawn((
                    Node {
                        width: Val::Percent(48.0),
                        min_width: Val::Px(220.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new(format!(
                            "CARGO  {}  /  TARGET {} UNIT{}",
                            draft.good.label().to_uppercase(),
                            draft.cargo_target,
                            if draft.cargo_target == 1 { "" } else { "S" }
                        )),
                        crate::ui::typography::text(13.5),
                        TextColor(INK),
                    ));
                    card.spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(5.0),
                        row_gap: Val::Px(5.0),
                        ..default()
                    })
                    .with_children(|buttons| {
                        editor_button(buttons, TradeRouteEditorAction::PreviousGood, "‹ GOOD");
                        editor_button(buttons, TradeRouteEditorAction::NextGood, "GOOD ›");
                        editor_button(buttons, TradeRouteEditorAction::CargoDown(1), "-1");
                        editor_button(buttons, TradeRouteEditorAction::CargoUp(1), "+1");
                        editor_button(buttons, TradeRouteEditorAction::CargoDown(5), "-5");
                        editor_button(buttons, TradeRouteEditorAction::CargoUp(5), "+5");
                    });
                });
        });

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.88, 0.86, 0.81, 0.52)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|prices| {
            prices.spawn((
                Text::new(format!(
                    "BUY CEILING  {} COIN  /  SALE FLOOR  {} COIN  /  {}",
                    format_money(draft.maximum_purchase_price),
                    format_money(draft.minimum_destination_price),
                    if draft.automatic {
                        "REPEAT CONTINUOUSLY"
                    } else {
                        "ONE CIRCUIT ON COMMAND"
                    }
                )),
                crate::ui::typography::text(13.0),
                TextColor(INK),
            ));
            prices
                .spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(5.0),
                    row_gap: Val::Px(5.0),
                    ..default()
                })
                .with_children(|buttons| {
                    editor_button(
                        buttons,
                        TradeRouteEditorAction::BuyPriceDown(25),
                        "BUY -0.25",
                    );
                    editor_button(buttons, TradeRouteEditorAction::BuyPriceUp(25), "BUY +0.25");
                    editor_button(
                        buttons,
                        TradeRouteEditorAction::SellPriceDown(25),
                        "SELL -0.25",
                    );
                    editor_button(
                        buttons,
                        TradeRouteEditorAction::SellPriceUp(25),
                        "SELL +0.25",
                    );
                    editor_button(
                        buttons,
                        TradeRouteEditorAction::ToggleAutomatic,
                        if draft.automatic {
                            "MAKE ONE-CIRCUIT"
                        } else {
                            "REPEAT ROUTE"
                        },
                    );
                });
        });

    spawn_section_title(
        parent,
        "ORDERED STOPS",
        "the highlighted instruction runs when the wagon reaches that town",
    );
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|lane| {
            for (index, stop) in draft.stops.iter().enumerate() {
                if index > 0 {
                    lane.spawn((
                        Text::new(">"),
                        crate::ui::typography::text(19.0),
                        TextColor(INK_MUTED),
                    ));
                }
                lane.spawn((
                    Node {
                        width: Val::Px(164.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        padding: UiRect::all(Val::Px(9.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.96, 0.95, 0.91, 0.82)),
                    BorderColor::from(if index == 0 { EMBER } else { PLATE_RULE_SOFT }),
                ))
                .with_children(|stop_card| {
                    stop_card.spawn((
                        Text::new(format!(
                            "STOP {}  /  {}",
                            index + 1,
                            if index == 0 { "HOME" } else { "TOWN" }
                        )),
                        crate::ui::typography::text(11.0),
                        TextColor(EMBER),
                    ));
                    stop_card.spawn((
                        Text::new(settlement_name(stop.settlement).to_uppercase()),
                        crate::ui::typography::text(13.5),
                        TextColor(INK),
                    ));
                    let has_marketplace = directory.settlements.iter().any(|settlement| {
                        settlement.id == stop.settlement && settlement.has_marketplace
                    });
                    if !has_marketplace {
                        stop_card.spawn((
                            Text::new("LOCAL MOOT — BUILD A MARKETPLACE"),
                            crate::ui::typography::text(11.0),
                            TextColor(EMBER),
                        ));
                    }
                    if index > 0 {
                        stop_card
                            .spawn(Node {
                                column_gap: Val::Px(4.0),
                                ..default()
                            })
                            .with_children(|buttons| {
                                editor_button(
                                    buttons,
                                    TradeRouteEditorAction::PreviousStopSettlement(index),
                                    "‹ TOWN",
                                );
                                editor_button(
                                    buttons,
                                    TradeRouteEditorAction::NextStopSettlement(index),
                                    "TOWN ›",
                                );
                            });
                    }
                    stop_card.spawn((
                        Text::new(stop.action.label().to_uppercase()),
                        crate::ui::typography::text(11.5),
                        TextColor(INK_MUTED),
                    ));
                    stop_card
                        .spawn(Node {
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(4.0),
                            row_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|buttons| {
                            editor_button(
                                buttons,
                                TradeRouteEditorAction::PreviousStopAction(index),
                                "‹ ORDER",
                            );
                            editor_button(
                                buttons,
                                TradeRouteEditorAction::NextStopAction(index),
                                "ORDER ›",
                            );
                            if index > 1 {
                                editor_button(
                                    buttons,
                                    TradeRouteEditorAction::MoveStopLeft(index),
                                    "<",
                                );
                            }
                            if index > 0 && index + 1 < draft.stops.len() {
                                editor_button(
                                    buttons,
                                    TradeRouteEditorAction::MoveStopRight(index),
                                    ">",
                                );
                            }
                            if index > 0 && draft.stops.len() > 2 {
                                editor_button(
                                    buttons,
                                    TradeRouteEditorAction::RemoveStop(index),
                                    "REMOVE",
                                );
                            }
                        });
                });
            }
            if draft.stops.len() < MAX_TRADE_ROUTE_STOPS {
                editor_button(lane, TradeRouteEditorAction::AddStop, "+ ADD TOWN");
            }
        });

    let storage_settlements: Vec<_> = company
        .sites
        .iter()
        .filter(|site| site.kind == SettlementBuildingKind::StorageHall)
        .map(|site| site.settlement_id)
        .collect();
    let has_invalid_private_stop = draft.stops.iter().any(|stop| {
        matches!(
            stop.action,
            TradeRouteStopAction::Load | TradeRouteStopAction::Unload
        ) && !storage_settlements.contains(&stop.settlement)
    });
    let repeats_town = draft
        .stops
        .windows(2)
        .any(|pair| pair[0].settlement == pair[1].settlement);
    if has_invalid_private_stop || repeats_town {
        spawn_note(
            parent,
            if has_invalid_private_stop {
                "Load and Unload require this company to own a Storage Hall in that town. Use Buy or Sell for a public market stop."
            } else {
                "The same town cannot appear in two consecutive stops. Returning to the home town as the final stop is allowed."
            },
        );
    }
}
