use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::math::primitives::{Cuboid, Cylinder};
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use shared::map::{
    load_map, load_map_from_parts, map_definition_path, save_map_definition_atomic,
    save_map_edits_atomic, MapObjectSpawn, SpawnMarkerKind, DEFAULT_MAP_ID, MAP_EDITS_VERSION,
};
use shared::props::PropKind;
use shared::terrain::{
    apply_terrain_paint_op_to_weights, terrain_paint_op_chunk_coords, ChunkCoord, ChunkMeshData,
    TerrainLayer, TerrainPaintOp, TerrainPaintShape, WorldTerrain, CHUNK_RESOLUTION, CHUNK_SIZE,
    TERRAIN_WEIGHTMAP_RESOLUTION, VERTEX_SPACING,
};

use bevy::app::AppExit;
use bevy::window::WindowCloseRequested;

use crate::city::CityEditorState;
use crate::lighting::{EditorFillLight, EditorSunLight};
use crate::session::{
    BrushStroke, CursorTerrainHit, EditorCursorVisual, EditorEnvironmentState, EditorMainCamera,
    EditorPropCullDistance, EditorPropIndex, EditorPropPreviewVisual, EditorPropVisual,
    EditorSession, EditorSnapshot, EditorSpawnVisual, EditorUiState, EditorWaterChunk,
    EditorWaterVisual, ForestBrushPreset, ForestBrushSettings, PropPreviewState, TerrainBrushMode,
    TerrainChunkEntry, TerrainChunkRegistry, TerrainChunkVisual, TerrainEditMode, ToolMode,
    UiActionRequests, WaterChunkRegistry,
};
use crate::terrain_material::{
    create_weightmap_image, update_weightmap_image, EditorTerrainSplatExtension,
    EditorTerrainSplatMaterial, EditorTerrainTextureAssets,
};

#[derive(Resource, Default)]
pub struct VisualRefreshFlags {
    pub terrain_all: bool,
    pub terrain_chunks: HashSet<ChunkCoord>,
    pub paint_all: bool,
    pub paint_chunks: HashSet<ChunkCoord>,
    pub water_all: bool,
    pub water_chunks: HashSet<ChunkCoord>,
    /// Full despawn + respawn of every prop visual. Only for undo/load/reset.
    pub props: bool,
    /// Cheap incremental paths used by the paint tools: objects appended at
    /// the tail of the list, indices removed this frame (sorted ascending,
    /// pre-removal), and terrain-followed re-grounding.
    pub props_appended: usize,
    pub props_removed: Vec<usize>,
    pub props_reground: bool,
    pub markers: bool,
    pub city_layout: bool,
    /// Map bounds changed: despawn and respawn the entire terrain/water
    /// chunk visual set (regenerating existing meshes is not enough when
    /// the chunk GRID itself grows or shrinks).
    pub rebuild_chunks: bool,
}

pub fn setup_editor_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<EditorTerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    terrain_textures: Res<EditorTerrainTextureAssets>,
    mut terrain_registry: ResMut<TerrainChunkRegistry>,
    mut water_registry: ResMut<WaterChunkRegistry>,
    mut ui_state: ResMut<EditorUiState>,
    mut env_state: ResMut<EditorEnvironmentState>,
) {
    let map_id = std::env::var("CITYSIM_MAP_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MAP_ID.to_string());

    let loaded_map = load_map(&map_id).unwrap_or_else(|err| {
        panic!("Failed to load map '{}': {err}", map_id);
    });
    let map_path = map_definition_path(&loaded_map.map_dir);

    let world = WorldTerrain::default();
    if world.generator.active_map_id() != loaded_map.definition.map_id {
        warn!(
            "Editor map mismatch: world loaded '{}' while session map is '{}'",
            world.generator.active_map_id(),
            loaded_map.definition.map_id
        );
    }

    let mut session = EditorSession::new(
        map_id.clone(),
        loaded_map.map_dir.clone(),
        map_path,
        loaded_map.definition.clone(),
        loaded_map.edits.clone(),
    );
    // Migrate legacy stroke-history paint into baked per-chunk weightmaps;
    // the op list is cleared and the baked form is written on next save.
    if session.map_edits.bake_legacy_paint_ops(&world.generator) {
        session.mark_edits_dirty();
        info!("Baked legacy terrain paint ops into stored weightmaps");
    }
    *env_state = EditorEnvironmentState::from_map(&loaded_map.definition);

    commands.insert_resource(DirectionalLightShadowMap { size: 2048 });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.62, 0.72, 0.82),
        brightness: 16.0,
        ..default()
    });
    commands.spawn((
        Name::new("EditorSun"),
        EditorSunLight,
        DirectionalLight {
            illuminance: 24_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 360.0,
            first_cascade_far_bound: 16.0,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.1, 0.8, 0.0)),
    ));
    commands.spawn((
        Name::new("EditorFill"),
        EditorFillLight,
        DirectionalLight {
            illuminance: 900.0,
            shadow_maps_enabled: false,
            color: Color::srgb(0.80, 0.87, 0.96),
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, -0.9, 0.0)),
    ));

    let water_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.12, 0.46, 0.58, 0.36),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    water_registry.material = Some(water_material);

    spawn_all_terrain_chunks(
        &mut commands,
        &world,
        &session.map_edits,
        &env_state,
        &mut meshes,
        &mut terrain_materials,
        &mut images,
        &terrain_textures,
        &mut terrain_registry,
    );
    spawn_all_water_chunks(
        &mut commands,
        &world,
        &env_state,
        &mut meshes,
        &mut water_registry,
    );

    spawn_prop_visuals(
        &mut commands,
        &asset_server,
        &mut meshes,
        &mut materials,
        &world,
        &loaded_map.definition.objects,
        0,
    );
    spawn_spawn_visuals(
        &mut commands,
        &mut meshes,
        &mut materials,
        &world,
        &loaded_map.definition.player_spawn,
        &loaded_map.edits.spawn_markers,
    );

    let cursor_mesh = meshes.add(Cylinder::new(1.0, 0.05));
    let cursor_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.15, 0.9, 0.4, 0.35),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Name::new("BrushCursor"),
        Mesh3d(cursor_mesh),
        MeshMaterial3d(cursor_material),
        Transform::from_translation(Vec3::new(0.0, 0.1, 0.0)),
        Visibility::Hidden,
        EditorCursorVisual,
    ));

    ui_state.status = format!(
        "Loaded map '{}' ({} objects, {} edit markers)",
        map_id,
        loaded_map.definition.objects.len(),
        loaded_map.edits.spawn_markers.len()
    );

    commands.insert_resource(world);
    commands.insert_resource(session);
}

pub fn handle_editor_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut actions: ResMut<UiActionRequests>,
    mut session: ResMut<EditorSession>,
    mut world: ResMut<WorldTerrain>,
    mut env_state: ResMut<EditorEnvironmentState>,
    mut city_state: ResMut<CityEditorState>,
    mut flags: ResMut<VisualRefreshFlags>,
    mut ui_state: ResMut<EditorUiState>,
    mut exit: MessageWriter<AppExit>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        actions.save = true;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            actions.redo = true;
        } else {
            actions.undo = true;
        }
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        actions.redo = true;
    }

    // Tool hotkeys + bracket brush sizing, suppressed while egui owns the
    // keyboard (e.g. typing in the asset search box).
    if !ui_state.keyboard_captured && !ctrl {
        const TOOL_KEYS: [(KeyCode, ToolMode); 8] = [
            (KeyCode::Digit1, ToolMode::Terrain),
            (KeyCode::Digit2, ToolMode::Road),
            (KeyCode::Digit3, ToolMode::Plot),
            (KeyCode::Digit4, ToolMode::PlaceProp),
            (KeyCode::Digit5, ToolMode::ForestBrush),
            (KeyCode::Digit6, ToolMode::EraseProp),
            (KeyCode::Digit7, ToolMode::SetPlayerSpawn),
            (KeyCode::Digit8, ToolMode::PlaceSpawnMarker),
        ];
        for (key, tool) in TOOL_KEYS {
            if keys.just_pressed(key) {
                ui_state.tool = tool;
            }
        }

        let size_step = if keys.just_pressed(KeyCode::BracketLeft) {
            Some(0.8)
        } else if keys.just_pressed(KeyCode::BracketRight) {
            Some(1.25)
        } else {
            None
        };
        if let Some(step) = size_step {
            match ui_state.tool {
                ToolMode::Terrain | ToolMode::EraseProp => {
                    ui_state.brush_radius = (ui_state.brush_radius * step).clamp(1.0, 64.0);
                }
                ToolMode::ForestBrush => {
                    ui_state.forest.radius = (ui_state.forest.radius * step).clamp(4.0, 96.0);
                }
                ToolMode::PlaceSpawnMarker => {
                    ui_state.spawn_marker_radius =
                        (ui_state.spawn_marker_radius * step).clamp(1.0, 64.0);
                }
                _ => {}
            }
        }
    }

    if actions.save || actions.save_and_exit {
        session
            .map_edits
            .set_terrain_deltas_from_world(world.delta_chunks());
        session.sync_paint_weights_into_edits();
        match save_all(&mut session) {
            Ok(()) => {
                ui_state.status = "Saved map.ron + edits.ron".to_string();
                if actions.save_and_exit {
                    exit.write(AppExit::Success);
                }
            }
            Err(err) => {
                ui_state.status = format!("Save failed: {err}");
            }
        }
        actions.save = false;
        actions.save_and_exit = false;
    }

    if actions.exit_without_saving {
        exit.write(AppExit::Success);
        actions.exit_without_saving = false;
    }

    if actions.undo {
        if let Some(snapshot) = session.undo.pop() {
            let current = session.capture_snapshot(&world);
            session.redo.push(current);
            match apply_snapshot(
                snapshot,
                &mut session,
                &mut world,
                &mut env_state,
                &mut flags,
            ) {
                Ok(()) => {
                    ui_state.status = "Undo".to_string();
                }
                Err(err) => {
                    ui_state.status = format!("Undo failed: {err}");
                }
            }
        }
        actions.undo = false;
    }

    if actions.redo {
        if let Some(snapshot) = session.redo.pop() {
            let current = session.capture_snapshot(&world);
            session.undo.push(current);
            match apply_snapshot(
                snapshot,
                &mut session,
                &mut world,
                &mut env_state,
                &mut flags,
            ) {
                Ok(()) => {
                    ui_state.status = "Redo".to_string();
                }
                Err(err) => {
                    ui_state.status = format!("Redo failed: {err}");
                }
            }
        }
        actions.redo = false;
    }

    if actions.reset_map_to_blank {
        session.push_undo_snapshot(&world);
        match reset_map_to_blank(
            &mut session,
            &mut world,
            &mut env_state,
            &mut city_state,
            &mut flags,
        ) {
            Ok(()) => {
                ui_state.status = "Reset map to blank flat state".to_string();
            }
            Err(err) => {
                ui_state.status = format!("Reset failed: {err}");
            }
        }
        actions.reset_map_to_blank = false;
    }

    if let Some(half) = actions.resize_map.take() {
        session.push_undo_snapshot(&world);
        match apply_map_resize(half, &mut session, &mut world, &mut flags) {
            Ok(()) => {
                ui_state.status = format!("Resized map to {:.0}x{:.0}m", half * 2.0, half * 2.0);
            }
            Err(err) => {
                ui_state.status = format!("Resize failed: {err}");
            }
        }
    }

    if let Some((style, seed)) = actions.generate_world.take() {
        session.push_undo_snapshot(&world);
        let result = reset_map_to_blank(
            &mut session,
            &mut world,
            &mut env_state,
            &mut city_state,
            &mut flags,
        )
        .and_then(|_| {
            crate::worldgen::generate_world(
                style,
                seed,
                &mut session,
                &mut world,
                &mut env_state,
                &mut city_state,
                &mut flags,
            )
        });
        match result {
            Ok(()) => {
                ui_state.status = format!(
                    "Generated {} world (seed {seed}) — {} props",
                    style.label(),
                    session.map_definition.objects.len()
                );
            }
            Err(err) => {
                ui_state.status = format!("World generation failed: {err}");
            }
        }
    }
}

/// Resize the map bounds, pruning content that falls outside. Growing keeps
/// everything; shrinking trims props/markers/paint/deltas beyond the edge.
fn apply_map_resize(
    half: f32,
    session: &mut EditorSession,
    world: &mut WorldTerrain,
    flags: &mut VisualRefreshFlags,
) -> Result<(), String> {
    let half = half.clamp(128.0, 1408.0);
    session.map_definition.bounds = shared::map::MapBounds {
        min: [-half, -half],
        max: [half, half],
    };
    let bounds = session.map_definition.bounds;

    session
        .map_definition
        .objects
        .retain(|object| bounds.contains_xz(object.position[0], object.position[2]));
    let chunk_center_in = |coord: &ChunkCoord| {
        let cx = (coord.x as f32 + 0.5) * CHUNK_SIZE;
        let cz = (coord.z as f32 + 0.5) * CHUNK_SIZE;
        bounds.contains_xz(cx, cz)
    };
    session
        .map_edits
        .terrain_deltas
        .retain(|chunk| chunk_center_in(&chunk.coord));
    session
        .map_edits
        .terrain_weightmaps
        .retain(|chunk| chunk_center_in(&chunk.coord));
    session
        .paint_weights
        .retain(|coord, _| chunk_center_in(coord));
    session
        .map_edits
        .spawn_markers
        .retain(|marker| bounds.contains_xz(marker.position[0], marker.position[2]));
    session
        .map_edits
        .roads
        .retain(|road| road.points.iter().any(|p| bounds.contains_xz(p[0], p[1])));
    session
        .map_edits
        .plots
        .retain(|plot| bounds.contains_xz(plot.center[0], plot.center[1]));
    if let Some(spawn) = session.map_definition.player_spawn.as_mut() {
        spawn[0] = spawn[0].clamp(-half + 8.0, half - 8.0);
        spawn[2] = spawn[2].clamp(-half + 8.0, half - 8.0);
    }

    let loaded_map = load_map_from_parts(
        &session.map_dir,
        &session.map_definition,
        &session.map_edits,
    )?;
    world.reload_from_loaded_map(loaded_map);
    session.refresh_next_ids();
    session.mark_map_dirty();
    session.mark_edits_dirty();

    flags.rebuild_chunks = true;
    flags.props = true;
    flags.markers = true;
    flags.city_layout = true;
    Ok(())
}

/// Despawn + respawn the full terrain/water chunk visual set when the map
/// bounds change. Runs before apply_visual_refresh.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_chunk_visuals(
    mut commands: Commands,
    mut flags: ResMut<VisualRefreshFlags>,
    world: Res<WorldTerrain>,
    session: Res<EditorSession>,
    env_state: Res<EditorEnvironmentState>,
    terrain_textures: Res<EditorTerrainTextureAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut terrain_materials: ResMut<Assets<EditorTerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_registry: ResMut<TerrainChunkRegistry>,
    mut water_registry: ResMut<WaterChunkRegistry>,
    chunk_visuals: Query<Entity, With<TerrainChunkVisual>>,
    water_visuals: Query<Entity, With<EditorWaterVisual>>,
) {
    if !flags.rebuild_chunks {
        return;
    }
    flags.rebuild_chunks = false;
    // The fresh spawn below already covers what these flags would redo.
    flags.terrain_all = false;
    flags.terrain_chunks.clear();
    flags.paint_all = false;
    flags.paint_chunks.clear();
    flags.water_all = false;
    flags.water_chunks.clear();

    for entity in chunk_visuals.iter() {
        commands.entity(entity).despawn();
    }
    for entity in water_visuals.iter() {
        commands.entity(entity).despawn();
    }
    terrain_registry.entries.clear();
    water_registry.chunks.clear();

    spawn_all_terrain_chunks(
        &mut commands,
        &world,
        &session.map_edits,
        &env_state,
        &mut meshes,
        &mut terrain_materials,
        &mut images,
        &terrain_textures,
        &mut terrain_registry,
    );
    spawn_all_water_chunks(
        &mut commands,
        &world,
        &env_state,
        &mut meshes,
        &mut water_registry,
    );
}

fn reset_map_to_blank(
    session: &mut EditorSession,
    world: &mut WorldTerrain,
    env_state: &mut EditorEnvironmentState,
    city_state: &mut CityEditorState,
    flags: &mut VisualRefreshFlags,
) -> Result<(), String> {
    session.map_definition.terrain.height_min = 0.0;
    session.map_definition.terrain.height_max = 0.0;
    session.map_definition.terrain.water_level = None;
    session.map_definition.generated = None;
    session.map_definition.player_spawn = None;
    session.map_definition.objects.clear();
    session.map_definition.blockers.clear();

    session.map_edits.terrain_deltas.clear();
    session.map_edits.terrain_paint_ops.clear();
    session.map_edits.terrain_weightmaps.clear();
    session.paint_weights.clear();
    session.map_edits.spawn_markers.clear();
    session.map_edits.roads.clear();
    session.map_edits.plots.clear();
    session.refresh_next_ids();

    env_state.show_water = false;
    env_state.water_level = 0.0;
    city_state.draft_road_points.clear();

    let loaded_map = load_map_from_parts(
        &session.map_dir,
        &session.map_definition,
        &session.map_edits,
    )?;
    world.reload_from_loaded_map(loaded_map);

    session.mark_map_dirty();
    session.mark_edits_dirty();

    flags.terrain_all = true;
    flags.paint_all = true;
    flags.water_all = true;
    flags.props = true;
    flags.markers = true;
    flags.city_layout = true;
    Ok(())
}

/// Keep the terrain material's wet-band/caustics water uniform in sync with
/// the editor's water level preview.
pub fn sync_editor_terrain_water(
    env_state: Res<EditorEnvironmentState>,
    registry: Res<TerrainChunkRegistry>,
    mut terrain_materials: ResMut<Assets<EditorTerrainSplatMaterial>>,
    mut last: Local<Option<(bool, f32)>>,
) {
    let current = (env_state.show_water, env_state.water_level);
    if *last == Some(current) {
        return;
    }
    *last = Some(current);

    let params = if env_state.show_water {
        Vec4::new(env_state.water_level, 1.0, 0.0, 0.0)
    } else {
        Vec4::ZERO
    };
    for entry in registry.entries.values() {
        if let Some(mut material) = terrain_materials.get_mut(&entry.material) {
            material.extension.water_params = params;
        }
    }
}

/// Intercepts the window close button: exit immediately when everything is
/// saved, otherwise pop the save/discard/cancel dialog (requires
/// `close_when_requested: false` on the WindowPlugin).
pub fn handle_window_close_requested(
    mut close_requested: MessageReader<WindowCloseRequested>,
    session: Option<Res<EditorSession>>,
    mut ui_state: ResMut<EditorUiState>,
    mut exit: MessageWriter<AppExit>,
) {
    for _ in close_requested.read() {
        let dirty = session
            .as_ref()
            .map(|session| session.dirty_map || session.dirty_edits)
            .unwrap_or(false);
        if dirty {
            ui_state.show_exit_confirm = true;
        } else {
            exit.write(AppExit::Success);
        }
    }
}

pub fn handle_tool_input(
    time: Res<Time>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_hit: Res<CursorTerrainHit>,
    env_state: Res<EditorEnvironmentState>,
    mut ui_state: ResMut<EditorUiState>,
    mut world: ResMut<WorldTerrain>,
    mut session: ResMut<EditorSession>,
    mut flags: ResMut<VisualRefreshFlags>,
    mut stroke: ResMut<BrushStroke>,
) {
    if !mouse_buttons.pressed(MouseButton::Left) {
        stroke.reset();
        return;
    }
    if ui_state.pointer_over_ui {
        return;
    }

    let Some(hit) = cursor_hit.0 else {
        return;
    };

    let just_pressed = mouse_buttons.just_pressed(MouseButton::Left);
    if just_pressed {
        stroke.reset();
    }
    let cursor_xz = Vec2::new(hit.x, hit.z);

    match ui_state.tool {
        ToolMode::Terrain => {
            if ui_state.terrain_edit_mode == TerrainEditMode::Paint {
                let spacing = (ui_state.brush_radius * 0.2).max(0.5);
                let should_paint = just_pressed
                    || stroke
                        .last_stamp
                        .is_none_or(|last| last.distance(cursor_xz) >= spacing);
                if !should_paint {
                    return;
                }

                if !stroke.undo_pushed {
                    session.push_undo_snapshot(&world);
                    stroke.undo_pushed = true;
                }

                let softness = ui_state.paint_softness.clamp(0.0, 0.9);
                let inner_radius = (ui_state.brush_radius * (1.0 - softness)).max(0.1);
                let falloff = (ui_state.brush_radius - inner_radius).max(0.0);
                let shape = match stroke.last_stamp {
                    Some(last) if !just_pressed => TerrainPaintShape::Line {
                        start: last,
                        end: cursor_xz,
                        width: inner_radius * 2.0,
                    },
                    _ => TerrainPaintShape::Circle {
                        center: cursor_xz,
                        radius: inner_radius,
                    },
                };
                // Transient op: applied incrementally into the per-chunk
                // working buffers, never stored. Cost per stamp is the brush
                // footprint, independent of how much has been painted.
                let operation = TerrainPaintOp {
                    id: 1,
                    layer: ui_state.terrain_layer,
                    strength: ui_state.paint_strength.clamp(0.0, 1.0),
                    falloff,
                    shape,
                };

                for coord in terrain_paint_op_chunk_coords(&operation) {
                    if !coord.in_world_bounds() {
                        continue;
                    }
                    if !session.paint_weights.contains_key(&coord) {
                        let resolved = session.map_edits.resolve_chunk_weights(
                            &world.generator,
                            coord,
                            TERRAIN_WEIGHTMAP_RESOLUTION,
                        );
                        session.paint_weights.insert(coord, resolved);
                    }
                    let weights = session
                        .paint_weights
                        .get_mut(&coord)
                        .expect("materialized above");
                    let origin = coord.world_pos();
                    apply_terrain_paint_op_to_weights(
                        &operation,
                        Vec2::new(origin.x, origin.z),
                        weights,
                        TERRAIN_WEIGHTMAP_RESOLUTION,
                    );
                    flags.paint_chunks.insert(coord);
                }
                session.mark_edits_dirty();
                stroke.last_stamp = Some(cursor_xz);
                ui_state.status = format!(
                    "Painted {}",
                    terrain_layer_display_name(ui_state.terrain_layer)
                );
                return;
            }

            if !stroke.undo_pushed {
                session.push_undo_snapshot(&world);
                stroke.undo_pushed = true;
            }

            let player_spawn_offset = session
                .map_definition
                .player_spawn
                .map(|spawn| spawn[1] - world.get_height(spawn[0], spawn[2]));
            let marker_offsets: Vec<f32> = session
                .map_edits
                .spawn_markers
                .iter()
                .map(|marker| {
                    marker.position[1] - world.get_height(marker.position[0], marker.position[2])
                })
                .collect();

            let affected = match ui_state.terrain_mode {
                TerrainBrushMode::Raise => world.apply_additive_circle(
                    Vec2::new(hit.x, hit.z),
                    ui_state.brush_radius,
                    ui_state.brush_strength * time.delta_secs(),
                ),
                TerrainBrushMode::Lower => world.apply_additive_circle(
                    Vec2::new(hit.x, hit.z),
                    ui_state.brush_radius,
                    -ui_state.brush_strength * time.delta_secs(),
                ),
                TerrainBrushMode::Flatten => {
                    if !just_pressed {
                        Vec::new()
                    } else {
                        world.apply_flatten_rect(
                            hit,
                            Vec2::splat(ui_state.brush_radius),
                            0.0,
                            ui_state.flatten_blend,
                        )
                    }
                }
            };

            if !affected.is_empty() {
                session
                    .map_edits
                    .set_terrain_deltas_from_world(world.delta_chunks());
                session.mark_edits_dirty();
                if let (Some(spawn), Some(offset)) = (
                    session.map_definition.player_spawn.as_mut(),
                    player_spawn_offset,
                ) {
                    spawn[1] = world.get_height(spawn[0], spawn[2]) + offset;
                    session.mark_map_dirty();
                }
                for (marker, offset) in session
                    .map_edits
                    .spawn_markers
                    .iter_mut()
                    .zip(marker_offsets.into_iter())
                {
                    marker.position[1] =
                        world.get_height(marker.position[0], marker.position[2]) + offset;
                }
                if !session.map_edits.spawn_markers.is_empty() {
                    session.mark_edits_dirty();
                }
                flags.terrain_chunks.extend(affected.iter().copied());
                if env_state.show_water {
                    flags.water_chunks.extend(affected.iter().copied());
                }
                flags.props_reground = true;
                flags.markers = true;
            }
        }
        ToolMode::PlaceProp => {
            let spacing = ui_state.prop_drag_spacing.max(0.25);
            let should_place = just_pressed
                || (ui_state.prop_drag_paint
                    && stroke
                        .last_stamp
                        .is_none_or(|last| last.distance(cursor_xz) >= spacing));
            if !should_place {
                return;
            }
            if !stroke.undo_pushed {
                session.push_undo_snapshot(&world);
                stroke.undo_pushed = true;
            }

            let seed = splitmix64(
                ((hit.x.to_bits() as u64) << 32)
                    ^ hit.z.to_bits() as u64
                    ^ session.map_definition.objects.len() as u64,
            );
            let rotation_degrees = if ui_state.prop_random_yaw {
                forest_random(seed, 0, 7) * 360.0
            } else {
                ui_state.prop_rotation_degrees
            };
            let jitter = ui_state.prop_scale_jitter.clamp(0.0, 0.9);
            let scale = (ui_state.prop_scale
                * (1.0 + (forest_random(seed, 1, 11) * 2.0 - 1.0) * jitter))
                .max(0.05);

            let object = MapObjectSpawn {
                kind: ui_state.selected_kind_or_path(),
                position: [hit.x, 0.0, hit.z],
                rotation_degrees,
                scale,
            };
            session.map_definition.objects.push(object);
            session.mark_map_dirty();
            flags.props_appended += 1;
            stroke.last_stamp = Some(cursor_xz);
        }
        ToolMode::ForestBrush => {
            let stamp_spacing = (ui_state.forest.radius * 0.5).max(1.0);
            let should_stamp = stroke
                .last_stamp
                .is_none_or(|last| last.distance(cursor_xz) >= stamp_spacing);
            if !should_stamp {
                return;
            }

            let forest = ui_state.forest.clone();
            let override_kind = forest
                .scatter_selected
                .then(|| ui_state.selected_kind_or_path());
            let objects = generate_forest_brush_spawns(
                hit,
                &forest,
                override_kind.as_deref(),
                &world,
                &session.map_definition.objects,
                &env_state,
            );
            stroke.last_stamp = Some(cursor_xz);
            if objects.is_empty() {
                if just_pressed {
                    ui_state.status = "Scatter brush found no valid placements".to_string();
                }
                return;
            }

            if !stroke.undo_pushed {
                session.push_undo_snapshot(&world);
                stroke.undo_pushed = true;
            }
            let added = objects.len();
            session.map_definition.objects.extend(objects);
            session.mark_map_dirty();
            flags.props_appended += added;
            ui_state.status = format!("Scatter brush added {added} prop(s)");
        }
        ToolMode::EraseProp => {
            let radius_sq = ui_state.brush_radius * ui_state.brush_radius;
            let mut removed: Vec<usize> = session
                .map_definition
                .objects
                .iter()
                .enumerate()
                .filter_map(|(idx, object)| {
                    let dx = object.position[0] - hit.x;
                    let dz = object.position[2] - hit.z;
                    (dx * dx + dz * dz <= radius_sq).then_some(idx)
                })
                .collect();
            if removed.is_empty() {
                return;
            }

            if !stroke.undo_pushed {
                session.push_undo_snapshot(&world);
                stroke.undo_pushed = true;
            }
            let mut index = 0usize;
            session.map_definition.objects.retain(|_| {
                let keep = removed.binary_search(&index).is_err();
                index += 1;
                keep
            });
            session.mark_map_dirty();
            ui_state.status = format!("Erased {} prop(s)", removed.len());
            flags.props_removed.append(&mut removed);
        }
        ToolMode::SetPlayerSpawn => {
            if !just_pressed {
                return;
            }
            session.push_undo_snapshot(&world);
            session.map_definition.player_spawn = Some([hit.x, hit.y, hit.z]);
            session.mark_map_dirty();
            flags.markers = true;
        }
        ToolMode::PlaceSpawnMarker => {
            if !just_pressed {
                return;
            }
            session.push_undo_snapshot(&world);
            let mut marker =
                session.next_spawn_marker(ui_state.selected_spawn_kind, [hit.x, hit.y, hit.z]);
            marker.radius = ui_state.spawn_marker_radius;
            session.map_edits.spawn_markers.push(marker);
            session.mark_edits_dirty();
            flags.markers = true;
        }
        ToolMode::Road | ToolMode::Plot => {}
    }
}

#[derive(Debug, Clone, Copy)]
enum ForestPropGroup {
    Tree,
    Bush,
    Rock,
    Grass,
    GroundCover,
}

fn generate_forest_brush_spawns(
    center: Vec3,
    settings: &ForestBrushSettings,
    override_kind: Option<&str>,
    world: &WorldTerrain,
    existing_objects: &[MapObjectSpawn],
    env_state: &EditorEnvironmentState,
) -> Vec<MapObjectSpawn> {
    let radius = settings.radius.clamp(1.0, 128.0);
    let density = settings.density_per_100m2.clamp(0.0, 12.0);
    let desired_in_disc =
        ((std::f32::consts::PI * radius * radius / 100.0) * density).round() as usize;
    if desired_in_disc == 0
        || (override_kind.is_none() && forest_makeup_total(settings) <= f32::EPSILON)
    {
        return Vec::new();
    }

    let min_spacing = settings.min_spacing.max(0.0);
    let min_spacing_sq = min_spacing * min_spacing;
    let center_xz = Vec2::new(center.x, center.z);
    let mut existing_in_disc = 0usize;
    let mut occupied: Vec<Vec2> = existing_objects
        .iter()
        .filter_map(|object| {
            let pos = Vec2::new(object.position[0], object.position[2]);
            let dist_sq = pos.distance_squared(center_xz);
            if dist_sq <= radius * radius {
                existing_in_disc += 1;
            }
            (dist_sq <= (radius + min_spacing) * (radius + min_spacing)).then_some(pos)
        })
        .collect();

    // Density is a TARGET, not an increment: props already inside the disc
    // count toward it, so repainting the same spot converges instead of
    // stacking more grass forever.
    let target_count = desired_in_disc.saturating_sub(existing_in_disc).min(300);
    if target_count == 0 {
        return Vec::new();
    }

    let seed = forest_seed(center, settings.preset);
    let max_attempts = target_count.saturating_mul(14).saturating_add(48);
    let bounds = world.generator.active_map_bounds();
    let mut out = Vec::with_capacity(target_count);

    for attempt in 0..max_attempts {
        if out.len() >= target_count {
            break;
        }

        let angle = forest_random(seed, attempt as u64, 0) * std::f32::consts::TAU;
        let distance = radius * forest_random(seed, attempt as u64, 1).sqrt();
        let x = center.x + angle.cos() * distance;
        let z = center.z + angle.sin() * distance;
        if !bounds.contains_xz(x, z) {
            continue;
        }
        let point = Vec2::new(x, z);

        if occupied
            .iter()
            .any(|existing| existing.distance_squared(point) < min_spacing_sq)
        {
            continue;
        }

        let ground_y = world.get_height(x, z);
        if settings.avoid_water && env_state.show_water && ground_y <= env_state.water_level + 0.15
        {
            continue;
        }

        if terrain_slope_at(world, x, z) > settings.max_slope.max(0.05) {
            continue;
        }

        let jitter = settings.scale_jitter.clamp(0.0, 0.9);
        let random_scale = 1.0 + (forest_random(seed, attempt as u64, 5) * 2.0 - 1.0) * jitter;
        let rotation_degrees = forest_random(seed, attempt as u64, 4) * 360.0;

        let (kind, scale) = if let Some(kind) = override_kind {
            (
                kind.to_string(),
                (settings.base_scale * random_scale).max(0.05),
            )
        } else {
            let Some(group) = choose_forest_group(settings, forest_random(seed, attempt as u64, 2))
            else {
                continue;
            };
            let kind = choose_forest_kind(
                settings.preset,
                group,
                forest_random(seed, attempt as u64, 3),
            );
            (
                kind.id().to_string(),
                (settings.base_scale * forest_group_scale(group) * random_scale).max(0.05),
            )
        };

        out.push(MapObjectSpawn {
            kind,
            position: [x, 0.0, z],
            rotation_degrees,
            scale,
        });
        occupied.push(point);
    }

    out
}

fn forest_makeup_total(settings: &ForestBrushSettings) -> f32 {
    settings.tree_weight.max(0.0)
        + settings.bush_weight.max(0.0)
        + settings.rock_weight.max(0.0)
        + settings.grass_weight.max(0.0)
        + settings.ground_cover_weight.max(0.0)
}

fn choose_forest_group(settings: &ForestBrushSettings, roll: f32) -> Option<ForestPropGroup> {
    let tree = settings.tree_weight.max(0.0);
    let bush = settings.bush_weight.max(0.0);
    let rock = settings.rock_weight.max(0.0);
    let grass = settings.grass_weight.max(0.0);
    let ground = settings.ground_cover_weight.max(0.0);
    let total = tree + bush + rock + grass + ground;
    if total <= f32::EPSILON {
        return None;
    }

    let t = roll.clamp(0.0, 0.999_999) * total;
    if t < tree {
        Some(ForestPropGroup::Tree)
    } else if t < tree + bush {
        Some(ForestPropGroup::Bush)
    } else if t < tree + bush + rock {
        Some(ForestPropGroup::Rock)
    } else if t < tree + bush + rock + grass {
        Some(ForestPropGroup::Grass)
    } else {
        Some(ForestPropGroup::GroundCover)
    }
}

fn choose_forest_kind(preset: ForestBrushPreset, group: ForestPropGroup, roll: f32) -> PropKind {
    use PropKind::*;

    const BROADLEAF_TREES: &[PropKind] = &[
        Tree_01, Tree_02, Tree_08, Tree_09, Tree_10, Tree_18, Tree_29,
    ];
    const PINE_TREES: &[PropKind] = &[Pine_Tree_1, Pine_Tree_2, Pine_Tree_3, Pine_Tree_4];
    const DEAD_TREES: &[PropKind] = &[Dead_tree_1, Dead_tree_2, Dead_tree_3];
    const MIXED_TREES: &[PropKind] = &[
        Tree_01,
        Tree_02,
        Tree_08,
        Tree_09,
        Tree_10,
        Tree_18,
        Tree_29,
        Pine_Tree_1,
        Pine_Tree_2,
        Pine_Tree_3,
        Pine_Tree_4,
    ];
    const BUSHES: &[PropKind] = &[Bush_01, Bush_02, Bush_03, Bush_04];
    const ROCKS: &[PropKind] = &[Rock_1, Rock_2, Rock_3, Rock_4, Rock_5];
    const FLOWERS: &[PropKind] = &[
        Flower_01,
        Flower_02,
        Flower_03,
        Flower_04,
        Flower_05,
        Spring_Flower_06,
        Spring_Flower_07,
        Spring_Flower_08,
        Spring_Flower_09,
    ];
    // The leaf-litter meshes were deleted (corrupt: 283-357 m bounding boxes on a
    // ground-detail prop). Small flowers are the nearest surviving ground dressing.
    const LEAVES: &[PropKind] = &[Flower_05, Spring_Flower_09];
    const DEAD_GROUND: &[PropKind] = &[Flower_05, Rock_1, Rock_2, Rock_3];

    match group {
        ForestPropGroup::Tree => match preset {
            ForestBrushPreset::Mixed => choose_from(MIXED_TREES, roll),
            ForestBrushPreset::Broadleaf => choose_from(BROADLEAF_TREES, roll),
            ForestBrushPreset::Pine => choose_from(PINE_TREES, roll),
            ForestBrushPreset::Deadwood => choose_from(DEAD_TREES, roll),
        },
        ForestPropGroup::Bush => choose_from(BUSHES, roll),
        ForestPropGroup::Rock => choose_from(ROCKS, roll),
        // ONE grass model everywhere: every tuft shares a mesh + wind
        // material, so the whole field renders as a single instanced batch.
        // 06/07 (4.6k/5.7k tris) and GrassBlade stay manual-placement only.
        ForestPropGroup::Grass => Env_Grass_Tall_04,
        ForestPropGroup::GroundCover => match preset {
            ForestBrushPreset::Deadwood => choose_from(DEAD_GROUND, roll),
            _ => {
                if roll < 0.72 {
                    choose_from(FLOWERS, roll / 0.72)
                } else {
                    choose_from(LEAVES, (roll - 0.72) / 0.28)
                }
            }
        },
    }
}

fn choose_from(pool: &[PropKind], roll: f32) -> PropKind {
    let index = (roll.clamp(0.0, 0.999_999) * pool.len() as f32) as usize;
    pool[index.min(pool.len().saturating_sub(1))]
}

fn forest_group_scale(group: ForestPropGroup) -> f32 {
    match group {
        ForestPropGroup::Tree => 1.0,
        ForestPropGroup::Bush => 0.85,
        ForestPropGroup::Rock => 0.9,
        // The grass model is a 1.6m tuft cluster at scale 1.0; 0.4 paints it
        // waist-high (with the shader's 1.3x stretch on top) while trees
        // keep the shared base_scale at full size.
        ForestPropGroup::Grass => 0.4,
        ForestPropGroup::GroundCover => 0.7,
    }
}

fn terrain_slope_at(world: &WorldTerrain, x: f32, z: f32) -> f32 {
    let step = 1.5;
    let dx = (world.get_height(x + step, z) - world.get_height(x - step, z)) / (step * 2.0);
    let dz = (world.get_height(x, z + step) - world.get_height(x, z - step)) / (step * 2.0);
    Vec2::new(dx, dz).length()
}

fn forest_seed(center: Vec3, preset: ForestBrushPreset) -> u64 {
    let x = (center.x * 10.0).round() as i64 as u64;
    let z = (center.z * 10.0).round() as i64 as u64;
    splitmix64(x ^ z.rotate_left(32) ^ forest_preset_seed(preset))
}

fn forest_preset_seed(preset: ForestBrushPreset) -> u64 {
    match preset {
        ForestBrushPreset::Mixed => 0x31b1_4f2d_c9a8_0173,
        ForestBrushPreset::Broadleaf => 0x6f3d_9a41_0c7e_55aa,
        ForestBrushPreset::Pine => 0xc43a_1e98_b75f_240d,
        ForestBrushPreset::Deadwood => 0x91de_6032_57bc_11f7,
    }
}

fn forest_random(seed: u64, index: u64, salt: u64) -> f32 {
    let value = splitmix64(seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.rotate_left(19));
    ((value >> 40) as f32) / ((1u64 << 24) as f32)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

pub fn apply_visual_refresh(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<EditorTerrainSplatMaterial>>,
    world: Res<WorldTerrain>,
    session: Res<EditorSession>,
    terrain_registry: Res<TerrainChunkRegistry>,
    env_state: Res<EditorEnvironmentState>,
    mut water_registry: ResMut<WaterChunkRegistry>,
    mut flags: ResMut<VisualRefreshFlags>,
    prop_visuals: Query<Entity, With<EditorPropVisual>>,
    mut indexed_props: Query<
        (Entity, &mut EditorPropIndex, &mut Transform),
        With<EditorPropVisual>,
    >,
    marker_visuals: Query<Entity, With<EditorSpawnVisual>>,
    water_visuals: Query<Entity, With<EditorWaterVisual>>,
) {
    if flags.paint_all {
        let paint_coords: Vec<ChunkCoord> = terrain_registry.entries.keys().copied().collect();
        for coord in paint_coords {
            refresh_terrain_weightmap(
                coord,
                &world,
                &session,
                &terrain_registry,
                &mut images,
                &mut terrain_materials,
            );
        }
        flags.paint_all = false;
        flags.paint_chunks.clear();
    } else if !flags.paint_chunks.is_empty() {
        let paint_coords: Vec<ChunkCoord> = flags.paint_chunks.drain().collect();
        for coord in paint_coords {
            refresh_terrain_weightmap(
                coord,
                &world,
                &session,
                &terrain_registry,
                &mut images,
                &mut terrain_materials,
            );
        }
    }

    let mut terrain_coords = Vec::new();
    if flags.terrain_all {
        terrain_coords = terrain_registry.entries.keys().copied().collect();
        for coord in &terrain_coords {
            regenerate_chunk_mesh(*coord, &world, &terrain_registry, &mut meshes);
        }
        flags.terrain_all = false;
        flags.terrain_chunks.clear();
        flags.water_all = true;
    } else if !flags.terrain_chunks.is_empty() {
        terrain_coords = flags.terrain_chunks.drain().collect();
        for coord in &terrain_coords {
            regenerate_chunk_mesh(*coord, &world, &terrain_registry, &mut meshes);
        }
        flags.water_chunks.extend(terrain_coords.iter().copied());
    }
    if !terrain_coords.is_empty() {
        flags.water_chunks.extend(terrain_coords);
    }

    if flags.water_all {
        let water_coords: Vec<ChunkCoord> = terrain_registry.entries.keys().copied().collect();
        refresh_water_chunks(
            &mut commands,
            &world,
            &env_state,
            &mut meshes,
            &mut water_registry,
            &water_coords,
        );
        flags.water_all = false;
        flags.water_chunks.clear();
    } else if !flags.water_chunks.is_empty() {
        let water_coords: Vec<ChunkCoord> = flags.water_chunks.drain().collect();
        refresh_water_chunks(
            &mut commands,
            &world,
            &env_state,
            &mut meshes,
            &mut water_registry,
            &water_coords,
        );
    }

    let has_no_water = !env_state.show_water;
    if has_no_water && !water_registry.chunks.is_empty() {
        for entity in water_visuals.iter() {
            commands.entity(entity).despawn();
        }
        water_registry.chunks.clear();
    }

    if flags.props {
        for entity in prop_visuals.iter() {
            commands.entity(entity).despawn();
        }
        spawn_prop_visuals(
            &mut commands,
            &asset_server,
            &mut meshes,
            &mut materials,
            &world,
            &session.map_definition.objects,
            0,
        );
        flags.props = false;
        flags.props_appended = 0;
        flags.props_removed.clear();
        flags.props_reground = false;
    } else {
        // Incremental paths for the paint tools: never rebuild the whole
        // prop scene for a single stamp or erase.
        if !flags.props_removed.is_empty() {
            let removed = std::mem::take(&mut flags.props_removed);
            for (entity, mut index, _) in indexed_props.iter_mut() {
                match removed.binary_search(&index.0) {
                    Ok(_) => commands.entity(entity).despawn(),
                    Err(shift) => index.0 -= shift,
                }
            }
        }

        if flags.props_appended > 0 {
            let count = flags
                .props_appended
                .min(session.map_definition.objects.len());
            flags.props_appended = 0;
            let start = session.map_definition.objects.len() - count;
            spawn_prop_visuals(
                &mut commands,
                &asset_server,
                &mut meshes,
                &mut materials,
                &world,
                &session.map_definition.objects[start..],
                start,
            );
        }

        if flags.props_reground {
            flags.props_reground = false;
            for (_, index, mut transform) in indexed_props.iter_mut() {
                let Some(object) = session.map_definition.objects.get(index.0) else {
                    continue;
                };
                transform.translation.y =
                    world.get_height(object.position[0], object.position[2]) + object.position[1];
            }
        }
    }

    if flags.markers {
        for entity in marker_visuals.iter() {
            commands.entity(entity).despawn();
        }
        spawn_spawn_visuals(
            &mut commands,
            &mut meshes,
            &mut materials,
            &world,
            &session.map_definition.player_spawn,
            &session.map_edits.spawn_markers,
        );
        flags.markers = false;
    }
}

pub fn refresh_cursor_indicator(
    cursor_hit: Res<CursorTerrainHit>,
    ui_state: Res<EditorUiState>,
    mut query: Query<(&mut Transform, &mut Visibility), With<EditorCursorVisual>>,
) {
    let Ok((mut transform, mut visibility)) = query.single_mut() else {
        return;
    };

    let Some(hit) = cursor_hit.0 else {
        *visibility = Visibility::Hidden;
        return;
    };

    let radius = match ui_state.tool {
        ToolMode::Terrain | ToolMode::EraseProp => ui_state.brush_radius.max(0.5),
        ToolMode::ForestBrush => ui_state.forest.radius.max(0.5),
        ToolMode::PlaceSpawnMarker => ui_state.spawn_marker_radius.max(0.5),
        ToolMode::Plot => ui_state.plot.half_extents.max_element().max(0.5),
        _ => 1.0,
    };

    transform.translation = Vec3::new(hit.x, hit.y + 0.04, hit.z);
    transform.scale = Vec3::new(radius, 1.0, radius);
    *visibility = Visibility::Visible;
}

pub fn update_prop_preview_visual(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    cursor_hit: Res<CursorTerrainHit>,
    ui_state: Res<EditorUiState>,
    mut preview_state: ResMut<PropPreviewState>,
    mut preview_query: Query<&mut Transform, With<EditorPropPreviewVisual>>,
) {
    let should_preview = ui_state.tool == ToolMode::PlaceProp && cursor_hit.0.is_some();
    if !should_preview {
        if let Some(entity) = preview_state.entity.take() {
            commands.entity(entity).despawn();
        }
        preview_state.kind_id = None;
        return;
    }

    let Some(hit) = cursor_hit.0 else {
        return;
    };
    let scene_path = ui_state.selected_scene_path();
    let preview_transform = Transform::from_xyz(hit.x, hit.y, hit.z)
        .with_rotation(Quat::from_rotation_y(
            ui_state.prop_rotation_degrees.to_radians(),
        ))
        .with_scale(Vec3::splat(ui_state.prop_scale));

    let kind_changed = preview_state.kind_id.as_deref() != Some(scene_path.as_str());
    if kind_changed || preview_state.entity.is_none() {
        if let Some(entity) = preview_state.entity.take() {
            commands.entity(entity).despawn();
        }
        let entity = commands
            .spawn((
                Name::new(format!("PropPreview({})", ui_state.selected_asset_label())),
                WorldAssetRoot(asset_server.load(scene_path.clone())),
                preview_transform,
                EditorPropPreviewVisual,
            ))
            .id();
        preview_state.entity = Some(entity);
        preview_state.kind_id = Some(scene_path);
        return;
    }

    if let Some(entity) = preview_state.entity {
        if let Ok(mut transform) = preview_query.get_mut(entity) {
            *transform = preview_transform;
        }
    }
}

fn save_all(session: &mut EditorSession) -> Result<(), String> {
    session.map_edits.version = MAP_EDITS_VERSION;
    session.map_definition.validate()?;
    session.map_edits.validate()?;

    save_map_definition_atomic(&session.map_path, &session.map_definition)?;
    save_map_edits_atomic(&session.map_dir, &session.map_edits)?;
    session.dirty_map = false;
    session.dirty_edits = false;
    Ok(())
}

fn apply_snapshot(
    snapshot: EditorSnapshot,
    session: &mut EditorSession,
    world: &mut WorldTerrain,
    env_state: &mut EditorEnvironmentState,
    flags: &mut VisualRefreshFlags,
) -> Result<(), String> {
    let loaded_map = load_map_from_parts(
        &session.map_dir,
        &snapshot.map_definition,
        &snapshot.map_edits,
    )?;
    let bounds_changed = session.map_definition.bounds.min != snapshot.map_definition.bounds.min
        || session.map_definition.bounds.max != snapshot.map_definition.bounds.max;
    session.map_definition = snapshot.map_definition;
    session.map_edits = snapshot.map_edits;
    session.paint_weights.clear();
    session.refresh_next_ids();
    world.reload_from_loaded_map(loaded_map);
    env_state.show_water = session.map_definition.terrain.water_level.is_some();
    env_state.water_level = session.map_definition.terrain.water_level.unwrap_or(0.0);
    session.mark_map_dirty();
    session.mark_edits_dirty();
    flags.terrain_all = true;
    flags.paint_all = true;
    flags.water_all = true;
    flags.props = true;
    flags.markers = true;
    flags.city_layout = true;
    if bounds_changed {
        flags.rebuild_chunks = true;
    }
    Ok(())
}

fn spawn_all_terrain_chunks(
    commands: &mut Commands,
    world: &WorldTerrain,
    edits: &shared::map::MapEditsDefinition,
    env_state: &EditorEnvironmentState,
    meshes: &mut Assets<Mesh>,
    terrain_materials: &mut Assets<EditorTerrainSplatMaterial>,
    images: &mut Assets<Image>,
    terrain_textures: &EditorTerrainTextureAssets,
    registry: &mut TerrainChunkRegistry,
) {
    let bounds = world.generator.active_map_bounds();
    let min_chunk_x = (bounds.min[0] / CHUNK_SIZE).floor() as i32;
    let max_chunk_x = (bounds.max[0] / CHUNK_SIZE).floor() as i32;
    let min_chunk_z = (bounds.min[1] / CHUNK_SIZE).floor() as i32;
    let max_chunk_z = (bounds.max[1] / CHUNK_SIZE).floor() as i32;

    for chunk_x in min_chunk_x..=max_chunk_x {
        for chunk_z in min_chunk_z..=max_chunk_z {
            let coord = ChunkCoord::new(chunk_x, chunk_z);
            if !coord.in_world_bounds() {
                continue;
            }
            let mesh_data = world.generate_chunk(coord);
            let mesh = meshes.add(build_chunk_mesh(&mesh_data));
            let weights =
                edits.resolve_chunk_weights(&world.generator, coord, TERRAIN_WEIGHTMAP_RESOLUTION);
            let weightmap = images.add(create_weightmap_image(
                &weights,
                TERRAIN_WEIGHTMAP_RESOLUTION,
            ));
            let material = terrain_materials.add(EditorTerrainSplatMaterial {
                base: StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.97,
                    metallic: 0.0,
                    reflectance: 0.08,
                    ..default()
                },
                extension: EditorTerrainSplatExtension {
                    weight_map: weightmap.clone(),
                    albedo_array: terrain_textures.albedo_array.clone(),
                    normal_array: terrain_textures.normal_array.clone(),
                    layer_tiling: terrain_textures.layer_tiling,
                    debug_mode: 0,
                    normal_strength: 1.0,
                    water_params: if env_state.show_water {
                        Vec4::new(env_state.water_level, 1.0, 0.0, 0.0)
                    } else {
                        Vec4::ZERO
                    },
                },
            });
            commands.spawn((
                Name::new(format!("TerrainChunk({}, {})", coord.x, coord.z)),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(coord.world_pos()),
                TerrainChunkVisual,
            ));
            registry.entries.insert(
                coord,
                TerrainChunkEntry {
                    mesh,
                    weightmap,
                    material,
                },
            );
        }
    }
}

fn spawn_all_water_chunks(
    commands: &mut Commands,
    world: &WorldTerrain,
    env_state: &EditorEnvironmentState,
    meshes: &mut Assets<Mesh>,
    registry: &mut WaterChunkRegistry,
) {
    if !env_state.show_water {
        return;
    }
    let Some(material) = registry.material.clone() else {
        return;
    };

    for coord in world_chunks_for_map(world) {
        refresh_single_water_chunk(
            commands, world, env_state, coord, meshes, &material, registry,
        );
    }
}

fn refresh_water_chunks(
    commands: &mut Commands,
    world: &WorldTerrain,
    env_state: &EditorEnvironmentState,
    meshes: &mut Assets<Mesh>,
    registry: &mut WaterChunkRegistry,
    coords: &[ChunkCoord],
) {
    let Some(material) = registry.material.clone() else {
        return;
    };
    for coord in coords {
        if let Some(chunk) = registry.chunks.remove(coord) {
            if let Some(entity) = chunk.entity {
                commands.entity(entity).despawn();
            }
        }

        if !env_state.show_water {
            continue;
        }

        let Some(water_mesh) = build_editor_water_mesh(world, *coord, env_state.water_level) else {
            continue;
        };
        let mesh = meshes.add(water_mesh);
        let entity = commands
            .spawn((
                Name::new(format!("WaterChunk({}, {})", coord.x, coord.z)),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(coord.world_pos()),
                EditorWaterVisual,
            ))
            .id();
        registry.chunks.insert(
            *coord,
            EditorWaterChunk {
                entity: Some(entity),
            },
        );
    }
}

fn refresh_single_water_chunk(
    commands: &mut Commands,
    world: &WorldTerrain,
    env_state: &EditorEnvironmentState,
    coord: ChunkCoord,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    registry: &mut WaterChunkRegistry,
) {
    if let Some(chunk) = registry.chunks.remove(&coord) {
        if let Some(entity) = chunk.entity {
            commands.entity(entity).despawn();
        }
    }

    if !env_state.show_water {
        return;
    }

    let Some(water_mesh) = build_editor_water_mesh(world, coord, env_state.water_level) else {
        return;
    };
    let mesh = meshes.add(water_mesh);
    let entity = commands
        .spawn((
            Name::new(format!("WaterChunk({}, {})", coord.x, coord.z)),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(coord.world_pos()),
            EditorWaterVisual,
        ))
        .id();
    registry.chunks.insert(
        coord,
        EditorWaterChunk {
            entity: Some(entity),
        },
    );
}

fn world_chunks_for_map(world: &WorldTerrain) -> Vec<ChunkCoord> {
    let bounds = world.generator.active_map_bounds();
    let min_chunk_x = (bounds.min[0] / CHUNK_SIZE).floor() as i32;
    let max_chunk_x = (bounds.max[0] / CHUNK_SIZE).floor() as i32;
    let min_chunk_z = (bounds.min[1] / CHUNK_SIZE).floor() as i32;
    let max_chunk_z = (bounds.max[1] / CHUNK_SIZE).floor() as i32;

    let mut coords = Vec::new();
    for chunk_x in min_chunk_x..=max_chunk_x {
        for chunk_z in min_chunk_z..=max_chunk_z {
            let coord = ChunkCoord::new(chunk_x, chunk_z);
            if coord.in_world_bounds() {
                coords.push(coord);
            }
        }
    }
    coords
}

fn regenerate_chunk_mesh(
    coord: ChunkCoord,
    world: &WorldTerrain,
    registry: &TerrainChunkRegistry,
    meshes: &mut Assets<Mesh>,
) {
    let Some(entry) = registry.entries.get(&coord) else {
        return;
    };
    let Some(mut mesh) = meshes.get_mut(&entry.mesh) else {
        return;
    };

    let mesh_data = world.generate_chunk(coord);
    *mesh = build_chunk_mesh(&mesh_data);
}

fn refresh_terrain_weightmap(
    coord: ChunkCoord,
    world: &WorldTerrain,
    session: &EditorSession,
    registry: &TerrainChunkRegistry,
    images: &mut Assets<Image>,
    terrain_materials: &mut Assets<EditorTerrainSplatMaterial>,
) {
    let Some(entry) = registry.entries.get(&coord) else {
        return;
    };
    let Some(mut image) = images.get_mut(&entry.weightmap) else {
        return;
    };
    let resolved;
    let weights: &[[u8; 4]] = if let Some(weights) = session.paint_weights.get(&coord) {
        weights
    } else {
        resolved = session.map_edits.resolve_chunk_weights(
            &world.generator,
            coord,
            TERRAIN_WEIGHTMAP_RESOLUTION,
        );
        &resolved
    };
    update_weightmap_image(&mut image, weights);
    // Modifying an Image asset makes the renderer create a NEW GPU texture,
    // but material bind groups are only rebuilt on MATERIAL asset events —
    // without this poke the chunk keeps rendering the old texture until
    // restart. AssetMut only fires Modified on real mutable access, so
    // into_inner() is needed to mark the material modified.
    if let Some(material) = terrain_materials.get_mut(&entry.material) {
        material.into_inner();
    }
}

fn terrain_layer_display_name(layer: TerrainLayer) -> &'static str {
    match layer {
        TerrainLayer::Grass => "Grass",
        TerrainLayer::Dirt => "Dark Ground",
        TerrainLayer::Sand => "Dry Dirt",
        TerrainLayer::Cobblestone => "Cobblestone",
    }
}

fn spawn_prop_visuals(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &WorldTerrain,
    objects: &[MapObjectSpawn],
    start_index: usize,
) {
    let mut fallback: Option<(Handle<Mesh>, Handle<StandardMaterial>)> = None;

    for (offset, object) in objects.iter().enumerate() {
        let x = object.position[0];
        let z = object.position[2];
        let y = world.get_height(x, z) + object.position[1];
        let transform = Transform::from_xyz(x, y, z)
            .with_rotation(Quat::from_rotation_y(object.rotation_degrees.to_radians()))
            .with_scale(Vec3::splat(object.scale));
        let index = EditorPropIndex(start_index + offset);

        // Same draw-distance budget the game applies, so a painted meadow
        // doesn't render thousands of tufts across the whole map in-editor.
        let tuning = PropKind::from_id(&object.kind)
            .map(shared::props::default_render_tuning)
            .unwrap_or_else(shared::props::default_unmapped_render_tuning);
        let cull = EditorPropCullDistance(tuning.visible_end_distance.unwrap_or(f32::INFINITY));

        if let Some(scene_path) = object.resolved_scene_path() {
            commands.spawn((
                Name::new(format!("Prop({})", object.kind)),
                WorldAssetRoot(asset_server.load(scene_path)),
                transform,
                EditorPropVisual,
                index,
                cull,
            ));
        } else {
            let (fallback_mesh, fallback_material) = fallback.get_or_insert_with(|| {
                (
                    meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.9, 0.2, 0.2),
                        unlit: true,
                        ..default()
                    }),
                )
            });
            commands.spawn((
                Name::new("Prop(unknown)"),
                Mesh3d(fallback_mesh.clone()),
                MeshMaterial3d(fallback_material.clone()),
                transform,
                EditorPropVisual,
                index,
                cull,
            ));
        }
    }
}

/// Root-level distance culling for editor prop visuals. `Visibility` on the
/// root propagates into the GLB scene children, which per-entity
/// `VisibilityRange` would not.
pub fn cull_distant_prop_visuals(
    camera: Query<&Transform, (With<EditorMainCamera>, Without<EditorPropVisual>)>,
    mut props: Query<
        (&Transform, &EditorPropCullDistance, &mut Visibility),
        With<EditorPropVisual>,
    >,
) {
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    let camera_pos = camera_transform.translation;
    for (transform, cull, mut visibility) in props.iter_mut() {
        let in_range = transform.translation.distance_squared(camera_pos) <= cull.0 * cull.0;
        let desired = if in_range {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
    }
}

fn spawn_spawn_visuals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &WorldTerrain,
    player_spawn: &Option<[f32; 3]>,
    markers: &[shared::map::MapSpawnMarker],
) {
    let ring_mesh = meshes.add(Cylinder::new(1.0, 0.08));
    let player_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.2, 0.6, 1.0, 0.6),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let npc_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.3, 1.0, 0.35, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let poi_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.82, 0.25, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    if let Some(spawn) = player_spawn {
        let ground = world.get_height(spawn[0], spawn[2]);
        commands.spawn((
            Name::new("PlayerSpawn"),
            Mesh3d(ring_mesh.clone()),
            MeshMaterial3d(player_material.clone()),
            Transform::from_translation(Vec3::new(spawn[0], ground + 0.05, spawn[2]))
                .with_scale(Vec3::new(2.5, 1.0, 2.5)),
            EditorSpawnVisual,
        ));
    }

    for marker in markers {
        let mat = match marker.kind {
            SpawnMarkerKind::Player => player_material.clone(),
            SpawnMarkerKind::NpcGroup => npc_material.clone(),
            SpawnMarkerKind::Poi => poi_material.clone(),
        };
        let radius = marker.radius.max(0.5);
        let ground = world.get_height(marker.position[0], marker.position[2]);
        commands.spawn((
            Name::new(format!("SpawnMarker({:?})", marker.kind)),
            Mesh3d(ring_mesh.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(Vec3::new(
                marker.position[0],
                ground + 0.05,
                marker.position[2],
            ))
            .with_scale(Vec3::new(radius, 1.0, radius))
            .with_rotation(Quat::from_rotation_y(marker.rotation_degrees.to_radians())),
            EditorSpawnVisual,
        ));
    }
}

const EDITOR_WATER_SURFACE_OFFSET: f32 = 0.02;
const EDITOR_WATER_SHORE_OVERLAP: f32 = 0.12;
const EDITOR_WATER_DEPTH_MAX: f32 = 2.5;

#[derive(Clone, Copy)]
struct EditorWaterCorner {
    local_x: f32,
    local_z: f32,
    height: f32,
}

#[derive(Clone, Copy)]
struct EditorWaterVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    depth_norm: f32,
}

fn build_editor_water_mesh(
    terrain: &WorldTerrain,
    coord: ChunkCoord,
    water_level: f32,
) -> Option<Mesh> {
    let origin = coord.world_pos();
    let origin_x = origin.x;
    let origin_z = origin.z;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let waterline = water_level + EDITOR_WATER_SHORE_OVERLAP;
    let water_y = water_level + EDITOR_WATER_SURFACE_OFFSET;
    let depth_norm =
        |height: f32| ((water_level - height).max(0.0) / EDITOR_WATER_DEPTH_MAX).clamp(0.0, 1.0);
    let add_triangle = |positions: &mut Vec<[f32; 3]>,
                        normals: &mut Vec<[f32; 3]>,
                        uvs: &mut Vec<[f32; 2]>,
                        colors: &mut Vec<[f32; 4]>,
                        indices: &mut Vec<u32>,
                        a: EditorWaterVertex,
                        b: EditorWaterVertex,
                        c: EditorWaterVertex| {
        let base = positions.len() as u32;
        positions.push(a.pos);
        positions.push(b.pos);
        positions.push(c.pos);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push(a.uv);
        uvs.push(b.uv);
        uvs.push(c.uv);
        colors.push([1.0, 1.0, 1.0, a.depth_norm]);
        colors.push([1.0, 1.0, 1.0, b.depth_norm]);
        colors.push([1.0, 1.0, 1.0, c.depth_norm]);
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    };

    let make_vertex = |local_x: f32, local_z: f32, depth: f32| {
        let world_x = origin_x + local_x;
        let world_z = origin_z + local_z;
        EditorWaterVertex {
            pos: [local_x, water_y, local_z],
            uv: [world_x / CHUNK_SIZE, world_z / CHUNK_SIZE],
            depth_norm: depth,
        }
    };

    let edge_vertex = |a: EditorWaterCorner, b: EditorWaterCorner| {
        let denom = b.height - a.height;
        let mut t = if denom.abs() < 1e-6 {
            0.5
        } else {
            (waterline - a.height) / denom
        };
        t = t.clamp(0.0, 1.0);
        let local_x = a.local_x + (b.local_x - a.local_x) * t;
        let local_z = a.local_z + (b.local_z - a.local_z) * t;
        make_vertex(local_x, local_z, 0.0)
    };

    for zi in 0..(CHUNK_RESOLUTION - 1) {
        for xi in 0..(CHUNK_RESOLUTION - 1) {
            let x0 = xi as f32 * VERTEX_SPACING;
            let z0 = zi as f32 * VERTEX_SPACING;
            let x1 = (xi + 1) as f32 * VERTEX_SPACING;
            let z1 = (zi + 1) as f32 * VERTEX_SPACING;

            let h0 = terrain.get_height(origin.x + x0, origin.z + z0);
            let h1 = terrain.get_height(origin.x + x1, origin.z + z0);
            let h2 = terrain.get_height(origin.x + x1, origin.z + z1);
            let h3 = terrain.get_height(origin.x + x0, origin.z + z1);

            let c0 = EditorWaterCorner {
                local_x: x0,
                local_z: z0,
                height: h0,
            };
            let c1 = EditorWaterCorner {
                local_x: x1,
                local_z: z0,
                height: h1,
            };
            let c2 = EditorWaterCorner {
                local_x: x1,
                local_z: z1,
                height: h2,
            };
            let c3 = EditorWaterCorner {
                local_x: x0,
                local_z: z1,
                height: h3,
            };

            let w0 = h0 < waterline;
            let w1 = h1 < waterline;
            let w2 = h2 < waterline;
            let w3 = h3 < waterline;
            let mask = (w0 as u8) | ((w1 as u8) << 1) | ((w2 as u8) << 2) | ((w3 as u8) << 3);
            if mask == 0 {
                continue;
            }

            let v0 = make_vertex(c0.local_x, c0.local_z, depth_norm(c0.height));
            let v1 = make_vertex(c1.local_x, c1.local_z, depth_norm(c1.height));
            let v2 = make_vertex(c2.local_x, c2.local_z, depth_norm(c2.height));
            let v3 = make_vertex(c3.local_x, c3.local_z, depth_norm(c3.height));

            let e0 = if w0 != w1 {
                Some(edge_vertex(c0, c1))
            } else {
                None
            };
            let e1 = if w1 != w2 {
                Some(edge_vertex(c1, c2))
            } else {
                None
            };
            let e2 = if w2 != w3 {
                Some(edge_vertex(c2, c3))
            } else {
                None
            };
            let e3 = if w3 != w0 {
                Some(edge_vertex(c3, c0))
            } else {
                None
            };

            match mask {
                1 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v0,
                    e0.unwrap(),
                    e3.unwrap(),
                ),
                2 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v1,
                    e1.unwrap(),
                    e0.unwrap(),
                ),
                3 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        e1.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e1.unwrap(),
                        e3.unwrap(),
                    );
                }
                4 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v2,
                    e2.unwrap(),
                    e1.unwrap(),
                ),
                5 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e0.unwrap(),
                        e3.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v2,
                        e2.unwrap(),
                        e1.unwrap(),
                    );
                }
                6 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        v2,
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        e2.unwrap(),
                        e0.unwrap(),
                    );
                }
                7 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        v2,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v2,
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e2.unwrap(),
                        e3.unwrap(),
                    );
                }
                8 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v3,
                    e3.unwrap(),
                    e2.unwrap(),
                ),
                9 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e0.unwrap(),
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e2.unwrap(),
                        v3,
                    );
                }
                10 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        e1.unwrap(),
                        e0.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v3,
                        e3.unwrap(),
                        e2.unwrap(),
                    );
                }
                11 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        e1.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e1.unwrap(),
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e2.unwrap(),
                        v3,
                    );
                }
                12 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v2,
                        v3,
                        e3.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v2,
                        e3.unwrap(),
                        e1.unwrap(),
                    );
                }
                13 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e0.unwrap(),
                        e1.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e1.unwrap(),
                        v2,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v2,
                        v3,
                    );
                }
                14 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        v2,
                        v3,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        v3,
                        e3.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        e3.unwrap(),
                        e0.unwrap(),
                    );
                }
                15 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        v2,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v2,
                        v3,
                    );
                }
                _ => {}
            }
        }
    }

    if indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(positions),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(normals),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, VertexAttributeValues::Float32x2(uvs));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(colors),
    );
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn build_chunk_mesh(mesh_data: &ChunkMeshData) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(mesh_data.positions.clone()),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(mesh_data.normals.clone()),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        VertexAttributeValues::Float32x2(mesh_data.uvs.clone()),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(mesh_data.colors.clone()),
    );
    let tangents: Vec<[f32; 4]> = mesh_data
        .normals
        .iter()
        .map(|normal| {
            let normal = Vec3::from_array(*normal).normalize_or_zero();
            let axis = if normal.dot(Vec3::X).abs() > 0.9 {
                Vec3::Z
            } else {
                Vec3::X
            };
            let tangent = (axis - normal * normal.dot(axis)).normalize_or_zero();
            let handedness = if normal.cross(tangent).dot(Vec3::Z) < 0.0 {
                -1.0
            } else {
                1.0
            };
            [tangent.x, tangent.y, tangent.z, handedness]
        })
        .collect();
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_TANGENT,
        VertexAttributeValues::Float32x4(tangents),
    );
    mesh.insert_indices(Indices::U32(mesh_data.indices.clone()));
    mesh
}
