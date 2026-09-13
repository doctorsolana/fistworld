//! Opt-in connected player-session inspection and production-input driver.
//!
//! No fixture, teleport, inventory grant or direct gameplay message lives here.
//! The external runner writes numbered JSON commands and waits for replies or
//! replicated state. Screenshots use the real renderer's completion observer.

mod audio;

use super::{live_capture_request, CaptureInspection};
use crate::{camera_rts::CommanderCamera, capture_artifact::*, states::GameState};
use bevy::{
    ecs::system::SystemState,
    input::{
        keyboard::{Key, KeyboardInput, NativeKeyCode},
        ButtonState, InputSystems,
    },
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
    Key {
        key: String,
    },
    Text {
        text: String,
        #[serde(default)]
        replace: bool,
    },
    Button {
        name: String,
    },
    /// Drag a retained UI slider through its normal pointer handler.
    Slider {
        name: String,
        value: f32,
    },
    RightClick {
        x: f32,
        z: f32,
    },
    View {
        x: f32,
        z: f32,
        zoom: f32,
    },
    Capture {
        name: String,
    },
    /// Continuous connected gameplay, after the same initial readiness as a still.
    Record {
        name: String,
        frames: u32,
        interval_ms: u64,
    },
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
    session_started: Instant,
    poll: Instant,
    status: Instant,
    last_chunks: usize,
    stable: u32,
    startup_phase: Option<(GameState, crate::ui::name_entry::NameEntryPhase)>,
    recorded: u32,
    next_capture: Instant,
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
        session_started: Instant::now(),
        poll: Instant::now(),
        status: Instant::now(),
        last_chunks: 0,
        stable: 0,
        startup_phase: None,
        recorded: 0,
        next_capture: Instant::now(),
    });
    app.insert_resource(bevy::winit::WinitSettings::continuous());
    app.add_systems(
        PreUpdate,
        drive
            .after(InputSystems)
            .after(UiSystems::Focus)
            .before(crate::ui::sound::collect_button_press),
    );
}

fn write_json(path: &Path, value: &Value) {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(value).unwrap())
        .expect("session evidence write");
    std::fs::rename(temporary, path).expect("session evidence publish");
}

fn snapshot(world: &mut World) -> Value {
    let audio = audio::snapshot(world);
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
    // Read-only replicated garden routes for the opt-in connected movement lab.
    let yards: Vec<_> = world
        .query::<(
            &BuildingId,
            &HouseholdYard,
            &PlayerPosition,
            &PlayerRotation,
        )>()
        .iter(world)
        .filter_map(|(id, yard, at, yaw)| {
            let (Some(entry), Some(approach)) = (yard.entry, yard.approach) else {
                return None;
            };
            let world_point = |p| {
                let xz = at.0.xz() + shared::rotation::local_to_world_xz(p, yaw.0);
                [xz.x, at.0.y, xz.y]
            };
            Some(json!({"id":id.0,"house":at.0.to_array(),"recipe":yard,
                "center":world_point(yard.center()),"entry":world_point(entry),
                "approach":world_point(approach)}))
        })
        .collect();
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
    let game_state = world
        .get_resource::<State<GameState>>()
        .map(|state| format!("{:?}", state.get()));
    let name_phase = world
        .get_resource::<crate::ui::name_entry::NameEntryPhase>()
        .map(|phase| format!("{phase:?}"));
    let submitted = world
        .get_resource::<crate::ui::name_entry::PlayerNameInput>()
        .is_some_and(|input| input.submitted);
    let name_error = world
        .get_resource::<crate::ui::name_entry::NameSubmissionFeedback>()
        .and_then(|feedback| feedback.error_message.clone());
    let connection_error = world
        .get_resource::<crate::render::systems::ConnectionFeedback>()
        .and_then(|feedback| feedback.error_message.clone());
    let startup_art_ready = world
        .get_resource::<crate::ui::startup::StartupArtwork>()
        .is_some_and(|art| art.ready(world.resource::<AssetServer>()));
    let focused_control = world
        .get_resource::<bevy::input_focus::InputFocus>()
        .and_then(|focus| focus.get())
        .and_then(|entity| world.get::<Name>(entity))
        .map(|name| name.as_str().to_owned());
    let window_title = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|window| window.title.clone());
    let server_address = world
        .get_resource::<crate::ui::main_menu::ServerAddress>()
        .map(|address| json!({"host":address.ip,"port":address.port}));
    let presets_expanded = world
        .get_resource::<crate::ui::main_menu::DropdownState>()
        .is_some_and(|state| state.expanded);
    json!({"audio":audio,"game_state":game_state,"name_phase":name_phase,"submitted":submitted,"name_error":name_error,"connection_error":connection_error,"startup_art_ready":startup_art_ready,
        "focused_control":focused_control,"window_title":window_title,"server_address":server_address,"presets_expanded":presets_expanded,
        "account":account,"hero":own.map(|(_,_,hero)|hero),"markets":markets,"towns":towns,"yards":yards,"clock":clock,"camera":camera,"buttons":buttons,"selection":selected,"notice":notice,
        "ui_blocking":world.get_resource::<crate::input::InputState>().is_some_and(|s|s.ui_blocking()),
        "playing":world.get_resource::<State<GameState>>().is_some_and(|s|*s.get()==GameState::Playing),
        "creator":world.get_resource::<crate::ui::hero_creator::HeroCreatorOpen>().is_some_and(|s|s.0),
        "cinematic":world.get_resource::<crate::boat::OpeningCinematic>().is_some_and(|s|s.is_active()),
        "god":world.get_resource::<crate::ui::hud::GodCapability>().is_some_and(|s|s.0)})
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<SessionCapture>| {
        if let (Some(game), Some(phase)) = (world.get_resource::<State<GameState>>(), world.get_resource::<crate::ui::name_entry::NameEntryPhase>()) {
            let current = (*game.get(), *phase);
            if state.startup_phase != Some(current) {
                state.startup_phase = Some(current);
                use std::io::Write;
                let record = json!({"elapsed_seconds":state.session_started.elapsed().as_secs_f64(),"state":snapshot(world)});
                writeln!(std::fs::OpenOptions::new().create(true).append(true).open(state.out.join("startup.transitions.jsonl")).expect("startup phase journal"), "{record}").expect("startup phase evidence");
            }
        }
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
                state.phase = 0; state.stable = 0; state.recorded = 0;
                state.next_capture = Instant::now();
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
        "Enter" => Ok(KeyCode::Enter),
        "Backspace" => Ok(KeyCode::Backspace),
        "Tab" => Ok(KeyCode::Tab),
        _ => Err(format!("unsupported session key {key}")),
    }
}

fn logical_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Home => Key::Home,
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::ControlLeft => Key::Control,
        KeyCode::KeyA => Key::Character("a".into()),
        KeyCode::KeyE => Key::Character("e".into()),
        KeyCode::KeyN => Key::Character("n".into()),
        KeyCode::KeyM => Key::Character("m".into()),
        _ => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
    }
}

fn keyboard_event(
    world: &mut World,
    code: KeyCode,
    key: Key,
    text: Option<String>,
    pressed: bool,
) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .map_err(|_| "no primary window for keyboard input")?;
    world.write_message(KeyboardInput {
        key_code: code,
        logical_key: key,
        text: text.map(Into::into),
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        repeat: false,
        window,
    });
    Ok(())
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
                    keyboard_event(world, code, logical_key(code), None, true)?;
                }
                1 => {
                    world.resource_mut::<ButtonInput<KeyCode>>().release(code);
                    keyboard_event(world, code, logical_key(code), None, false)?;
                }
                _ => return Ok(true),
            }
        }
        Command::Text { text, replace } => {
            if text.len() > 1024 {
                return Err("text command is too large".into());
            }
            if *replace && state.phase < 2 {
                let pressed = state.phase == 0;
                for code in [KeyCode::ControlLeft, KeyCode::KeyA] {
                    if pressed {
                        world.resource_mut::<ButtonInput<KeyCode>>().press(code);
                    } else {
                        world.resource_mut::<ButtonInput<KeyCode>>().release(code);
                    }
                    keyboard_event(world, code, logical_key(code), None, pressed)?;
                }
            } else if state.phase == if *replace { 2 } else { 0 } {
                // Text may contain composed characters. Send the same logical
                // text events used by the production editors; never mutate drafts.
                let code = KeyCode::Unidentified(NativeKeyCode::Unidentified);
                for ch in text.chars() {
                    let value = ch.to_string();
                    keyboard_event(
                        world,
                        code,
                        Key::Character(value.clone().into()),
                        Some(value.clone()),
                        true,
                    )?;
                    keyboard_event(world, code, Key::Character(value.into()), None, false)?;
                }
            } else {
                return Ok(true);
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
        Command::Slider { name, value } => {
            if !value.is_finite() || !(0.0..=1.0).contains(value) {
                return Err("slider value must be between zero and one".into());
            }
            if state.phase == 0 {
                let candidate = world.query_filtered::<(Entity, &Name, &ComputedNode, Has<InteractionDisabled>), With<Button>>()
                    .iter(world).find(|(_, label, node, disabled)| label.as_str() == name && !disabled && node.size().min_element() > 0.0)
                    .map(|(entity, ..)| entity);
                let Some(entity) = candidate else {
                    return Ok(false);
                };
                state.button = Some(entity);
                world
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Left);
            } else if state.phase == 2 {
                world
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .release(MouseButton::Left);
            } else if state.phase > 2 {
                return Ok(true);
            }
            if let Some(entity) = state.button {
                if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
                    *interaction = if state.phase < 2 {
                        Interaction::Pressed
                    } else {
                        Interaction::Hovered
                    };
                }
                if let Some(mut cursor) = world.get_mut::<RelativeCursorPosition>(entity) {
                    cursor.cursor_over = true;
                    cursor.normalized = Some(Vec2::new(value - 0.5, 0.0));
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
        Command::Capture { name } | Command::Record { name, .. } => {
            let (frames, interval) = match command {
                Command::Record {
                    frames,
                    interval_ms,
                    ..
                } if (2..=180).contains(frames)
                    && (50..=1000).contains(interval_ms)
                    && u64::from(*frames) * interval_ms <= 60_000 =>
                {
                    (*frames, Duration::from_millis(*interval_ms))
                }
                Command::Record { .. } => {
                    return Err(
                        "record requires 2–180 frames, 50–1000 ms spacing, at most 60 seconds"
                            .into(),
                    )
                }
                _ => (1, Duration::ZERO),
            };
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
                        state.recorded += 1;
                        Ok(state.recorded >= frames)
                    }
                };
            }
            if Instant::now() < state.next_capture {
                return Ok(false);
            }
            let frontend = world
                .get_resource::<State<GameState>>()
                .is_some_and(|game| *game.get() != GameState::Playing);
            if frontend {
                let art_ready = world
                    .resource::<crate::ui::startup::StartupArtwork>()
                    .ready(world.resource::<AssetServer>());
                let layout_ready =
                    world
                        .query::<(&Name, &ComputedNode)>()
                        .iter(world)
                        .any(|(name, node)| {
                            name.as_str() == "startup-root" && node.size().min_element() > 0.0
                        });
                let motion_ready = world
                    .query::<(&crate::ui::motion::UiReveal, &ComputedNode)>()
                    .iter(world)
                    .filter(|(_, node)| node.size().min_element() > 0.0)
                    .all(|(motion, _)| motion.is_settled());
                if !art_ready || !layout_ready || !motion_ready {
                    state.stable = 0;
                    return Ok(false);
                }
                state.stable += 1;
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
            if !frontend
                && (chunks < 64
                    || chunks != state.last_chunks
                    || !stable_camera
                    || !inspection.yard_presentation_ready())
            {
                state.stable = 0;
            } else if !frontend {
                state.stable += 1;
            }
            state.last_chunks = chunks;
            if state.recorded == 0 && state.stable < 12 {
                return Ok(false);
            }
            let Some(target) = inspection.presentation_target.as_ref() else {
                return Err("composed capture target unavailable".into());
            };
            let target = target.image.clone();
            let name = if frames > 1 {
                format!("{name}-{:04}", state.recorded)
            } else {
                name.clone()
            };
            let mut request = live_capture_request(
                state.out.join(format!("{name}.png")),
                if frontend {
                    "connected-startup"
                } else {
                    "first-session"
                },
                &name,
                CaptureTarget::Window,
            );
            if frontend {
                // A pre-join menu has not accepted the server map yet. Record
                // actual counters without claiming a validated in-world view.
                request.metadata.map = "startup-unjoined".into();
                request.metadata.world = inspection.world_snapshot(clocks.iter().next());
                request.metadata.readiness_frames = state.stable;
            } else {
                inspection.complete_live_request(
                    &mut request,
                    cameras.iter().next(),
                    clocks.iter().next(),
                    state.stable,
                )?;
            }
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
            state.next_capture = Instant::now() + interval;
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
