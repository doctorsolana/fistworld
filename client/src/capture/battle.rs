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
    spent_arrows: std::collections::BTreeMap<u64, u8>,
    peak_projectiles: usize,
    peak_archers_drawing: usize,
    archers_in_melee: BTreeSet<u64>,
    independent_engaged: BTreeSet<u64>,
    raider_wave: u8,
    accepted_attack: bool,
    selection_verified: bool,
    retargeted: bool,
    metrics_only: bool,
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
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct BattleVisuals<'w, 's> {
    dressed: Query<'w, 's, (), With<crate::hero::HeroDressed>>,
    bows: Query<'w, 's, (), With<crate::hero::BowDressed>>,
    arrows: Query<'w, 's, &'static ArrowProjectile>,
    archery: crate::hero::ArcheryInspection<'w, 's>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn drive_battle_capture(
    mut commands: Commands,
    mut state: ResMut<BattleCapture>,
    roster: Res<ArmyRoster>,
    input: Res<crate::input::InputState>,
    notice: Res<crate::ui::hud::GodNotice>,
    visuals: BattleVisuals,
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
            Option<&SoldierRole>,
            Option<&Quiver>,
            Option<&BowShot>,
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
        state.metrics_only = crate::profiling::env_flag("FISTWORLD_BATTLE_METRICS_ONLY");
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
            assert_eq!(
                selection.len(),
                scenario.total(),
                "a member must select their entire battalion"
            );
            state.selection_verified = true;
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
            || visuals.dressed.iter().count() < total
            || people.iter().count() != total
            || roster.soldiers.len() != scenario.total() + battle.independent_attackers
            || (!scenario.archer_battalions.is_empty() && visuals.bows.is_empty())
            || state.stable < 30
        {
            return;
        }
        // Start from one ordinary member per block; production selection must
        // expand these before the actual enemy click is allowed to run.
        selection.set(
            roster
                .battalions
                .iter()
                .filter_map(|b| b.members.get(1).copied())
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
    for (id, _, _, _, _, _, _, swing, _, role, quiver, _) in &people {
        if role == Some(&SoldierRole::Archer) {
            if let Some(q) = quiver {
                let spent = QUIVER_CAPACITY.saturating_sub(q.arrows);
                let recorded = state.spent_arrows.entry(id.0).or_default();
                *recorded = (*recorded).max(spent);
            }
            if swing.is_some_and(|s| s.impact_at <= now) {
                state.archers_in_melee.insert(id.0);
            }
        }
    }
    state.peak_projectiles = state.peak_projectiles.max(
        visuals
            .arrows
            .iter()
            .filter(|a| a.stopped_at.is_none())
            .count(),
    );
    state.peak_archers_drawing = state
        .peak_archers_drawing
        .max(visuals.archery.active_draws());
    let released_arrows: usize = state.spent_arrows.values().map(|n| usize::from(*n)).sum();
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
    if !state.retargeted
        && battle.retarget_after_seconds.is_some_and(|after| {
            state
                .contact_at
                .is_some_and(|at| now - at > f64::from(after))
        })
    {
        if let Some(target) = people
            .iter()
            .filter(|(_, _, _, h, owner, ..)| owner.0 != scenario.account && !h.is_dead())
            .max_by(|a, b| a.1 .0.x.total_cmp(&b.1 .0.x))
        {
            selection.set(
                roster
                    .battalions
                    .iter()
                    .flat_map(|b| b.members.iter().copied())
                    .collect(),
            );
            state.cursor = target.1 .0.xz();
            state.input = 1;
            state.retargeted = true;
            info!("Battle lab: reissuing army attack during contact");
        }
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
        let passed = state.selection_verified
            && state.accepted_attack
            && state.independent_engaged.len() >= battle.independent_attackers
            && state.engaged.len() >= battle.minimum_engaged_battalions
            && state.max_casualties >= 5
            && (scenario.archer_battalions.is_empty()
                || (released_arrows
                    >= scenario.archer_battalions.len() * scenario.soldiers_per_battalion
                    && state.peak_projectiles > 0
                    && state.peak_archers_drawing > 0))
            && (scenario.archer_battalions.len() != scenario.battalions
                || scenario.counterattack_after_seconds.is_none()
                || !state.archers_in_melee.is_empty())
            && state.shots >= 10
            && (battle.retarget_after_seconds.is_none() || state.retargeted);
        std::fs::write(out.join("summary.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":passed,"metrics_only":state.metrics_only,"retargeted":state.retargeted,"selection_verified":state.selection_verified,"accepted_attack":state.accepted_attack,"independent_attackers_engaged":state.independent_engaged,"attacking_battalions_engaged":state.engaged,"peak_contacts":state.max_contacts,"arrows_released":released_arrows,"peak_projectiles":state.peak_projectiles,"peak_archers_drawing":state.peak_archers_drawing,"archers_in_melee":state.archers_in_melee,"casualties":state.max_casualties,"samples":state.shots,"shots":if state.metrics_only {1} else {state.shots},"observed_world_seconds":now-state.contact_at.unwrap()})).unwrap()).unwrap();
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
    let data:Vec<_>=people.iter().map(|(id,p,r,h,o,b,e,s,hit,role,quiver,bow)|serde_json::json!({"person":id.0,"position":p.0.to_array(),"yaw":r.0,"health":h.current,"owner":o.0,"battalion":b.map(|b|b.0.0),"target":e.map(|e|e.0.0),"impact_at":s.map(|s|s.impact_at),"reaction":hit.map(|h|(h.at,h.fatal)),"role":role,"arrows":quiver.map(|q|q.arrows),"bow_release_at":bow.map(|b|b.release_at)})).collect();
    std::fs::create_dir_all(&out).expect("battle output");
    let name = format!("{:04}", state.shots);
    std::fs::write(out.join(format!("{name}.battle.json")),serde_json::to_vec_pretty(&serde_json::json!({"world_seconds":now,"since_order":now-state.order_at,"contacts":contacts,"alive":alive,"people":data,"archery_visuals":visuals.archery.snapshot(),"arrows":visuals.arrows.iter().map(|a|serde_json::json!({"position":a.position(now).to_array(),"launched_at":a.launched_at,"stopped_at":a.stopped_at})).collect::<Vec<_>>()})).unwrap()).unwrap();
    if state.metrics_only && state.stage >= 2 {
        // Keep the initial readiness/selection screenshot, then measure the
        // ordinary renderer without repeated GPU readback and PNG encoding.
        // Sparse position samples still tie timing logs to the active battle.
        state.shots += 1;
        state.next_shot = now + 1.0;
        return;
    }
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
