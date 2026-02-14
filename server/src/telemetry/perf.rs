//! Fixed-tick server performance diagnostics.

use bevy::prelude::*;
use shared::components::{Bullet, Npc, Player};
use shared::items::GroundItem;
use shared::protocol::FIXED_TIMESTEP_HZ;
use std::cmp::Ordering;
use std::time::{Duration, Instant};

use crate::net::input::{ClientInputIngressStats, ClientInputs};

const LOG_INTERVAL: Duration = Duration::from_secs(3);
const TARGET_TICK_SECS: f64 = 1.0 / FIXED_TIMESTEP_HZ;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Core,
    NpcInventoryBuild,
    Collision,
    Weapons,
    AiCadence,
    Pathfinding,
    BulletHits,
    WorldHits,
}

impl Phase {
    const COUNT: usize = 8;

    fn idx(self) -> usize {
        match self {
            Phase::Core => 0,
            Phase::NpcInventoryBuild => 1,
            Phase::Collision => 2,
            Phase::Weapons => 3,
            Phase::AiCadence => 4,
            Phase::Pathfinding => 5,
            Phase::BulletHits => 6,
            Phase::WorldHits => 7,
        }
    }
}

#[derive(Resource)]
pub struct ServerPerfMonitor {
    enabled: bool,
    announced: bool,
    last_tick: Option<Instant>,
    active_phase: Option<(Phase, Instant)>,
    window_started_at: Instant,
    ticks: u32,
    tick_sum: Duration,
    tick_max: Duration,
    tick_over_budget: u32,
    phase_sum: [Duration; Phase::COUNT],
    phase_max: [Duration; Phase::COUNT],
}

impl Default for ServerPerfMonitor {
    fn default() -> Self {
        let enabled = std::env::var("FISTFORCE_SERVER_PERF")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                !(v == "0" || v == "false" || v == "off")
            })
            .unwrap_or(true);

        Self {
            enabled,
            announced: false,
            last_tick: None,
            active_phase: None,
            window_started_at: Instant::now(),
            ticks: 0,
            tick_sum: Duration::ZERO,
            tick_max: Duration::ZERO,
            tick_over_budget: 0,
            phase_sum: [Duration::ZERO; Phase::COUNT],
            phase_max: [Duration::ZERO; Phase::COUNT],
        }
    }
}

impl ServerPerfMonitor {
    fn add_phase_duration(&mut self, phase: Phase, elapsed: Duration) {
        let idx = phase.idx();
        self.phase_sum[idx] += elapsed;
        if elapsed > self.phase_max[idx] {
            self.phase_max[idx] = elapsed;
        }
    }

    fn finalize_active_phase(&mut self, now: Instant) {
        let Some((phase, started_at)) = self.active_phase.take() else {
            return;
        };
        let elapsed = now.saturating_duration_since(started_at);
        self.add_phase_duration(phase, elapsed);
    }

    fn start_phase(&mut self, phase: Phase) {
        let now = Instant::now();
        self.finalize_active_phase(now);
        self.active_phase = Some((phase, now));
    }

    fn end_phase(&mut self, phase: Phase) {
        let Some((active_phase, started_at)) = self.active_phase else {
            return;
        };
        if active_phase != phase {
            return;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(started_at);
        self.add_phase_duration(phase, elapsed);
        self.active_phase = None;
    }

    fn reset_window(&mut self) {
        self.window_started_at = Instant::now();
        self.ticks = 0;
        self.tick_sum = Duration::ZERO;
        self.tick_max = Duration::ZERO;
        self.tick_over_budget = 0;
        self.phase_sum = [Duration::ZERO; Phase::COUNT];
        self.phase_max = [Duration::ZERO; Phase::COUNT];
    }

    pub fn record_collision_ms(&mut self, ms: f32) {
        if !self.enabled || ms <= 0.0 {
            return;
        }
        self.add_phase_duration(
            Phase::Collision,
            Duration::from_secs_f64(ms as f64 / 1000.0),
        );
    }

    pub fn record_ai_cadence_ms(&mut self, ms: f32) {
        if !self.enabled || ms <= 0.0 {
            return;
        }
        self.add_phase_duration(
            Phase::AiCadence,
            Duration::from_secs_f64(ms as f64 / 1000.0),
        );
    }

    pub fn record_pathfinding_ms(&mut self, ms: f32) {
        if !self.enabled || ms <= 0.0 {
            return;
        }
        self.add_phase_duration(
            Phase::Pathfinding,
            Duration::from_secs_f64(ms as f64 / 1000.0),
        );
    }

    pub fn record_bullet_hits_ms(&mut self, ms: f32) {
        if !self.enabled || ms <= 0.0 {
            return;
        }
        self.add_phase_duration(
            Phase::BulletHits,
            Duration::from_secs_f64(ms as f64 / 1000.0),
        );
    }

    pub fn record_world_hits_ms(&mut self, ms: f32) {
        if !self.enabled || ms <= 0.0 {
            return;
        }
        self.add_phase_duration(
            Phase::WorldHits,
            Duration::from_secs_f64(ms as f64 / 1000.0),
        );
    }
}

pub fn handle_perf_tick_begin(mut perf: ResMut<ServerPerfMonitor>) {
    if !perf.enabled {
        return;
    }

    if !perf.announced {
        info!("Server perf monitor enabled (set FISTFORCE_SERVER_PERF=0 to disable)");
        perf.announced = true;
    }

    let now = Instant::now();
    if let Some(last_tick) = perf.last_tick {
        let dt = now.saturating_duration_since(last_tick);
        perf.ticks = perf.ticks.saturating_add(1);
        perf.tick_sum += dt;
        if dt > perf.tick_max {
            perf.tick_max = dt;
        }
        if dt.as_secs_f64() > TARGET_TICK_SECS * 1.20 {
            perf.tick_over_budget = perf.tick_over_budget.saturating_add(1);
        }
    }
    perf.last_tick = Some(now);
}

pub fn handle_perf_core_phase_begin(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.start_phase(Phase::Core);
    }
}

pub fn handle_perf_core_phase_end(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.end_phase(Phase::Core);
    }
}

pub fn handle_perf_npc_inventory_build_phase_begin(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.start_phase(Phase::NpcInventoryBuild);
    }
}

pub fn handle_perf_npc_inventory_build_phase_end(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.end_phase(Phase::NpcInventoryBuild);
    }
}

pub fn handle_perf_weapons_phase_begin(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.start_phase(Phase::Weapons);
    }
}

pub fn handle_perf_weapons_phase_end(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.end_phase(Phase::Weapons);
    }
}

pub fn update_server_perf_log(
    mut perf: ResMut<ServerPerfMonitor>,
    players: Query<&Player>,
    npcs: Query<(), With<Npc>>,
    bullets: Query<(), With<Bullet>>,
    ground_items: Query<(), With<GroundItem>>,
    client_inputs: Res<ClientInputs>,
    mut input_ingress: ResMut<ClientInputIngressStats>,
) {
    if !perf.enabled {
        return;
    }

    let now = Instant::now();
    perf.finalize_active_phase(now);

    if now.saturating_duration_since(perf.window_started_at) < LOG_INTERVAL || perf.ticks == 0 {
        return;
    }

    let ticks_f = perf.ticks as f64;
    let tick_avg_ms = perf.tick_sum.as_secs_f64() * 1000.0 / ticks_f;
    let tick_max_ms = perf.tick_max.as_secs_f64() * 1000.0;
    let over_budget_pct = perf.tick_over_budget as f64 * 100.0 / ticks_f;

    let core_avg_ms = perf.phase_sum[Phase::Core.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let core_max_ms = perf.phase_max[Phase::Core.idx()].as_secs_f64() * 1000.0;
    let npc_avg_ms =
        perf.phase_sum[Phase::NpcInventoryBuild.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let npc_max_ms = perf.phase_max[Phase::NpcInventoryBuild.idx()].as_secs_f64() * 1000.0;
    let collision_avg_ms = perf.phase_sum[Phase::Collision.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let collision_max_ms = perf.phase_max[Phase::Collision.idx()].as_secs_f64() * 1000.0;
    let weapons_avg_ms = perf.phase_sum[Phase::Weapons.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let weapons_max_ms = perf.phase_max[Phase::Weapons.idx()].as_secs_f64() * 1000.0;
    let ai_cadence_avg_ms = perf.phase_sum[Phase::AiCadence.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let ai_cadence_max_ms = perf.phase_max[Phase::AiCadence.idx()].as_secs_f64() * 1000.0;
    let pathfinding_avg_ms =
        perf.phase_sum[Phase::Pathfinding.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let pathfinding_max_ms = perf.phase_max[Phase::Pathfinding.idx()].as_secs_f64() * 1000.0;
    let bullet_hits_avg_ms =
        perf.phase_sum[Phase::BulletHits.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let bullet_hits_max_ms = perf.phase_max[Phase::BulletHits.idx()].as_secs_f64() * 1000.0;
    let world_hits_avg_ms = perf.phase_sum[Phase::WorldHits.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let world_hits_max_ms = perf.phase_max[Phase::WorldHits.idx()].as_secs_f64() * 1000.0;

    let mut players_count = 0usize;
    let mut missing_input_players = 0usize;
    for player in players.iter() {
        players_count += 1;
        if !client_inputs.latest.contains_key(&player.client_id) {
            missing_input_players += 1;
        }
    }

    let window_secs = now
        .saturating_duration_since(perf.window_started_at)
        .as_secs_f64()
        .max(0.001);
    let input_ingress_total_per_sec = input_ingress.total_messages as f64 / window_secs;

    let mut per_client_input_rates: Vec<(String, f64)> = input_ingress
        .messages_by_client
        .iter()
        .map(|(peer, count)| (format!("{peer:?}"), *count as f64 / window_secs))
        .collect();
    per_client_input_rates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

    let per_client_input_summary = if per_client_input_rates.is_empty() {
        "none".to_string()
    } else {
        let max_clients = 8usize;
        let mut out = per_client_input_rates
            .iter()
            .take(max_clients)
            .map(|(peer, msgs_per_sec)| format!("{peer}:{msgs_per_sec:.1}/s"))
            .collect::<Vec<_>>()
            .join(", ");
        if per_client_input_rates.len() > max_clients {
            out.push_str(&format!(
                " (+{} more)",
                per_client_input_rates.len() - max_clients
            ));
        }
        out
    };

    info!(
        "ServerPerf tick avg={:.2}ms max={:.2}ms over_20%={:.1}% | phases core={:.2}/{:.2} npc={:.2}/{:.2} collision={:.2}/{:.2} weapons={:.2}/{:.2} ai_cadence={:.3}/{:.3} pathfinding={:.3}/{:.3} bullet_hits={:.3}/{:.3} world_hits={:.3}/{:.3} ms | inputs buffered={} missing_for_players={} ingress={:.1}/s per_client=[{}] | entities players={} npcs={} bullets={} ground_items={}",
        tick_avg_ms,
        tick_max_ms,
        over_budget_pct,
        core_avg_ms,
        core_max_ms,
        npc_avg_ms,
        npc_max_ms,
        collision_avg_ms,
        collision_max_ms,
        weapons_avg_ms,
        weapons_max_ms,
        ai_cadence_avg_ms,
        ai_cadence_max_ms,
        pathfinding_avg_ms,
        pathfinding_max_ms,
        bullet_hits_avg_ms,
        bullet_hits_max_ms,
        world_hits_avg_ms,
        world_hits_max_ms,
        client_inputs.latest.len(),
        missing_input_players,
        input_ingress_total_per_sec,
        per_client_input_summary,
        players_count,
        npcs.iter().count(),
        bullets.iter().count(),
        ground_items.iter().count(),
    );

    input_ingress.reset_window();
    perf.reset_window();
}
