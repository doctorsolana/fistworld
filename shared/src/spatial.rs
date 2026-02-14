//! Spatial hashing for fast obstacle lookups.
//!
//! Instead of iterating over all obstacles O(n), we use a spatial hash grid
//! to get O(1) average-case lookups. This is critical for pathfinding where
//! we need to check thousands of grid cells against potentially hundreds of obstacles.

use bevy::prelude::*;
use std::collections::HashMap;

/// Size of each spatial grid cell in world units.
/// Should be roughly the size of your largest obstacle footprint.
pub const SPATIAL_CELL_SIZE: f32 = 8.0;

/// An axis-aligned bounding box (AABB) for obstacle footprints.
#[derive(Clone, Copy, Debug)]
pub struct ObstacleAABB {
    pub min: Vec2,
    pub max: Vec2,
    pub rotation: f32,
}

impl ObstacleAABB {
    /// Create from center position and half-extents.
    pub fn from_center_extents(center: Vec2, half_extents: Vec2, rotation: f32) -> Self {
        // For rotated rectangles, we compute the AABB that contains the rotated rect
        let cos_r = rotation.cos();
        let sin_r = rotation.sin();

        // Calculate the rotated corners' max extent
        let extent_x = (half_extents.x * cos_r.abs()) + (half_extents.y * sin_r.abs());
        let extent_y = (half_extents.x * sin_r.abs()) + (half_extents.y * cos_r.abs());

        Self {
            min: Vec2::new(center.x - extent_x, center.y - extent_y),
            max: Vec2::new(center.x + extent_x, center.y + extent_y),
            rotation,
        }
    }

    /// Check if a point is inside the actual rotated rectangle (not just AABB).
    pub fn contains_point(&self, point: Vec2, center: Vec2, half_extents: Vec2) -> bool {
        // Transform point to local space of the rotated rectangle
        let local = point - center;
        let cos_r = (-self.rotation).cos();
        let sin_r = (-self.rotation).sin();
        let rotated = Vec2::new(
            local.x * cos_r - local.y * sin_r,
            local.x * sin_r + local.y * cos_r,
        );

        rotated.x.abs() <= half_extents.x && rotated.y.abs() <= half_extents.y
    }
}

/// A single obstacle entry in the spatial grid.
#[derive(Clone, Debug)]
pub struct ObstacleEntry {
    /// Center position in world XZ coords.
    pub center: Vec2,
    /// Half-extents of the footprint.
    pub half_extents: Vec2,
    /// Rotation in radians.
    pub rotation: f32,
    /// Optional identifier for the obstacle type.
    pub obstacle_type: u32,
}

#[derive(Clone, Copy, Debug)]
struct InverseRotationBasis {
    cos: f32,
    sin: f32,
}

impl ObstacleEntry {
    #[inline]
    fn contains_point_with_basis(&self, point: Vec2, cos_r: f32, sin_r: f32) -> bool {
        let local = point - self.center;
        let rotated = Vec2::new(
            local.x * cos_r - local.y * sin_r,
            local.x * sin_r + local.y * cos_r,
        );

        rotated.x.abs() <= self.half_extents.x && rotated.y.abs() <= self.half_extents.y
    }

    /// Check if a point is inside this obstacle's footprint.
    pub fn contains_point(&self, point: Vec2) -> bool {
        let cos_r = (-self.rotation).cos();
        let sin_r = (-self.rotation).sin();
        self.contains_point_with_basis(point, cos_r, sin_r)
    }
}

/// Spatial hash grid for fast obstacle lookups.
///
/// This is a Bevy Resource that caches obstacle positions in a spatial hash map.
/// Instead of checking all obstacles O(n) for each pathfinding cell, we only
/// check obstacles in nearby grid cells O(1) average case.
#[derive(Resource, Default, Debug)]
pub struct SpatialObstacleGrid {
    /// Map from grid cell (x, z) to list of obstacles overlapping that cell.
    cells: HashMap<(i32, i32), Vec<usize>>,
    /// All obstacles in the grid.
    obstacles: Vec<ObstacleEntry>,
    /// Cached inverse rotation basis for each obstacle (parallel to `obstacles`).
    obstacle_inverse_basis: Vec<InverseRotationBasis>,
    /// Version number - incremented when obstacles change.
    pub version: u64,
}

impl SpatialObstacleGrid {
    /// Create a new empty spatial grid.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert world position to grid cell coordinates.
    #[inline]
    fn world_to_cell(pos: Vec2) -> (i32, i32) {
        (
            (pos.x / SPATIAL_CELL_SIZE).floor() as i32,
            (pos.y / SPATIAL_CELL_SIZE).floor() as i32,
        )
    }

    /// Clear all obstacles from the grid.
    pub fn clear(&mut self) {
        self.cells.clear();
        self.obstacles.clear();
        self.obstacle_inverse_basis.clear();
        self.version += 1;
    }

    /// Add an obstacle to the grid.
    pub fn insert(&mut self, entry: ObstacleEntry) {
        let aabb =
            ObstacleAABB::from_center_extents(entry.center, entry.half_extents, entry.rotation);

        // Find all grid cells this obstacle overlaps
        let min_cell = Self::world_to_cell(aabb.min);
        let max_cell = Self::world_to_cell(aabb.max);

        let inverse_basis = InverseRotationBasis {
            cos: (-entry.rotation).cos(),
            sin: (-entry.rotation).sin(),
        };
        let idx = self.obstacles.len();
        self.obstacles.push(entry);
        self.obstacle_inverse_basis.push(inverse_basis);

        // Insert into all overlapping cells
        for cx in min_cell.0..=max_cell.0 {
            for cz in min_cell.1..=max_cell.1 {
                self.cells.entry((cx, cz)).or_default().push(idx);
            }
        }

        self.version += 1;
    }

    /// Check if a point is inside any obstacle.
    /// This is the O(1) replacement for the old O(n) linear search.
    #[inline]
    pub fn point_blocked(&self, point: Vec2) -> bool {
        let cell = Self::world_to_cell(point);

        if let Some(indices) = self.cells.get(&cell) {
            for &idx in indices {
                let Some(entry) = self.obstacles.get(idx) else {
                    continue;
                };
                let Some(basis) = self.obstacle_inverse_basis.get(idx) else {
                    continue;
                };
                if entry.contains_point_with_basis(point, basis.cos, basis.sin) {
                    return true;
                }
            }
        }

        false
    }

    /// Get all obstacles near a point (within the same or adjacent cells).
    /// Useful for collision detection.
    pub fn get_nearby(&self, point: Vec2) -> impl Iterator<Item = &ObstacleEntry> {
        let cell = Self::world_to_cell(point);
        let mut estimated = 0usize;

        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(indices) = self.cells.get(&(cell.0 + dx, cell.1 + dz)) {
                    estimated += indices.len();
                }
            }
        }
        let mut seen = Vec::with_capacity(estimated);

        // Check 3x3 neighborhood of cells
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(indices) = self.cells.get(&(cell.0 + dx, cell.1 + dz)) {
                    seen.extend(indices.iter().copied());
                }
            }
        }

        seen.sort_unstable();
        seen.dedup();

        seen.into_iter().map(move |idx| &self.obstacles[idx])
    }

    /// Get the number of obstacles in the grid.
    pub fn len(&self) -> usize {
        self.obstacles.len()
    }

    /// Check if the grid is empty.
    pub fn is_empty(&self) -> bool {
        self.obstacles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_grid_basic() {
        let mut grid = SpatialObstacleGrid::new();

        // Add a 4x4 obstacle at origin
        grid.insert(ObstacleEntry {
            center: Vec2::ZERO,
            half_extents: Vec2::splat(2.0),
            rotation: 0.0,
            obstacle_type: 0,
        });

        // Point inside should be blocked
        assert!(grid.point_blocked(Vec2::new(1.0, 1.0)));

        // Point outside should not be blocked
        assert!(!grid.point_blocked(Vec2::new(10.0, 10.0)));
    }

    #[test]
    fn test_spatial_grid_rotated() {
        let mut grid = SpatialObstacleGrid::new();

        // Add a 4x2 obstacle rotated 45 degrees
        grid.insert(ObstacleEntry {
            center: Vec2::ZERO,
            half_extents: Vec2::new(2.0, 1.0),
            rotation: std::f32::consts::FRAC_PI_4, // 45 degrees
            obstacle_type: 0,
        });

        // Point at origin should be blocked
        assert!(grid.point_blocked(Vec2::ZERO));
    }

    #[test]
    fn test_get_nearby_dedupes_indices() {
        let mut grid = SpatialObstacleGrid::new();
        grid.insert(ObstacleEntry {
            center: Vec2::ZERO,
            half_extents: Vec2::splat(12.0),
            rotation: 0.0,
            obstacle_type: 0,
        });

        let nearby: Vec<&ObstacleEntry> = grid.get_nearby(Vec2::ZERO).collect();
        assert_eq!(nearby.len(), 1);
    }
}
