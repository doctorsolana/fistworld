//! Trade-route draft editing, command submission and server feedback.

use super::controls::{
    EditTradeRouteButton, NewTradeRouteButton, TradeRouteEditorButton, TradeRouteQuickActionButton,
};
use super::model::{
    CompanyDirectory, TradeRouteDraft, TradeRouteEditorAction, TradeRouteEditorState,
    TradeRouteQuickAction,
};
use crate::ui::encyclopedia::*;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::components::{
    SettlementBuildingKind, TradeRouteMode, TradeRouteStop, TradeRouteStopAction,
    MAX_TRADE_ROUTE_STOPS,
};
use shared::economy::Good;
use shared::protocol::{
    HeroTradeRouteAction, HeroTradeRouteOrder, HeroTradeRouteResult, ReliableChannel,
};

pub(in crate::ui::encyclopedia) fn handle_trade_route_open_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    new_buttons: Query<(&Interaction, &NewTradeRouteButton), Changed<Interaction>>,
    edit_buttons: Query<(&Interaction, &EditTradeRouteButton), Changed<Interaction>>,
    directory: Res<CompanyDirectory>,
    mut editor: ResMut<TradeRouteEditorState>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, NewTradeRouteButton(company_id)) in new_buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(company) = directory
            .records
            .iter()
            .find(|company| company.id == *company_id)
        else {
            continue;
        };
        let Some(warehouse) = company
            .sites
            .iter()
            .find(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0)
        else {
            editor.message =
                "Build a Storage Hall and employ a Company Porter before creating a route."
                    .to_string();
            editor.success = false;
            continue;
        };
        if !directory.settlements.iter().any(|settlement| {
            settlement.id == warehouse.settlement_id && settlement.has_marketplace
        }) {
            editor.message =
                "This Storage Hall's settlement needs a completed Marketplace before public caravan trade can begin."
                    .to_string();
            editor.success = false;
            continue;
        }
        let Some(destination) = directory.settlements.iter().find(|settlement| {
            settlement.id != warehouse.settlement_id && settlement.has_marketplace
        }) else {
            editor.message =
                "A second settlement must complete its Marketplace before this route can trade."
                    .to_string();
            editor.success = false;
            continue;
        };
        editor.draft = Some(TradeRouteDraft {
            company: *company_id,
            route: None,
            warehouse: warehouse.id,
            good: Good::Stone,
            cargo_target: 4,
            maximum_purchase_price: shared::economy::PENNIES_PER_COIN,
            minimum_destination_price: shared::economy::PENNIES_PER_COIN + 25,
            automatic: true,
            stops: vec![
                TradeRouteStop {
                    settlement: warehouse.settlement_id,
                    action: TradeRouteStopAction::Buy,
                },
                TradeRouteStop {
                    settlement: destination.id,
                    action: TradeRouteStopAction::Sell,
                },
            ],
            pending: false,
        });
        editor.message.clear();
    }
    for (interaction, button) in edit_buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(company) = directory
            .records
            .iter()
            .find(|company| company.id == button.company)
        else {
            continue;
        };
        let Some(route) = company.routes.iter().find(|route| route.id == button.route) else {
            continue;
        };
        if route.mode != TradeRouteMode::Merchant {
            editor.message = "Civic contract stops are fixed by the buyer.".to_string();
            editor.success = false;
            continue;
        }
        editor.draft = Some(TradeRouteDraft {
            company: button.company,
            route: Some(button.route),
            warehouse: route.warehouse,
            good: route.good,
            cargo_target: route.cargo_target,
            maximum_purchase_price: route.maximum_purchase_price,
            minimum_destination_price: route.minimum_destination_price,
            automatic: route.automatic,
            stops: route
                .stops
                .iter()
                .map(|stop| TradeRouteStop {
                    settlement: stop.settlement,
                    action: stop.action,
                })
                .collect(),
            pending: false,
        });
        editor.message.clear();
    }
}

pub(in crate::ui::encyclopedia) fn handle_trade_route_quick_actions(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &TradeRouteQuickActionButton), Changed<Interaction>>,
    mut clients: Query<
        &mut MessageSender<HeroTradeRouteOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, button) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Ok(mut sender) = clients.single_mut() else {
            continue;
        };
        let action = match button.action {
            TradeRouteQuickAction::DispatchOnce => HeroTradeRouteAction::DispatchOnce {
                route: button.route,
            },
            TradeRouteQuickAction::Mothball => HeroTradeRouteAction::SetMothballed {
                route: button.route,
                mothballed: true,
            },
            TradeRouteQuickAction::Reopen => HeroTradeRouteAction::SetMothballed {
                route: button.route,
                mothballed: false,
            },
        };
        sender.send::<ReliableChannel>(HeroTradeRouteOrder {
            company: button.company,
            action,
        });
    }
}

pub(super) fn cycle_index<T: PartialEq>(items: &[T], current: &T, direction: isize) -> usize {
    let index = items.iter().position(|item| item == current).unwrap_or(0);
    (index as isize + direction).rem_euclid(items.len() as isize) as usize
}

pub(in crate::ui::encyclopedia) fn handle_trade_route_editor_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &TradeRouteEditorButton), Changed<Interaction>>,
    directory: Res<CompanyDirectory>,
    mut editor: ResMut<TradeRouteEditorState>,
    mut clients: Query<
        &mut MessageSender<HeroTradeRouteOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, TradeRouteEditorButton(action)) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if *action == TradeRouteEditorAction::Cancel {
            editor.draft = None;
            editor.message.clear();
            continue;
        }
        let Some(mut draft) = editor.draft.take() else {
            continue;
        };
        if draft.pending {
            editor.draft = Some(draft);
            continue;
        }
        let company = directory
            .records
            .iter()
            .find(|company| company.id == draft.company);
        let warehouses: Vec<_> = company
            .into_iter()
            .flat_map(|company| company.sites.iter())
            .filter(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0)
            .collect();
        match *action {
            TradeRouteEditorAction::Cancel => unreachable!(),
            TradeRouteEditorAction::Save => {
                let action = if let Some(route) = draft.route {
                    HeroTradeRouteAction::Update {
                        route,
                        good: draft.good,
                        cargo_target: draft.cargo_target,
                        maximum_purchase_price: draft.maximum_purchase_price,
                        minimum_destination_price: draft.minimum_destination_price,
                        automatic: draft.automatic,
                        stops: draft.stops.clone(),
                    }
                } else {
                    HeroTradeRouteAction::Create {
                        warehouse: draft.warehouse,
                        good: draft.good,
                        cargo_target: draft.cargo_target,
                        maximum_purchase_price: draft.maximum_purchase_price,
                        minimum_destination_price: draft.minimum_destination_price,
                        automatic: draft.automatic,
                        stops: draft.stops.clone(),
                    }
                };
                if let Ok(mut sender) = clients.single_mut() {
                    sender.send::<ReliableChannel>(HeroTradeRouteOrder {
                        company: draft.company,
                        action,
                    });
                    draft.pending = true;
                }
            }
            TradeRouteEditorAction::PreviousWarehouse | TradeRouteEditorAction::NextWarehouse => {
                if !warehouses.is_empty() && draft.route.is_none() {
                    let direction = if *action == TradeRouteEditorAction::PreviousWarehouse {
                        -1
                    } else {
                        1
                    };
                    let ids: Vec<_> = warehouses.iter().map(|site| site.id).collect();
                    let next = cycle_index(&ids, &draft.warehouse, direction);
                    draft.warehouse = ids[next];
                    if let Some(first) = draft.stops.first_mut() {
                        first.settlement = warehouses[next].settlement_id;
                    }
                }
            }
            TradeRouteEditorAction::PreviousGood | TradeRouteEditorAction::NextGood => {
                let direction = if *action == TradeRouteEditorAction::PreviousGood {
                    -1
                } else {
                    1
                };
                let next = cycle_index(&Good::ALL, &draft.good, direction);
                draft.good = Good::ALL[next];
                let capacity =
                    shared::economy::capacity::PORTER / draft.good.bulk_per_unit().max(1);
                draft.cargo_target = draft.cargo_target.min(capacity.max(1));
            }
            TradeRouteEditorAction::CargoDown(amount) => {
                draft.cargo_target = draft.cargo_target.saturating_sub(amount).max(1);
            }
            TradeRouteEditorAction::CargoUp(amount) => {
                let capacity =
                    shared::economy::capacity::PORTER / draft.good.bulk_per_unit().max(1);
                draft.cargo_target = draft
                    .cargo_target
                    .saturating_add(amount)
                    .min(capacity.max(1));
            }
            TradeRouteEditorAction::BuyPriceDown(amount) => {
                draft.maximum_purchase_price =
                    draft.maximum_purchase_price.saturating_sub(amount).max(1);
            }
            TradeRouteEditorAction::BuyPriceUp(amount) => {
                draft.maximum_purchase_price = draft
                    .maximum_purchase_price
                    .saturating_add(amount)
                    .min(1_000 * shared::economy::PENNIES_PER_COIN);
            }
            TradeRouteEditorAction::SellPriceDown(amount) => {
                draft.minimum_destination_price = draft
                    .minimum_destination_price
                    .saturating_sub(amount)
                    .max(1);
            }
            TradeRouteEditorAction::SellPriceUp(amount) => {
                draft.minimum_destination_price = draft
                    .minimum_destination_price
                    .saturating_add(amount)
                    .min(1_000 * shared::economy::PENNIES_PER_COIN);
            }
            TradeRouteEditorAction::ToggleAutomatic => draft.automatic = !draft.automatic,
            TradeRouteEditorAction::AddStop => {
                if draft.stops.len() < MAX_TRADE_ROUTE_STOPS {
                    let last = draft.stops.last().map(|stop| stop.settlement);
                    if let Some(settlement) = directory.settlements.iter().find(|settlement| {
                        Some(settlement.id) != last && settlement.has_marketplace
                    }) {
                        draft.stops.push(TradeRouteStop {
                            settlement: settlement.id,
                            action: TradeRouteStopAction::Sell,
                        });
                    }
                }
            }
            TradeRouteEditorAction::RemoveStop(index) => {
                if index > 0 && draft.stops.len() > 2 && index < draft.stops.len() {
                    draft.stops.remove(index);
                }
            }
            TradeRouteEditorAction::MoveStopLeft(index) => {
                if index > 1 && index < draft.stops.len() {
                    draft.stops.swap(index, index - 1);
                }
            }
            TradeRouteEditorAction::MoveStopRight(index) => {
                if index > 0 && index + 1 < draft.stops.len() {
                    draft.stops.swap(index, index + 1);
                }
            }
            TradeRouteEditorAction::PreviousStopSettlement(index)
            | TradeRouteEditorAction::NextStopSettlement(index) => {
                if index > 0 && index < draft.stops.len() && !directory.settlements.is_empty() {
                    let direction =
                        if matches!(action, TradeRouteEditorAction::PreviousStopSettlement(_)) {
                            -1
                        } else {
                            1
                        };
                    let ids: Vec<_> = directory
                        .settlements
                        .iter()
                        .filter(|settlement| settlement.has_marketplace)
                        .map(|settlement| settlement.id)
                        .collect();
                    if ids.is_empty() {
                        continue;
                    }
                    let next = cycle_index(&ids, &draft.stops[index].settlement, direction);
                    draft.stops[index].settlement = ids[next];
                }
            }
            TradeRouteEditorAction::PreviousStopAction(index)
            | TradeRouteEditorAction::NextStopAction(index) => {
                if index < draft.stops.len() {
                    const ACTIONS: [TradeRouteStopAction; 4] = [
                        TradeRouteStopAction::Load,
                        TradeRouteStopAction::Buy,
                        TradeRouteStopAction::Unload,
                        TradeRouteStopAction::Sell,
                    ];
                    let direction =
                        if matches!(action, TradeRouteEditorAction::PreviousStopAction(_)) {
                            -1
                        } else {
                            1
                        };
                    let next = cycle_index(&ACTIONS, &draft.stops[index].action, direction);
                    draft.stops[index].action = ACTIONS[next];
                    if !directory.settlements.iter().any(|settlement| {
                        settlement.id == draft.stops[index].settlement && settlement.has_marketplace
                    }) {
                        if let Some(market) = directory
                            .settlements
                            .iter()
                            .find(|settlement| settlement.has_marketplace)
                        {
                            draft.stops[index].settlement = market.id;
                        }
                    }
                }
            }
        }
        editor.draft = Some(draft);
    }
}

pub(in crate::ui::encyclopedia) fn receive_trade_route_results(
    mut receivers: Query<&mut MessageReceiver<HeroTradeRouteResult>, With<crate::GameClient>>,
    mut editor: ResMut<TradeRouteEditorState>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            editor.message = result.message;
            editor.success = result.success;
            if result.success {
                editor.draft = None;
            } else if let Some(draft) = editor.draft.as_mut() {
                draft.pending = false;
            }
        }
    }
}
