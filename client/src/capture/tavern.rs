//! Connected visual proof; reads replicated patrons and never places or seats them locally.
use super::{inspection::CaptureInspection, live::live_capture_request};
use crate::{
    camera_rts::CommanderCamera,
    capture_artifact::{request_capture, CaptureCompletions, CaptureTarget},
};
use bevy::{prelude::*, render::view::screenshot::Screenshot};
use shared::components::*;
use std::{collections::HashSet, path::PathBuf, time::Instant};

#[derive(Default)]
pub(crate) struct TavernReview {
    started: Option<Instant>,
    stage: u8,
    ticket: Option<u64>,
    stable: u32,
    seen_sitting: HashSet<u64>,
    seen_completed: HashSet<u64>,
    max_seated: usize,
    last_probe: u32,
    samples: Vec<serde_json::Value>,
}

pub(crate) fn drive_tavern_review(
    mut commands: Commands,
    mut state: Local<TavernReview>,
    people: Query<(
        &PersonId,
        &CharacterName,
        &PlayerPosition,
        &PlayerRotation,
        &CharacterActivity,
        &CharacterDayPlan,
    )>,
    taverns: Query<(&SettlementBuilding, &PlayerPosition)>,
    clocks: Query<&WorldTime>,
    mut cameras: Query<&mut CommanderCamera>,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if std::env::var("FISTWORLD_TAVERN_REVIEW").as_deref() != Ok("1") || state.stage == 4 {
        return;
    }
    let started = *state.started.get_or_insert_with(Instant::now);
    let out = PathBuf::from("logs/captures/tavern-connected");
    std::fs::create_dir_all(&out).expect("tavern review directory");
    if let Some(ticket) = state.ticket {
        if let Some(done) = completions.take(ticket) {
            assert!(done.error.is_none(), "tavern capture: {:?}", done.error);
            state.ticket = None;
            state.stage += 1;
            if state.stage == 4 {
                assert_eq!(
                    state.seen_sitting.len(),
                    8,
                    "not every customer used a seat"
                );
                info!("Tavern review: all eight customers sat and completed their visits; maximum {} at once", state.max_seated);
                exit.write(AppExit::Success);
                return;
            }
        } else {
            return;
        }
    }
    if started.elapsed().as_secs() > 240 {
        error!(
            "Tavern review timed out at stage {}; seated people={}, maximum={}",
            state.stage,
            state.seen_sitting.len(),
            state.max_seated
        );
        state.stage = 4;
        exit.write(AppExit::error());
        return;
    }
    let Some((_, at)) = taverns
        .iter()
        .find(|(b, _)| b.kind == SettlementBuildingKind::Tavern)
    else {
        return;
    };
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let focus = at.0 + Vec3::new(0.0, 0.0, -3.5);
    camera.focus = focus;
    camera.focus_target = focus;
    camera.zoom = 26.0;
    camera.zoom_target = 26.0;
    camera.yaw = 3.48;
    camera.yaw_target = 3.48;
    let patrons: Vec<_> = people
        .iter()
        .filter(|(_, name, ..)| name.0.starts_with("Patron "))
        .collect();
    let seated = patrons
        .iter()
        .filter(|(_, _, _, _, activity, _)| **activity == CharacterActivity::Sitting)
        .count();
    let completed = patrons
        .iter()
        .filter(|(_, _, _, _, _, plan)| plan.leisure_status == PlannedLeisureStatus::Completed)
        .count();
    for (person, _, _, _, activity, plan) in &patrons {
        if plan.leisure_status == PlannedLeisureStatus::Completed {
            state.seen_completed.insert(person.0);
        }
        if **activity == CharacterActivity::Sitting {
            state.seen_sitting.insert(person.0);
        }
    }
    state.max_seated = state.max_seated.max(seated);
    let world = inspection.world_snapshot(clocks.iter().next());
    if world.loaded_chunks >= 25 && patrons.len() == 8 {
        state.stable += 1;
    } else {
        state.stable = 0;
    }
    // Always include the terminal state even when it falls between trace probes.
    if world.frame.saturating_sub(state.last_probe) >= 30
        || (state.stage == 3 && state.seen_completed.len() == 8)
    {
        state.last_probe = world.frame;
        state.samples.push(serde_json::json!({"frame":world.frame,"seated":seated,"completed":completed,"people":patrons.iter().map(|(id,name,pos,rot,activity,plan)|serde_json::json!({"id":id.0,"name":name.0,"position":pos.0.to_array(),"yaw":rot.0,"activity":format!("{activity:?}"),"status":format!("{:?}",plan.leisure_status)})).collect::<Vec<_>>()}));
        std::fs::write(
            out.join("visits.json"),
            serde_json::to_vec_pretty(&state.samples).unwrap(),
        )
        .expect("tavern trace");
    }
    if state.stable < 30 {
        return;
    }
    let shot = match state.stage {
        0 => "01-arriving",
        1 if seated >= 1 => "02-first-table",
        2 if seated == 8 => "03-courtyard",
        3 if state.seen_completed.len() == 8 => "04-leaving",
        _ => return,
    };
    let Some(scene) = inspection.scene_target.as_ref() else {
        return;
    };
    let mut request = live_capture_request(
        out.join(format!("{shot}.png")),
        "tavern-connected",
        shot,
        CaptureTarget::Scene,
    );
    inspection
        .complete_live_request(
            &mut request,
            Some(&camera),
            clocks.iter().next(),
            state.stable,
        )
        .expect("tavern capture metadata");
    state.ticket = Some(request_capture(
        &mut commands,
        Screenshot::image(scene.image.clone()),
        request,
        &mut completions,
    ));
}
