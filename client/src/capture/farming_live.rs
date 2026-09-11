//! Opt-in evidence from real replicated farm labour. This observer changes only
//! its camera; it never creates workers, grants stock, changes time or issues orders.

use super::{
    advance_readiness, inspection::CaptureInspection, live_capture_request, ReadinessOutcome,
    ReadinessProgress,
};
use crate::camera_rts::CommanderCamera;
use crate::capture_artifact::{
    request_capture, CaptureCompletions, CaptureReadiness, CaptureTarget,
};
use bevy::{prelude::*, render::view::screenshot::Screenshot};
use serde_json::{json, Value};
use shared::{
    components::*,
    economy::{CarriedLoad, Good, GoodsInventory},
    terrain::WorldTerrain,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Resource)]
struct FarmingCapture {
    out: PathBuf,
    started: Instant,
    timeout: Duration,
    exit: bool,
    done: bool,
    track: Option<TrackedFarmer>,
    shots: usize,
    ticket: Option<u64>,
    readiness: ReadinessProgress,
    last_sample: Instant,
    events: Vec<Value>,
    samples: Vec<Value>,
    latest: Value,
}

struct TrackedFarmer {
    worker: Entity,
    person: PersonId,
    farm: Entity,
    building: BuildingId,
    field: Entity,
    was_carrying_wheat: bool,
    last_exact_cargo: Option<u32>,
    last_store: u32,
    carried: bool,
    carry_origin: Vec3,
    walked_with_cargo: bool,
    stock_rises: VecDeque<(Instant, u32)>,
    pending_drop: Option<(Instant, Option<u32>)>,
    deposited: bool,
}

pub(crate) fn install(app: &mut App) {
    let Some(out) = std::env::var_os("FISTWORLD_FARM_CAPTURE_DIR") else {
        return;
    };
    assert_eq!(
        std::env::var("FISTFORCE_NO_SETTINGS_FILE").as_deref(),
        Ok("1"),
        "connected farming capture must isolate player settings"
    );
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("create connected farm evidence directory");
    assert!(
        !out.join("farming.json").exists(),
        "use a fresh connected farm output directory"
    );
    let timeout = std::env::var("FISTWORLD_FARM_CAPTURE_TIMEOUT_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1200)
        .clamp(30, 7200);
    let state = FarmingCapture {
        out,
        started: Instant::now(),
        timeout: Duration::from_secs(timeout),
        exit: std::env::var("FISTWORLD_FARM_CAPTURE_EXIT").as_deref() == Ok("1"),
        done: false,
        track: None,
        shots: 0,
        ticket: None,
        readiness: default(),
        last_sample: Instant::now(),
        events: vec![],
        samples: vec![],
        latest: json!({"waiting":"client startup"}),
    };
    state
        .publish("waiting", None)
        .expect("publish farm observer startup evidence");
    app.insert_resource(state);
    app.insert_resource(bevy::winit::WinitSettings::continuous());
    app.add_systems(Update, drive.after(crate::boat::drive_opening_cinematic));
}

impl FarmingCapture {
    fn event(&mut self, kind: &str, detail: Value) {
        self.events.push(json!({"real_seconds": self.started.elapsed().as_secs_f64(), "event": kind, "detail": detail}));
    }

    fn publish(&self, status: &str, reason: Option<&str>) -> std::io::Result<()> {
        let report = json!({
            "schema_version": 2, "status": status, "reason": reason,
            "observation": "Real replicated worker, field, public CarriedLoad and workplace stock; no gameplay mutation. Personal GoodsInventory is private to its owner. Deposit requires a public Wheat load to disappear at the owning farm after carrying movement, with a public farm stock increase within two real seconds. Where exact cargo is legitimately visible, the increase must cover that quantity.",
            "elapsed_real_seconds": self.started.elapsed().as_secs_f64(),
            "completed_shots": self.shots, "latest": self.latest,
            "events": self.events, "samples": self.samples,
        });
        let temporary = self.out.join("farming.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(&report).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(temporary, self.out.join("farming.json"))
    }

    fn finish(&mut self, reason: Option<String>, exit: &mut MessageWriter<AppExit>) {
        self.done = true;
        let status = if reason.is_some() { "failed" } else { "passed" };
        let write = self.publish(status, reason.as_deref());
        if let Some(reason) = &reason {
            error!("connected farm capture: {reason}");
        }
        if let Err(error) = &write {
            error!("connected farm capture evidence: {error}");
        }
        info!("connected farm capture {status}: {}", self.out.display());
        if self.exit {
            exit.write(if reason.is_none() && write.is_ok() {
                AppExit::Success
            } else {
                AppExit::error()
            });
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive(
    mut commands: Commands,
    mut state: ResMut<FarmingCapture>,
    game: Res<State<crate::states::GameState>>,
    opening: Res<crate::boat::OpeningCinematic>,
    terrain: Option<Res<WorldTerrain>>,
    workers: Query<(
        Entity,
        &PersonId,
        &CharacterName,
        &EmployedAt,
        &PlayerPosition,
        &CharacterActivity,
        &CarriedLoad,
        Option<&GoodsInventory>,
        Has<crate::hero::HeroDressed>,
        Option<&CharacterNavigationStatus>,
    )>,
    actors: Query<(
        &CharacterName,
        Option<&PersonId>,
        Option<&EmployedAt>,
        Option<&PlayerPosition>,
        Option<&CharacterActivity>,
        Option<&CarriedLoad>,
        Option<&GoodsInventory>,
    )>,
    fields: Query<(
        Entity,
        &FarmField,
        &AttachedTo,
        &PlayerPosition,
        &PlayerRotation,
        Option<&crate::settlement::FarmFieldVisual>,
    )>,
    farms: Query<(
        Entity,
        &BuildingId,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &GoodsInventory,
    )>,
    clocks: Query<&WorldTime>,
    mut cameras: Query<&mut CommanderCamera>,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if state.done {
        return;
    }
    if state.started.elapsed() > state.timeout {
        let reason = format!(
            "timed out after {}s; completed shots={}, latest={}",
            state.timeout.as_secs(),
            state.shots,
            state.latest
        );
        state.finish(Some(reason), &mut exit);
        return;
    }
    if *game.get() != crate::states::GameState::Playing || opening.is_active() || terrain.is_none()
    {
        if state.last_sample.elapsed().as_secs() >= 2 {
            state.latest = json!({
                "waiting":"gameplay observer readiness",
                "game_state":format!("{:?}",game.get()),
                "opening_active":opening.is_active(),
                "terrain_available":terrain.is_some(),
            });
            state.last_sample = Instant::now();
            if let Err(error) = state.publish("waiting", None) {
                state.finish(Some(error.to_string()), &mut exit);
            }
        }
        return;
    }
    let Some(terrain) = terrain else { return };
    let now = Instant::now();
    if state.track.is_none() {
        let candidate = workers
            .iter()
            .filter(|(_, _, _, _, _, activity, load, _, _, _)| {
                **activity == CharacterActivity::Farming && load.is_empty()
            })
            .filter_map(|(worker, person, name, employment, position, ..)| {
                let (field_entity, field, _, at, rotation, visual) =
                    fields.iter().find(|(_, field, owner, at, rotation, _)| {
                        owner.0 == employment.0
                            && field.shape.as_ref().is_some_and(|s| s.is_valid())
                            && field.contains_world_point(position.0.xz(), at.0, rotation.0, -0.1)
                    })?;
                if !visual.is_some_and(|v| v.matches(field, at.0, rotation.0, &terrain)) {
                    return None;
                }
                let (farm_entity, building, farm, _, _, store) =
                    farms.iter().find(|(_, id, farm, ..)| {
                        **id == employment.0 && farm.kind == SettlementBuildingKind::Farmstead
                    })?;
                Some((
                    worker,
                    *person,
                    name.0.clone(),
                    farm_entity,
                    *building,
                    field_entity,
                    field.clone(),
                    farm.quality,
                    store.amount(Good::Wheat),
                ))
            })
            .min_by_key(|(_, person, ..)| person.0);
        if let Some((worker, person, name, farm, building, field, geometry, quality, store)) =
            candidate
        {
            state.event("farming_inside_accepted_field", json!({"person_id":person.0,"name":name,"building_id":building.0,"field":geometry,"farm_quality":quality,"store_wheat":store}));
            state.track = Some(TrackedFarmer {
                worker,
                person,
                farm,
                building,
                field,
                was_carrying_wheat: false,
                last_exact_cargo: None,
                last_store: store,
                carried: false,
                carry_origin: Vec3::ZERO,
                walked_with_cargo: false,
                stock_rises: VecDeque::new(),
                pending_drop: None,
                deposited: false,
            });
        } else {
            if state.last_sample.elapsed().as_secs() >= 2 {
                let mut components = [0usize; 7];
                for (_, person, employment, position, activity, load, inventory) in &actors {
                    for (slot, present) in components.iter_mut().zip([
                        true,
                        person.is_some(),
                        employment.is_some(),
                        position.is_some(),
                        activity.is_some(),
                        load.is_some(),
                        inventory.is_some(),
                    ]) {
                        *slot += usize::from(present);
                    }
                }
                let farming_candidates: Vec<_> = workers.iter()
                    .filter(|(_, _, _, _, _, activity, ..)| **activity == CharacterActivity::Farming)
                    .take(8)
                    .map(|(_, person, name, employment, position, _, load, ..)| {
                        let owned: Vec<_> = fields.iter().filter(|(_, _, owner, ..)| owner.0 == employment.0).map(|(_, field, _, at, yaw, visual)| json!({
                            "plot":field.plot_index,
                            "shape_valid":field.shape.as_ref().is_some_and(|s| s.is_valid()),
                            "shape_area":field.shape.as_ref().map(|s| s.area()),
                            "position":at.0.to_array(),
                            "rotation":yaw.0,
                            "contains_worker":field.contains_world_point(position.0.xz(), at.0, yaw.0, -0.1),
                            "visual_ready":visual.is_some_and(|v| v.matches(field, at.0, yaw.0, &terrain)),
                        })).collect();
                        json!({"person_id":person.0,"name":name.0,"position":position.0.to_array(),"load":load,"building_id":employment.0.0,"fields":owned})
                    }).collect();
                state.latest = json!({"waiting":"real farmer working inside a shaped field", "workers": workers.iter().len(), "farming_candidates":farming_candidates, "actor_components":{"named":components[0],"person_id":components[1],"employed":components[2],"position":components[3],"activity":components[4],"public_carried_load":components[5],"visible_private_inventory":components[6]}, "fields":fields.iter().len(), "farms":farms.iter().filter(|(_,_,b,..)|b.kind==SettlementBuildingKind::Farmstead).count(),"day":clocks.iter().next().map(|c|c.day)});
                state.last_sample = now;
                if let Err(error) = state.publish("waiting", None) {
                    state.finish(Some(error.to_string()), &mut exit);
                }
            }
            return;
        }
    }
    let mut tracked = state.track.take().unwrap();
    let Some((
        (_, _, name, employed, at, activity, load, cargo, dressed, nav),
        (_, _, farm, farm_at, farm_yaw, store),
        (_, field, owner, field_at, field_yaw, visual),
    )) = workers
        .get(tracked.worker)
        .ok()
        .zip(farms.get(tracked.farm).ok())
        .zip(fields.get(tracked.field).ok())
        .map(|((a, b), c)| (a, b, c))
    else {
        state.track = Some(tracked);
        state.latest = json!({"waiting":"tracked farmer/farm/field temporarily not replicated"});
        return;
    };
    if employed.0 != tracked.building || owner.0 != tracked.building {
        state.finish(
            Some("tracked worker or field changed owner before the observed loop completed".into()),
            &mut exit,
        );
        return;
    }
    let wheat = load.good == Some(Good::Wheat);
    let exact_wheat = cargo.map(|inventory| inventory.amount(Good::Wheat));
    let stocked = store.amount(Good::Wheat);
    let entrance = farm.kind.entrance_position(farm_at.0, farm_yaw.0);
    if stocked > tracked.last_store {
        tracked
            .stock_rises
            .push_back((now, stocked - tracked.last_store));
    }
    while tracked
        .stock_rises
        .front()
        .is_some_and(|(time, _)| now.duration_since(*time).as_secs_f32() > 2.)
    {
        tracked.stock_rises.pop_front();
    }
    if !tracked.carried && wheat {
        tracked.carried = true;
        tracked.carry_origin = at.0;
        state.event("harvest_load_materialized", json!({"person_id":tracked.person.0,"public_load":load,"exact_wheat_if_visible":exact_wheat,"position":at.0.to_array(),"store_wheat":stocked}));
    }
    if wheat && at.0.xz().distance(tracked.carry_origin.xz()) > 0.75 {
        tracked.walked_with_cargo = true;
    }
    if tracked.carried
        && tracked.was_carrying_wheat
        && !wheat
        && at.0.xz().distance(entrance.xz()) < 6.0
    {
        let dropped = tracked
            .last_exact_cargo
            .zip(exact_wheat)
            .map(|(before, after)| before.saturating_sub(after));
        tracked.pending_drop = Some((now, dropped));
        state.event("cargo_released_at_owning_farm", json!({"exact_wheat_if_visible":dropped,"position":at.0.to_array(),"entrance":entrance.to_array(),"store_wheat":stocked}));
    }
    if let Some((when, dropped)) = tracked.pending_drop {
        let rise: u32 = tracked.stock_rises.iter().map(|(_, v)| *v).sum();
        if tracked.walked_with_cargo && rise >= dropped.unwrap_or(1).max(1) && !tracked.deposited {
            tracked.deposited = true;
            state.event("workplace_deposit_observed", json!({"exact_dropped_wheat_if_visible":dropped,"observed_store_increase":rise,"matching_window_seconds":2,"building_id":tracked.building.0}));
        } else if now.duration_since(when).as_secs_f32() > 2.0 {
            state.event(
                "deposit_unconfirmed_retry_next_cycle",
                json!({"exact_dropped_wheat_if_visible":dropped,"observed_store_increase":rise}),
            );
            tracked.pending_drop = None;
            tracked.carried = false;
            tracked.walked_with_cargo = false;
            tracked.stock_rises.clear();
        }
    }
    tracked.was_carrying_wheat = wheat;
    tracked.last_exact_cargo = exact_wheat;
    tracked.last_store = stocked;
    state.latest = json!({"person_id":tracked.person.0,"name":name.0,"building_id":tracked.building.0,"position":at.0.to_array(),"activity":format!("{activity:?}"),"navigation":nav.map(|n|format!("{n:?}")),"public_load":load,"exact_wheat_if_visible":exact_wheat,"workplace_wheat":stocked,"walked_with_cargo":tracked.walked_with_cargo,"deposit_observed":tracked.deposited,"inside_field":field.contains_world_point(at.0.xz(),field_at.0,field_yaw.0,0.),"day":clocks.iter().next().map(|c|c.day)});
    if state.last_sample.elapsed().as_secs_f32() >= 1.0 {
        state.last_sample = now;
        if state.samples.len() < 4096 {
            let sample = state.latest.clone();
            let elapsed = state.started.elapsed().as_secs_f64();
            state
                .samples
                .push(json!({"real_seconds":elapsed,"state":sample}));
        }
        if let Err(error) = state.publish("running", None) {
            state.finish(Some(error.to_string()), &mut exit);
            return;
        }
    }
    if let Some(ticket) = state.ticket {
        if let Some(done) = completions.take(ticket) {
            state.ticket = None;
            if let Some(error) = done.error {
                state.finish(Some(error), &mut exit);
                return;
            }
            state.shots += 1;
            state.readiness = default();
            state.event("capture_complete", json!({"path":done.path}));
        }
    }
    let Ok(mut camera) = cameras.single_mut() else {
        state.track = Some(tracked);
        return;
    };
    let focus = (field_at.0 + farm_at.0) * 0.5;
    camera.focus = focus;
    camera.focus_target = focus;
    camera.zoom = 34.;
    camera.zoom_target = 34.;
    camera.yaw = farm_yaw.0 + if state.shots == 1 { 0.3 } else { 2.8 };
    camera.yaw_target = camera.yaw;
    let field_ready = visual.is_some_and(|v| v.matches(field, field_at.0, field_yaw.0, &terrain));
    let framing_phase_ready =
        state.shots >= 2 || (*activity == CharacterActivity::Farming && dressed);
    if state.ticket.is_none()
        && state.shots < 3
        && field_ready
        && framing_phase_ready
        && (state.shots < 2 || tracked.deposited)
    {
        let ready = advance_readiness(
            &mut state.readiness,
            &CaptureReadiness {
                minimum_frames: 25,
                maximum_frames: 1800,
                minimum_loaded_chunks: 25,
                stable_loaded_chunk_frames: 20,
            },
            inspection.loaded_chunk_count(),
        );
        match ready {
            ReadinessOutcome::TimedOut(error) => {
                state.finish(Some(error), &mut exit);
                return;
            }
            ReadinessOutcome::Ready => {
                let shot =
                    ["01-farming-near", "02-farming-reverse", "03-after-deposit"][state.shots];
                if let Some(target) = inspection.scene_target.as_ref() {
                    let path = state.out.join(format!("{shot}.png"));
                    let mut request = live_capture_request(
                        path,
                        "connected-shaped-farm",
                        shot,
                        CaptureTarget::Scene,
                    );
                    if let Err(error) = inspection.complete_live_request(
                        &mut request,
                        Some(&camera),
                        clocks.iter().next(),
                        state.readiness.frames,
                    ) {
                        state.finish(Some(error), &mut exit);
                        return;
                    }
                    state.ticket = Some(request_capture(
                        &mut commands,
                        Screenshot::image(target.image.clone()),
                        request,
                        &mut completions,
                    ));
                    let detail = state.latest.clone();
                    state.event("capture_requested", json!({"shot":shot,"state":detail}));
                }
            }
            _ => {}
        }
    }
    if state.shots == 3 && tracked.deposited {
        state.finish(None, &mut exit);
    }
    state.track = Some(tracked);
}
