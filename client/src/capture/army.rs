//! Continuous connected army verification. No local soldiers or position writes:
//! selections enter the ordinary input pipeline; all arrivals are replicated.
use super::{inspection::CaptureInspection, live::live_capture_request};
use crate::{
    army_roster::ArmyRoster,
    camera_rts::{CommanderCamera, CursorTerrainOverride},
    capture_artifact::{
        request_capture, CaptureAssertion, CaptureAssertionResult, CaptureCompletions,
        CaptureTarget,
    },
    selection::Selection,
};
use bevy::{prelude::*, render::view::screenshot::Screenshot};
use shared::{
    army_lab::ArmyLabScenario,
    components::*,
    formation::{FormationGroup, FormationSoldier},
    protocol::FormationFrontage,
};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Waiting,
    Start,
    Prepare,
    Preview,
    Release,
    Moving,
    Arrival,
    Ui,
    Next,
    Done,
}
#[derive(Resource, Default)]
pub(crate) struct ArmyCapture {
    initialized: bool,
    scenario: Option<ArmyLabScenario>,
    started: Option<Instant>,
    stage: Stage,
    deployment: usize,
    ticket: Option<(u64, Stage)>,
    input_phase: u8,
    stable: u32,
    chunks: usize,
    last_progress: u32,
    expected: Vec<(Entity, Vec3, Vec3, f32)>,
}

pub(crate) fn drive_army_input(
    mut commands: Commands,
    mut state: ResMut<ArmyCapture>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window>,
) {
    let Some(scenario) = state.scenario.as_ref() else {
        return;
    };
    if state.input_phase == 0 || state.input_phase >= 6 {
        return;
    }
    let deployment = &scenario.deployments[state.deployment];
    let centre = Vec3::from_array(deployment.target).xz();
    let facing = Vec2::from_array(deployment.facing).normalize();
    let right = Vec2::new(facing.y, -facing.x);
    if let Ok(mut window) = windows.single_mut() {
        let centre = Vec2::new(window.width(), window.height()) * 0.5;
        window.set_cursor_position(Some(centre));
    }
    match state.input_phase {
        1 => {
            commands.insert_resource(CursorTerrainOverride(
                centre - right * deployment.width * 0.5,
            ));
            state.input_phase = 2;
        }
        2 => {
            mouse.press(MouseButton::Right);
            state.input_phase = 3;
        }
        3 => {
            commands.insert_resource(CursorTerrainOverride(
                centre + right * deployment.width * 0.5,
            ));
            state.input_phase = 4;
        }
        5 => {
            mouse.release(MouseButton::Right);
            state.input_phase = 6;
            info!("Army lab: released formation drag through ordinary order input");
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn drive_army_capture(
    mut commands: Commands,
    mut state: ResMut<ArmyCapture>,
    roster: Res<ArmyRoster>,
    input: Res<crate::input::InputState>,
    preview: Res<crate::selection::formation_preview::PreviewReadiness>,
    dressed: Query<(), With<crate::hero::HeroDressed>>,
    mut selection: ResMut<Selection>,
    mut mode: ResMut<crate::combat_mode::CombatMode>,
    mut cameras: Query<&mut CommanderCamera>,
    people: Query<(&PlayerPosition, &PlayerRotation, Option<&PersonId>)>,
    terrain: Res<shared::terrain::WorldTerrain>,
    clocks: Query<&WorldTime>,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if !state.initialized {
        state.initialized = true;
        state.scenario = ArmyLabScenario::from_env().filter(|s| s.battle.is_none());
        state.started = Some(Instant::now());
    }
    let Some(scenario) = state.scenario.clone() else {
        return;
    };
    if state.stage == Stage::Done {
        return;
    }
    let out = PathBuf::from(
        std::env::var("FISTWORLD_ARMY_CAPTURE_DIR")
            .unwrap_or_else(|_| "/tmp/fistworld-army-250".into()),
    );
    if state.started.unwrap().elapsed().as_secs_f32() > scenario.timeout_seconds {
        let remaining: Vec<_> = state
            .expected
            .iter()
            .filter_map(|(e, _, destination, _)| {
                people.get(*e).ok().map(|(p, _, _)| {
                    (
                        format!("{e:?}"),
                        p.0.to_array(),
                        destination.to_array(),
                        p.0.xz().distance(destination.xz()),
                    )
                })
            })
            .filter(|(_, _, _, error)| *error > 0.3)
            .collect();
        std::fs::create_dir_all(&out).expect("army output directory");
        std::fs::write(
            out.join("failure.json"),
            serde_json::to_vec_pretty(&remaining).unwrap(),
        )
        .expect("army failure evidence");
        error!("Army lab timed out: {} soldiers not in their slots; {} replicated soldiers, {} battalions", remaining.len(), roster.soldiers.len(), roster.battalions.len());
        state.stage = Stage::Done;
        exit.write(AppExit::error());
        return;
    }
    if let Some((ticket, next)) = state.ticket {
        let Some(done) = completions.take(ticket) else {
            return;
        };
        if let Some(error) = done.error {
            panic!("army capture failed: {error}");
        }
        info!("Army lab: wrote {}", done.path.display());
        state.ticket = None;
        state.stage = next;
    }
    let mut camera = match cameras.single_mut() {
        Ok(camera) => camera,
        Err(_) => return,
    };
    let mut focus = Vec3::from_array(scenario.camera_focus);
    focus.y = terrain.get_height(focus.x, focus.z);
    camera.focus = focus;
    camera.focus_target = focus;
    camera.zoom = scenario.camera_zoom;
    camera.zoom_target = scenario.camera_zoom;
    camera.yaw = 0.0;
    camera.yaw_target = 0.0;
    mode.0 = true;
    let world = inspection.world_snapshot(clocks.iter().next());
    if state.chunks == world.loaded_chunks && world.loaded_chunks > 0 {
        state.stable += 1;
    } else {
        state.chunks = world.loaded_chunks;
        state.stable = 0;
    }
    if state.stage == Stage::Waiting {
        if input.ui_blocking()
            || dressed.iter().count() < scenario.total()
            || roster.account != scenario.account
            || roster.battalions.len() != scenario.battalions
            || roster.soldiers.len() != scenario.total()
            || state.stable < 30
        {
            return;
        }
        if !roster
            .battalions
            .iter()
            .all(|b| b.count == scenario.soldiers_per_battalion)
        {
            return;
        }
        selection.set(
            roster
                .battalions
                .iter()
                .flat_map(|b| b.members.iter().copied())
                .collect(),
        );
        info!(
            "Army lab: selected {} people in {} complete battalions",
            selection.len(),
            roster.battalions.len()
        );
        state.stage = Stage::Start;
    }
    if state.stage == Stage::Next {
        state.deployment += 1;
        if state.deployment == scenario.deployments.len() {
            info!(
                "Army lab PASS: {} soldiers, {} battalions, {} complete deployments",
                scenario.total(),
                scenario.battalions,
                scenario.deployments.len()
            );
            state.stage = Stage::Done;
            commands.remove_resource::<CursorTerrainOverride>();
            exit.write(AppExit::Success);
            return;
        }
        state.stage = Stage::Prepare;
    }
    if state.stage == Stage::Prepare {
        let mut groups = BTreeMap::<u64, Vec<FormationSoldier>>::new();
        for soldier in roster.soldiers.values() {
            let Ok((point, _, _)) = people.get(soldier.entity) else {
                return;
            };
            groups
                .entry(soldier.battalion.unwrap().0)
                .or_default()
                .push(FormationSoldier {
                    entity: soldier.entity,
                    identity: soldier.identity,
                    position: point.0,
                    strength: soldier.strength,
                });
        }
        let deployment = &scenario.deployments[state.deployment];
        let blocks = shared::formation::layout(
            groups
                .into_iter()
                .map(|(key, soldiers)| FormationGroup { key, soldiers })
                .collect(),
            Vec3::from_array(deployment.target),
            Some(FormationFrontage {
                facing: Vec2::from_array(deployment.facing),
                width: deployment.width,
            }),
        );
        state.expected.clear();
        for block in blocks {
            for (entity, destination) in block.slots {
                state.expected.push((
                    entity,
                    people.get(entity).unwrap().0 .0,
                    destination,
                    f32::atan2(-block.facing.x, -block.facing.y),
                ));
            }
        }
        state.input_phase = 1;
        state.stage = Stage::Preview;
        return;
    }
    if state.stage == Stage::Release {
        state.input_phase = 5;
        state.stage = Stage::Moving;
        return;
    }
    let mut arrived = 0;
    let mut moved = 0;
    let mut aligned = 0;
    let mut max_error = 0.0_f32;
    let mut evidence = Vec::new();
    for (entity, start, goal, facing) in &state.expected {
        let Ok((point, rotation, person)) = people.get(*entity) else {
            continue;
        };
        let error = point.0.xz().distance(goal.xz());
        let angle_error = (rotation.0 - facing)
            .sin()
            .atan2((rotation.0 - facing).cos())
            .abs();
        max_error = max_error.max(error);
        arrived += usize::from(error < 0.3);
        moved += usize::from(point.0.xz().distance(start.xz()) > 3.0);
        aligned += usize::from(angle_error < 0.06);
        evidence.push(serde_json::json!({"entity": format!("{entity:?}"), "person_id": person.map(|p| p.0), "battalion": roster.soldiers.get(entity).and_then(|s| s.battalion).map(|b| b.0), "position": point.0.to_array(), "destination": goal.to_array(), "error_metres": error, "facing_error_radians": angle_error}));
    }
    if state.stage == Stage::Arrival && world.frame.saturating_sub(state.last_progress) >= 120 {
        state.last_progress = world.frame;
        info!("Army lab deployment {}: arrived={arrived}, aligned={aligned}, max_error={max_error:.3}m", state.deployment + 1);
    }
    let (name, next, target) = match state.stage {
        Stage::Start => ("start", Stage::Prepare, CaptureTarget::Scene),
        Stage::Preview
            if state.input_phase == 4
                && preview.slots == scenario.total()
                && preview.stable_frames >= 12 =>
        {
            ("preview", Stage::Release, CaptureTarget::Scene)
        }
        Stage::Moving if moved == scenario.total() => {
            ("moving", Stage::Arrival, CaptureTarget::Scene)
        }
        Stage::Arrival if arrived == scenario.total() && aligned == scenario.total() => {
            ("arrival", Stage::Ui, CaptureTarget::Scene)
        }
        Stage::Ui => ("selected-ui", Stage::Next, CaptureTarget::Window),
        _ => return,
    };
    let Some(scene) = inspection.scene_target.as_ref() else {
        return;
    };
    std::fs::create_dir_all(&out).expect("army capture directory");
    let stem = format!("{:02}-{name}", state.deployment + 1);
    let report = serde_json::json!({"deployment": state.deployment + 1, "soldiers": roster.soldiers.len(), "selected": selection.len(), "battalions": roster.battalions.len(), "arrived": arrived, "aligned": aligned, "moved": moved, "max_error_metres": max_error, "people": evidence});
    std::fs::write(
        out.join(format!("{stem}.army.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .expect("army measurements");
    let mut request = live_capture_request(
        out.join(format!("{stem}.png")),
        "connected-army-250",
        &stem,
        target,
    );
    inspection
        .complete_live_request(
            &mut request,
            Some(&camera),
            clocks.iter().next(),
            state.stable,
        )
        .expect("army capture inspection");
    request.metadata.assertions.push(CaptureAssertionResult {
        assertion: CaptureAssertion::VillagersAtLeast {
            count: scenario.total(),
        },
        passed: roster.soldiers.len() == scenario.total(),
        observed: roster.soldiers.len(),
    });
    let screenshot = if target == CaptureTarget::Scene {
        Screenshot::image(scene.image.clone())
    } else {
        Screenshot::image(
            inspection
                .presentation_target
                .as_ref()
                .expect("army composed capture target")
                .image
                .clone(),
        )
    };
    let ticket = request_capture(&mut commands, screenshot, request, &mut completions);
    state.ticket = Some((ticket, next));
}
