//! Connected siege rehearsal through the ordinary F / RMB / H input path.
use super::{inspection::CaptureInspection, live::live_capture_request};
use crate::{
    camera_rts::{CommanderCamera, CursorTerrainOverride},
    capture_artifact::{request_capture, CaptureCompletions, CaptureTarget},
    selection::Selection,
};
use bevy::{ecs::system::SystemParam, prelude::*, render::view::screenshot::Screenshot};
use shared::{army_lab::ArmyLabScenario, components::*};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    time::Instant,
};
#[derive(Resource, Default)]
pub(crate) struct SiegeCapture {
    initialized: bool,
    scenario: Option<ArmyLabScenario>,
    started: Option<Instant>,
    stage: u8,
    input: u8,
    cursor: Vec2,
    stable: u32,
    chunks: usize,
    tickets: VecDeque<u64>,
    preview_ticket: Option<u64>,
    finishing: bool,
    shots: usize,
    next_shot: f64,
    ordered_at: f64,
    held_at: f64,
    held_ammo: u16,
    damage: HashSet<u64>,
    impacts: HashSet<u64>,
    moved: f32,
    max_speed: f32,
    last: Option<Vec3>,
    winding: bool,
    held_ui: bool,
}
pub(crate) fn drive_siege_input(
    mut commands: Commands,
    mut state: ResMut<SiegeCapture>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window>,
) {
    if state.input == 0 {
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        let middle = window.size() * 0.5;
        window.set_cursor_position(Some(middle));
    }
    match state.input {
        1 => {
            commands.insert_resource(CursorTerrainOverride(state.cursor));
            state.input = 2;
        }
        2 => {
            mouse.press(MouseButton::Right);
            state.input = 3;
        }
        3 => {
            mouse.release(MouseButton::Right);
            state.input = 0;
        }
        10 => {
            commands.insert_resource(CursorTerrainOverride(state.cursor));
            keys.press(KeyCode::KeyF);
            state.input = 11;
        }
        11 => {
            keys.release(KeyCode::KeyF);
            state.input = 0;
        }
        20 => {
            keys.press(KeyCode::KeyH);
            state.input = 21;
        }
        21 => {
            keys.release(KeyCode::KeyH);
            state.input = 0;
        }
        _ => {}
    }
}
pub(crate) fn drive_siege_ray(
    state: Res<SiegeCapture>,
    terrain: Res<shared::terrain::WorldTerrain>,
    mut ray: ResMut<crate::camera_rts::CursorRay>,
) {
    if state.stage == 0 || state.stage >= 6 {
        return;
    }
    let p = state.cursor;
    ray.0 = Some(Ray3d::new(
        Vec3::new(p.x, terrain.get_height(p.x, p.y) + 20.0, p.y),
        Dir3::NEG_Y,
    ));
}
#[derive(SystemParam)]
pub(crate) struct SiegeScene<'w, 's> {
    machines: Query<
        'w,
        's,
        (
            Entity,
            &'static Catapult,
            &'static CatapultStatus,
            &'static PlayerPosition,
            &'static CharacterMotion,
            Has<crate::siege::CatapultSceneReady>,
        ),
    >,
    people: Query<
        'w,
        's,
        (
            &'static PersonId,
            &'static PlayerPosition,
            &'static Health,
            &'static CommandedBy,
        ),
        With<CharacterKind>,
    >,
    dressed: Query<'w, 's, (), With<crate::hero::HeroDressed>>,
    stones: Query<'w, 's, &'static SiegeProjectile>,
    impacts: Query<'w, 's, &'static SiegeImpact>,
    terrain: Res<'w, shared::terrain::WorldTerrain>,
    clock: Query<'w, 's, &'static WorldTime>,
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_siege_capture(
    mut commands: Commands,
    mut state: ResMut<SiegeCapture>,
    scene: SiegeScene,
    input: Res<crate::input::InputState>,
    aim: Res<crate::siege::SiegeAim>,
    mut selection: ResMut<Selection>,
    mut combat: ResMut<crate::combat_mode::CombatMode>,
    mut cameras: Query<&mut CommanderCamera>,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if !state.initialized {
        state.initialized = true;
        state.scenario = ArmyLabScenario::from_env().filter(|s| s.catapult.is_some());
        state.started = Some(Instant::now());
    }
    let Some(scenario) = state.scenario.clone() else {
        return;
    };
    if state.stage >= 6 {
        return;
    }
    let fixture = scenario.catapult.as_ref().unwrap();
    if state.started.unwrap().elapsed().as_secs_f32() > scenario.timeout_seconds {
        error!(
            "Siege capture timeout: stage={} shots={} impacts={} damage={}",
            state.stage,
            state.shots,
            state.impacts.len(),
            state.damage.len()
        );
        state.stage = 6;
        exit.write(AppExit::error());
        return;
    }
    let Some(clock) = scene.clock.iter().next() else {
        return;
    };
    let now = crate::siege::seconds(clock);
    let Ok((machine, catapult, status, position, motion, ready)) = scene.machines.single() else {
        return;
    };
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let out = PathBuf::from(
        std::env::var("FISTWORLD_ARMY_CAPTURE_DIR")
            .unwrap_or_else(|_| "/tmp/fistworld-catapult".into()),
    );
    let world = inspection.world_snapshot(Some(clock));
    if state.chunks > 0 && state.chunks == world.loaded_chunks {
        state.stable += 1;
    } else {
        state.stable = 0;
        state.chunks = world.loaded_chunks;
    }
    combat.0 = true;
    let mut focus = if state.stage <= 1 || state.stage == 4 {
        position.0
    } else {
        Vec3::from_array(scenario.camera_focus)
    };
    let mut zoom = if state.stage <= 1 || state.stage == 4 {
        16.0
    } else {
        scenario.camera_zoom
    };
    if state.stage == 5 {
        zoom = 18.0;
        focus = position.0;
        if let Some(stone) = scene.stones.iter().next() {
            if now > stone.launched_at + 0.35 {
                focus = stone.position(now.min(stone.impact_at));
            }
        }
        if let Some(impact) = scene.impacts.iter().next() {
            focus = impact.position;
        }
    }
    focus.y = focus
        .y
        .max(scene.terrain.get_height(focus.x, focus.z) + 1.0);
    camera.focus = focus;
    camera.focus_target = focus;
    camera.zoom = zoom;
    camera.zoom_target = zoom;
    let yaw = if state.stage == 2 || state.stage == 3 {
        1.4
    } else {
        0.5
    };
    camera.yaw = yaw;
    camera.yaw_target = yaw;
    let mut completed = false;
    // Keep a small screenshot pipeline so a 220 ms arm release is captured
    // across several real rendered frames. All tickets drain before exit.
    while let Some(ticket) = state.tickets.front().copied() {
        let Some(done) = completions.take(ticket) else {
            break;
        };
        assert!(done.error.is_none(), "siege capture: {:?}", done.error);
        state.tickets.pop_front();
        completed |= state.preview_ticket == Some(ticket);
    }
    for (id, _, health, _) in &scene.people {
        if health.current < health.max {
            state.damage.insert(id.0);
        }
    }
    for impact in &scene.impacts {
        state.impacts.insert(impact.seed);
    }
    state.winding |= status.phase == SiegePhase::Winding;
    if state.stage == 0 {
        let expected = scenario.total()
            + scenario
                .battle
                .as_ref()
                .map_or(0, |b| b.defender_battalions * b.defenders_per_battalion);
        if !ready
            || input.ui_blocking()
            || state.stable < 30
            || scene.dressed.iter().count() < expected
            || scene.people.iter().count() != expected
        {
            return;
        }
        selection.set(vec![machine]);
        state.cursor = Vec3::from_array(fixture.move_to).xz();
        state.input = 1;
        state.stage = 1;
        state.ordered_at = now;
        state.last = Some(position.0);
        info!("Siege lab ready; moving through ordinary RMB");
    } else if state.stage == 1 {
        if let Some(previous) = state.last {
            let moved = previous.xz().distance(position.0.xz());
            state.moved += moved;
            state.max_speed = state.max_speed.max(motion.velocity.length());
        }
        state.last = Some(position.0);
        assert_eq!(
            catapult.ammunition, CATAPULT_AMMUNITION,
            "fired while moving"
        );
        if position
            .0
            .xz()
            .distance(Vec3::from_array(fixture.move_to).xz())
            < 0.25
            && status.phase == SiegePhase::Ready
            && now - state.ordered_at > 1.0
        {
            state.cursor = Vec3::from_array(fixture.aim).xz();
            state.input = 10;
            state.stage = 2;
            state.next_shot = now + 0.2;
        }
    } else if state.stage == 2 {
        if !aim.0 {
            return;
        }
        if completed {
            state.stage = 3;
            state.input = 1;
            state.ordered_at = now;
            state.next_shot = now;
        }
    } else if state.stage == 3 {
        if !state.impacts.is_empty() && scene.impacts.iter().any(|p| now - p.at > 0.9) {
            state.input = 20;
            state.stage = 4;
            state.held_at = now;
            state.held_ammo = catapult.ammunition;
        }
    } else if state.stage == 4 {
        assert_eq!(
            catapult.ammunition, state.held_ammo,
            "Hold failed: catapult fired again"
        );
        if now - state.held_at > CATAPULT_RELOAD + 2.0 {
            let target = scene
                .people
                .iter()
                .filter(|(_, _, h, o)| o.0 != scenario.account && !h.is_dead())
                .min_by(|a, b| {
                    a.1 .0
                        .distance_squared(Vec3::from_array(fixture.aim))
                        .total_cmp(&b.1 .0.distance_squared(Vec3::from_array(fixture.aim)))
                });
            let Some(target) = target else {
                panic!("no surviving enemy for ordinary attack test")
            };
            state.cursor = target.1 .0.xz();
            state.input = 1;
            state.stage = 5;
            state.ordered_at = now;
        }
    } else if state.stage == 5
        && (state.finishing
            || (state.impacts.len() >= 2 && scene.impacts.iter().any(|p| now - p.at > 1.7)))
    {
        state.finishing = true;
        if !state.tickets.is_empty() {
            return;
        }
        let passed = state.moved > 5.5
            && state.max_speed < 1.5
            && state.winding
            && state.damage.len() >= 2
            && catapult.ammunition == 18
            && state.shots >= 15;
        std::fs::write(out.join("summary.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":passed,"moved_metres":state.moved,"max_observed_speed":state.max_speed,"windup_seen":state.winding,"impacts":state.impacts.len(),"damaged_people":state.damage,"ammo":catapult.ammunition,"hold_observed_seconds":CATAPULT_RELOAD+2.0,"frames":state.shots})).unwrap()).unwrap();
        info!("Siege lab completed: passed={passed}");
        exit.write(if passed {
            AppExit::Success
        } else {
            AppExit::error()
        });
        state.stage = 6;
        return;
    }
    if state.tickets.len() >= 4
        || now < state.next_shot
        || (state.stage == 2 && state.preview_ticket.is_some())
    {
        return;
    }
    let held_ui = state.stage == 4 && now - state.held_at > 2.0 && !state.held_ui;
    let window = state.stage == 2 || held_ui;
    state.held_ui |= held_ui;
    let image = if window {
        inspection
            .presentation_target
            .as_ref()
            .map(|p| p.image.clone())
    } else {
        inspection.scene_target.as_ref().map(|p| p.image.clone())
    };
    let Some(image) = image else { return };
    std::fs::create_dir_all(&out).unwrap();
    let name = format!("{:04}", state.shots);
    let people:Vec<_>=scene.people.iter().map(|(id,p,h,o)|serde_json::json!({"id":id.0,"position":p.0.to_array(),"health":h.current,"owner":o.0})).collect();
    let stones:Vec<_>=scene.stones.iter().map(|p|serde_json::json!({"position":p.position(now).to_array(),"launch":p.launched_at,"impact_at":p.impact_at})).collect();
    std::fs::write(out.join(format!("{name}.siege.json")),serde_json::to_vec_pretty(&serde_json::json!({"time":now,"stage":state.stage,"phase":status.phase.label(),"position":position.0.to_array(),"ammunition":catapult.ammunition,"fire_at":status.fire_at,"arm_angle":catapult_arm_angle(status,now),"aim_preview":aim.0,"people":people,"stones":stones,"impacts":state.impacts.len()})).unwrap()).unwrap();
    let mut request = live_capture_request(
        out.join(format!("{name}.png")),
        "connected-catapult",
        &name,
        if window {
            CaptureTarget::Window
        } else {
            CaptureTarget::Scene
        },
    );
    inspection
        .complete_live_request(&mut request, Some(&camera), Some(clock), state.stable)
        .unwrap();
    let ticket = request_capture(
        &mut commands,
        Screenshot::image(image),
        request,
        &mut completions,
    );
    state.tickets.push_back(ticket);
    if state.stage == 2 {
        state.preview_ticket = Some(ticket);
    }
    state.shots += 1;
    state.next_shot = now
        + if state.stage == 3 || state.stage == 5 {
            1.0 / 24.0
        } else if state.stage == 2 {
            0.2
        } else {
            0.5
        };
}

/// The ordinary RTS camera intentionally looks at terrain, even when its focus
/// carries an elevated Y. A projectile close-up needs an explicit cinematic pose.
/// This opt-in lab camera runs after the production camera and is recorded by
/// CaptureInspection, so the artifact reports the pose that actually rendered.
pub(crate) fn frame_siege_flight(
    state: Res<SiegeCapture>,
    clocks: Query<&WorldTime>,
    stones: Query<&SiegeProjectile>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    if state.stage != 5 {
        return;
    }
    let Some(clock) = clocks.iter().next() else {
        return;
    };
    let now = crate::siege::seconds(clock);
    let Some(stone) = stones
        .iter()
        .find(|stone| now > stone.launched_at + 0.35 && now < stone.impact_at)
    else {
        return;
    };
    let point = stone.position(now);
    for mut camera in &mut cameras {
        *camera = Transform::from_translation(point + Vec3::new(7.0, 10.0, 13.0))
            .looking_at(point, Vec3::Y);
    }
}
