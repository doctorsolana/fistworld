//! Client performance overlay (F3) and rolling frame-time telemetry.
//!
//! Owns the on-screen FPS/stats overlay, the rolling percentile snapshot behind
//! `FISTFORCE_CLIENT_PERF`, and the low-FPS drop monitor. Deliberately independent
//! of any gameplay domain so it survives gameplay refactors.

use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;
use std::collections::VecDeque;

use shared::components::{Player, PlayerPosition};
use shared::debug::DebugGizmoMode;
use shared::terrain::ChunkCoord;

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for the debug overlay UI
#[derive(Component)]
pub struct DebugOverlay;

/// Marker for the FPS text specifically
#[derive(Component)]
pub struct FpsText;

/// Multi-line performance stats text in debug overlay
#[derive(Component)]
pub struct PerfStatsText;

// =============================================================================
// RESOURCES
// =============================================================================

/// Rolling client performance configuration.
#[derive(Resource, Clone, Debug)]
pub struct ClientPerfConfig {
    pub enabled: bool,
    pub emit_interval_secs: f32,
    pub rolling_window_samples: usize,
    pub hitch_threshold_ms: f32,
    pub stats_update_interval_secs: f32,
}

impl Default for ClientPerfConfig {
    fn default() -> Self {
        let enabled = crate::profiling::hitch_profiling_enabled()
            || crate::profiling::env_flag("FISTFORCE_CLIENT_PERF");
        let emit_interval_secs = std::env::var("FISTFORCE_CLIENT_PERF_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(5.0);
        let hitch_threshold_ms =
            crate::profiling::env_f32("FISTFORCE_HITCH_THRESHOLD_MS", 35.0).max(1.0);

        Self {
            enabled,
            emit_interval_secs,
            rolling_window_samples: 600,
            hitch_threshold_ms,
            stats_update_interval_secs: 0.25,
        }
    }
}

/// Rolling frame-time snapshot used by overlay and optional perf logging.
#[derive(Resource, Default)]
pub struct ClientPerfSnapshot {
    pub frame_times_ms: VecDeque<f32>,
    pub hitch_count_window: u32,
    pub total_samples: u64,
    pub p50_ms: f32,
    pub p95_ms: f32,
    pub p99_ms: f32,
    pub last_stats_update_secs: f32,
    pub last_emit_secs: f32,
}

/// Toggle the perf overlay (FPS + counters) with F3.
///
/// This is intentionally separate from debug gizmos so you can inspect performance
/// without paying the cost of drawing lots of gizmo lines.
#[derive(Resource, Default)]
pub struct PerfOverlayEnabled(pub bool);

/// Logs a snapshot when FPS stays low for a bit.
#[derive(Resource, Default)]
pub struct PerfDropMonitor {
    below_seconds: f32,
    sample_timer: f32,
    last_snapshot: f32,
}

// =============================================================================
// INPUT TOGGLES
// =============================================================================

/// Toggle debug gizmos / trajectory drawing with F4.
pub fn handle_toggle_debug_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut debug_mode: ResMut<DebugGizmoMode>,
) {
    if keyboard.just_pressed(KeyCode::F4) {
        debug_mode.0 = !debug_mode.0;
        info!("Debug gizmos: {}", if debug_mode.0 { "ON" } else { "OFF" });
    }
}

pub fn handle_toggle_perf_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut overlay: ResMut<PerfOverlayEnabled>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        overlay.0 = !overlay.0;
        info!("Perf overlay: {}", if overlay.0 { "ON" } else { "OFF" });
    }
}

// =============================================================================
// SAMPLING
// =============================================================================

/// Sample frame times and maintain rolling percentile stats.
pub fn update_client_perf_snapshot(
    time: Res<Time>,
    config: Res<ClientPerfConfig>,
    mut snapshot: ResMut<ClientPerfSnapshot>,
) {
    let frame_ms = time.delta_secs() * 1000.0;
    if !frame_ms.is_finite() || frame_ms <= 0.0 {
        return;
    }

    snapshot.total_samples = snapshot.total_samples.saturating_add(1);
    snapshot.frame_times_ms.push_back(frame_ms);
    if frame_ms > config.hitch_threshold_ms {
        snapshot.hitch_count_window = snapshot.hitch_count_window.saturating_add(1);
    }

    let target_len = config.rolling_window_samples.max(60);
    while snapshot.frame_times_ms.len() > target_len {
        if let Some(removed_ms) = snapshot.frame_times_ms.pop_front() {
            if removed_ms > config.hitch_threshold_ms {
                snapshot.hitch_count_window = snapshot.hitch_count_window.saturating_sub(1);
            }
        }
    }

    let now = time.elapsed_secs();
    if now - snapshot.last_stats_update_secs < config.stats_update_interval_secs {
        return;
    }
    snapshot.last_stats_update_secs = now;

    if snapshot.frame_times_ms.is_empty() {
        snapshot.p50_ms = 0.0;
        snapshot.p95_ms = 0.0;
        snapshot.p99_ms = 0.0;
        return;
    }

    let mut samples: Vec<f32> = snapshot.frame_times_ms.iter().copied().collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    snapshot.p50_ms = percentile_sorted(&samples, 0.50);
    snapshot.p95_ms = percentile_sorted(&samples, 0.95);
    snapshot.p99_ms = percentile_sorted(&samples, 0.99);
}

/// Emit periodic machine-readable client perf summary.
pub fn emit_client_perf_summary(
    time: Res<Time>,
    config: Res<ClientPerfConfig>,
    mut snapshot: ResMut<ClientPerfSnapshot>,
) {
    if !config.enabled {
        return;
    }

    let now = time.elapsed_secs();
    if now - snapshot.last_emit_secs < config.emit_interval_secs {
        return;
    }
    snapshot.last_emit_secs = now;

    let window_samples = snapshot.frame_times_ms.len();
    if window_samples == 0 {
        return;
    }

    info!(
        "ClientPerf frame_ms_p50={:.2} frame_ms_p95={:.2} frame_ms_p99={:.2} hitch_count={} hitch_threshold_ms={:.1} samples_window={} samples_total={}",
        snapshot.p50_ms,
        snapshot.p95_ms,
        snapshot.p99_ms,
        snapshot.hitch_count_window,
        config.hitch_threshold_ms,
        window_samples,
        snapshot.total_samples,
    );
}

fn percentile_sorted(sorted: &[f32], percentile: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let max_index = sorted.len().saturating_sub(1);
    let clamped = percentile.clamp(0.0, 1.0);
    let index = ((max_index as f32) * clamped).round() as usize;
    sorted[index.min(max_index)]
}

// =============================================================================
// OVERLAY UI
// =============================================================================

/// Spawn the debug overlay UI (hidden by default)
pub fn spawn_debug_overlay(mut commands: Commands) {
    // Root container - top-left corner
    commands
        .spawn((
            DebugOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            Visibility::Hidden, // Hidden until debug mode is enabled
        ))
        .with_children(|parent| {
            // FPS text
            parent.spawn((
                FpsText,
                Text::new("FPS: --"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.0, 1.0, 0.0)), // Green text
            ));

            // Perf stats text (multi-line)
            parent.spawn((
                PerfStatsText,
                Text::new("Perf: --"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
            ));

            // Key hints
            parent.spawn((
                Text::new("[F3] Perf Overlay  |  [F4] Gizmos"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ));
        });
}

/// Update the debug overlay - show/hide based on debug mode, update FPS
#[allow(clippy::type_complexity)]
pub fn update_debug_overlay(
    overlay_enabled: Res<PerfOverlayEnabled>,
    debug_mode: Res<DebugGizmoMode>,
    perf: (Res<ClientPerfConfig>, Res<ClientPerfSnapshot>),
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time>,
    mut last_update: Local<f32>,
    mut overlay_query: Query<&mut Visibility, With<DebugOverlay>>,
    mut fps_text_query: Query<(&mut Text, &mut TextColor), With<FpsText>>,
    mut perf_text_query: Query<&mut Text, (With<PerfStatsText>, Without<FpsText>)>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    loaded_chunks: Res<crate::terrain::LoadedChunks>,
    collider_library: Option<Res<crate::props::ClientDerivedColliderLibrary>>,
    mut counts_a: ParamSet<(
        Query<(), With<crate::props::EnvironmentProp>>,
        Query<&crate::props::PropKindTag, With<crate::props::EnvironmentProp>>,
        Query<&PlayerPosition, With<Player>>,
        Query<(), With<crate::render::systems::SandParticle>>,
    )>,
    mut counts_b: ParamSet<(
        Query<(), With<crate::render::systems::CloudLayer>>,
        Query<(), With<crate::render::systems::CloudLayerPlane>>,
        Query<&crate::camera_rts::CommanderCamera>,
    )>,
) {
    // Show/hide overlay based on debug mode
    for mut visibility in overlay_query.iter_mut() {
        *visibility = if overlay_enabled.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    // Update FPS text (only if visible)
    if overlay_enabled.0 {
        let now = time.elapsed_secs();
        // Throttle overlay updates to reduce per-frame cost.
        if now - *last_update < 0.25 {
            return;
        }
        *last_update = now;

        if let Some(fps_diagnostic) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps) = fps_diagnostic.smoothed() {
                for (mut text, mut color) in fps_text_query.iter_mut() {
                    text.0 = format!("FPS: {:.0}", fps);

                    // Color code based on FPS performance
                    *color = if fps >= 55.0 {
                        TextColor(Color::srgb(0.2, 1.0, 0.2)) // Green - good
                    } else if fps >= 30.0 {
                        TextColor(Color::srgb(1.0, 0.8, 0.0)) // Yellow - okay
                    } else {
                        TextColor(Color::srgb(1.0, 0.2, 0.2)) // Red - bad
                    };
                }
            }
        }

        // Build a compact perf readout:
        // - entity count
        // - chunk/prop counts
        // - top render CPU passes (if RenderDiagnosticsPlugin is enabled)
        let entity_count = diagnostics
            .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
            .and_then(|d| d.smoothed())
            .unwrap_or(0.0);

        // Collect top render/*/elapsed_cpu diagnostics
        let mut render_cpu: Vec<(&str, f64)> = diagnostics
            .iter()
            .filter_map(|d| {
                let path = d.path().as_str();
                if !path.starts_with("render/") || !path.ends_with("/elapsed_cpu") {
                    return None;
                }
                let v = d.smoothed()?;
                Some((path, v))
            })
            .collect();
        render_cpu.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut lines = String::new();
        let collidable_props = collider_library
            .as_ref()
            .map(|lib| {
                counts_a
                    .p1()
                    .iter()
                    .filter(|k| lib.by_kind.contains_key(&k.0))
                    .count()
            })
            .unwrap_or(0);
        let baked_kinds = collider_library
            .as_ref()
            .map(|lib| lib.by_kind.len())
            .unwrap_or(0);

        // Approximate the server-side collider chunk set (radius=3 chunks) by unioning all players.
        // This matches the current streaming radius used by `server/src/collision/streaming.rs`.
        let collider_chunk_radius = 3;
        let mut collider_chunks = std::collections::HashSet::new();
        for p in counts_a.p2().iter() {
            let center = ChunkCoord::from_world_pos(p.0);
            collider_chunks.extend(center.chunks_in_radius(collider_chunk_radius));
        }
        // Camera state first: bug reports lead with "at zoom X near (x, z)".
        if let Ok(cam) = counts_b.p2().single() {
            lines.push_str(&format!(
                "Camera: zoom {:.0}m | tilt {:.2} | focus ({:.0}, {:.0})\n",
                cam.zoom, cam.tilt, cam.focus.x, cam.focus.z,
            ));
        }
        lines.push_str(&format!(
            "Gizmos: {}\nEntities: {:.0}\nChunks: {}\nProps: {}\nSand particles: {}\nCollider chunks: {}\nCollidable props: {} (baked kinds: {})\nCloud layers: {} | Cloud plane: {}\nFrame ms p50/p95/p99: {:.2}/{:.2}/{:.2}\nHitches > {:.1}ms (window): {}\nAssets: meshes {} | materials {} | images {}\n",
            if debug_mode.0 { "ON" } else { "OFF" },
            entity_count,
            loaded_chunks.chunks.len(),
            counts_a.p0().iter().count(),
            counts_a.p3().iter().count(),
            collider_chunks.len(),
            collidable_props,
            baked_kinds,
            counts_b.p0().iter().count(),
            counts_b.p1().iter().count(),
            perf.1.p50_ms,
            perf.1.p95_ms,
            perf.1.p99_ms,
            perf.0.hitch_threshold_ms,
            perf.1.hitch_count_window,
            meshes.len(),
            materials.len(),
            images.len()
        ));

        if !render_cpu.is_empty() {
            lines.push_str("Render (CPU ms, top):\n");
            for (path, v) in render_cpu.iter().take(5) {
                // Show only the span name, not the whole prefix.
                let name = path
                    .strip_prefix("render/")
                    .unwrap_or(path)
                    .strip_suffix("/elapsed_cpu")
                    .unwrap_or(path);
                lines.push_str(&format!("  {name}: {v:.2}\n"));
            }
        } else {
            lines.push_str("Render: (enable RenderDiagnosticsPlugin)\n");
        }

        for mut text in perf_text_query.iter_mut() {
            text.0 = lines.clone();
        }
    }
}

/// Emit a perf snapshot when FPS stays low for a while.
pub fn update_perf_drop_monitor(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    mut monitor: ResMut<PerfDropMonitor>,
    loaded_chunks: Res<crate::terrain::LoadedChunks>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    counts_a: Query<(), With<crate::props::EnvironmentProp>>,
    counts_c: Query<(), With<crate::render::systems::SandParticle>>,
    counts_d: Query<(), With<crate::render::systems::CloudLayer>>,
    counts_e: Query<(), With<crate::render::systems::CloudLayerPlane>>,
) {
    monitor.sample_timer += time.delta_secs();
    if monitor.sample_timer < 1.0 {
        return;
    }
    monitor.sample_timer = 0.0;

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(60.0);
    if fps < 15.0 {
        monitor.below_seconds += 1.0;
    } else {
        monitor.below_seconds = 0.0;
    }

    if monitor.below_seconds < 5.0 {
        return;
    }

    let now = time.elapsed_secs();
    if now - monitor.last_snapshot < 10.0 {
        return;
    }
    monitor.last_snapshot = now;

    let entity_count = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    info!(
        "PERF DROP snapshot: fps={:.1} entities={:.0} chunks={} props={} sand={} clouds={} cloud_plane={} assets(mesh={}, mat={}, img={})",
        fps,
        entity_count,
        loaded_chunks.chunks.len(),
        counts_a.iter().count(),
        counts_c.iter().count(),
        counts_d.iter().count(),
        counts_e.iter().count(),
        meshes.len(),
        materials.len(),
        images.len(),
    );
}

/// Despawn the debug overlay
pub fn despawn_debug_overlay(
    mut commands: Commands,
    overlay_query: Query<Entity, With<DebugOverlay>>,
) {
    for entity in overlay_query.iter() {
        commands.entity(entity).despawn();
    }
}
