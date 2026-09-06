//! Connected army screen actions and a continuous Hold Line / Defensive rehearsal.
use super::{inspection::CaptureInspection, live::live_capture_request};
use crate::{
    army_roster::ArmyRoster,
    camera_rts::CommanderCamera,
    capture_artifact::{request_capture, CaptureCompletions, CaptureTarget},
    ui::encyclopedia::{
        army::{ArmyAction, ArmyManagement, BoundText},
        ClickGuard, EncyclopediaOpen, EncyclopediaTab,
    },
};
use bevy::{
    ecs::system::SystemParam, prelude::*, render::view::screenshot::Screenshot,
    ui::InteractionDisabled,
};
use shared::{army_lab::ArmyLabScenario, components::*};
use std::{collections::HashMap, path::PathBuf, time::Instant};
#[derive(Resource, Default)]
pub(crate) struct ManagementCapture {
    initialized: bool,
    scenario: Option<ArmyLabScenario>,
    started: Option<Instant>,
    stage: u8,
    action: Option<ArmyAction>,
    release: Option<Entity>,
    ticket: Option<u64>,
    transfer: Option<Entity>,
    baseline: HashMap<Entity, Vec3>,
    chunks: usize,
    stable: u32,
    next_shot: f64,
    impacted_at: Option<f64>,
    shots: usize,
    diagnostic_at: f32,
}
pub(crate) fn drive_management_input(
    mut state: ResMut<ManagementCapture>,
    guard: Res<ClickGuard>,
    management: Res<ArmyManagement>,
    time: Res<Time>,
    mut buttons: Query<(
        Entity,
        &ArmyAction,
        &mut Interaction,
        Has<InteractionDisabled>,
    )>,
) {
    if state.scenario.is_some() && time.elapsed_secs() >= state.diagnostic_at {
        info!(
            "Management lab: stage={} action={:?} guard={} pending={} now={} ticket={:?}",
            state.stage,
            state.action,
            guard.0,
            management.pending_until,
            time.elapsed_secs(),
            state.ticket
        );
        state.diagnostic_at = time.elapsed_secs() + 5.0;
    }
    if let Some(release) = state.release.take() {
        if let Ok((_, _, mut pressed, _)) = buttons.get_mut(release) {
            *pressed = Interaction::None;
        }
    }
    if !guard.0 {
        return;
    }
    let Some(action) = state.action else {
        return;
    };
    for (entity, current, mut interaction, disabled) in &mut buttons {
        if *current == action && !disabled {
            info!("Management lab: activating {action:?}");
            *interaction = Interaction::Pressed;
            state.release = Some(entity);
            state.action = None;
            break;
        }
    }
}
#[derive(SystemParam)]
pub(crate) struct ManagementScene<'w, 's> {
    people: Query<
        'w,
        's,
        (
            Entity,
            &'static PlayerPosition,
            &'static Health,
            &'static MemberOfBattalion,
        ),
        With<CharacterKind>,
    >,
    dressed: Query<'w, 's, (), With<crate::hero::HeroDressed>>,
    clocks: Query<'w, 's, &'static WorldTime>,
    texts: Query<'w, 's, &'static Text>,
    bound: Query<'w, 's, &'static Text, With<BoundText>>,
    changed_text: Query<'w, 's, (), (With<BoundText>, Changed<Text>)>,
    terrain: Res<'w, shared::terrain::WorldTerrain>,
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_management_capture(
    mut commands: Commands,
    mut state: ResMut<ManagementCapture>,
    roster: Res<ArmyRoster>,
    management: Res<ArmyManagement>,
    mut open: ResMut<EncyclopediaOpen>,
    mut tab: ResMut<EncyclopediaTab>,
    mut cameras: Query<&mut CommanderCamera>,
    scene: ManagementScene,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if !state.initialized {
        state.initialized = true;
        state.scenario = ArmyLabScenario::from_env().filter(|s| s.management);
        state.started = Some(Instant::now());
    }
    let Some(scenario) = state.scenario.clone() else {
        return;
    };
    if state.stage == 14 {
        return;
    }
    assert!(
        state.started.unwrap().elapsed().as_secs_f32() < scenario.timeout_seconds,
        "management lab timeout stage {}",
        state.stage
    );
    if let Some(ticket) = state.ticket {
        let Some(done) = completions.take(ticket) else {
            return;
        };
        assert!(done.error.is_none(), "management capture {:?}", done.error);
        state.ticket = None;
    }
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let mut focus = Vec3::from_array(scenario.camera_focus);
    focus.y = scene.terrain.get_height(focus.x, focus.z);
    camera.focus = focus;
    camera.focus_target = focus;
    camera.zoom = scenario.camera_zoom;
    camera.zoom_target = scenario.camera_zoom;
    camera.yaw = 0.15;
    camera.yaw_target = 0.15;
    let Some(clock) = scene.clocks.iter().next() else {
        return;
    };
    let now = crate::siege::seconds(clock);
    let world = inspection.world_snapshot(Some(clock));
    if state.chunks > 0 && state.chunks == world.loaded_chunks {
        state.stable += 1;
    } else {
        state.stable = 0;
        state.chunks = world.loaded_chunks;
    }
    if roster.battalions.len() != 2 || roster.soldiers.len() != 24 {
        return;
    }
    let first = &roster.battalions[0];
    let second = &roster.battalions[1];
    let out = PathBuf::from(
        std::env::var("FISTWORLD_ARMY_CAPTURE_DIR")
            .unwrap_or_else(|_| "/tmp/fistworld-army-management".into()),
    );
    let mut shot = None;
    // A roster transition spawns new rows with deferred commands. Wait for
    // their labels to bind and pass through layout before photographing them.
    if (1..=11).contains(&state.stage)
        && (!scene.changed_text.is_empty() || scene.bound.iter().any(|t| t.0.is_empty()))
    {
        return;
    }
    match state.stage {
        0 if state.stable >= 20 && scene.dressed.iter().count() >= 24 => {
            open.0 = true;
            *tab = EncyclopediaTab::Army;
            state.stage = 1;
        }
        1 if scene.texts.iter().any(|t| t.0 == "IN THIS BATTALION (12)") => {
            shot = Some("01-overview".to_string());
            state.action = Some(ArmyAction::SelectMembers);
            state.stage = 2;
        }
        2 if management.members.len() == 12 => {
            state.action = Some(ArmyAction::RemoveChecked);
            state.stage = 3;
        }
        3 if first.count == 0 => {
            shot = Some("02-empty-battalion".into());
            state.action = Some(ArmyAction::Fill);
            state.stage = 4;
        }
        4 if first.count == 12 => {
            state.action = Some(ArmyAction::Source(true));
            state.stage = 5;
        }
        5 if management.other_battalions => {
            let unit = second.members[0];
            state.transfer = Some(unit);
            state.action = Some(ArmyAction::Add(unit));
            state.stage = 6;
        }
        6 if first.count == 13 && second.count == 11 => {
            shot = Some("03-transferred".into());
            state.action = Some(ArmyAction::Remove(state.transfer.unwrap()));
            state.stage = 7;
        }
        7 if first.count == 12
            && roster.soldiers[&state.transfer.unwrap()]
                .battalion
                .is_none() =>
        {
            state.action = Some(ArmyAction::Choose(second.entity));
            state.stage = 8;
        }
        8 if management.selected == Some(second.entity) => {
            state.action = Some(ArmyAction::Source(false));
            state.stage = 9;
        }
        9 if !management.other_battalions => {
            state.action = Some(ArmyAction::Add(state.transfer.unwrap()));
            state.stage = 10;
        }
        10 if second.count == 12 => {
            state.baseline = scene.people.iter().map(|(e, p, _, _)| (e, p.0)).collect();
            state.action = Some(ArmyAction::Stance(BattalionStance::HoldLine));
            state.stage = 11;
        }
        11 if second.stance == BattalionStance::HoldLine => {
            shot = Some("04-hold-line".into());
            state.stage = 12;
        }
        12 => {
            open.0 = false;
            let mut defensive = Vec::new();
            let mut held = Vec::new();
            let mut damage = [0, 0];
            for (entity, position, health, member) in &scene.people {
                let Some(initial) = state.baseline.get(&entity) else {
                    continue;
                };
                let distance = position.0.xz().distance(initial.xz());
                if member.0 == first.id {
                    defensive.push(distance);
                    damage[0] += usize::from(health.current < health.max);
                } else if member.0 == second.id {
                    held.push(distance);
                    damage[1] += usize::from(health.current < health.max);
                }
            }
            if damage.iter().all(|n| *n > 0) && state.impacted_at.is_none() {
                state.impacted_at = Some(now);
            }
            if now >= state.next_shot {
                shot = Some(format!("motion-{:03}", state.shots));
                state.next_shot = now + 0.7;
            }
            if state.impacted_at.is_some_and(|t| now - t > 8.0) {
                let min_defensive = defensive.iter().copied().fold(f32::INFINITY, f32::min);
                let max_held = held.iter().copied().fold(0.0_f32, f32::max);
                let passed = defensive.len() == 12
                    && held.len() == 12
                    && min_defensive > 8.0
                    && max_held < 0.05;
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(out.join("summary.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":passed,"bulk_remove_refill":true,"transfer_roundtrip":true,"stance_replicated":true,"min_defensive_travel":min_defensive,"max_held_travel":max_held,"damaged_by_battalion":damage,"troops":roster.soldiers.len()})).unwrap()).unwrap();
                assert!(passed,"management stance rehearsal failed: defensive {min_defensive}, held {max_held}");
                state.stage = 13;
                shot = Some("05-repositioned".into());
            }
        }
        13 => {
            info!("Management lab passed: UI membership edits and both bombardment responses");
            state.stage = 14;
            exit.write(AppExit::Success);
        }
        _ => {}
    }
    let Some(name) = shot else {
        return;
    };
    let target = if state.stage < 12 || name == "04-hold-line" {
        CaptureTarget::Window
    } else {
        CaptureTarget::Scene
    };
    let image = if target == CaptureTarget::Window {
        inspection
            .presentation_target
            .as_ref()
            .map(|t| t.image.clone())
    } else {
        inspection.scene_target.as_ref().map(|t| t.image.clone())
    };
    let Some(image) = image else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    let people:Vec<_>=scene.people.iter().map(|(e,p,h,m)|serde_json::json!({"entity":e.to_bits(),"position":p.0.to_array(),"health":h.current,"battalion":m.0.0})).collect();
    std::fs::write(
        out.join(format!("{name}.army.json")),
        serde_json::to_vec_pretty(
            &serde_json::json!({"time":now,"stage":state.stage,"people":people}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut request = live_capture_request(
        out.join(format!("{name}.png")),
        "army-management",
        &name,
        target,
    );
    inspection
        .complete_live_request(&mut request, Some(&camera), Some(clock), state.stable)
        .unwrap();
    state.ticket = Some(request_capture(
        &mut commands,
        Screenshot::image(image),
        request,
        &mut completions,
    ));
    state.shots += 1;
}
