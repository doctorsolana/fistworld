use bevy::prelude::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::components::NpcArchetype;
use crate::props::PropKind;

pub const DEFAULT_MAP_ID: &str = "city_alpha";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapDefinition {
    pub map_id: String,
    pub bounds: MapBounds,
    pub terrain: MapTerrain,
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

        for (index, object) in self.objects.iter().enumerate() {
            if object.kind.trim().is_empty() {
                return Err(format!("objects[{index}] kind must not be empty"));
            }
            if object.prop_kind().is_none() {
                return Err(format!(
                    "objects[{index}] unknown kind '{}' (expected one of shared prop ids)",
                    object.kind
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
}

impl MapNpcGroup {
    pub fn zone_center_vec2(&self) -> Vec2 {
        Vec2::new(self.zone_center[0], self.zone_center[1])
    }

    pub fn zone_half_extents_vec2(&self) -> Vec2 {
        Vec2::new(self.zone_half_extents[0], self.zone_half_extents[1])
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
}
