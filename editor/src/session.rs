use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;

use crate::city::{PlotToolSettings, RoadToolSettings};
use shared::map::{MapDefinition, MapEditsDefinition, MapSpawnMarker, SpawnMarkerKind};
use shared::props::{PropKind, ALL_PROP_KINDS};
use shared::terrain::{ChunkCoord, TerrainLayer, WorldTerrain};

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
pub enum TerrainEditMode {
    Sculpt,
    Paint,
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
    pub grass_weight: f32,
    pub ground_cover_weight: f32,
    pub base_scale: f32,
    pub scale_jitter: f32,
    pub max_slope: f32,
    pub avoid_water: bool,
    /// Scatter only the asset currently selected in the catalog instead of
    /// the preset species pools.
    pub scatter_selected: bool,
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
            grass_weight: 0.0,
            ground_cover_weight: 0.08,
            base_scale: 1.0,
            scale_jitter: 0.28,
            max_slope: 1.35,
            avoid_water: true,
            scatter_selected: false,
        }
    }
}

impl ForestBrushSettings {
    /// One-click weight mixes ("what grows here"), independent of the
    /// species preset row.
    pub fn apply_mix(&mut self, mix: BrushMix) {
        let (tree, bush, rock, grass, ground) = match mix {
            BrushMix::Forest => (0.55, 0.25, 0.12, 0.0, 0.08),
            BrushMix::Meadow => (0.04, 0.08, 0.02, 0.68, 0.18),
            BrushMix::GrassOnly => (0.0, 0.0, 0.0, 1.0, 0.0),
            BrushMix::Rocky => (0.08, 0.12, 0.68, 0.06, 0.06),
        };
        self.tree_weight = tree;
        self.bush_weight = bush;
        self.rock_weight = rock;
        self.grass_weight = grass;
        self.ground_cover_weight = ground;
        if matches!(mix, BrushMix::Meadow | BrushMix::GrassOnly) {
            // Dense-field defaults: tufts nearly shoulder-to-shoulder. All
            // grass is one instanced mesh, so density is cheap; the short
            // grass draw distance (55m) bounds the in-view count.
            self.density_per_100m2 = self.density_per_100m2.max(match mix {
                BrushMix::GrassOnly => 12.0,
                _ => 8.0,
            });
            self.min_spacing = self.min_spacing.min(0.9);
            // NOTE: base_scale is deliberately NOT touched here — it is
            // shared across all groups, and clamping it for grass silently
            // shrank every tree painted afterwards. Grass gets its size cut
            // from forest_group_scale instead.
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BrushMix {
    Forest,
    Meadow,
    GrassOnly,
    Rocky,
}

#[derive(Resource)]
pub struct EditorUiState {
    pub tool: ToolMode,
    pub terrain_edit_mode: TerrainEditMode,
    pub terrain_mode: TerrainBrushMode,
    pub brush_radius: f32,
    pub brush_strength: f32,
    pub flatten_blend: f32,
    pub terrain_layer: TerrainLayer,
    pub paint_strength: f32,
    pub paint_softness: f32,
    pub selected_prop_index: usize,
    pub selected_spawn_kind: SpawnMarkerKind,
    pub spawn_marker_radius: f32,
    pub forest: ForestBrushSettings,
    pub prop_scale: f32,
    pub prop_rotation_degrees: f32,
    pub prop_random_yaw: bool,
    pub prop_scale_jitter: f32,
    pub prop_drag_paint: bool,
    pub prop_drag_spacing: f32,
    pub road: RoadToolSettings,
    pub plot: PlotToolSettings,
    pub prop_search: String,
    pub selected_custom_scene: Option<String>,
    pub recent_assets: Vec<RecentAsset>,
    pub show_reset_map_confirm: bool,
    pub show_exit_confirm: bool,
    /// Pending world-generation request awaiting modal confirmation.
    pub pending_generate: Option<crate::worldgen::WorldStyle>,
    /// Pending map-resize request (half extent) awaiting confirmation.
    pub pending_resize: Option<f32>,
    /// Custom-size picker window state (full size, meters).
    pub show_custom_size: bool,
    pub custom_map_size: f32,
    pub pointer_over_ui: bool,
    pub keyboard_captured: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecentAsset {
    pub scene_path: String,
    pub display_name: String,
    pub mapped_index: Option<usize>,
}

impl Default for EditorUiState {
    fn default() -> Self {
        Self {
            tool: ToolMode::Terrain,
            terrain_edit_mode: TerrainEditMode::Sculpt,
            terrain_mode: TerrainBrushMode::Raise,
            brush_radius: 6.0,
            brush_strength: 4.0,
            flatten_blend: 2.0,
            terrain_layer: TerrainLayer::Grass,
            paint_strength: 0.55,
            paint_softness: 0.35,
            selected_prop_index: 0,
            selected_spawn_kind: SpawnMarkerKind::NpcGroup,
            spawn_marker_radius: 4.0,
            forest: ForestBrushSettings::default(),
            prop_scale: 1.0,
            prop_rotation_degrees: 0.0,
            prop_random_yaw: true,
            prop_scale_jitter: 0.15,
            prop_drag_paint: false,
            prop_drag_spacing: 2.0,
            road: RoadToolSettings::default(),
            plot: PlotToolSettings::default(),
            prop_search: String::new(),
            selected_custom_scene: None,
            recent_assets: Vec::new(),
            show_reset_map_confirm: false,
            show_exit_confirm: false,
            pending_generate: None,
            pending_resize: None,
            show_custom_size: false,
            custom_map_size: 1408.0,
            pointer_over_ui: false,
            keyboard_captured: false,
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
            .map(|kind| format!("{} ({})", kind.display_name(), kind.id()))
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// The string stored in `MapObjectSpawn.kind` for the current selection:
    /// a `PropKind` id when mapped, otherwise the raw scene path.
    pub fn selected_kind_or_path(&self) -> String {
        self.selected_prop_kind()
            .map(|kind| kind.id().to_string())
            .unwrap_or_else(|| self.selected_scene_path())
    }

    pub fn note_recent_asset(&mut self, asset: RecentAsset) {
        self.recent_assets.retain(|entry| *entry != asset);
        self.recent_assets.insert(0, asset);
        self.recent_assets.truncate(8);
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
    /// Working surface-paint buffers per chunk (decoded, mutated in place by
    /// the paint brush). Synced into `map_edits.terrain_weightmaps` (RLE) at
    /// snapshot and save boundaries.
    pub paint_weights: HashMap<ChunkCoord, Vec<[u8; 4]>>,
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
            paint_weights: HashMap::new(),
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
        for (coord, weights) in &self.paint_weights {
            edits.set_weightmap_for_chunk(*coord, weights);
        }
        EditorSnapshot {
            map_definition: self.map_definition.clone(),
            map_edits: edits,
        }
    }

    /// Bake the working paint buffers into `map_edits` (RLE) — call before
    /// saving.
    pub fn sync_paint_weights_into_edits(&mut self) {
        for (coord, weights) in &self.paint_weights {
            self.map_edits.set_weightmap_for_chunk(*coord, weights);
        }
    }

    pub fn push_undo_snapshot(&mut self, terrain: &WorldTerrain) {
        // Snapshots are full map clones; cap the stack so long paint
        // sessions don't grow memory without bound.
        const MAX_UNDO_STEPS: usize = 64;
        if self.undo.len() >= MAX_UNDO_STEPS {
            self.undo.remove(0);
        }
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
    pub save_and_exit: bool,
    pub exit_without_saving: bool,
    /// Generate a whole random world (style, seed). Destructive; confirmed
    /// through a modal first.
    pub generate_world: Option<(crate::worldgen::WorldStyle, u64)>,
    /// Resize the map to this half-extent (meters). Content outside the new
    /// bounds is pruned; confirmed through a modal first.
    pub resize_map: Option<f32>,
}

/// Per-stroke state for paint-style tools: one undo snapshot per stroke and
/// distance-based stamp spacing while the button is held.
#[derive(Resource, Default)]
pub struct BrushStroke {
    pub last_stamp: Option<Vec2>,
    pub undo_pushed: bool,
}

impl BrushStroke {
    pub fn reset(&mut self) {
        self.last_stamp = None;
        self.undo_pushed = false;
    }
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
    pub weightmap: Handle<Image>,
    pub material: Handle<crate::terrain_material::EditorTerrainSplatMaterial>,
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

/// Index of this visual's object in `map_definition.objects`, kept in sync
/// by the incremental append/remove paths in `apply_visual_refresh`.
#[derive(Component, Clone, Copy)]
pub struct EditorPropIndex(pub usize);

/// Camera distance beyond which this prop visual is hidden in the editor
/// (mirrors the game's per-kind draw distance).
#[derive(Component, Clone, Copy)]
pub struct EditorPropCullDistance(pub f32);

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
