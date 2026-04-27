use bevy::prelude::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

use crate::components::NpcArchetype;
use crate::props::PropKind;

pub const DEFAULT_MAP_ID: &str = "city_alpha";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapDefinition {
    pub map_id: String,
    pub bounds: MapBounds,
    pub terrain: MapTerrain,
    #[serde(default)]
    pub player_spawn: Option<[f32; 3]>,
    #[serde(default)]
    pub objects: Vec<MapObjectSpawn>,
    #[serde(default)]
    pub npc_groups: Vec<MapNpcGroup>,
    #[serde(default)]
    pub blockers: Vec<MapBlocker>,
}

impl MapDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if self.map_id.trim().is_empty() {
            return Err("map_id must not be empty".to_string());
        }

        self.bounds.validate()?;

        if self.terrain.height_max < self.terrain.height_min {
            return Err("terrain.height_max must be >= terrain.height_min".to_string());
        }

        if let Some(spawn) = self.player_spawn {
            if !self.bounds.contains_xz(spawn[0], spawn[2]) {
                return Err("player_spawn must be inside map bounds".to_string());
            }
        }

        for (index, object) in self.objects.iter().enumerate() {
            if object.kind.trim().is_empty() {
                return Err(format!("objects[{index}] kind must not be empty"));
            }
            if object.resolved_scene_path().is_none() {
                return Err(format!(
                    "objects[{index}] invalid kind '{}' (expected shared prop id or game_assets/*.glb[#SceneN])",
                    object.kind.trim()
                ));
            }
            if object.scale <= 0.0 {
                return Err(format!("objects[{index}] scale must be > 0"));
            }
        }

        for (index, npc_group) in self.npc_groups.iter().enumerate() {
            if npc_group.count == 0 {
                return Err(format!("npc_groups[{index}] count must be > 0"));
            }
            if npc_group.zone_half_extents[0] <= 0.0 || npc_group.zone_half_extents[1] <= 0.0 {
                return Err(format!("npc_groups[{index}] zone_half_extents must be > 0"));
            }
            if matches!(npc_group.preset, MapBehaviorPreset::PatrolRoute)
                && npc_group.route.len() < 2
            {
                return Err(format!(
                    "npc_groups[{index}] PatrolRoute requires at least 2 route points"
                ));
            }
            if npc_group.occupation.is_some() && npc_group.authored_occupation().is_none() {
                return Err(format!(
                    "npc_groups[{index}] occupation must not be empty when provided"
                ));
            }
            if npc_group.faction.is_some() && npc_group.authored_faction().is_none() {
                return Err(format!(
                    "npc_groups[{index}] faction must not be empty when provided"
                ));
            }
        }

        for (index, blocker) in self.blockers.iter().enumerate() {
            blocker
                .validate()
                .map_err(|err| format!("blockers[{index}] {err}"))?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MapBounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl MapBounds {
    pub fn validate(&self) -> Result<(), String> {
        if self.max[0] <= self.min[0] || self.max[1] <= self.min[1] {
            return Err("bounds.max must be greater than bounds.min on each axis".to_string());
        }
        Ok(())
    }

    #[inline]
    pub fn min_vec2(&self) -> Vec2 {
        Vec2::new(self.min[0], self.min[1])
    }

    #[inline]
    pub fn max_vec2(&self) -> Vec2 {
        Vec2::new(self.max[0], self.max[1])
    }

    #[inline]
    pub fn width(&self) -> f32 {
        self.max[0] - self.min[0]
    }

    #[inline]
    pub fn depth(&self) -> f32 {
        self.max[1] - self.min[1]
    }

    #[inline]
    pub fn contains_xz(&self, x: f32, z: f32) -> bool {
        x >= self.min[0] && x <= self.max[0] && z >= self.min[1] && z <= self.max[1]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapTerrain {
    pub heightmap: String,
    #[serde(default)]
    pub minimap: Option<String>,
    #[serde(default)]
    pub water_level: Option<f32>,
    #[serde(default = "default_height_min")]
    pub height_min: f32,
    #[serde(default = "default_height_max")]
    pub height_max: f32,
}

fn default_height_min() -> f32 {
    0.0
}

fn default_height_max() -> f32 {
    32.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapObjectSpawn {
    /// Either a stable shared prop id (e.g. `rock_1`) or a direct scene path
    /// (e.g. `game_assets/buildings/village/House_05.glb#Scene0`).
    pub kind: String,
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_degrees: f32,
    #[serde(default = "default_scale")]
    pub scale: f32,
}

impl MapObjectSpawn {
    pub fn position_vec3(&self) -> Vec3 {
        Vec3::new(self.position[0], self.position[1], self.position[2])
    }

    pub fn prop_kind(&self) -> Option<PropKind> {
        PropKind::from_id(self.kind.trim())
    }

    pub fn resolved_scene_path(&self) -> Option<String> {
        resolve_object_scene_path(self.kind.trim())
    }
}

fn resolve_object_scene_path(kind_or_path: &str) -> Option<String> {
    let token = kind_or_path.trim();
    if token.is_empty() {
        return None;
    }

    if let Some(kind) = PropKind::from_id(token) {
        return Some(kind.scene_path().to_string());
    }

    // Allow direct authored scene paths so new assets are placeable without enum churn.
    // This accepts any safe relative .glb path under the configured asset roots.
    let normalized = token.replace('\\', "/");
    let (path_part, scene_part) = match normalized.split_once('#') {
        Some((path, scene)) => (path.trim(), Some(scene.trim())),
        None => (normalized.trim(), None),
    };
    if !is_safe_relative_glb_path(path_part) {
        return None;
    }

    match scene_part {
        Some(scene) if !scene.is_empty() => Some(format!("{path_part}#{scene}")),
        Some(_) => None,
        None => Some(format!("{path_part}#Scene0")),
    }
}

fn is_safe_relative_glb_path(path_part: &str) -> bool {
    if path_part.is_empty() || !path_part.to_ascii_lowercase().ends_with(".glb") {
        return false;
    }
    if path_part.contains(':') {
        return false;
    }

    let path = Path::new(path_part);
    if path.is_absolute() {
        return false;
    }

    !path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir | Component::CurDir
        )
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapNpcGroup {
    pub archetype: NpcArchetype,
    pub count: u32,
    pub zone_center: [f32; 2],
    pub zone_half_extents: [f32; 2],
    pub preset: MapBehaviorPreset,
    #[serde(default)]
    pub route: Vec<[f32; 2]>,
    #[serde(default)]
    pub occupation: Option<String>,
    #[serde(default)]
    pub faction: Option<String>,
}

impl MapNpcGroup {
    pub fn zone_center_vec2(&self) -> Vec2 {
        Vec2::new(self.zone_center[0], self.zone_center[1])
    }

    pub fn zone_half_extents_vec2(&self) -> Vec2 {
        Vec2::new(self.zone_half_extents[0], self.zone_half_extents[1])
    }

    pub fn authored_occupation(&self) -> Option<&str> {
        self.occupation
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn authored_faction(&self) -> Option<&str> {
        self.faction
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MapBehaviorPreset {
    IdleWanderZone,
    PatrolRoute,
    StandAndFaceFlow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapBlocker {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl MapBlocker {
    pub fn validate(&self) -> Result<(), String> {
        if self.max[0] <= self.min[0] || self.max[1] <= self.min[1] {
            return Err("max must be greater than min".to_string());
        }
        Ok(())
    }

    pub fn contains_xz(&self, x: f32, z: f32) -> bool {
        x >= self.min[0] && x <= self.max[0] && z >= self.min[1] && z <= self.max[1]
    }
}

#[inline]
fn default_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone)]
pub struct HeightmapData {
    pub bounds: MapBounds,
    pub width: u32,
    pub height: u32,
    pub heights: Vec<f32>,
    pub water_level: Option<f32>,
    inv_bounds_width: f32,
    inv_bounds_depth: f32,
    max_x: f32,
    max_z: f32,
    width_usize: usize,
    max_x_idx: usize,
    max_z_idx: usize,
}

impl HeightmapData {
    pub fn new(
        bounds: MapBounds,
        width: u32,
        height: u32,
        heights: Vec<f32>,
        water_level: Option<f32>,
    ) -> Self {
        let width_usize = width as usize;
        let height_usize = height as usize;
        let max_x_idx = width_usize.saturating_sub(1);
        let max_z_idx = height_usize.saturating_sub(1);
        let bounds_width = bounds.width();
        let bounds_depth = bounds.depth();

        Self {
            bounds,
            width,
            height,
            heights,
            water_level,
            inv_bounds_width: if bounds_width.abs() > f32::EPSILON {
                1.0 / bounds_width
            } else {
                0.0
            },
            inv_bounds_depth: if bounds_depth.abs() > f32::EPSILON {
                1.0 / bounds_depth
            } else {
                0.0
            },
            max_x: max_x_idx as f32,
            max_z: max_z_idx as f32,
            width_usize,
            max_x_idx,
            max_z_idx,
        }
    }

    #[inline]
    pub fn contains_xz(&self, x: f32, z: f32) -> bool {
        self.bounds.contains_xz(x, z)
    }

    #[inline]
    pub fn get_water_height(&self, x: f32, z: f32) -> Option<f32> {
        let water = self.water_level?;
        (self.sample_height(x, z) < water).then_some(water)
    }

    pub fn sample_height(&self, x: f32, z: f32) -> f32 {
        if self.max_x_idx == 0 || self.max_z_idx == 0 {
            return self.heights.first().copied().unwrap_or(0.0);
        }

        let u = ((x - self.bounds.min[0]) * self.inv_bounds_width).clamp(0.0, 1.0);
        let v = ((z - self.bounds.min[1]) * self.inv_bounds_depth).clamp(0.0, 1.0);

        let fx = u * self.max_x;
        let fz = v * self.max_z;

        let x0 = fx.floor() as usize;
        let z0 = fz.floor() as usize;
        let x1 = (x0 + 1).min(self.max_x_idx);
        let z1 = (z0 + 1).min(self.max_z_idx);

        let tx = fx - x0 as f32;
        let tz = fz - z0 as f32;

        let h00 = self.heights[z0 * self.width_usize + x0];
        let h10 = self.heights[z0 * self.width_usize + x1];
        let h01 = self.heights[z1 * self.width_usize + x0];
        let h11 = self.heights[z1 * self.width_usize + x1];

        let hx0 = h00 + (h10 - h00) * tx;
        let hx1 = h01 + (h11 - h01) * tx;
        hx0 + (hx1 - hx0) * tz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_height_bilinear_center() {
        let hm = HeightmapData::new(
            MapBounds {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            },
            2,
            2,
            vec![0.0, 10.0, 20.0, 30.0],
            None,
        );

        let h = hm.sample_height(0.5, 0.5);
        assert!((h - 15.0).abs() < 1e-5);
    }

    #[test]
    fn sample_height_clamps_to_edges() {
        let hm = HeightmapData::new(
            MapBounds {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            },
            2,
            2,
            vec![0.0, 10.0, 20.0, 30.0],
            Some(5.0),
        );

        assert!((hm.sample_height(-10.0, -10.0) - 0.0).abs() < 1e-5);
        assert!((hm.sample_height(10.0, 10.0) - 30.0).abs() < 1e-5);
        assert_eq!(hm.get_water_height(-10.0, -10.0), Some(5.0));
    }

    #[test]
    fn object_scene_path_resolves_known_kind_and_custom_path() {
        let known = MapObjectSpawn {
            kind: "rock_1".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 0.0,
            scale: 1.0,
        };
        assert_eq!(
            known.resolved_scene_path().as_deref(),
            Some("game_assets/environment/rocks/Rock_1.glb#Scene0")
        );

        let custom = MapObjectSpawn {
            kind: "game_assets/buildings/village/House_05.glb".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 0.0,
            scale: 1.0,
        };
        assert_eq!(
            custom.resolved_scene_path().as_deref(),
            Some("game_assets/buildings/village/House_05.glb#Scene0")
        );
    }

    #[test]
    fn object_scene_path_rejects_invalid_custom_paths() {
        let bad_parent_dir = MapObjectSpawn {
            kind: "../assets/buildings/village/House_05.glb".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 0.0,
            scale: 1.0,
        };
        assert!(bad_parent_dir.resolved_scene_path().is_none());

        let bad_ext = MapObjectSpawn {
            kind: "game_assets/buildings/village/House_05.fbx".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 0.0,
            scale: 1.0,
        };
        assert!(bad_ext.resolved_scene_path().is_none());

        let absolute = MapObjectSpawn {
            kind: "/tmp/House_05.glb".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 0.0,
            scale: 1.0,
        };
        assert!(absolute.resolved_scene_path().is_none());
    }

    #[test]
    fn object_scene_path_accepts_relative_non_game_assets_paths() {
        let direct = MapObjectSpawn {
            kind: "buildings/village/House_05.glb".to_string(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: 0.0,
            scale: 1.0,
        };
        assert_eq!(
            direct.resolved_scene_path().as_deref(),
            Some("buildings/village/House_05.glb#Scene0")
        );
    }

    #[test]
    fn npc_group_trims_authored_metadata() {
        let group = MapNpcGroup {
            archetype: NpcArchetype::Oilman,
            count: 3,
            zone_center: [0.0, 0.0],
            zone_half_extents: [10.0, 10.0],
            preset: MapBehaviorPreset::IdleWanderZone,
            route: Vec::new(),
            occupation: Some("  Mechanic  ".to_string()),
            faction: Some("  Dock Union ".to_string()),
        };

        assert_eq!(group.authored_occupation(), Some("Mechanic"));
        assert_eq!(group.authored_faction(), Some("Dock Union"));
    }
}
