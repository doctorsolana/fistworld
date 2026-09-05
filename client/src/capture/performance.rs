//! Wall-clock frame pacing for the real renderer. Fixed simulation time is not FPS.
//!
//! Sample in First, before PreUpdate clears the preceding frame's streaming counters.
//! Screenshots and video are disabled in this mode; run the same path normally for
//! visual evidence. These intervals include scheduling/render backpressure, not GPU
//! timestamp measurements (Bevy's GPU diagnostic recorder does not support Metal).

use super::{CaptureConfig, CaptureRunStatus, CaptureState};
use crate::render::systems::scaled_target::SceneRenderTarget;
use crate::render::systems::GraphicsSettings;
use crate::terrain::{LoadedChunks, PerfHitchStats};
use bevy::prelude::*;
use serde::Serialize;
use std::time::Instant;

#[derive(Resource)]
pub(super) struct CapturePerformance {
    previous: Option<(Instant, usize)>,
    frames: Vec<FrameSample>,
    written: bool,
}

impl CapturePerformance {
    pub(super) fn new(frame_count: usize) -> Self {
        Self {
            previous: None,
            frames: Vec::with_capacity(frame_count),
            written: false,
        }
    }
}

#[derive(Serialize)]
struct FrameSample {
    shot: usize,
    frame_ms: f64,
    terrain_finalize_ms: f32,
    terrain_spawn_ms: f32,
    props_spawn_ms: f32,
    chunks_finalized: u32,
    props_spawned: u32,
    loaded_chunks: usize,
}

pub(super) fn measure_frames(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut performance: ResMut<CapturePerformance>,
    mut status: ResMut<CaptureRunStatus>,
    perf: Res<PerfHitchStats>,
    chunks: Res<LoadedChunks>,
    settings: Res<GraphicsSettings>,
    target: Option<Res<SceneRenderTarget>>,
    images: Res<Assets<Image>>,
) {
    let now = Instant::now();
    if let Some((start, shot)) = performance.previous.take() {
        performance.frames.push(FrameSample {
            shot,
            frame_ms: now.duration_since(start).as_secs_f64() * 1000.0,
            terrain_finalize_ms: perf.terrain_finalize_ms,
            terrain_spawn_ms: perf.terrain_spawn_ms,
            props_spawn_ms: perf.props_spawn_ms,
            chunks_finalized: perf.terrain_chunks_finalized,
            props_spawned: perf.props_instances_spawned,
            loaded_chunks: chunks.chunks.len(),
        });
    }
    if let CaptureState::Settling { shot, .. } = *state {
        performance.previous = Some((now, shot));
    } else if !matches!(*state, CaptureState::Warmup { .. }) && !performance.written {
        performance.written = true;
        let times: Vec<_> = performance
            .frames
            .iter()
            .map(|frame| frame.frame_ms)
            .collect();
        let report = serde_json::json!({
            "version": 1,
            "scenario": config.scenario_name,
            "git_commit": crate::capture_artifact::git_commit(),
            "resolution": config.resolution,
            "scene_resolution": target.as_ref().and_then(|target| images.get(&target.image)).map(|image| [image.width(), image.height()]),
            "graphics_settings": *settings,
            "target": config.target,
            "fixed_simulation_delta_seconds": config.fixed_delta_seconds,
            "timing": "wall-clock frame intervals, uncapped, warmup excluded, no screenshot readbacks",
            "summary": summarize(&times),
            "frames": performance.frames,
        });
        let path = config.out_dir.join("performance.json");
        let result = serde_json::to_vec_pretty(&report)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(&path, bytes));
        if let Err(error) = result {
            error!("capture: cannot write {}: {error}", path.display());
            status.failed = true;
        } else if times.len() != config.shots.len() {
            error!(
                "capture: benchmark measured {} of {} frames",
                times.len(),
                config.shots.len()
            );
            status.failed = true;
        } else {
            info!(
                "capture: benchmark {} -> {}",
                report["summary"],
                path.display()
            );
        }
    }
}

fn summarize(times: &[f64]) -> serde_json::Value {
    if times.is_empty() {
        return serde_json::Value::Null;
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        sorted[((sorted.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
    };
    serde_json::json!({
        "frames": times.len(),
        "total_ms": times.iter().sum::<f64>(),
        "p50_ms": percentile(0.50),
        "p95_ms": percentile(0.95),
        "p99_ms": percentile(0.99),
        "max_ms": sorted[sorted.len() - 1],
        "over_16_67_ms": times.iter().filter(|&&ms| ms > 1000.0 / 60.0).count(),
        "over_33_33_ms": times.iter().filter(|&&ms| ms > 1000.0 / 30.0).count(),
        "over_50_ms": times.iter().filter(|&&ms| ms > 50.0).count(),
    })
}

#[cfg(test)]
mod tests {
    use super::summarize;

    #[test]
    fn reports_slow_frames_without_clamping_them_to_simulation_time() {
        let report = summarize(&[100.0, 8.0, 12.0, 20.0, 10.0]);
        assert_eq!(report["p50_ms"], 12.0);
        assert_eq!(report["p99_ms"], 100.0);
        assert_eq!(report["over_16_67_ms"], 2);
        assert_eq!(report["over_50_ms"], 1);
        assert_eq!(report["total_ms"], 150.0);
    }

    #[test]
    fn an_empty_run_is_not_a_zero_latency_result() {
        assert!(summarize(&[]).is_null());
    }
}
