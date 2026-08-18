//! Fixed-tick server performance diagnostics.

use bevy::prelude::*;
use shared::components::{CharacterKind, Player};
use shared::protocol::FIXED_TIMESTEP_HZ;
use std::cmp::Ordering;
use std::time::{Duration, Instant};

use crate::net::input::{ClientInputIngressStats, ClientInputs};
use crate::world::village::{MigrationCooldown, VillagerIntent};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending};

const LOG_INTERVAL: Duration = Duration::from_secs(3);
const TARGET_TICK_SECS: f64 = 1.0 / FIXED_TIMESTEP_HZ;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Core,
    Navigation,
}

impl Phase {
    const COUNT: usize = 2;

    fn idx(self) -> usize {
        match self {
            Phase::Core => 0,
            Phase::Navigation => 1,
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

pub fn handle_perf_navigation_phase_begin(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.start_phase(Phase::Navigation);
    }
}

pub fn handle_perf_navigation_phase_end(mut perf: ResMut<ServerPerfMonitor>) {
    if perf.enabled {
        perf.end_phase(Phase::Navigation);
    }
}

pub fn update_server_perf_log(
    mut perf: ResMut<ServerPerfMonitor>,
    players: Query<&Player>,
    villagers: Query<(
        &CharacterKind,
        Option<&VillagerIntent>,
        Has<NavigationRoutePending>,
        Has<NavigationRouteFailed>,
        Has<MigrationCooldown>,
    )>,
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
    let navigation_avg_ms =
        perf.phase_sum[Phase::Navigation.idx()].as_secs_f64() * 1000.0 / ticks_f;
    let navigation_max_ms = perf.phase_max[Phase::Navigation.idx()].as_secs_f64() * 1000.0;
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

    let mut villager_total = 0usize;
    let mut villager_idle = 0usize;
    let mut villager_migrating = 0usize;
    let mut villager_settled = 0usize;
    let mut villager_nav_pending = 0usize;
    let mut villager_nav_failed = 0usize;
    let mut villager_migration_cooldown = 0usize;
    for (kind, intent, pending, failed, cooldown) in villagers.iter() {
        if *kind != CharacterKind::Villager {
            continue;
        }
        villager_total += 1;
        match intent {
            Some(VillagerIntent::Idle) | None => villager_idle += 1,
            Some(VillagerIntent::Travelling { .. }) => villager_migrating += 1,
            Some(
                VillagerIntent::Resident { .. }
                | VillagerIntent::Building { .. }
                | VillagerIntent::RoadBuilding { .. },
            ) => villager_settled += 1,
        }
        villager_nav_pending += usize::from(pending);
        villager_nav_failed += usize::from(failed);
        villager_migration_cooldown += usize::from(cooldown);
    }

    info!(
        "ServerPerf tick avg={:.2}ms max={:.2}ms over_20%={:.1}% | phases core={:.2}/{:.2} navigation={:.2}/{:.2} ms | inputs buffered={} missing_for_players={} ingress={:.1}/s per_client=[{}] | entities players={} villagers={} idle={} migrating={} settled={} nav_pending={} nav_failed={} migration_cooldown={}",
        tick_avg_ms,
        tick_max_ms,
        over_budget_pct,
        core_avg_ms,
        core_max_ms,
        navigation_avg_ms,
        navigation_max_ms,
        client_inputs.latest.len(),
        missing_input_players,
        input_ingress_total_per_sec,
        per_client_input_summary,
        players_count,
        villager_total,
        villager_idle,
        villager_migrating,
        villager_settled,
        villager_nav_pending,
        villager_nav_failed,
        villager_migration_cooldown,
    );

    input_ingress.reset_window();
    perf.reset_window();
}
