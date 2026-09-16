//! Company fleet and shipyard controls use public town-market ports. Physical
//! vessels and their transactions stay authoritative even when off camera.
use super::{
    model::*,
    widgets::{detail_button, spawn_note, spawn_section_title},
};
use crate::ui::{
    encyclopedia::ClickGuard,
    ledger,
    styles::{INK_MUTED, PLATE_RULE_SOFT},
};
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::{
    components::*,
    economy::Good,
    protocol::{HeroMaritimeAction, HeroMaritimeOrder, HeroMaritimeResult, ReliableChannel},
};
use std::collections::VecDeque;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FleetButton {
    pub company: CompanyId,
    pub action: FleetAction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FleetAction {
    NewRoute(ShipId),
    Order { port: BuildingId, kind: ShipKind },
    Cancel(ShipOrderId),
    Assign { route: TradeRouteId, ship: ShipId },
}
#[derive(Debug)]
struct Pending {
    company: CompanyId,
    create_route: Option<ShipId>,
}
#[derive(Resource, Default)]
pub struct MaritimeRequests {
    pending: VecDeque<Pending>,
}

pub(super) fn submit(
    sender: &mut MessageSender<HeroMaritimeOrder>,
    requests: &mut MaritimeRequests,
    company: CompanyId,
    action: HeroMaritimeAction,
) -> bool {
    if requests.pending.len() >= 16
        || requests
            .pending
            .iter()
            .any(|pending| pending.company == company)
    {
        return false;
    }
    let create_route = match &action {
        HeroMaritimeAction::CreateRoute { ship, .. } => Some(*ship),
        _ => None,
    };
    requests.pending.push_back(Pending {
        company,
        create_route,
    });
    sender.send::<ReliableChannel>(HeroMaritimeOrder { company, action });
    true
}
fn row(parent: &mut ChildSpawnerCommands<'_>, f: impl FnOnce(&mut ChildSpawnerCommands<'_>)) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.),
                min_width: Val::Px(0.),
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(12.),
                row_gap: Val::Px(8.),
                align_items: AlignItems::Center,
                padding: UiRect::vertical(Val::Px(8.)),
                border: UiRect::bottom(Val::Px(1.)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(f);
}
fn button(
    parent: &mut ChildSpawnerCommands<'_>,
    company: CompanyId,
    action: FleetAction,
    label: &str,
) {
    detail_button(parent, FleetButton { company, action }, label);
}

pub(super) fn spawn_fleet(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    directory: &CompanyDirectory,
) {
    let manager = directory.local_person == Some(company.master);
    spawn_section_title(
        parent,
        "Fleet",
        "company-owned ships; one good per timetable",
    );
    if company.fleet.ships.is_empty() {
        spawn_note(
            parent,
            "No ships owned. Order a hull at a completed town port below.",
        );
    }
    for (id, ship) in &company.fleet.ships {
        row(parent, |row| {
            row.spawn(Node {
                flex_grow: 1.,
                min_width: Val::Px(200.),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn(ledger::body_strong(
                    format!("{} #{}", ship.kind.label(), id.0),
                    15.,
                ));
                copy.spawn((
                    Text::new(format!(
                        "{}  ·  {} bulk  ·  {}",
                        ship.status.label(),
                        ship.kind.capacity(),
                        ship.assigned_route.map_or_else(
                            || "No route assigned".into(),
                            |route| format!("Route #{}", route.0)
                        )
                    )),
                    ledger::reading(12.),
                    TextColor(INK_MUTED),
                ));
            });
            if manager {
                if let Some(route) = ship.assigned_route {
                    if company.routes.iter().any(|r| {
                        r.id == route
                            && r.assigned_caravaner.is_none()
                            && matches!(
                                r.status,
                                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                            )
                    }) {
                        detail_button(
                            row,
                            super::controls::EditTradeRouteButton {
                                company: company.id,
                                route,
                            },
                            "EDIT TIMETABLE",
                        );
                    }
                } else {
                    button(
                        row,
                        company.id,
                        FleetAction::NewRoute(*id),
                        "NEW SHIP ROUTE",
                    );
                    for route in company.routes.iter().filter(|r| {
                        r.ship.is_some()
                            && r.assigned_caravaner.is_none()
                            && matches!(
                                r.status,
                                TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                            )
                    }) {
                        button(
                            row,
                            company.id,
                            FleetAction::Assign {
                                route: route.id,
                                ship: *id,
                            },
                            &format!("ASSIGN ROUTE #{}", route.id.0),
                        );
                    }
                }
            }
        });
    }
    spawn_note(
        parent,
        "A staffed company Storage Hall supplies the sailor. Port calls use that town’s existing market, prices and fees; purchases and sales still need real stock and buyers.",
    );
    spawn_section_title(
        parent,
        "Shipyard",
        "wood, iron and wool must be delivered before the hull is built",
    );
    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(18.),
            row_gap: Val::Px(8.),
            ..default()
        })
        .with_children(|types| {
            for kind in ShipKind::ALL {
                types
                    .spawn(Node {
                        min_width: Val::Px(210.),
                        flex_grow: 1.,
                        flex_basis: Val::Px(0.),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.),
                        ..default()
                    })
                    .with_children(|copy| {
                        copy.spawn(ledger::body_strong(
                            format!("{}  ·  {} bulk", kind.label(), kind.capacity()),
                            14.,
                        ));
                        copy.spawn((
                            Text::new(
                                kind.materials()
                                    .iter()
                                    .map(|(good, n)| format!("{n} {}", good.label()))
                                    .collect::<Vec<_>>()
                                    .join("  ·  "),
                            ),
                            ledger::reading(12.),
                            TextColor(INK_MUTED),
                        ));
                    });
            }
        });
    for (id, order) in &company.fleet.orders {
        if matches!(
            order.status,
            ShipOrderStatus::Completed | ShipOrderStatus::Cancelled
        ) {
            continue;
        }
        let port_name = directory
            .settlements
            .iter()
            .find(|town| town.port.is_some_and(|port| port.port == order.port))
            .map_or_else(
                || format!("Port #{}", order.port.0),
                |town| town.name.clone(),
            );
        row(parent, |row| {
            row.spawn(Node {
                min_width: Val::Px(210.),
                flex_grow: 1.,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn(ledger::body_strong(
                    format!(
                        "{} at {}  ·  {}%",
                        order.kind.label(),
                        port_name,
                        order.progress.min(1000) / 10
                    ),
                    14.,
                ));
                let delivered = order
                    .kind
                    .materials()
                    .iter()
                    .enumerate()
                    .map(|(i, (good, n))| format!("{}/{} {}", order.delivered[i], n, good.label()))
                    .collect::<Vec<_>>()
                    .join("  ·  ");
                copy.spawn((
                    Text::new(format!("{}\n{delivered}", order.status.label())),
                    ledger::reading(12.),
                    TextColor(INK_MUTED),
                ));
            });
            if manager {
                button(row, company.id, FleetAction::Cancel(*id), "CANCEL ORDER");
            }
        });
    }
    if !manager {
        return;
    }
    let ports: Vec<_> = directory
        .settlements
        .iter()
        .filter_map(|town| town.port.filter(|port| port.built).map(|port| (town, port)))
        .collect();
    if ports.is_empty() {
        spawn_note(
            parent,
            "No completed port is known. Ports can be developed by towns and cities on suitable coasts.",
        );
    }
    for (town, port) in ports {
        row(parent, |row| {
            row.spawn((
                Node {
                    min_width: Val::Px(160.),
                    flex_grow: 1.,
                    ..default()
                },
                ledger::body_strong(format!("{} port", town.name), 14.),
            ));
            for kind in ShipKind::ALL
                .into_iter()
                .filter(|kind| *kind <= port.maximum_ship)
            {
                button(
                    row,
                    company.id,
                    FleetAction::Order {
                        port: port.port,
                        kind,
                    },
                    &format!("ORDER {}", kind.label().to_uppercase()),
                );
            }
        });
    }
}

pub(super) fn new_ship_draft(
    company: &CompanyRecord,
    directory: &CompanyDirectory,
    id: ShipId,
) -> Result<TradeRouteDraft, &'static str> {
    let Some((_, ship)) = company
        .fleet
        .ships
        .iter()
        .find(|(candidate, _)| *candidate == id)
    else {
        return Err("This ship is no longer in the company fleet.");
    };
    if ship.assigned_route.is_some() {
        return Err("This ship already has a route. Edit its existing timetable.");
    }
    let Some(origin) = directory.settlements.iter().find(|town| {
        town.has_marketplace
            && town.port.is_some_and(|port| {
                port.port == ship.home_port && port.built && ship.kind <= port.maximum_ship
            })
    }) else {
        return Err("The ship's home port must have a completed market and a suitable berth.");
    };
    let Some(destination) = directory.settlements.iter().find(|town| {
        town.id != origin.id
            && town.has_marketplace
            && town
                .port
                .is_some_and(|port| port.built && ship.kind <= port.maximum_ship)
    }) else {
        return Err("A second completed port with a market and a suitable berth is needed.");
    };
    let warehouse = company
        .sites
        .iter()
        .find(|site| {
            site.settlement_id == origin.id && site.kind == SettlementBuildingKind::StorageHall
        })
        .map_or(BuildingId(0), |site| site.id);
    Ok(TradeRouteDraft {
        company: company.id,
        route: None,
        ship: Some((id, ship.kind)),
        warehouse,
        good: Good::Wood,
        cargo_target: 24,
        maximum_purchase_price: 100,
        minimum_destination_price: 125,
        automatic: true,
        stops: vec![
            TradeRouteStop {
                settlement: origin.id,
                action: TradeRouteStopAction::Buy,
            },
            TradeRouteStop {
                settlement: destination.id,
                action: TradeRouteStopAction::Sell,
            },
        ],
        pending: false,
    })
}

pub(in crate::ui::encyclopedia) fn handle_fleet_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &FleetButton), Changed<Interaction>>,
    directory: Res<CompanyDirectory>,
    mut editor: ResMut<TradeRouteEditorState>,
    mut feedback: ResMut<CompanyPolicyFeedback>,
    mut requests: ResMut<MaritimeRequests>,
    mut clients: Query<
        &mut MessageSender<HeroMaritimeOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(company) = directory.records.iter().find(|company| {
            company.id == button.company && directory.local_person == Some(company.master)
        }) else {
            continue;
        };
        if let FleetAction::NewRoute(ship) = button.action {
            match new_ship_draft(company, &directory, ship) {
                Ok(draft) => {
                    editor.draft = Some(draft);
                    editor.message.clear();
                }
                Err(message) => {
                    feedback.company = Some(company.id);
                    feedback.message = message.into();
                    feedback.success = false;
                }
            }
            continue;
        }
        let action = match button.action {
            FleetAction::Order { port, kind } => HeroMaritimeAction::OrderShip { port, kind },
            FleetAction::Cancel(order) => HeroMaritimeAction::CancelShipOrder { order },
            FleetAction::Assign { route, ship } => HeroMaritimeAction::AssignShip { route, ship },
            FleetAction::NewRoute(_) => unreachable!(),
        };
        feedback.company = Some(company.id);
        feedback.success = false;
        if let Ok(mut sender) = clients.single_mut() {
            feedback.message = if submit(&mut sender, &mut requests, company.id, action) {
                "Order sent. Waiting for the town ledger."
            } else {
                "Previous orders are still awaiting a reply."
            }
            .into();
        } else {
            feedback.message = "Connect to the server before issuing an order.".into();
        }
    }
}

pub(in crate::ui::encyclopedia) fn receive_maritime_results(
    mut receivers: Query<&mut MessageReceiver<HeroMaritimeResult>, With<crate::GameClient>>,
    mut requests: ResMut<MaritimeRequests>,
    mut feedback: ResMut<CompanyPolicyFeedback>,
    mut editor: ResMut<TradeRouteEditorState>,
) {
    for mut receiver in &mut receivers {
        for result in receiver.receive() {
            let Some(pending) = requests.pending.pop_front() else {
                continue;
            };
            feedback.company = Some(pending.company);
            feedback.message = result.message.clone();
            feedback.success = result.success;
            if let Some(ship) = pending.create_route {
                if editor.draft.as_ref().is_some_and(|draft| {
                    draft.company == pending.company
                        && draft.route.is_none()
                        && draft.ship.is_some_and(|(id, _)| id == ship)
                        && draft.pending
                }) {
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
    }
}
