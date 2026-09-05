//! Client performance overlay (F3) and rolling frame-time telemetry.
//!
//! Owns the on-screen FPS/stats overlay, the rolling percentile snapshot behind
//! `FISTFORCE_CLIENT_PERF`, and the low-FPS drop monitor. Deliberately independent
//! of any gameplay domain so it survives gameplay refactors.

use bevy::app::AppExit;
use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;
use std::collections::VecDeque;

use shared::components::{Player, PlayerPosition, Settlement, SettlementBuildingKind};
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
                Text::new("[F3] Perf + Village Info  |  [F4] Gizmos + Planning Rings"),
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
    // Grouped: a Bevy system takes at most 16 parameters and this was already at
    // the cap. Both are "what the world is like here", so they travel together
    // rather than being split by an arbitrary limit.
    world: (
        Res<crate::terrain::LoadedChunks>,
        Option<Res<shared::terrain::WorldTerrain>>,
    ),
    collider_library: Option<Res<crate::props::ClientDerivedColliderLibrary>>,
    mut counts_a: ParamSet<(
        Query<(), With<crate::props::EnvironmentProp>>,
        Query<&crate::props::PropKindTag, With<crate::props::EnvironmentProp>>,
        Query<&PlayerPosition, With<Player>>,
    )>,
    mut counts_b: ParamSet<(
        Query<(), With<crate::render::systems::CloudLayerPlane>>,
        Query<&crate::camera_rts::CommanderCamera>,
        Query<(&Settlement, &PlayerPosition)>,
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
        let focus = counts_b.p1().single().ok().map(|cam| {
            lines.push_str(&format!(
                "Camera: zoom {:.0}m | tilt {:.2} | focus ({:.0}, {:.0})\n",
                cam.zoom, cam.tilt, cam.focus.x, cam.focus.z,
            ));
            cam.focus
        });

        // What the ground under the screen centre actually IS.
        //
        // Sampled at the camera focus rather than at the player: the focus is
        // what you are looking at, and it is what every other distance in this
        // engine is measured from.
        //
        // These are the same numbers the simulation reads -- the biome that
        // decides which species scatter here, the climate that decides whether
        // it snows or bakes, and the resource profile that sets grass density
        // and what a building sited here would yield. If the view and these
        // ever disagree, the numbers are the truth and the view is the bug.
        if let (Some(focus), Some(terrain)) = (focus, world.1.as_ref()) {
            let map = terrain.generator.loaded_map();
            if let (Some(field), Some(generated)) = (
                map.biome_field.as_deref(),
                map.definition.generated.as_ref(),
            ) {
                let height = terrain.get_height(focus.x, focus.z);
                // Gradient magnitude (rise per metre): the convention
                // `BiomeField` was written against and the terrain mesh uses.
                // Two other slope formulas live in this repo; neither belongs
                // here, and using one would quietly report the wrong biome on
                // any hillside.
                let normal = terrain.get_normal(focus.x, focus.z);
                let slope = (normal.x * normal.x + normal.z * normal.z).sqrt() / normal.y.max(0.01);
                let biome = field.biome(focus.x, focus.z, height, slope);
                let profile = field.resources(focus.x, focus.z, height, slope);
                let climate = shared::worldgen::climate_at(
                    generated.seed,
                    focus.x,
                    focus.z,
                    height,
                    generated.half_extent,
                );
                lines.push_str(&format!(
                    "Biome: {:?} | ground {:.0}m | slope {:.2}\n\
                     Climate: snow {:.2} | frost {:.2} | dry {:.2}\n\
                     Resources: farm {:.2} | wood {:.2} | stone {:.2} | iron {:.2}\n",
                    biome,
                    height,
                    slope,
                    climate.snow,
                    climate.frost,
                    climate.dry,
                    profile.farmland,
                    profile.wood,
                    profile.stone,
                    profile.iron,
                ));
            }
        }
        if let Some(focus) = focus {
            let nearest = counts_b
                .p2()
                .iter()
                .map(|(settlement, position)| {
                    let distance =
                        Vec2::new(position.0.x - focus.x, position.0.z - focus.z).length();
                    (
                        distance,
                        settlement.name.clone(),
                        settlement.tier,
                        settlement.residents,
                    )
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((distance, name, tier, residents)) =
                nearest.filter(|(distance, ..)| *distance <= 500.0)
            {
                let house = SettlementBuildingKind::House.preferred_ring();
                let work = SettlementBuildingKind::Farmstead.preferred_ring();
                let fishing = SettlementBuildingKind::FishermansHut.preferred_ring();
                lines.push_str(&format!(
                    "Village: {} | {:?} | pop {} | {:.0}m from focus\n\
                     Planning: houses {:.0}-{:.0}m | work {:.0}-{:.0}m | fishing {:.0}-{:.0}m [F4]\n",
                    name,
                    tier,
                    residents,
                    distance,
                    house.0,
                    house.1,
                    work.0,
                    work.1,
                    fishing.0,
                    fishing.1,
                ));
            }
        }
        lines.push_str(&format!(
            "Gizmos: {}\nEntities: {:.0}\nChunks: {}\nProps: {}\nCollider chunks: {}\nCollidable props: {} (baked kinds: {})\nCloud plane: {}\nFrame ms p50/p95/p99: {:.2}/{:.2}/{:.2}\nHitches > {:.1}ms (window): {}\nAssets: meshes {} | materials {} | images {}\n",
            if debug_mode.0 { "ON" } else { "OFF" },
            entity_count,
            world.0.chunks.len(),
            counts_a.p0().iter().count(),
            collider_chunks.len(),
            collidable_props,
            baked_kinds,
            counts_b.p0().iter().count(),
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
        "PERF DROP snapshot: fps={:.1} entities={:.0} chunks={} props={} cloud_plane={} assets(mesh={}, mat={}, img={})",
        fps,
        entity_count,
        loaded_chunks.chunks.len(),
        counts_a.iter().count(),
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

/// Developer harness only: `FISTFORCE_EXIT_AFTER_SECS=<n>` shuts the client
/// down cleanly after `n` seconds of wall time. Unattended profiling runs need
/// it because `trace_chrome` writes its file only on a clean exit, and this Mac
/// Mesh census for perf runs: every 10 s under `FISTFORCE_CLIENT_PERF`, count
/// the entities carrying a `Mesh3d`, grouped by their `Name` (glTF primitives
/// are named after their node). A static camera whose frame time still climbs
/// means something keeps ADDING rendered instances, and this line names it.
/// Software frame cap (see `GraphicsSettings::frame_cap_fps`). Runs in `Last`
/// so the pause lands after this frame's extraction hand-off: both the main
/// and render threads get idle time, which is what keeps the SoC out of its
/// power limit. Vsync on or a cap of 0 disables it.
///
/// macOS `thread::sleep` overshoots by 1-3 ms (timer coalescing), which would
/// turn a 60 fps cap into 52 fps. The limiter therefore sleeps only up to an
/// adaptive margin - the recent average overshoot plus a little - and spins
/// the remainder, so the period lands on the target within ~0.1 ms while
/// burning well under a millisecond of CPU per frame.
pub fn limit_frame_rate(
    settings: Res<crate::render::systems::GraphicsSettings>,
    mut state: Local<FrameCapState>,
) {
    let cap = settings.frame_cap_fps;
    if cap == 0 || settings.vsync_enabled {
        state.last_frame_end = None;
        return;
    }
    let target = std::time::Duration::from_secs_f64(1.0 / f64::from(cap));
    if let Some(previous) = state.last_frame_end {
        let elapsed = previous.elapsed();
        if elapsed < target {
            let remaining = target - elapsed;
            let margin = std::time::Duration::from_secs_f32(state.sleep_margin_secs);
            if remaining > margin {
                let requested = remaining - margin;
                let before = std::time::Instant::now();
                std::thread::sleep(requested);
                let overshoot = before.elapsed().saturating_sub(requested).as_secs_f32();
                // Track the platform's real sleep error; never trust it below 0.3 ms.
                state.sleep_margin_secs = (state.sleep_margin_secs * 0.8
                    + (overshoot + 0.0002) * 0.2)
                    .clamp(0.0003, 0.006);
            }
            while previous.elapsed() < target {
                std::hint::spin_loop();
            }
        }
    }
    state.last_frame_end = Some(std::time::Instant::now());
}

/// Frame-cap pacing state: the previous frame boundary and the learned sleep
/// overshoot margin.
pub struct FrameCapState {
    last_frame_end: Option<std::time::Instant>,
    sleep_margin_secs: f32,
}

impl Default for FrameCapState {
    fn default() -> Self {
        Self {
            last_frame_end: None,
            sleep_margin_secs: 0.0015,
        }
    }
}

/// Which archetypes keep changing their `Mesh3d` in a supposedly static scene:
/// every 10 s under `FISTFORCE_CLIENT_PERF`, tally the changed-mesh entities
/// by their component signature (top 4). Exclusive so it can read archetypes.
pub fn log_changed_mesh_archetypes(world: &mut World) {
    let enabled = std::env::var("FISTFORCE_CLIENT_PERF").is_ok_and(|v| !v.is_empty());
    if !enabled {
        return;
    }
    let dt = world.resource::<Time>().delta_secs();
    let mut state = world
        .remove_resource::<ChangedMeshArchetypeTally>()
        .unwrap_or_default();
    state.since += dt;
    state.frames += 1;
    let mut query = world.query_filtered::<Entity, Changed<Mesh3d>>();
    let entities: Vec<Entity> = query.iter(world).collect();
    for entity in entities {
        let mut names: Vec<String> = world
            .inspect_entity(entity)
            .map(|components| {
                components
                    .map(|info| info.name().shortname().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.retain(|n| {
            !matches!(
                n.as_str(),
                "Transform"
                    | "GlobalTransform"
                    | "Visibility"
                    | "InheritedVisibility"
                    | "ViewVisibility"
                    | "Aabb"
                    | "Mesh3d"
                    | "ChildOf"
                    | "Children"
            )
        });
        names.sort_unstable();
        names.truncate(6);
        *state.by_signature.entry(names.join("+")).or_default() += 1;
    }
    if state.since >= 10.0 {
        let frames = state.frames.max(1);
        let mut rows: Vec<(String, u32)> = state.by_signature.drain().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        let top: Vec<String> = rows
            .iter()
            .take(4)
            .map(|(sig, n)| format!("[{sig}] x{:.1}/frame", *n as f32 / frames as f32))
            .collect();
        info!("ClientPerfChangedMeshes {}", top.join("  "));
        state.since = 0.0;
        state.frames = 0;
    }
    world.insert_resource(state);
}

#[derive(Resource, Default)]
struct ChangedMeshArchetypeTally {
    since: f32,
    frames: u32,
    by_signature: std::collections::HashMap<String, u32>,
}

/// Asset stores the census reports on, bundled so the census stays under
/// Bevy's 16-parameter system limit.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CensusAssets<'w> {
    meshes: Res<'w, Assets<Mesh>>,
    std_materials: Res<'w, Assets<StandardMaterial>>,
    images: Res<'w, Assets<Image>>,
    wind_materials: Option<Res<'w, Assets<crate::props::WindFoliageMaterial>>>,
    terrain_materials: Option<Res<'w, Assets<crate::terrain::TerrainSplatMaterial>>>,
    water_materials: Option<Res<'w, Assets<crate::water::material::ToonWaterMaterial>>>,
}

/// Terrain streaming counters for the census: what actually spawns,
/// regenerates and unloads chunks in a scene that should be static.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CensusTerrain<'w, 's> {
    hitch_stats: Res<'w, crate::terrain::PerfHitchStats>,
    delta_state: Res<'w, crate::terrain::TerrainDeltaState>,
    delta_chunks: Query<'w, 's, &'static shared::terrain::TerrainDeltaChunk>,
    changed_delta_chunks: Query<'w, 's, (), Changed<shared::terrain::TerrainDeltaChunk>>,
    added_chunks: Query<'w, 's, (), Added<crate::terrain::TerrainChunk>>,
}

#[allow(clippy::too_many_arguments)]
pub fn log_mesh_census(
    time: Res<Time>,
    mut since: Local<f32>,
    mut enabled: Local<Option<bool>>,
    mut churn: Local<(u32, u64, u64, u64)>,
    meshes: Query<(Option<&Name>, Option<&InheritedVisibility>), With<Mesh3d>>,
    changed_meshes: Query<(), Changed<Mesh3d>>,
    changed_std_materials: Query<(), Changed<MeshMaterial3d<StandardMaterial>>>,
    changed_transforms: Query<(), (Changed<GlobalTransform>, With<Mesh3d>)>,
    assets: CensusAssets,
    world_time: Query<&shared::components::WorldTime>,
    cover: Option<Res<crate::render::systems::CloudCover>>,
    changed_named: Query<Option<&Name>, Changed<Mesh3d>>,
    mut changed_sample: Local<Vec<String>>,
    terrain: CensusTerrain,
    mut terrain_tally: Local<[u64; 6]>,
) {
    let on = *enabled
        .get_or_insert_with(|| std::env::var("FISTFORCE_CLIENT_PERF").is_ok_and(|v| !v.is_empty()));
    if !on {
        return;
    }
    churn.0 += 1;
    churn.1 += changed_meshes.iter().count() as u64;
    churn.2 += changed_std_materials.iter().count() as u64;
    churn.3 += changed_transforms.iter().count() as u64;
    terrain_tally[0] += u64::from(terrain.hitch_stats.terrain_chunks_spawned);
    terrain_tally[1] += u64::from(terrain.hitch_stats.terrain_chunks_finalized);
    terrain_tally[2] += u64::from(terrain.hitch_stats.terrain_chunks_regen);
    terrain_tally[3] += u64::from(terrain.hitch_stats.terrain_chunks_unloaded);
    terrain_tally[4] += terrain.changed_delta_chunks.iter().count() as u64;
    terrain_tally[5] += terrain.added_chunks.iter().count() as u64;
    if changed_sample.len() < 8 {
        for name in changed_named.iter() {
            if changed_sample.len() >= 8 {
                break;
            }
            let label: String = name
                .map_or("<unnamed>", |n| n.as_str())
                .chars()
                .take(22)
                .collect();
            if !changed_sample.contains(&label) {
                changed_sample.push(label);
            }
        }
    }
    *since += time.delta_secs();
    if *since < 10.0 {
        return;
    }
    *since = 0.0;
    let frames = churn.0.max(1) as u64;
    info!(
        "ClientPerfAssets meshes={} std_materials={} images={} wind_materials={} terrain_materials={} water_materials={} | per-frame changed: mesh3d={} std_material={} mesh_transform={}",
        assets.meshes.len(),
        assets.std_materials.len(),
        assets.images.len(),
        assets.wind_materials.as_deref().map_or(0, |m| m.len()),
        assets.terrain_materials.as_deref().map_or(0, |m| m.len()),
        assets.water_materials.as_deref().map_or(0, |m| m.len()),
        churn.1 / frames,
        churn.2 / frames,
        churn.3 / frames,
    );
    *churn = (0, 0, 0, 0);
    let clock = world_time.iter().next().map_or_else(
        || "n/a".to_string(),
        |t| {
            format!(
                "day={} t={:.0}s sun_phase={:.2} is_day={}",
                t.day,
                t.seconds_in_cycle,
                t.sun_phase(),
                t.is_day()
            )
        },
    );
    let weather = cover.as_deref().map_or_else(
        || "n/a".to_string(),
        |c| format!("cover={:.2} storminess={:.2}", c.current, c.storminess),
    );
    info!(
        "ClientPerfWorld {clock} | {weather} | changed-mesh names: {}",
        changed_sample.join(", ")
    );
    changed_sample.clear();
    let max_version = terrain
        .delta_chunks
        .iter()
        .map(|c| c.version)
        .max()
        .unwrap_or(0);
    info!(
        "ClientPerfTerrain per-interval: spawned={} finalized={} regen={} unloaded={} replicated_delta_changes={} added_chunk_entities={} | delta_chunks={} max_version={} dirty_queue={} known_versions={}",
        terrain_tally[0], terrain_tally[1], terrain_tally[2], terrain_tally[3], terrain_tally[4], terrain_tally[5],
        terrain.delta_chunks.iter().count(), max_version,
        terrain.delta_state.dirty_queue.len(), terrain.delta_state.chunk_versions.len()
    );
    *terrain_tally = [0; 6];
    let mut by_name: std::collections::HashMap<String, (u32, u32)> =
        std::collections::HashMap::new();
    let mut total = 0u32;
    let mut visible = 0u32;
    for (name, inherited) in meshes.iter() {
        total += 1;
        let shown = inherited.is_some_and(|v| v.get());
        visible += u32::from(shown);
        let key = name.map_or("<unnamed>", |n| n.as_str());
        let key: String = key.chars().take(18).collect();
        let entry = by_name.entry(key).or_default();
        entry.0 += 1;
        entry.1 += u32::from(shown);
    }
    let mut rows: Vec<(String, (u32, u32))> = by_name.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let top: Vec<String> = rows
        .iter()
        .take(12)
        .map(|(name, (count, shown))| format!("{name}={count}/{shown}"))
        .collect();
    info!(
        "ClientPerfMeshes total={total} visible={visible} top(name=count/visible): {}",
        top.join(" ")
    );
}

/// has no input automation to press EXIT GAME. Unset in normal play: the
/// deadline is infinite and the system is a single float compare per frame.
pub fn exit_after_deadline(
    time: Res<Time<Real>>,
    mut exit: MessageWriter<AppExit>,
    mut deadline: Local<Option<f64>>,
) {
    let deadline = *deadline.get_or_insert_with(|| {
        std::env::var("FISTFORCE_EXIT_AFTER_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|secs| *secs > 0.0)
            .unwrap_or(f64::INFINITY)
    });
    if time.elapsed_secs_f64() >= deadline {
        info!("FISTFORCE_EXIT_AFTER_SECS reached: exiting cleanly");
        exit.write(AppExit::Success);
    }
}
