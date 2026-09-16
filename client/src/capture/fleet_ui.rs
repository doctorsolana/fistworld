//! Offline fleet facts exercised through the production book and route buttons.
//! No order is sent and no cargo, payment, launch or navigation is claimed here.
use super::{
    CaptureConfig, CaptureState,
    ledger_nested::{bounds, entities, scroll_ancestor, scroll_limit, shot, visible},
};
use crate::ui::{
    encyclopedia::{
        self, ClickGuard, EncyclopediaOpen, EncyclopediaPanel, EncyclopediaTab, companies,
    },
    ledger::LedgerArtwork,
    portraits::{PortraitMetrics, PortraitStatus},
};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{InteractionDisabled, UiSystems},
    window::PrimaryWindow,
};
use shared::{components::*, economy::*};
#[derive(Resource, Default)]
pub(super) struct Rehearsal {
    staged: bool,
    pressed: Option<Entity>,
    ready_shot: Option<usize>,
    inspected: Option<usize>,
    error: String,
}
pub(super) fn install(app: &mut App) {
    if std::env::var("FISTWORLD_CAPTURE_FLEET_UI").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<Rehearsal>();
    app.add_systems(Update, stage);
    app.add_systems(
        PreUpdate,
        input
            .after(InputSystems)
            .after(UiSystems::Focus)
            .before(crate::ui::chat::ChatInput),
    );
    app.add_systems(Last, inspect);
}
fn stage(world: &mut World) {
    if world.resource::<Rehearsal>().staged {
        return;
    }
    let Some(hero) = entities::<Hero>(world).into_iter().next() else {
        return;
    };
    let Some(company) = entities::<CompanyId>(world)
        .into_iter()
        .find(|e| world.get::<CompanyId>(*e) == Some(&CompanyId(501)))
    else {
        return;
    };
    let owner = world.get::<Hero>(hero).unwrap().owner;
    world
        .entity_mut(hero)
        .insert((PersonId(1), CharacterName("Aldric".into())));
    world.insert_resource(crate::camera_rts::LocalPeerId(
        shared::player::peer_id_to_u64(owner),
    ));
    let cast: Vec<_> = world
        .resource::<encyclopedia::KnownPeople>()
        .records
        .iter()
        .map(|p| p.id)
        .filter(|id| id.is_assigned())
        .collect();
    for id in cast {
        if !world
            .query::<(&PersonId, &HeroOutfit)>()
            .iter(world)
            .any(|(person, _)| *person == id)
        {
            world.spawn((id, HeroOutfit::varied(500 + id.0 * 19)));
        }
    }
    world.entity_mut(company).insert(CompanyFleet {
        cargo: vec![(ShipId(92), Good::Wool, 80)],
        ships: vec![
            (
                ShipId(91),
                CompanyShip {
                    company: CompanyId(501),
                    kind: ShipKind::Coaster,
                    home_port: BuildingId(801),
                    assigned_route: None,
                    status: ShipStatus::Moored,
                },
            ),
            (
                ShipId(92),
                CompanyShip {
                    company: CompanyId(501),
                    kind: ShipKind::Cog,
                    home_port: BuildingId(801),
                    assigned_route: Some(TradeRouteId(901)),
                    status: ShipStatus::Sailing,
                },
            ),
        ],
        orders: vec![(
            ShipOrderId(81),
            ShipConstructionOrder {
                company: CompanyId(501),
                port: BuildingId(801),
                kind: ShipKind::Cog,
                status: ShipOrderStatus::Building,
                delivered: [96, 20, 24],
                progress: 450,
            },
        )],
    });
    for (id, name, port, kind) in [
        (701, "Brackwater", 801, ShipKind::Cog),
        (702, "High Meadow", 802, ShipKind::Coaster),
        (703, "Rivermeet", 803, ShipKind::Cog),
    ] {
        world.spawn((
            SettlementSummary {
                id: SettlementId(id),
                name: name.into(),
                tier: SettlementTier::Town,
                residents: 84,
                treasury: 2000,
                prosperity: 0.6,
                reserve_days: 8.,
                recent_food_production: 24.,
                recent_food_consumption: 18.,
                hungry: 0,
                housing_capacity: 90,
                homeless: 0,
                job_seekers: 0,
                unpaid_workers: 0,
                unrest: 0.,
                unrest_change: 0.,
                unrest_target: 0.,
                unrest_hunger_pressure: 0.,
                unrest_housing_pressure: 0.,
                unrest_wage_pressure: 0.,
                houses: 24,
                farmsteads: 4,
                fishing_huts: 2,
                lumber_huts: 2,
                windmills: 2,
                bakeries: 2,
                has_marketplace: true,
            },
            SettlementPortSummary {
                port: BuildingId(port),
                maximum_ship: kind,
                built: true,
            },
        ));
    }
    let stops = [
        TradeRouteStop {
            settlement: SettlementId(701),
            action: TradeRouteStopAction::Buy,
        },
        TradeRouteStop {
            settlement: SettlementId(703),
            action: TradeRouteStopAction::Sell,
        },
    ];
    world.spawn((
        TradeRouteId(901),
        CompanyTradeRoute {
            company: CompanyId(501),
            warehouse: BuildingId(605),
            mode: TradeRouteMode::Merchant,
            origin: SettlementId(701),
            destination: SettlementId(703),
            good: Good::Wool,
            cargo_target: 120,
            maximum_purchase_price: 100,
            minimum_destination_price: 130,
            automatic: true,
            autonomous_management: false,
            expected_trip_profit: 0,
            decision_confidence: 0,
            active_contract: None,
            assigned_caravaner: Some(PersonId(2)),
            current_stop: 1,
            status: TradeRouteStatus::InTransit,
            completed_trips: 0,
            lifetime_units: 0,
            lifetime_delivery_revenue: 0,
            lifetime_purchase_cost: 12000,
            lifetime_consigned_value: 0,
        },
        TradeRouteSchedule::new(stops).unwrap(),
        TradeRouteHistory::default(),
        MaritimeTradeRoute { ship: ShipId(92) },
    ));
    world.resource_mut::<EncyclopediaOpen>().0 = true;
    *world.resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Companies;
    world.resource_mut::<companies::SelectedCompany>().0 = Some(CompanyId(501));
    for mut window in world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .iter_mut(world)
    {
        window.focused = true;
    }
    world.resource_mut::<Rehearsal>().staged = true;
}
fn find<T: Component>(world: &mut World, predicate: impl Fn(&T) -> bool) -> Option<Entity> {
    entities::<T>(world)
        .into_iter()
        .find(|e| world.get::<T>(*e).is_some_and(&predicate))
}
fn text(world: &mut World, prefix: &str) -> Option<Entity> {
    find::<Text>(world, |text| text.0.starts_with(prefix))
}
fn reveal(world: &mut World, entity: Entity, top: bool) {
    let (Some(target), Some(ancestor)) = (bounds(world, entity), scroll_ancestor(world, entity))
    else {
        return;
    };
    let Some(rect) = bounds(world, ancestor) else {
        return;
    };
    let inverse = world
        .get::<ComputedNode>(ancestor)
        .unwrap()
        .inverse_scale_factor();
    let maximum = scroll_limit(world, ancestor);
    let delta = if top {
        target.min.y - rect.min.y - 12.
    } else {
        target.center().y - rect.center().y
    };
    if let Some(mut scroll) = world.get_mut::<ScrollPosition>(ancestor) {
        let next = (scroll.y + delta * inverse).clamp(0., maximum);
        if (next - scroll.y).abs() > 0.5 {
            scroll.y = next;
        }
    }
}
fn press(world: &mut World, entity: Option<Entity>) {
    let Some(entity) = entity else {
        return;
    };
    if !world.resource::<ClickGuard>().0 || world.get::<InteractionDisabled>(entity).is_some() {
        return;
    }
    if !visible(world, entity) {
        reveal(world, entity, false);
        return;
    }
    *world.get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.resource_mut::<Rehearsal>().pressed = Some(entity);
}
fn input(world: &mut World) {
    if let Some(pressed) = world.resource_mut::<Rehearsal>().pressed.take() {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        if let Some(mut interaction) = world.get_mut::<Interaction>(pressed) {
            *interaction = Interaction::None;
        }
        return;
    }
    let Some((index, _)) = shot(world) else {
        return;
    };
    if !world.resource::<Rehearsal>().staged {
        return;
    }
    match index {
        0 | 4 => {
            if world
                .resource::<companies::TradeRouteEditorState>()
                .draft
                .is_some()
            {
                let e = find::<companies::TradeRouteEditorButton>(world, |b| {
                    b.0 == companies::TradeRouteEditorAction::Cancel
                });
                press(world, e);
                return;
            }
            if let Some(e) = text(world, "Fleet") {
                reveal(world, e, true);
            }
        }
        1 => {
            if let Some(e) = text(world, "Shipyard") {
                reveal(world, e, true);
            }
        }
        2 | 3 => {
            if world
                .resource::<companies::TradeRouteEditorState>()
                .draft
                .is_none()
            {
                let e = find::<companies::FleetButton>(world, |b| {
                    b.action == companies::FleetAction::NewRoute(ShipId(91))
                });
                press(world, e);
                return;
            }
            if index == 3 {
                let draft = world
                    .resource::<companies::TradeRouteEditorState>()
                    .draft
                    .as_ref()
                    .unwrap();
                let action = if draft.cargo_target == 24 {
                    Some(companies::TradeRouteEditorAction::CargoUp(25))
                } else if draft.stops.len() == 2 {
                    Some(companies::TradeRouteEditorAction::AddStop)
                } else {
                    None
                };
                if let Some(action) = action {
                    let e = find::<companies::TradeRouteEditorButton>(world, |b| b.0 == action);
                    press(world, e);
                    return;
                }
            }
            let label = if index == 2 {
                "NEW SHIP ROUTE"
            } else {
                "STOP 1"
            };
            if let Some(e) = text(world, label) {
                reveal(world, e, true);
            }
        }
        5 => {
            if let Some(e) = text(world, "SHIP ROUTE #901") {
                reveal(world, e, true);
            }
        }
        _ => panic!("unknown fleet UI phase"),
    }
}
fn check(world: &mut World, index: usize) -> Result<(), String> {
    if !world.resource::<Rehearsal>().staged {
        return Err("fleet fixture not staged".into());
    }
    if !world
        .resource::<LedgerArtwork>()
        .ready(world.resource::<AssetServer>())
    {
        return Err("ledger artwork loading".into());
    }
    let panels = entities::<EncyclopediaPanel>(world);
    if panels.len() != 1 || !visible(world, panels[0]) {
        return Err("one visible book required".into());
    }
    let rect = bounds(world, panels[0]).unwrap();
    let size = world.resource::<CaptureConfig>().resolution;
    if rect.min.min_element() < -1.
        || rect.max.x > size[0] as f32 + 1.
        || rect.max.y > size[1] as f32 + 1.
    {
        return Err("book exceeds window".into());
    }
    for e in entities::<PortraitStatus>(world) {
        if visible(world, e) && !world.get::<PortraitStatus>(e).unwrap().ready {
            return Err("visible portrait is still queued".into());
        }
    }
    if world.resource::<PortraitMetrics>().bytes > 32 * 1024 * 1024 {
        return Err("portrait budget exceeded".into());
    }
    let label = match index {
        0 | 4 => "Fleet",
        1 => "Shipyard",
        2 => "NEW SHIP ROUTE",
        3 => "STOP 1",
        5 => "SHIP ROUTE #901",
        _ => unreachable!(),
    };
    let Some(label) = text(world, label) else {
        return Err("requested page text missing".into());
    };
    if !visible(world, label) {
        return Err("requested section is clipped".into());
    }
    if index == 2 || index == 3 {
        let draft = world
            .resource::<companies::TradeRouteEditorState>()
            .draft
            .as_ref()
            .ok_or("ship draft missing")?;
        if draft.company != CompanyId(501)
            || draft.ship != Some((ShipId(91), ShipKind::Coaster))
            || draft.pending
        {
            return Err("ship draft identity or pending state wrong".into());
        }
        if index == 3 && (draft.cargo_target != 49 || draft.stops.len() != 3) {
            return Err("native cargo/add-stop actions not consumed".into());
        }
        if draft.stops.iter().any(|s| {
            !matches!(
                s.action,
                TradeRouteStopAction::Buy | TradeRouteStopAction::Sell
            )
        }) {
            return Err("ship editor exposed private warehouse action".into());
        }
    } else if world
        .resource::<companies::TradeRouteEditorState>()
        .draft
        .is_some()
    {
        return Err("return did not close draft".into());
    }
    Ok(())
}
fn inspect(world: &mut World) {
    let Some((index, name)) = shot(world) else {
        return;
    };
    let result = check(world, index);
    let error = result.as_ref().err().cloned().unwrap_or_default();
    {
        let mut state = world.resource_mut::<Rehearsal>();
        state.ready_shot = result.is_ok().then_some(index);
        state.error = error;
    }
    if !matches!(
        *world.resource::<CaptureState>(),
        CaptureState::AwaitingCapture { .. }
    ) || world.resource::<Rehearsal>().inspected == Some(index)
    {
        return;
    }
    result.expect("fleet UI acceptance");
    let visible_text: Vec<_> = entities::<Text>(world)
        .into_iter()
        .filter(|e| visible(world, *e))
        .filter_map(|e| world.get::<Text>(e).map(|t| t.0.clone()))
        .collect();
    let draft = world
        .resource::<companies::TradeRouteEditorState>()
        .draft
        .as_ref();
    let evidence = serde_json::json!({"passed":true,"shot":name,"fixture":"offline fleet facts; real input handlers and scroll; no transactions or sailing claimed","visible_text":visible_text,"draft":draft.map(|d|serde_json::json!({"ship":d.ship.map(|(id,kind)|(id.0,kind.label())),"cargo":d.cargo_target,"stops":d.stops.len(),"pending":d.pending}))});
    let path = world
        .resource::<CaptureConfig>()
        .out_dir
        .join(format!("{name}.fleet-ui.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    world.resource_mut::<Rehearsal>().inspected = Some(index);
}
pub(super) fn ready(
    rehearsal: Option<Res<Rehearsal>>,
    state: Res<CaptureState>,
    config: Res<CaptureConfig>,
    mut waiting: Local<u32>,
) -> bool {
    let Some(rehearsal) = rehearsal else {
        return true;
    };
    let index = match *state {
        CaptureState::Warmup { .. } => 0,
        CaptureState::Settling { shot, .. } => shot,
        _ => return true,
    };
    if rehearsal.ready_shot == Some(index) {
        *waiting = 0;
        return true;
    }
    *waiting += 1;
    assert!(
        *waiting
            < config.shots[index]
                .readiness
                .maximum_frames
                .max(config.warmup_frames),
        "fleet readiness timed out: {}",
        rehearsal.error
    );
    false
}
