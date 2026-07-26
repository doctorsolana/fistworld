//! Grid A* pathfinding over the terrain heightfield and building obstacle grid.
//!
//! Salvaged from the deleted NPC AI. Deliberately free of any unit/agent type:
//! the search takes only `(&WorldTerrain, &SpatialObstacleGrid, from, to, &mut scratch)`,
//! so it can back whatever the tactics unit sim ends up being.
//!
//! NOTE: this is per-agent A*. Moving hundreds of units toward a shared destination
//! wants a flow field instead (one Dijkstra sweep from the goal, then every unit reads
//! a direction from the grid). Keep this for single-agent queries and formation anchors.

#![allow(dead_code)]

use bevy::prelude::*;
use shared::physics::ground_clearance_center;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

const GRID_CELL_SIZE: f32 = 2.0; // meters
const GRID_MAX_STEP: f32 = 1.2; // max height delta between neighbor cells
const GRID_MAX_NODES: usize = 4000; // hard cap per path search (safety)
const DEFAULT_PATHFINDING_REQUESTS_PER_TICK: usize = 6;

#[derive(Resource, Clone, Debug)]
pub struct PathfindingBudgetSettings {
    pub max_requests_per_tick: usize,
}

impl Default for PathfindingBudgetSettings {
    fn default() -> Self {
        let max_requests_per_tick = std::env::var("CITYSIM_PATHFINDING_REQUESTS_PER_TICK")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_PATHFINDING_REQUESTS_PER_TICK)
            .clamp(1, 128);
        Self {
            max_requests_per_tick,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GridPos {
    x: i32,
    z: i32,
}

fn world_to_grid(p: Vec3) -> GridPos {
    GridPos {
        x: (p.x / GRID_CELL_SIZE).round() as i32,
        z: (p.z / GRID_CELL_SIZE).round() as i32,
    }
}

fn grid_to_world(terrain: &WorldTerrain, g: GridPos) -> Vec3 {
    let x = g.x as f32 * GRID_CELL_SIZE;
    let z = g.z as f32 * GRID_CELL_SIZE;
    let y = terrain.get_height(x, z) + ground_clearance_center();
    Vec3::new(x, y, z)
}

fn heuristic(a: GridPos, b: GridPos) -> f32 {
    let dx = (a.x - b.x) as f32;
    let dz = (a.z - b.z) as f32;
    (dx * dx + dz * dz).sqrt()
}

#[derive(Clone, Copy, Debug)]
struct OpenNode {
    f_cost: i32,
    pos: GridPos,
}

impl Eq for OpenNode {}
impl PartialEq for OpenNode {
    fn eq(&self, other: &Self) -> bool {
        self.f_cost == other.f_cost && self.pos == other.pos
    }
}
impl Ord for OpenNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap behavior.
        other
            .f_cost
            .cmp(&self.f_cost)
            .then_with(|| self.pos.x.cmp(&other.pos.x))
            .then_with(|| self.pos.z.cmp(&other.pos.z))
    }
}
impl PartialOrd for OpenNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
pub(crate) struct PathfindingScratch {
    open: BinaryHeap<OpenNode>,
    came_from: HashMap<GridPos, GridPos>,
    g_score: HashMap<GridPos, f32>,
    height_cache: HashMap<GridPos, f32>,
}

pub fn find_path_a_star_with_scratch(
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    start_world: Vec3,
    goal_world: Vec3,
    scratch: &mut PathfindingScratch,
) -> Vec<Vec3> {
    let start = world_to_grid(start_world);
    let goal = world_to_grid(goal_world);

    if start == goal {
        return vec![goal_world];
    }

    scratch.open.clear();
    scratch.came_from.clear();
    scratch.g_score.clear();
    scratch.height_cache.clear();

    let height = |p: GridPos, terrain: &WorldTerrain, cache: &mut HashMap<GridPos, f32>| -> f32 {
        if let Some(h) = cache.get(&p) {
            return *h;
        }
        let w = grid_to_world(terrain, p);
        let h = w.y;
        cache.insert(p, h);
        h
    };

    scratch.g_score.insert(start, 0.0);
    scratch.open.push(OpenNode {
        f_cost: (heuristic(start, goal) * 1000.0) as i32,
        pos: start,
    });

    let neighbors = |p: GridPos| -> [GridPos; 8] {
        [
            GridPos { x: p.x + 1, z: p.z },
            GridPos { x: p.x - 1, z: p.z },
            GridPos { x: p.x, z: p.z + 1 },
            GridPos { x: p.x, z: p.z - 1 },
            GridPos {
                x: p.x + 1,
                z: p.z + 1,
            },
            GridPos {
                x: p.x + 1,
                z: p.z - 1,
            },
            GridPos {
                x: p.x - 1,
                z: p.z + 1,
            },
            GridPos {
                x: p.x - 1,
                z: p.z - 1,
            },
        ]
    };

    let mut expanded = 0_usize;
    while let Some(OpenNode { pos: current, .. }) = scratch.open.pop() {
        expanded += 1;
        if expanded > GRID_MAX_NODES {
            return Vec::new();
        }

        if current == goal {
            // Reconstruct path.
            let mut path = vec![current];
            let mut cur = current;
            while let Some(prev) = scratch.came_from.get(&cur).copied() {
                path.push(prev);
                cur = prev;
            }
            path.reverse();
            return path
                .into_iter()
                .map(|gp| grid_to_world(terrain, gp))
                .collect();
        }

        let current_h = height(current, terrain, &mut scratch.height_cache);

        for n in neighbors(current) {
            let n_h = height(n, terrain, &mut scratch.height_cache);
            if (n_h - current_h).abs() > GRID_MAX_STEP {
                continue;
            }

            let neighbor_xz = Vec2::new(n.x as f32 * GRID_CELL_SIZE, n.z as f32 * GRID_CELL_SIZE);
            if obstacles.point_blocked(neighbor_xz) {
                continue;
            }

            let diag = (n.x != current.x) && (n.z != current.z);
            let step_cost = if diag { std::f32::consts::SQRT_2 } else { 1.0 };

            let tentative_g = scratch
                .g_score
                .get(&current)
                .copied()
                .unwrap_or(f32::INFINITY)
                + step_cost;
            if tentative_g < scratch.g_score.get(&n).copied().unwrap_or(f32::INFINITY) {
                scratch.came_from.insert(n, current);
                scratch.g_score.insert(n, tentative_g);

                let f = tentative_g + heuristic(n, goal);
                scratch.open.push(OpenNode {
                    f_cost: (f * 1000.0) as i32,
                    pos: n,
                });
            }
        }
    }

    Vec::new()
}
