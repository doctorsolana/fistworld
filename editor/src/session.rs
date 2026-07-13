use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;

use crate::city::{PlotToolSettings, RoadToolSettings};
use shared::map::{MapDefinition, MapEditsDefinition, MapSpawnMarker, SpawnMarkerKind};
use shared::props::{PropKind, ALL_PROP_KINDS};
use shared::terrain::{ChunkCoord, WorldTerrain};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolMode {
    Terrain,
    PlaceProp,
    ForestBrush,
    EraseProp,
    Road,
    Plot,
    SetPlayerSpawn,
    PlaceSpawnMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainBrushMode {
    Raise,
    Lower,
    Flatten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForestBrushPreset {
    Mixed,
    Broadleaf,
    Pine,
    Deadwood,
}

#[derive(Debug, Clone)]
pub struct ForestBrushSettings {
    pub preset: ForestBrushPreset,
    pub radius: f32,
    pub density_per_100m2: f32,
    pub min_spacing: f32,
    pub tree_weight: f32,
    pub bush_weight: f32,
    pub rock_weight: f32,
    pub ground_cover_weight: f32,
    pub base_scale: f32,
    pub scale_jitter: f32,
    pub max_slope: f32,
    pub avoid_water: bool,
}

impl Default for ForestBrushSettings {
    fn default() -> Self {
        Self {
            preset: ForestBrushPreset::Mixed,
            radius: 22.0,
            density_per_100m2: 2.4,
            min_spacing: 2.6,
            tree_weight: 0.55,
            bush_weight: 0.25,
            rock_weight: 0.12,
            ground_cover_weight: 0.08,
            base_scale: 1.0,
            scale_jitter: 0.28,
            max_slope: 1.35,
            avoid_water: true,
        }
    }
}

#[derive(Resource)]
pub struct EditorUiState {
    pub tool: ToolMode,
    pub terrain_mode: TerrainBrushMode,
    pub brush_radius: f32,
    pub brush_strength: f32,
    pub flatten_blend: f32,
    pub selected_prop_index: usize,
    pub selected_spawn_kind: SpawnMarkerKind,
    pub spawn_marker_radius: f32,
    pub forest: ForestBrushSettings,
    pub prop_scale: f32,
    pub prop_rotation_degrees: f32,
    pub road: RoadToolSettings,
    pub plot: PlotToolSettings,
    pub prop_search: String,
    pub selected_custom_scene: Option<String>,
    pub show_reset_map_confirm: bool,
    pub pointer_over_ui: bool,
    pub status: String,
}

impl Default for EditorUiState {
    fn default() -> Self {
        Self {
            tool: ToolMode::Terrain,
            terrain_mode: TerrainBrushMode::Raise,
            brush_radius: 6.0,
            brush_strength: 4.0,
            flatten_blend: 2.0,
            selected_prop_index: 0,
            selected_spawn_kind: SpawnMarkerKind::NpcGroup,
            spawn_marker_radius: 4.0,
            forest: ForestBrushSettings::default(),
            prop_scale: 1.0,
            prop_rotation_degrees: 0.0,
            road: RoadToolSettings::default(),
            plot: PlotToolSettings::default(),
            prop_search: String::new(),
            selected_custom_scene: None,
            show_reset_map_confirm: false,
            pointer_over_ui: false,
            status: "Ready".to_string(),
        }
    }
}

impl EditorUiState {
    pub fn selected_prop_kind(&self) -> Option<PropKind> {
        if self.selected_custom_scene.is_some() {
            return None;
        }
        let index = self
            .selected_prop_index
            .min(ALL_PROP_KINDS.len().saturating_sub(1));
        Some(ALL_PROP_KINDS[index])
    }

    pub fn selected_scene_path(&self) -> String {
        if let Some(path) = &self.selected_custom_scene {
            return format!("{path}#Scene0");
        }
        self.selected_prop_kind()
            .map(|kind| kind.scene_path().to_string())
            .unwrap_or_else(|| ALL_PROP_KINDS[0].scene_path().to_string())
    }

    pub fn selected_asset_label(&self) -> String {
        if let Some(path) = &self.selected_custom_scene {
            return format!("{path} (direct-path)");
        }
        self.selected_prop_kind()
            .map(|kind| kind.id().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone)]
pub struct EditorSnapshot {
    pub map_definition: MapDefinition,
    pub map_edits: MapEditsDefinition,
}

#[derive(Resource)]
pub struct EditorSession {
    pub map_id: String,
    pub map_dir: PathBuf,
    pub map_path: PathBuf,
    pub map_definition: MapDefinition,
    pub map_edits: MapEditsDefinition,
    pub dirty_map: bool,
    pub dirty_edits: bool,
    pub undo: Vec<EditorSnapshot>,
    pub redo: Vec<EditorSnapshot>,
    pub next_spawn_marker_id: u64,
    pub next_road_id: u64,
    pub next_plot_id: u64,
}

impl EditorSession {
    pub fn new(
        map_id: String,
        map_dir: PathBuf,
        map_path: PathBuf,
        map_definition: MapDefinition,
        map_edits: MapEditsDefinition,
    ) -> Self {
        let mut session = Self {
            map_id,
            map_dir,
            map_path,
            map_definition,
            map_edits,
            dirty_map: false,
            dirty_edits: false,
            undo: Vec::new(),
            redo: Vec::new(),
            next_spawn_marker_id: 1,
            next_road_id: 1,
            next_plot_id: 1,
        };
        session.refresh_next_ids();
        session
    }

    pub fn mark_map_dirty(&mut self) {
        self.dirty_map = true;
    }

    pub fn mark_edits_dirty(&mut self) {
        self.dirty_edits = true;
    }

    pub fn capture_snapshot(&self, terrain: &WorldTerrain) -> EditorSnapshot {
        let mut edits = self.map_edits.clone();
        edits.set_terrain_deltas_from_world(terrain.delta_chunks());
        EditorSnapshot {
            map_definition: self.map_definition.clone(),
            map_edits: edits,
        }
    }

    pub fn push_undo_snapshot(&mut self, terrain: &WorldTerrain) {
        self.undo.push(self.capture_snapshot(terrain));
        self.redo.clear();
    }

    pub fn refresh_next_ids(&mut self) {
        self.next_spawn_marker_id =
            next_id_after(self.map_edits.spawn_markers.iter().map(|marker| marker.id));
        self.next_road_id = next_id_after(self.map_edits.roads.iter().map(|road| road.id));
        self.next_plot_id = next_id_after(self.map_edits.plots.iter().map(|plot| plot.id));
    }

    pub fn next_spawn_marker(
        &mut self,
        kind: SpawnMarkerKind,
        position: [f32; 3],
    ) -> MapSpawnMarker {
        let marker = MapSpawnMarker {
            id: self.next_spawn_marker_id,
            kind,
            position,
            rotation_degrees: 0.0,
            radius: 4.0,
        };
        self.next_spawn_marker_id = self.next_spawn_marker_id.saturating_add(1);
        marker
    }

    pub fn allocate_road_id(&mut self) -> u64 {
        let id = self.next_road_id;
        self.next_road_id = self.next_road_id.saturating_add(1);
        id
    }

    pub fn allocate_plot_id(&mut self) -> u64 {
        let id = self.next_plot_id;
        self.next_plot_id = self.next_plot_id.saturating_add(1);
        id
    }
}

#[derive(Resource, Default)]
pub struct CursorTerrainHit(pub Option<Vec3>);

#[derive(Resource, Default)]
pub struct UiActionRequests {
    pub save: bool,
    pub undo: bool,
    pub redo: bool,
    pub reset_map_to_blank: bool,
    pub finish_road_draft: bool,
    pub clear_road_draft: bool,
    pub delete_nearest_road: bool,
    pub delete_nearest_plot: bool,
}

#[derive(Resource, Default)]
pub struct PropPreviewState {
    pub entity: Option<Entity>,
    pub kind_id: Option<String>,
}

#[derive(Resource, Default)]
pub struct TerrainChunkRegistry {
    pub entries: HashMap<ChunkCoord, TerrainChunkEntry>,
}

#[derive(Debug, Clone)]
pub struct TerrainChunkEntry {
    pub mesh: Handle<Mesh>,
}

#[derive(Resource, Default)]
pub struct WaterChunkRegistry {
    pub chunks: HashMap<ChunkCoord, EditorWaterChunk>,
    pub material: Option<Handle<StandardMaterial>>,
}

fn next_id_after(ids: impl Iterator<Item = u64>) -> u64 {
    ids.max().unwrap_or(0).saturating_add(1)
}

#[derive(Debug, Clone)]
pub struct EditorWaterChunk {
    pub entity: Option<Entity>,
}

#[derive(Debug, Clone, Resource)]
pub struct EditorEnvironmentState {
    pub water_level: f32,
    pub show_water: bool,
    pub day_time_hours: f32,
}

impl Default for EditorEnvironmentState {
    fn default() -> Self {
        Self {
            water_level: 0.0,
            show_water: false,
            day_time_hours: 12.0,
        }
    }
}

impl EditorEnvironmentState {
    pub fn from_map(map: &MapDefinition) -> Self {
        Self {
            water_level: map.terrain.water_level.unwrap_or(0.0),
            show_water: map.terrain.water_level.is_some(),
            day_time_hours: 12.0,
        }
    }
}

#[derive(Component)]
pub struct TerrainChunkVisual;

#[derive(Component)]
pub struct EditorPropVisual;

#[derive(Component)]
pub struct EditorPropPreviewVisual;

#[derive(Component)]
pub struct EditorSpawnVisual;

#[derive(Component)]
pub struct EditorCursorVisual;

#[derive(Component)]
pub struct EditorMainCamera;

#[derive(Component)]
pub struct EditorWaterVisual;
