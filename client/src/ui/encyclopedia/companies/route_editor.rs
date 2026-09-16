//! Trade-route editor layout using the current local draft.
//!
//! The editor's structure is the draft's shape -- route or new, ship or
//! caravan, how many stops, whether more than one home Storage Hall exists.
//! Every stepped value (cargo, prices, stop towns and orders, the SAVE label,
//! validation notes) binds under [`CompanyBound::Editor`] /
//! [`CompanyBound::EditorStop`], so a `+1` press updates the number under the
//! pointer instead of respawning the stepper that was just pressed.

use super::binding::{bound_text, CompanyBound, CompanyView, EditorField, EditorStopField};
use super::controls::TradeRouteEditorButton;
use super::model::{TradeRouteDraft, TradeRouteEditorAction};
use super::widgets::{bound_note, detail_button, spawn_note, spawn_section_title};
use crate::ui::foundation::UiButtonLabel;
use crate::ui::styles::{BUTTON_NORMAL, EMBER, INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::prelude::*;
use shared::components::MAX_TRADE_ROUTE_STOPS;

pub(super) fn editor_button(
    parent: &mut ChildSpawnerCommands<'_>,
    action: TradeRouteEditorAction,
    label: &str,
) {
    detail_button(parent, TradeRouteEditorButton(action), label);
}

/// An editor button whose label binds (SAVE ROUTE / SAVING..., REPEAT / ONE-CIRCUIT).
fn bound_editor_button(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    action: TradeRouteEditorAction,
    key: CompanyBound,
) {
    parent
        .spawn((
            Button,
            TradeRouteEditorButton(action),
            Node {
                min_height: Val::Px(31.0),
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(11.0), Val::Px(5.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            crate::ui::foundation::button_chrome(crate::ui::foundation::UiButtonVariant::Secondary),
        ))
        .with_children(|button| {
            bound_text(
                button,
                view,
                key,
                crate::ui::ledger::reading_strong(12.0),
                INK,
            )
            .insert((UiButtonLabel, Pickable::IGNORE));
        });
}

pub(super) fn spawn_trade_route_editor(
    parent: &mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    draft: &TradeRouteDraft,
) {
    let warehouses = view.warehouses().count();
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
                    bound_text(
                        copy,
                        view,
                        CompanyBound::Editor(EditorField::Title),
                        crate::ui::typography::heading(24.0),
                        INK,
                    );
                    bound_text(
                        copy,
                        view,
                        CompanyBound::Editor(EditorField::Subtitle),
                        crate::ui::ledger::reading(12.0),
                        EMBER,
                    );
                });
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|actions| {
                    editor_button(actions, TradeRouteEditorAction::Cancel, "BACK");
                    bound_editor_button(
                        actions,
                        view,
                        TradeRouteEditorAction::Save,
                        CompanyBound::Editor(EditorField::SaveLabel),
                    );
                });
        });
    spawn_note(
        parent,
        if draft.ship.is_some() {
            "The ship follows these ports in order. Buy and Sell use each town’s existing market, company cash and normal market fees. Cargo is moved through the port by real workers; ship stops do not load private warehouses."
        } else {
            "The caravan follows these stops from left to right, then loops back to stop one. Buy/Sell use public markets and real company cash. Load/Unload move owned stock through company Storage Halls without a sale."
        },
    );
    bound_note(parent, view, CompanyBound::Editor(EditorField::Message));

    spawn_section_title(
        parent,
        if draft.ship.is_some() {
            "SHIP & CARGO"
        } else {
            "CARAVAN & CARGO"
        },
        if draft.ship.is_some() {
            "one hull and one good per route"
        } else {
            "one porter cart and one good per route"
        },
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
                        Text::new(if draft.ship.is_some() {
                            "SHIP / HOME PORT"
                        } else {
                            "HOME STORAGE HALL"
                        }),
                        crate::ui::ledger::reading(11.5),
                        TextColor(INK_MUTED),
                    ));
                    bound_text(
                        card,
                        view,
                        CompanyBound::Editor(EditorField::Home),
                        crate::ui::ledger::reading(13.5),
                        INK,
                    );
                    if draft.route.is_none() && draft.ship.is_none() && warehouses > 1 {
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
                    bound_text(
                        card,
                        view,
                        CompanyBound::Editor(EditorField::Cargo),
                        crate::ui::ledger::reading(13.5),
                        INK,
                    );
                    bound_text(
                        card,
                        view,
                        CompanyBound::Editor(EditorField::Capacity),
                        crate::ui::ledger::reading(12.),
                        INK_MUTED,
                    );
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
                        if draft.ship.is_some() {
                            editor_button(buttons, TradeRouteEditorAction::CargoDown(25), "-25");
                            editor_button(buttons, TradeRouteEditorAction::CargoUp(25), "+25");
                        }
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
            bound_text(
                prices,
                view,
                CompanyBound::Editor(EditorField::Prices),
                crate::ui::ledger::reading(13.0),
                INK,
            );
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
                    bound_editor_button(
                        buttons,
                        view,
                        TradeRouteEditorAction::ToggleAutomatic,
                        CompanyBound::Editor(EditorField::ToggleLabel),
                    );
                });
        });

    spawn_section_title(
        parent,
        "ORDERED STOPS",
        if draft.ship.is_some() {
            "the highlighted instruction runs when the ship reaches that port"
        } else {
            "the highlighted instruction runs when the wagon reaches that town"
        },
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
            for index in 0..draft.stops.len() {
                let slot = index as u8;
                if index > 0 {
                    lane.spawn((
                        Text::new(">"),
                        crate::ui::ledger::reading(19.0),
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
                        crate::ui::ledger::reading(11.0),
                        TextColor(EMBER),
                    ));
                    bound_text(
                        stop_card,
                        view,
                        CompanyBound::EditorStop(slot, EditorStopField::Name),
                        crate::ui::ledger::reading(13.5),
                        INK,
                    );
                    bound_text(
                        stop_card,
                        view,
                        CompanyBound::EditorStop(slot, EditorStopField::Warning),
                        crate::ui::ledger::reading(11.0),
                        EMBER,
                    );
                    if index > 0 {
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
                    bound_text(
                        stop_card,
                        view,
                        CompanyBound::EditorStop(slot, EditorStopField::Action),
                        crate::ui::ledger::reading(11.5),
                        INK_MUTED,
                    );
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

    bound_note(parent, view, CompanyBound::Editor(EditorField::Warning));
}
