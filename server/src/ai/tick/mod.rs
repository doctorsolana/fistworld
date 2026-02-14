//! NPC runtime behavior systems.

mod state_steps;

use bevy::prelude::*;
use shared::components::{
    Health, Npc, NpcDamageEvent, NpcFleeing, NpcPosition, NpcRotation, Player,
};
use shared::npc::{
    NPC_IDLE_TIME_MAX, NPC_IDLE_TIME_MIN, NPC_MIN_TARGET_DIST, NPC_MOVE_SPEED, NPC_TURN_SPEED,
    NPC_WANDER_RADIUS,
};
use shared::physics::ground_clearance_center;
use shared::protocol::FIXED_TIMESTEP_HZ;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;
use std::time::Instant;

use crate::ai::pathfinding::PathfindingScratch;
use crate::ai::state::{
    NpcAiPerfAccumulator, NpcState, NpcWander, NPC_AI_BACKGROUND_CADENCE, NPC_AI_FAR_CADENCE,
    NPC_AI_FAR_RADIUS, NPC_AI_MID_CADENCE, NPC_AI_MID_RADIUS, NPC_AI_NEAR_RADIUS,
    NPC_AI_PERF_LOG_SECS, OILMAN_WALK_SPEED_SCALE,
};
use crate::player::spatial::PlayerSpatialIndex;
use state_steps::{tick_fleeing_state, tick_idle_state, tick_walking_state};

/// Damage events currently push NPCs into a flee loop.
pub fn handle_npc_damage_events(
    mut commands: Commands,
    mut npcs: Query<(Entity, &Npc, &mut NpcWander, &NpcDamageEvent)>,
) {
    for (entity, _npc, mut wander, damage_event) in npcs.iter_mut() {
        // Randomize flee parameters.
        let flee_duration = 5.0 + wander.rng.next_f32() * 3.0; // 5-8 seconds
        let panic_boost = 1.5 + wander.rng.next_f32() * 0.3; // 1.5-1.8x speed

        wander.state = NpcState::Fleeing {
            from_position: damage_event.damage_source_position,
            flee_timer: flee_duration,
            panic_speed_boost: panic_boost,
        };

        // Clear current path so flee logic takes over immediately.
        wander.path.clear();
        wander.waypoint = 0;

        let mut entity_cmd = commands.entity(entity);
        entity_cmd.insert(NpcFleeing);
        entity_cmd.remove::<NpcDamageEvent>();
    }
}

/// Tick NPC wander/flee AI (server-authoritative).
pub fn update_npc_ai(
    mut commands: Commands,
    time: Res<Time>,
    terrain: Res<WorldTerrain>,
    obstacle_grid: Res<SpatialObstacleGrid>,
    player_spatial: Res<PlayerSpatialIndex>,
    mut server_perf: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    mut ai_tick: Local<u64>,
    mut perf: Local<NpcAiPerfAccumulator>,
    mut pathfinding_scratch: Local<PathfindingScratch>,
    mut npcs: Query<
        (
            Entity,
            &Npc,
            &mut NpcPosition,
            &mut NpcRotation,
            &Health,
            &mut NpcWander,
            Option<&NpcFleeing>,
        ),
        Without<Player>,
    >,
) {
    let frame_start = Instant::now();
    *ai_tick = ai_tick.wrapping_add(1);
    let ai_tick_value = *ai_tick;
    let base_dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    let near_sq = NPC_AI_NEAR_RADIUS * NPC_AI_NEAR_RADIUS;
    let mid_sq = NPC_AI_MID_RADIUS * NPC_AI_MID_RADIUS;
    let far_sq = NPC_AI_FAR_RADIUS * NPC_AI_FAR_RADIUS;
    let alive_players = player_spatial.alive_count();

    let mut total_npcs = 0u64;
    let mut updated_npcs = 0u64;
    let mut throttled_npcs = 0u64;
    let mut cadence_eval_ms = 0.0f32;
    let mut pathfinding_ms = 0.0f32;

    for (entity, npc, mut pos, mut rot, health, mut wander, has_fleeing) in npcs.iter_mut() {
        total_npcs = total_npcs.saturating_add(1);

        if health.is_dead() {
            wander.path.clear();
            wander.waypoint = 0;
            if has_fleeing.is_some() {
                commands.entity(entity).remove::<NpcFleeing>();
            }
            continue;
        }

        let cadence = if alive_players == 0 {
            NPC_AI_BACKGROUND_CADENCE
        } else {
            let cadence_eval_start = Instant::now();
            let min_dist_sq = player_spatial
                .nearest_alive_distance_sq(pos.0, NPC_AI_FAR_RADIUS)
                .unwrap_or(f32::INFINITY);
            cadence_eval_ms += cadence_eval_start.elapsed().as_secs_f32() * 1000.0;

            if min_dist_sq <= near_sq {
                1
            } else if min_dist_sq <= mid_sq {
                NPC_AI_MID_CADENCE
            } else if min_dist_sq <= far_sq {
                NPC_AI_FAR_CADENCE
            } else {
                NPC_AI_BACKGROUND_CADENCE
            }
        };

        if cadence > 1 {
            let phase = npc.id % cadence;
            if ai_tick_value.wrapping_add(phase) % cadence != 0 {
                throttled_npcs = throttled_npcs.saturating_add(1);
                continue;
            }
        }

        updated_npcs = updated_npcs.saturating_add(1);
        let dt = base_dt * cadence as f32;
        match &wander.state {
            NpcState::Fleeing {
                from_position,
                flee_timer,
                panic_speed_boost,
            } => {
                let from_pos = *from_position;
                let timer = *flee_timer;
                let boost = *panic_speed_boost;
                tick_fleeing_state(
                    &mut wander,
                    &mut pos,
                    &mut rot,
                    &terrain,
                    &obstacle_grid,
                    from_pos,
                    timer,
                    boost,
                    dt,
                    npc.id,
                    &mut pathfinding_scratch,
                    &mut pathfinding_ms,
                );
            }
            NpcState::Idle => {
                tick_idle_state(
                    &mut wander,
                    &mut rot,
                    &terrain,
                    &obstacle_grid,
                    pos.0,
                    dt,
                    npc.id,
                    &mut pathfinding_scratch,
                    &mut pathfinding_ms,
                );
            }
            NpcState::Walking => {
                let base_walk_speed = NPC_MOVE_SPEED * OILMAN_WALK_SPEED_SCALE;
                tick_walking_state(
                    &mut wander,
                    &mut pos,
                    &mut rot,
                    &terrain,
                    dt,
                    npc.id,
                    base_walk_speed,
                );
            }
        }

        let is_fleeing = matches!(&wander.state, NpcState::Fleeing { .. });
        match (is_fleeing, has_fleeing.is_some()) {
            (true, false) => {
                commands.entity(entity).insert(NpcFleeing);
            }
            (false, true) => {
                commands.entity(entity).remove::<NpcFleeing>();
            }
            _ => {}
        }
    }

    let frame_ms = frame_start.elapsed().as_secs_f64() as f32 * 1000.0;
    perf.elapsed_secs += time.delta_secs();
    perf.samples = perf.samples.saturating_add(1);
    perf.total_npcs = perf.total_npcs.saturating_add(total_npcs);
    perf.updated_npcs = perf.updated_npcs.saturating_add(updated_npcs);
    perf.throttled_npcs = perf.throttled_npcs.saturating_add(throttled_npcs);
    perf.total_tick_ms += frame_ms;
    perf.peak_tick_ms = perf.peak_tick_ms.max(frame_ms);
    perf.total_cadence_eval_ms += cadence_eval_ms;
    perf.total_pathfinding_ms += pathfinding_ms;

    if let Some(perf_monitor) = server_perf.as_deref_mut() {
        perf_monitor.record_ai_cadence_ms(cadence_eval_ms);
        perf_monitor.record_pathfinding_ms(pathfinding_ms);
    }

    if perf.elapsed_secs >= NPC_AI_PERF_LOG_SECS {
        let samples = perf.samples.max(1) as f32;
        let avg_npcs = perf.total_npcs as f32 / samples;
        let avg_updated = perf.updated_npcs as f32 / samples;
        let avg_throttled = perf.throttled_npcs as f32 / samples;
        let avg_tick_ms = perf.total_tick_ms / samples;
        let avg_cadence_eval_ms = perf.total_cadence_eval_ms / samples;
        let avg_pathfinding_ms = perf.total_pathfinding_ms / samples;
        info!(
            "NPC AI perf: alive_players={}, avg_npcs={:.1}, avg_updated={:.1}, avg_throttled={:.1}, avg_tick_ms={:.2}, avg_cadence_eval_ms={:.3}, avg_pathfinding_ms={:.3}, peak_tick_ms={:.2}",
            alive_players,
            avg_npcs,
            avg_updated,
            avg_throttled,
            avg_tick_ms,
            avg_cadence_eval_ms,
            avg_pathfinding_ms,
            perf.peak_tick_ms
        );
        *perf = NpcAiPerfAccumulator::default();
    }
}
