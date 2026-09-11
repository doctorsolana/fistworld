//! Opt-in connected player-session inspection and production-input driver.
//!
//! No fixture, teleport, inventory grant or direct gameplay message lives here.
//! The external runner writes numbered JSON commands and waits for replies or
//! replicated state. Screenshots use the real renderer's completion observer.

use super::{live_capture_request, CaptureInspection};
use crate::{camera_rts::CommanderCamera, capture_artifact::*, states::GameState};
use bevy::{
    ecs::system::SystemState,
    input::InputSystems,
    prelude::*,
    render::view::screenshot::Screenshot,
    ui::{InteractionDisabled, RelativeCursorPosition, UiSystems},
};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{
    components::*,
    economy::{Good, GoodsInventory, MarketSeller, MootMarket, Wallet},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    command: Command,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Key { key: String },
    Button { name: String },
    RightClick { x: f32, z: f32 },
    View { x: f32, z: f32, zoom: f32 },
    Capture { name: String },
    Quit,
}

#[derive(Resource)]
struct SessionCapture {
    out: PathBuf,
    last_id: u64,
    current: Option<Request>,
    phase: u8,
    button: Option<Entity>,
    ticket: Option<u64>,
    started: Instant,
    poll: Instant,
    status: Instant,
    last_chunks: usize,
    stable: u32,
}

pub(crate) fn install(app: &mut App) {
    let Some(out) = std::env::var_os("FISTWORLD_SESSION_CAPTURE_DIR") else {
        return;
    };
    assert_eq!(
        std::env::var("FISTFORCE_NO_SETTINGS_FILE").as_deref(),
        Ok("1"),
        "session captures must isolate player preferences"
    );
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("session capture output directory");
    app.insert_resource(SessionCapture {
        out,
        last_id: 0,
        current: None,
        phase: 0,
        button: None,
        ticket: None,
        started: Instant::now(),
        poll: Instant::now(),
        status: Instant::now(),
        last_chunks: 0,
        stable: 0,
    });
    app.insert_resource(bevy::winit::WinitSettings::continuous());
    app.add_systems(PreUpdate, drive.after(InputSystems).after(UiSystems::Focus));
}

fn write_json(path: &Path, value: &Value) {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(value).unwrap())
        .expect("session evidence write");
    std::fs::rename(temporary, path).expect("session evidence publish");
}

fn snapshot(world: &mut World) -> Value {
    let local = world
        .get_resource::<crate::camera_rts::LocalPeerId>()
        .map(|id| id.0);
    let account = world
        .get_resource::<crate::ui::name_entry::PlayerNameInput>()
        .map(|input| input.name.clone());
    let own = world.query::<(Entity, &Hero, &PersonId, &PlayerPosition, &Wallet, &GoodsInventory, Has<AboardBoat>)>()
        .iter(world).find(|(_, hero, ..)| Some(shared::player::peer_id_to_u64(hero.owner)) == local)
        .map(|(entity, _, id, position, wallet, cargo, aboard)| (entity, *id, json!({
            "entity": format!("{entity:?}"), "person_id": id.0, "position": position.0.to_array(),
            "wallet": wallet.balance(), "bulk": cargo.used_bulk(), "capacity": cargo.bulk_capacity(),
            "cargo": Good::ALL.into_iter().map(|good| (format!("{good:?}"), json!(cargo.amount(good)))).collect::<serde_json::Map<_,_>>(),
            "aboard": aboard,
        })));
    let person = own.as_ref().map(|(_, person, _)| *person);
    let markets: Vec<_> = world.query::<(&SettlementId, &Settlement, &PlayerPosition, &MootMarket, &GoodsInventory, Option<&PlayerRotation>)>()
        .iter(world).map(|(id, town, position, market, store, rotation)| json!({
            "id": id.0, "name": town.name, "position": position.0.to_array(),
            "entrance": SettlementBuildingKind::Hall.entrance_position(position.0, rotation.map_or(0.0, |rotation| rotation.0)).to_array(),
            "fee_bps": market.market_fee_bps(),
            "my_offers": market.listings().iter().filter(|listing| person.is_some_and(|person| listing.seller == MarketSeller::Person(person))).collect::<Vec<_>>(),
            "goods": Good::ALL.into_iter().map(|good| (format!("{good:?}"), json!({"stock": store.amount(good), "listed": market.listed_units(good), "ask": market.pool(good).ask}))).collect::<serde_json::Map<_,_>>(),
        })).collect();
    let towns: Vec<_> = world.query::<(&SettlementSummary, &PlayerPosition)>().iter(world)
        .map(|(town, position)| json!({"id":town.id.0,"name":town.name,"residents":town.residents,"position":position.0.to_array()})).collect();
    let clock = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map(|clock| json!({"day":clock.day,"time":clock.normalized_time()}));
    let camera = world.query::<&CommanderCamera>().iter(world).next().map(|camera| json!({
        "focus":camera.focus.to_array(),"target":camera.focus_target.to_array(),"zoom":camera.zoom,"zoom_target":camera.zoom_target}));
    let buttons: Vec<_> = world
        .query_filtered::<(&Name, &ComputedNode, Has<InteractionDisabled>), With<Button>>()
        .iter(world)
        .filter(|(_, node, _)| node.size().min_element() > 0.0)
        .map(|(name, _, disabled)| json!({"name": name.as_str(),"enabled": !disabled}))
        .collect();
    let selected = world
        .get_resource::<crate::selection::Selection>()
        .map(|selection| {
            selection
                .entities
                .iter()
                .map(|entity| format!("{entity:?}"))
                .collect::<Vec<_>>()
        });
    let notice = world
        .get_resource::<crate::ui::hud::GodNotice>()
        .map(|notice| json!({"text":notice.text,"remaining":notice.seconds_left}));
    json!({"account":account,"hero":own.map(|(_,_,hero)|hero),"markets":markets,"towns":towns,"clock":clock,"camera":camera,"buttons":buttons,"selection":selected,"notice":notice,
        "ui_blocking":world.get_resource::<crate::input::InputState>().is_some_and(|s|s.ui_blocking()),
        "playing":world.get_resource::<State<GameState>>().is_some_and(|s|*s.get()==GameState::Playing),
        "creator":world.get_resource::<crate::ui::hero_creator::HeroCreatorOpen>().is_some_and(|s|s.0),
        "cinematic":world.get_resource::<crate::boat::OpeningCinematic>().is_some_and(|s|s.is_active()),
        "god":world.get_resource::<crate::ui::hud::GodCapability>().is_some_and(|s|s.0)})
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<SessionCapture>| {
        if state.status.elapsed() >= Duration::from_secs(1) {
            write_json(&state.out.join("status.json"), &snapshot(world));
            state.status = Instant::now();
        }
        if state.current.is_none() {
            if state.poll.elapsed() < Duration::from_millis(100) { return; }
            state.poll = Instant::now();
            let path = state.out.join("command.json");
            if let Ok(bytes) = std::fs::read(&path) {
                let request: Request = serde_json::from_slice(&bytes).expect("valid session capture command");
                if request.id <= state.last_id { return; }
                state.last_id = request.id;
                state.current = Some(request);
                state.started = Instant::now();
                state.phase = 0; state.stable = 0;
            }
        }
        let Some(request) = state.current.clone() else { return; };
        let result = if state.started.elapsed() > Duration::from_secs(90) {
            Err("command timed out waiting for readiness".into())
        } else { advance(world, &mut state, &request.command) };
        match result {
            Ok(false) => {}
            result => {
                release_input(world, &mut state);
                let error = result.err();
                write_json(&state.out.join(format!("reply-{}.json", request.id)), &json!({"id":request.id,"ok":error.is_none(),"error":error,"state":snapshot(world)}));
                state.current = None;
            }
        }
    });
}

fn key_code(key: &str) -> Result<KeyCode, String> {
    match key {
        "Home" => Ok(KeyCode::Home),
        "E" => Ok(KeyCode::KeyE),
        "N" => Ok(KeyCode::KeyN),
        "M" => Ok(KeyCode::KeyM),
        "Escape" => Ok(KeyCode::Escape),
        _ => Err(format!("unsupported session key {key}")),
    }
}

fn release_input(world: &mut World, state: &mut SessionCapture) {
    if let Some(entity) = state.button.take() {
        if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
            *interaction = Interaction::None;
        }
        if let Some(mut cursor) = world.get_mut::<RelativeCursorPosition>(entity) {
            cursor.cursor_over = false;
        }
    }
    world.remove_resource::<crate::camera_rts::CursorTerrainOverride>();
}

fn advance(
    world: &mut World,
    state: &mut SessionCapture,
    command: &Command,
) -> Result<bool, String> {
    match command {
        Command::Key { key } => {
            let code = key_code(key)?;
            match state.phase {
                0 => {
                    world.resource_mut::<ButtonInput<KeyCode>>().press(code);
                }
                1 => {
                    world.resource_mut::<ButtonInput<KeyCode>>().release(code);
                }
                _ => return Ok(true),
            }
        }
        Command::Button { name } => {
            if state.phase == 0 {
                let candidate = world.query_filtered::<(Entity, &Name, &ComputedNode, Has<InteractionDisabled>), With<Button>>()
                    .iter(world).find(|(_, label, node, _)| label.as_str() == name && node.size().min_element() > 0.0)
                    .map(|(entity, _, _, disabled)| (entity, disabled));
                let Some((entity, disabled)) = candidate else {
                    return Ok(false);
                };
                if disabled {
                    return Err(format!("button is disabled: {name}"));
                }
                state.button = Some(entity);
                world
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Left);
            } else if state.phase == 1 {
                world
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .release(MouseButton::Left);
            } else {
                return Ok(true);
            }
            if let Some(entity) = state.button {
                if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
                    *interaction = if state.phase == 0 {
                        Interaction::Pressed
                    } else {
                        Interaction::Hovered
                    };
                }
                if let Some(mut cursor) = world.get_mut::<RelativeCursorPosition>(entity) {
                    cursor.cursor_over = true;
                }
            }
        }
        Command::RightClick { x, z } => {
            if !x.is_finite() || !z.is_finite() {
                return Err("invalid world point".into());
            }
            match state.phase {
                0 => {
                    world.insert_resource(crate::camera_rts::CursorTerrainOverride(Vec2::new(
                        *x, *z,
                    )));
                }
                1 => {
                    world
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .press(MouseButton::Right);
                }
                2 => {
                    world
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .release(MouseButton::Right);
                }
                _ => return Ok(true),
            }
        }
        Command::View { x, z, zoom } => {
            if !x.is_finite() || !z.is_finite() || !zoom.is_finite() {
                return Err("invalid view".into());
            }
            let height = world
                .resource::<shared::terrain::WorldTerrain>()
                .get_height(*x, *z);
            for mut camera in world.query::<&mut CommanderCamera>().iter_mut(world) {
                camera.focus_target = Vec3::new(*x, height, *z);
                camera.zoom_target = zoom.clamp(camera.zoom_min, camera.zoom_max);
            }
            return Ok(true);
        }
        Command::Capture { name } => {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                return Err("invalid capture name".into());
            }
            if let Some(ticket) = state.ticket {
                let Some(done) = world.resource_mut::<CaptureCompletions>().take(ticket) else {
                    return Ok(false);
                };
                state.ticket = None;
                return if let Some(error) = done.error {
                    Err(error)
                } else {
                    if done.comparison_failed {
                        Err("capture comparison failed".into())
                    } else {
                        Ok(true)
                    }
                };
            }
            let mut inspection = SystemState::<(
                CaptureInspection,
                Query<&CommanderCamera>,
                Query<&WorldTime>,
            )>::new(world);
            let (inspection, cameras, clocks) = inspection
                .get(world)
                .map_err(|error| format!("capture inspection is not ready: {error}"))?;
            let chunks = inspection.loaded_chunk_count();
            let stable_camera = cameras.iter().next().is_some_and(|camera| {
                // Commander focus.y is not interpolated; the camera derives
                // its height from terrain. Only the XZ view target can settle.
                camera.focus.xz().distance(camera.focus_target.xz()) < 0.3
                    && (camera.zoom - camera.zoom_target).abs() < 0.3
            });
            if chunks < 64 || chunks != state.last_chunks || !stable_camera {
                state.stable = 0;
            } else {
                state.stable += 1;
            }
            state.last_chunks = chunks;
            if state.stable < 12 {
                return Ok(false);
            }
            let Some(target) = inspection.presentation_target.as_ref() else {
                return Err("composed capture target unavailable".into());
            };
            let target = target.image.clone();
            let mut request = live_capture_request(
                state.out.join(format!("{name}.png")),
                "first-session",
                name,
                CaptureTarget::Window,
            );
            inspection.complete_live_request(
                &mut request,
                cameras.iter().next(),
                clocks.iter().next(),
                state.stable,
            )?;
            state.ticket = Some(world.resource_scope(
                |world, mut completions: Mut<CaptureCompletions>| {
                    request_capture(
                        &mut world.commands(),
                        Screenshot::image(target),
                        request,
                        &mut completions,
                    )
                },
            ));
            write_json(
                &state.out.join(format!("{name}.session.json")),
                &snapshot(world),
            );
            return Ok(false);
        }
        Command::Quit => {
            world.write_message(AppExit::Success);
            return Ok(true);
        }
    }
    state.phase += 1;
    Ok(false)
}
