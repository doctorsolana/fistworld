//! Continuous, connected clash inspection. The only injected state is selection
//! and ordinary mouse input; soldiers, damage and movement come from the server.
use super::{inspection::CaptureInspection, live::live_capture_request};
use crate::{
    army_roster::ArmyRoster,
    camera_rts::{CommanderCamera, CursorTerrainOverride},
    capture_artifact::{request_capture, CaptureCompletions, CaptureTarget},
    selection::Selection,
};
use bevy::{prelude::*, render::view::screenshot::Screenshot};
use shared::{army_lab::ArmyLabScenario, components::*};
use std::{collections::BTreeSet, path::PathBuf, time::Instant};
#[derive(Resource, Default)]
pub(crate) struct BattleCapture {
    initialized: bool,
    scenario: Option<ArmyLabScenario>,
    started: Option<Instant>,
    stage: u8,
    input: u8,
    cursor: Vec2,
    ticket: Option<u64>,
    stable: u32,
    chunks: usize,
    order_at: f64,
    contact_at: Option<f64>,
    next_shot: f64,
    shots: usize,
    engaged: BTreeSet<u64>,
    max_contacts: usize,
    max_casualties: usize,
    independent_engaged: BTreeSet<u64>,
    raider_wave: u8,
    accepted_attack: bool,
}
pub(crate) fn drive_battle_input(
    mut commands: Commands,
    mut state: ResMut<BattleCapture>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window>,
) {
    if state.input == 0 || state.input > 3 {
        return;
    }
    if let Ok(mut w) = windows.single_mut() {
        let middle = w.size() * 0.5;
        w.set_cursor_position(Some(middle));
    }
    match state.input {
        1 => {
            commands.insert_resource(CursorTerrainOverride(state.cursor));
        }
        2 => mouse.press(MouseButton::Right),
        3 => {
            mouse.release(MouseButton::Right);
            info!("Battle lab: normal attack click released");
        }
        _ => {}
    }
    state.input += 1;
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn drive_battle_capture(
    mut commands: Commands,
    mut state: ResMut<BattleCapture>,
    roster: Res<ArmyRoster>,
    input: Res<crate::input::InputState>,
    notice: Res<crate::ui::hud::GodNotice>,
    dressed: Query<(), With<crate::hero::HeroDressed>>,
    mut selection: ResMut<Selection>,
    mut mode: ResMut<crate::combat_mode::CombatMode>,
    mut cameras: Query<&mut CommanderCamera>,
    terrain: Res<shared::terrain::WorldTerrain>,
    clocks: Query<&WorldTime>,
    people: Query<
        (
            &PersonId,
            &PlayerPosition,
            &PlayerRotation,
            &Health,
            &CommandedBy,
            Option<&MemberOfBattalion>,
            Option<&EngagedWith>,
            Option<&CombatSwing>,
            Option<&CombatReaction>,
        ),
        With<CharacterKind>,
    >,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
) {
    if !state.initialized {
        state.initialized = true;
        state.scenario =
            ArmyLabScenario::from_env().filter(|s| s.catapult.is_none() && s.battle.is_some());
        state.started = Some(Instant::now());
    }
    let Some(scenario) = state.scenario.clone() else {
        return;
    };
    if state.stage == 4 {
        return;
    }
    let battle = scenario.battle.as_ref().unwrap();
    let total = scenario.total()
        + battle.defender_battalions * battle.defenders_per_battalion
        + battle.independent_attackers;
    state.accepted_attack |= notice.text.starts_with("Attacking with");
    let out = PathBuf::from(
        std::env::var("FISTWORLD_ARMY_CAPTURE_DIR")
            .unwrap_or_else(|_| "/tmp/fistworld-battle".into()),
    );
    if state.started.unwrap().elapsed().as_secs_f32() > scenario.timeout_seconds {
        error!(
            "Battle lab timed out: stage={}, engaged={:?}, shots={}",
            state.stage, state.engaged, state.shots
        );
        exit.write(AppExit::error());
        state.stage = 4;
        return;
    }
    if let Some(ticket) = state.ticket {
        let Some(done) = completions.take(ticket) else {
            return;
        };
        if let Some(error) = done.error {
            panic!("battle capture: {error}");
        }
        state.ticket = None;
        if state.stage == 1 {
            state.input = 1;
            state.stage = 2;
        }
    }
    let Some(clock) = clocks.iter().next() else {
        return;
    };
    let now = f64::from(clock.day) * f64::from(clock.cycle_duration())
        + f64::from(clock.seconds_in_cycle);
    let Ok(mut camera) = cameras.single_mut() else {
        return;
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
    let world = inspection.world_snapshot(Some(clock));
    if world.loaded_chunks == state.chunks && state.chunks > 0 {
        state.stable += 1;
    } else {
        state.chunks = world.loaded_chunks;
        state.stable = 0;
    }
    if state.stage == 0 {
        if input.ui_blocking()
            || dressed.iter().count() < total
            || people.iter().count() != total
            || roster.soldiers.len() != scenario.total() + battle.independent_attackers
            || state.stable < 30
        {
            return;
        }
        selection.set(
            roster
                .soldiers
                .values()
                .filter(|s| s.battalion.is_some())
                .map(|s| s.entity)
                .collect(),
        );
        let target = people
            .iter()
            .filter(|(_, _, _, h, o, ..)| o.0 != scenario.account && !h.is_dead())
            .min_by(|a, b| {
                a.1 .0
                    .distance_squared(focus)
                    .total_cmp(&b.1 .0.distance_squared(focus))
            })
            .unwrap();
        state.cursor = target.1 .0.xz();
        state.order_at = now;
        state.next_shot = now;
        state.stage = 1;
        info!(
            "Battle lab ready: {} vs {} people",
            scenario.total(),
            total - scenario.total()
        );
    }
    let mut contacts = 0;
    let mut alive = 0;
    for (person, _, _, health, owner, bat, target, ..) in &people {
        if health.is_dead() {
            continue;
        }
        alive += 1;
        if target.is_some() {
            contacts += 1;
            if owner.0 == scenario.account {
                if let Some(bat) = bat {
                    state.engaged.insert(bat.0 .0);
                } else {
                    state.independent_engaged.insert(person.0);
                }
            }
        }
    }
    state.max_contacts = state.max_contacts.max(contacts);
    state.max_casualties = state.max_casualties.max(total.saturating_sub(alive));
    if contacts > 0 && state.contact_at.is_none() {
        state.contact_at = Some(now);
        state.next_shot = now;
        info!("Battle lab: first contact");
    }
    if battle.independent_attackers > 0
        && state.raider_wave < 2
        && state
            .contact_at
            .is_some_and(|t| now - t > 6.0 + f64::from(state.raider_wave) * 4.0)
    {
        let mut raiders: Vec<_> = roster
            .soldiers
            .values()
            .filter(|s| s.battalion.is_none())
            .collect();
        raiders.sort_by_key(|s| s.identity);
        let chosen: Vec<_> = if state.raider_wave == 0 {
            raiders.iter().take(1).map(|s| s.entity).collect()
        } else {
            raiders.iter().skip(1).map(|s| s.entity).collect()
        };
        if !chosen.is_empty() {
            if let Some(target) = people
                .iter()
                .filter(|(_, _, _, h, o, ..)| o.0 != scenario.account && !h.is_dead())
                .min_by(|a, b| {
                    a.1 .0
                        .distance_squared(focus)
                        .total_cmp(&b.1 .0.distance_squared(focus))
                })
            {
                selection.set(chosen);
                state.cursor = target.1 .0.xz();
                state.input = 1;
            }
        }
        state.raider_wave += 1;
    }
    if state.stage >= 2
        && state
            .contact_at
            .is_some_and(|at| now - at >= f64::from(battle.observe_seconds))
    {
        let passed = state.accepted_attack
            && state.independent_engaged.len() >= battle.independent_attackers
            && state.engaged.len() >= battle.minimum_engaged_battalions
            && state.max_casualties >= 5
            && state.shots >= 10;
        std::fs::write(out.join("summary.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":passed,"accepted_attack":state.accepted_attack,"independent_attackers_engaged":state.independent_engaged,"attacking_battalions_engaged":state.engaged,"peak_contacts":state.max_contacts,"casualties":state.max_casualties,"shots":state.shots,"observed_world_seconds":now-state.contact_at.unwrap()})).unwrap()).unwrap();
        info!(
            "Battle lab {}: engaged={:?}, casualties={}, shots={}",
            if passed { "PASS" } else { "FAIL" },
            state.engaged,
            state.max_casualties,
            state.shots
        );
        exit.write(if passed {
            AppExit::Success
        } else {
            AppExit::error()
        });
        state.stage = 4;
        return;
    }
    if state.ticket.is_some() || now < state.next_shot {
        return;
    }
    let Some(scene) = inspection.scene_target.as_ref() else {
        return;
    };
    let data:Vec<_>=people.iter().map(|(id,p,r,h,o,b,e,s,hit)|serde_json::json!({"person":id.0,"position":p.0.to_array(),"yaw":r.0,"health":h.current,"owner":o.0,"battalion":b.map(|b|b.0.0),"target":e.map(|e|e.0.0),"impact_at":s.map(|s|s.impact_at),"reaction":hit.map(|h|(h.at,h.fatal))})).collect();
    std::fs::create_dir_all(&out).expect("battle output");
    let name = format!("{:04}", state.shots);
    std::fs::write(out.join(format!("{name}.battle.json")),serde_json::to_vec_pretty(&serde_json::json!({"world_seconds":now,"since_order":now-state.order_at,"contacts":contacts,"alive":alive,"people":data})).unwrap()).unwrap();
    let mut request = live_capture_request(
        out.join(format!("{name}.png")),
        "connected-battalion-clash",
        &name,
        CaptureTarget::Scene,
    );
    inspection
        .complete_live_request(&mut request, Some(&camera), Some(clock), state.stable)
        .expect("battle metadata");
    state.ticket = Some(request_capture(
        &mut commands,
        Screenshot::image(scene.image.clone()),
        request,
        &mut completions,
    ));
    state.shots += 1;
    state.next_shot = now
        + if state.contact_at.is_some() {
            0.25
        } else {
            2.0
        };
}

/// Ground-only cursor fixtures deliberately have no ray. A combat click must
/// exercise the normal person picker as well, so supply its matching vertical
/// ray after terrain probing and before the ordinary selection/order systems.
pub(crate) fn drive_battle_ray(
    state: Res<BattleCapture>,
    terrain: Res<shared::terrain::WorldTerrain>,
    mut ray: ResMut<crate::camera_rts::CursorRay>,
) {
    if state.input == 0 || state.stage >= 4 {
        return;
    }
    let p = state.cursor;
    ray.0 = Some(Ray3d::new(
        Vec3::new(p.x, terrain.get_height(p.x, p.y) + 20.0, p.y),
        Dir3::NEG_Y,
    ));
}
