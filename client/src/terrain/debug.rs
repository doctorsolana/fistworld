use crate::render::systems::ClientWorldRoot;
use crate::ui::DebugPerfSettings;
use bevy::asset::AssetEvent;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::diagnostic::{DiagnosticsStore, SystemInformationDiagnosticsPlugin};
use bevy::ecs::message::MessageReader;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::materials::{TerrainRenderAssets, TerrainSplatExtension, TerrainSplatMaterial};
use super::paint::build_weightmap_from_weights;

/// Resource tracking which chunks are currently loaded.
#[derive(Resource, Default, Clone)]
pub struct TerrainDebugSettings {
    pub mode: u32,
}

#[derive(Resource, Default)]
pub struct TerrainWarmupState {
    pub done: bool,
}

#[derive(Component)]
pub(super) struct WarmupTerrain {
    weightmap: Handle<Image>,
    material: Handle<TerrainSplatMaterial>,
    mesh: Handle<Mesh>,
}

#[derive(Component)]
pub(super) struct WarmupTimer(Timer);

/// Per-frame perf counters for hitch logging.
#[derive(Resource, Default, Clone)]
pub struct PerfHitchStats {
    pub terrain_spawn_ms: f32,
    pub terrain_finalize_ms: f32,
    pub terrain_regen_ms: f32,
    pub terrain_update_ms: f32,
    pub terrain_paint_ms: f32,
    pub terrain_delta_ms: f32,
    pub props_spawn_ms: f32,
    pub building_spawn_ms: f32,
    pub structure_spawn_ms: f32,
    pub terrain_chunks_spawned: u32,
    pub terrain_chunks_finalized: u32,
    pub terrain_chunks_regen: u32,
    pub terrain_chunks_unloaded: u32,
    pub paint_ops_added: u32,
    pub paint_chunks_updated: u32,
    pub delta_chunks_ingested: u32,
    pub props_chunks_spawned: u32,
    pub props_instances_spawned: u32,
    pub building_visuals_spawned: u32,
    pub building_visuals_spawned_scene: u32,
    pub building_visuals_spawned_instanced: u32,
    pub building_visuals_swapped_to_instanced: u32,
    pub structure_chunks_spawned: u32,
    pub asset_events_mesh: u32,
    pub asset_events_image: u32,
    pub asset_events_std_material: u32,
    pub asset_events_scene: u32,
}

#[derive(Resource, Clone, Copy)]
pub struct TerrainPerfLogConfig {
    pub detailed_hitches: bool,
    pub hitch_threshold_ms: f64,
}

impl Default for TerrainPerfLogConfig {
    fn default() -> Self {
        let detailed = crate::profiling::hitch_profiling_enabled()
            || crate::profiling::env_flag("FISTFORCE_HITCH_DETAIL");
        let hitch_threshold_ms =
            crate::profiling::env_f32("FISTFORCE_HITCH_THRESHOLD_MS", 35.0).max(1.0) as f64;
        Self {
            detailed_hitches: detailed,
            hitch_threshold_ms,
        }
    }
}

const DEBUG_TERRAIN_KEY: KeyCode = KeyCode::F9;

pub(super) fn handle_terrain_debug_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut debug_settings: ResMut<TerrainDebugSettings>,
) {
    if keyboard.just_pressed(DEBUG_TERRAIN_KEY) {
        debug_settings.mode = (debug_settings.mode + 1) % 4;
        info!(
            "Terrain debug mode: {}",
            terrain_debug_mode_label(debug_settings.mode)
        );
    }
}

pub(super) fn sync_terrain_debug_materials(
    debug_settings: Res<TerrainDebugSettings>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
) {
    if !debug_settings.is_changed() {
        return;
    }
    for (_id, material) in materials.iter_mut() {
        material.extension.debug_mode = debug_settings.mode;
    }
}

fn terrain_debug_mode_label(mode: u32) -> &'static str {
    match mode {
        1 => "Weights RGBA",
        2 => "Cobblestone Only",
        3 => "Grass Only",
        _ => "Lit (Normal)",
    }
}

pub(super) fn track_asset_activity(
    perf_config: Res<TerrainPerfLogConfig>,
    debug_perf: Res<DebugPerfSettings>,
    mut perf: ResMut<PerfHitchStats>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    mut material_events: MessageReader<AssetEvent<StandardMaterial>>,
    mut scene_events: MessageReader<AssetEvent<Scene>>,
) {
    if !perf_config.detailed_hitches || !debug_perf.render_diag_logging {
        return;
    }
    perf.asset_events_mesh += mesh_events.read().count() as u32;
    perf.asset_events_image += image_events.read().count() as u32;
    perf.asset_events_std_material += material_events.read().count() as u32;
    perf.asset_events_scene += scene_events.read().count() as u32;
}

pub(super) fn warmup_terrain_pipeline(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    render_assets: Option<Res<TerrainRenderAssets>>,
    debug_settings: Res<TerrainDebugSettings>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    mut warmup_state: ResMut<TerrainWarmupState>,
) {
    if warmup_state.done {
        return;
    }
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    let weights = vec![[255, 0, 0, 0]];
    let weightmap = build_weightmap_from_weights(weights, 1, &mut images);

    let material = materials.add(TerrainSplatMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            metallic: 0.0,
            reflectance: 0.2,
            ..default()
        },
        extension: TerrainSplatExtension {
            weight_map: weightmap.handle.clone(),
            albedo_array: render_assets.albedo_array.clone(),
            normal_array: render_assets.normal_array.clone(),
            layer_tiling: render_assets.layer_tiling,
            debug_mode: debug_settings.mode,
            normal_strength: 1.0,
        },
    });

    let mesh_handle = meshes.add(bevy::math::primitives::Plane3d::default());

    let entity = commands
        .spawn((
            WarmupTerrain {
                weightmap: weightmap.handle.clone(),
                material: material.clone(),
                mesh: mesh_handle.clone(),
            },
            WarmupTimer(Timer::from_seconds(0.25, TimerMode::Once)),
            Mesh3d(mesh_handle),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::new(0.0, -10000.0, 0.0)),
            Visibility::Visible,
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    commands.entity(world_root).add_child(entity);

    warmup_state.done = true;
    info!("Terrain pipeline warmup queued.");
}

pub(super) fn cleanup_warmup_terrain(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut query: Query<(Entity, &mut WarmupTimer, &WarmupTerrain)>,
) {
    for (entity, mut timer, warmup) in query.iter_mut() {
        timer.0.tick(time.delta());
        if timer.0.is_finished() {
            meshes.remove(warmup.mesh.id());
            materials.remove(warmup.material.id());
            images.remove(warmup.weightmap.id());
            commands.entity(entity).despawn();
        }
    }
}

pub(super) fn reset_perf_hitch_stats(mut perf: ResMut<PerfHitchStats>) {
    *perf = PerfHitchStats::default();
}

pub(super) fn log_perf_hitch_stats(
    perf: Res<PerfHitchStats>,
    perf_config: Res<TerrainPerfLogConfig>,
    debug_perf: Res<DebugPerfSettings>,
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    meshes: Res<Assets<Mesh>>,
    std_materials: Res<Assets<StandardMaterial>>,
    terrain_materials: Res<Assets<TerrainSplatMaterial>>,
    images: Res<Assets<Image>>,
    scenes: Res<Assets<Scene>>,
    loaded_chunks: Res<super::chunks::LoadedChunks>,
    loaded_prop_chunks: Res<crate::props::LoadedPropChunks>,
    prop_roots: Query<(), With<crate::props::EnvironmentProp>>,
    tree_roots: Query<(), With<crate::props::TreeLodRoot>>,
    pending_props: Query<(), With<crate::props::PendingPropVisibility>>,
    mut last_detail_log: Local<f64>,
) {
    if !debug_perf.render_diag_logging {
        return;
    }
    let frame_ms = time.delta_secs_f64() * 1000.0;
    if frame_ms < perf_config.hitch_threshold_ms {
        return;
    }
    info!(
        "Hitch {:.1}ms | terrain spawn {:.1}ms ({} chunks) finalize {:.1}ms ({} chunks) regen {:.1}ms ({} chunks) unload {:.1}ms ({} chunks) paint {:.1}ms ({} ops, {} chunks) delta {:.1}ms ({} chunks) props {:.1}ms ({} chunks, {} instances) buildings {:.1}ms ({} visuals, {} scene, {} instanced, {} swaps) structures {:.1}ms ({} chunks)",
        frame_ms,
        perf.terrain_spawn_ms,
        perf.terrain_chunks_spawned,
        perf.terrain_finalize_ms,
        perf.terrain_chunks_finalized,
        perf.terrain_regen_ms,
        perf.terrain_chunks_regen,
        perf.terrain_update_ms,
        perf.terrain_chunks_unloaded,
        perf.terrain_paint_ms,
        perf.paint_ops_added,
        perf.paint_chunks_updated,
        perf.terrain_delta_ms,
        perf.delta_chunks_ingested,
        perf.props_spawn_ms,
        perf.props_chunks_spawned,
        perf.props_instances_spawned,
        perf.building_spawn_ms,
        perf.building_visuals_spawned,
        perf.building_visuals_spawned_scene,
        perf.building_visuals_spawned_instanced,
        perf.building_visuals_swapped_to_instanced,
        perf.structure_spawn_ms,
        perf.structure_chunks_spawned,
    );

    if !perf_config.detailed_hitches {
        return;
    }

    // Throttle the detailed snapshot so we don't spam logs during sustained hitches.
    let now = time.elapsed_secs_f64();
    if now - *last_detail_log < 1.0 {
        return;
    }
    *last_detail_log = now;

    // Render diagnostics (CPU/GPU pass timings).
    let mut render_cpu: Vec<(String, f64)> = diagnostics
        .iter()
        .filter_map(|d| {
            let path = d.path().as_str();
            if !path.starts_with("render/") || !path.ends_with("/elapsed_cpu") {
                return None;
            }
            let v = d.smoothed()?;
            let name = path
                .strip_prefix("render/")
                .unwrap_or(path)
                .strip_suffix("/elapsed_cpu")
                .unwrap_or(path);
            Some((name.to_string(), v))
        })
        .collect();
    render_cpu.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut render_gpu: Vec<(String, f64)> = diagnostics
        .iter()
        .filter_map(|d| {
            let path = d.path().as_str();
            if !path.starts_with("render/") || !path.ends_with("/elapsed_gpu") {
                return None;
            }
            let v = d.smoothed()?;
            let name = path
                .strip_prefix("render/")
                .unwrap_or(path)
                .strip_suffix("/elapsed_gpu")
                .unwrap_or(path);
            Some((name.to_string(), v))
        })
        .collect();
    render_gpu.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if !render_cpu.is_empty() {
        let list = render_cpu
            .iter()
            .take(5)
            .map(|(name, v)| format!("{name}: {v:.2}ms"))
            .collect::<Vec<_>>()
            .join(", ");
        info!("Hitch render CPU (top): {}", list);
    } else {
        info!("Hitch render CPU: (no diagnostics available)");
    }

    if !render_gpu.is_empty() {
        let list = render_gpu
            .iter()
            .take(5)
            .map(|(name, v)| format!("{name}: {v:.2}ms"))
            .collect::<Vec<_>>()
            .join(", ");
        info!("Hitch render GPU (top): {}", list);
    }

    info!(
        "Hitch assets: meshes={} std_mats={} terrain_mats={} images={} scenes={}",
        meshes.len(),
        std_materials.len(),
        terrain_materials.len(),
        images.len(),
        scenes.len()
    );

    let entity_count = diagnostics
        .get(&bevy::diagnostic::EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    info!(
        "Hitch world: entities={:.0} terrain_chunks={} prop_chunks={} props={} tree_roots={} pending_props={}",
        entity_count,
        loaded_chunks.chunks.len(),
        loaded_prop_chunks.chunks.len(),
        prop_roots.iter().count(),
        tree_roots.iter().count(),
        pending_props.iter().count(),
    );

    info!(
        "Hitch asset events: meshes={} images={} std_mats={} scenes={}",
        perf.asset_events_mesh,
        perf.asset_events_image,
        perf.asset_events_std_material,
        perf.asset_events_scene,
    );

    if let Some(cpu) = diagnostics
        .get(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE)
        .and_then(|d| d.smoothed())
    {
        let mem = diagnostics
            .get(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE)
            .and_then(|d| d.smoothed())
            .unwrap_or(0.0);
        info!("Hitch process: cpu={:.1}% mem={:.2} GiB", cpu, mem);
    }

    let accounted = perf.terrain_spawn_ms
        + perf.terrain_finalize_ms
        + perf.terrain_regen_ms
        + perf.terrain_update_ms
        + perf.terrain_paint_ms
        + perf.terrain_delta_ms
        + perf.props_spawn_ms
        + perf.building_spawn_ms
        + perf.structure_spawn_ms;
    let unaccounted = frame_ms - accounted as f64;
    if unaccounted > frame_ms * 0.6 {
        info!(
            "Hitch unaccounted {:.1}ms (likely GPU/present stall or other systems outside tracked timers)",
            unaccounted
        );
    }
}
