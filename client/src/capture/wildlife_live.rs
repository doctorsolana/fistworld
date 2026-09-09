//! Opt-in connected smoke test of natural population, movement and rig LOD.
//! Reads replicated state only. Never spawns horses or drives their behavior.
use super::{inspection::CaptureInspection, live::live_capture_request};
use crate::{animals::HorseRig, camera_rts::CommanderCamera, capture_artifact::*};
use bevy::{prelude::*, render::view::screenshot::Screenshot};
use shared::components::*;
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

#[derive(Default)]
pub(crate) struct WildlifeCapture {
    initialized: bool,
    dir: Option<PathBuf>,
    started: Option<Instant>,
    focus: Option<Vec3>,
    subjects: BTreeMap<u64, [f32; 3]>,
    moved: bool,
    grazed: bool,
    stage: usize,
    stable: u32,
    chunks: usize,
    ticket: Option<u64>,
}

pub(crate) fn drive(
    mut commands: Commands,
    mut state: Local<WildlifeCapture>,
    horses: Query<(&Horse, &PlayerPosition, &HorseAnimation, Has<HorseRig>)>,
    mut cameras: Query<&mut CommanderCamera>,
    mut creator: ResMut<crate::ui::hero_creator::HeroCreatorOpen>,
    input: Res<crate::input::InputState>,
    clocks: Query<&WorldTime>,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if !state.initialized {
        state.initialized = true;
        state.dir = std::env::var_os("FISTWORLD_WILDLIFE_CAPTURE_DIR").map(PathBuf::from);
        if let Some(dir) = &state.dir {
            std::fs::create_dir_all(dir).expect("wildlife capture directory");
            state.started = Some(Instant::now());
        }
    }
    let Some(dir) = state.dir.clone() else {
        return;
    };
    if state.stage >= 6 {
        return;
    }
    if state.started.unwrap().elapsed().as_secs() > 120 {
        error!(
            "wildlife connected capture timed out at stage {} ({} horses)",
            state.stage,
            horses.iter().count()
        );
        state.stage = 6;
        exit.write(AppExit::error());
        return;
    }
    if creator.0 {
        creator.0 = false;
        state.stable = 0;
        return;
    }
    if input.ui_blocking() {
        return;
    }
    if let Some(ticket) = state.ticket {
        let Some(result) = completions.take(ticket) else {
            return;
        };
        if let Some(error) = result.error {
            error!("wildlife capture failed: {error}");
            state.stage = 6;
            exit.write(AppExit::error());
            return;
        }
        info!("wildlife capture wrote {}", result.path.display());
        state.ticket = None;
        state.stage += 1;
        state.stable = 0;
    }
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    if state.focus.is_none() {
        let chosen = horses
            .iter()
            .filter(|(_, p, _, _)| {
                horses
                    .iter()
                    .filter(|(_, other, _, _)| p.0.distance_squared(other.0) < 25.0_f32.powi(2))
                    .count()
                    >= 3
            })
            .min_by(|(_, a, _, _), (_, b, _, _)| {
                a.0.distance_squared(camera.focus)
                    .total_cmp(&b.0.distance_squared(camera.focus))
            })
            .map(|(_, p, _, _)| p.0);
        let Some(at) = chosen else {
            return;
        };
        state.focus = Some(at);
        state.subjects = horses
            .iter()
            .filter(|(_, p, _, _)| p.0.distance_squared(at) < 25.0_f32.powi(2))
            .map(|(h, p, _, _)| (h.id, p.0.to_array()))
            .collect();
        info!(
            "wildlife connected capture observing {} natural horses near {at:?}",
            state.subjects.len()
        );
    }
    let focus = state.focus.unwrap();
    let zoom = if state.stage == 3 { 1000. } else { 24. };
    camera.focus = focus;
    camera.focus_target = focus;
    camera.zoom = zoom;
    camera.zoom_target = zoom;
    camera.yaw = 2.55;
    camera.yaw_target = 2.55;
    for (h, p, a, _) in &horses {
        if let Some(original) = state.subjects.get(&h.id).copied() {
            state.moved |= p.0.distance_squared(Vec3::from_array(original)) > 0.6_f32.powi(2);
            state.grazed |= a.activity == HorseActivity::Graze;
        }
    }
    let chunks = inspection
        .loaded_chunks
        .as_ref()
        .map_or(0, |c| c.chunks.len());
    if chunks == state.chunks && chunks >= 25 {
        state.stable += 1;
    } else {
        state.chunks = chunks;
        state.stable = 0;
    }
    let rigs = horses.iter().filter(|(_, _, _, rig)| *rig).count();
    if state.stage == 3 {
        if rigs == 0 && state.stable >= 30 {
            info!("wildlife connected capture: wide zoom retains horse records with zero rigs");
            state.stage = 4;
            state.stable = 0;
        }
        return;
    }
    if state.stage == 5 {
        let returning: Vec<_> = horses.iter().filter(|(h,_,_,_)| state.subjects.contains_key(&h.id)).map(|(h,p,a,_)|
            serde_json::json!({"id":h.id,"position":p.0.to_array(),"activity":format!("{:?}",a.activity)})).collect();
        assert_eq!(
            returning.len(),
            state.subjects.len(),
            "zoom changed horse identities"
        );
        std::fs::write(
            dir.join("behavior.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "natural_spawn":true,"moved":state.moved,"grazed":state.grazed,
                "wide_zoom_rigs":0,"restored_rigs":rigs,"horses":returning,
                "elapsed_seconds":state.started.unwrap().elapsed().as_secs_f32()
            }))
            .unwrap(),
        )
        .expect("write wildlife evidence");
        info!("Wildlife connected smoke passed: natural spawn, grazing, movement, zero distant rigs and restored identities");
        state.stage = 6;
        exit.write(AppExit::Success);
        return;
    }
    if state.stable < 30 || rigs < 3 {
        return;
    }
    let shot = match state.stage {
        0 => "herd",
        1 if state.grazed => "grazing",
        2 if state.moved
            && horses
                .iter()
                .any(|(_, _, a, _)| matches!(a.activity, HorseActivity::Moving(_))) =>
        {
            "walking"
        }
        4 => "returned",
        _ => return,
    };
    let Some(target) = inspection.scene_target.as_ref() else {
        return;
    };
    let mut request = live_capture_request(
        dir.join(format!("{shot}.png")),
        "connected-wildlife",
        shot,
        CaptureTarget::Scene,
    );
    if let Err(error) = inspection.complete_live_request(
        &mut request,
        Some(&camera),
        clocks.iter().next(),
        state.stable,
    ) {
        error!("wildlife capture: {error}");
        state.stage = 6;
        exit.write(AppExit::error());
        return;
    }
    request.metadata.assertions.push(CaptureAssertionResult {
        assertion: CaptureAssertion::HorseRigsAtLeast { count: 3 },
        passed: rigs >= 3,
        observed: rigs,
    });
    state.ticket = Some(request_capture(
        &mut commands,
        Screenshot::image(target.image.clone()),
        request,
        &mut completions,
    ));
}
