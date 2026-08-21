//! Per-panel UI work telemetry behind `FISTFORCE_CLIENT_PERF`.
//!
//! Bevy UI panels in this game are rebuilt by despawning and respawning their
//! rows, which is cheap for a handful of rows and ruinous for a thousand. The
//! frame-time percentiles alone cannot say WHICH panel did that, so the heavy
//! panel systems wrap themselves in a [`UiPerf::scope`] and mark when they
//! actually rebuilt. The summary line names the system, how often it ran,
//! how often it rebuilt, and what it cost:
//!
//! `ClientPerfUi window_s=5.0 rebuild_people_list=300c/298r/1420.5ms/max9.80ms ...`
//!
//! (`c` = calls, `r` = rebuilds). Emitted on the same cadence as `ClientPerf`.
//!
//! The counters sit behind a mutex so instrumented systems take `Res<UiPerf>`:
//! a `ResMut` would serialise every instrumented panel system against each
//! other in the multithreaded executor, which is exactly the kind of cost a
//! profiler must not add.

use bevy::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

#[derive(Default, Clone, Copy)]
struct Entry {
    calls: u32,
    rebuilds: u32,
    total: Duration,
    max: Duration,
}

#[derive(Default)]
struct Counters {
    entries: BTreeMap<&'static str, Entry>,
    last_emit_secs: f32,
}

#[derive(Resource, Default)]
pub struct UiPerf {
    counters: Mutex<Counters>,
}

impl UiPerf {
    /// Time the rest of the calling system under `name`. Call
    /// [`UiPerfScope::rebuilt`] when the system tears down and respawns UI.
    pub fn scope(&self, name: &'static str) -> UiPerfScope<'_> {
        UiPerfScope {
            perf: self,
            name,
            start: Instant::now(),
            rebuilt: false,
        }
    }

    fn record(&self, name: &'static str, elapsed: Duration, rebuilt: bool) {
        let mut counters = self.counters.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = counters.entries.entry(name).or_default();
        entry.calls += 1;
        entry.rebuilds += u32::from(rebuilt);
        entry.total += elapsed;
        entry.max = entry.max.max(elapsed);
    }
}

pub struct UiPerfScope<'a> {
    perf: &'a UiPerf,
    name: &'static str,
    start: Instant,
    rebuilt: bool,
}

impl UiPerfScope<'_> {
    pub fn rebuilt(&mut self) {
        self.rebuilt = true;
    }
}

impl Drop for UiPerfScope<'_> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        self.perf.record(self.name, elapsed, self.rebuilt);
    }
}

pub struct UiPerfPlugin;

impl Plugin for UiPerfPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiPerf>();
        app.add_systems(Update, emit_ui_perf);
    }
}

fn emit_ui_perf(
    time: Res<Time>,
    config: Option<Res<crate::perf_overlay::ClientPerfConfig>>,
    perf: Res<UiPerf>,
) {
    let Some(config) = config.filter(|config| config.enabled) else {
        return;
    };
    let now = time.elapsed_secs();
    let mut counters = perf.counters.lock().unwrap_or_else(PoisonError::into_inner);
    if now - counters.last_emit_secs < config.emit_interval_secs {
        return;
    }
    let window = now - counters.last_emit_secs;
    counters.last_emit_secs = now;
    if counters.entries.is_empty() {
        return;
    }
    let parts: Vec<String> = counters
        .entries
        .iter()
        .map(|(name, entry)| {
            format!(
                "{name}={}c/{}r/{:.1}ms/max{:.2}ms",
                entry.calls,
                entry.rebuilds,
                entry.total.as_secs_f64() * 1000.0,
                entry.max.as_secs_f64() * 1000.0,
            )
        })
        .collect();
    info!("ClientPerfUi window_s={window:.1} {}", parts.join(" "));
    counters.entries.clear();
}
