//! Spatial index for alive player positions.

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use std::collections::HashMap;

use shared::components::{Health, Player, PlayerPosition};

use crate::player::lifecycle::is_player_alive;

const PLAYER_SPATIAL_CELL_SIZE: f32 = 64.0;

#[inline]
fn cell_key(cell_size: f32, pos: Vec3) -> (i32, i32) {
    (
        (pos.x / cell_size).floor() as i32,
        (pos.z / cell_size).floor() as i32,
    )
}

/// Spatially indexed alive player positions for fast nearest-player queries.
#[derive(Resource)]
pub struct PlayerSpatialIndex {
    cell_size: f32,
    alive_cells: HashMap<(i32, i32), Vec<Vec3>>,
    alive_positions: Vec<(PeerId, Vec3)>,
    pub version: u64,
}

impl Default for PlayerSpatialIndex {
    fn default() -> Self {
        Self {
            cell_size: PLAYER_SPATIAL_CELL_SIZE,
            alive_cells: HashMap::new(),
            alive_positions: Vec::new(),
            version: 0,
        }
    }
}

impl PlayerSpatialIndex {
    #[inline]
    pub fn alive_count(&self) -> usize {
        self.alive_positions.len()
    }

    /// Return the nearest alive-player distance squared up to `max_distance`.
    pub fn nearest_alive_distance_sq(&self, pos: Vec3, max_distance: f32) -> Option<f32> {
        if self.alive_positions.is_empty() {
            return None;
        }

        let (cx, cz) = cell_key(self.cell_size, pos);
        let max_ring = ((max_distance / self.cell_size).ceil() as i32 + 1).max(0);
        let max_dist_sq = max_distance * max_distance;

        let mut best = f32::INFINITY;

        for ring in 0..=max_ring {
            if ring == 0 {
                if let Some(cell_positions) = self.alive_cells.get(&(cx, cz)) {
                    for player_pos in cell_positions {
                        let dx = player_pos.x - pos.x;
                        let dz = player_pos.z - pos.z;
                        let dist_sq = dx * dx + dz * dz;
                        if dist_sq < best {
                            best = dist_sq;
                        }
                    }
                }
            } else {
                for dx in -ring..=ring {
                    for dz in -ring..=ring {
                        if dx.abs() != ring && dz.abs() != ring {
                            continue;
                        }
                        if let Some(cell_positions) = self.alive_cells.get(&(cx + dx, cz + dz)) {
                            for player_pos in cell_positions {
                                let dx = player_pos.x - pos.x;
                                let dz = player_pos.z - pos.z;
                                let dist_sq = dx * dx + dz * dz;
                                if dist_sq < best {
                                    best = dist_sq;
                                }
                            }
                        }
                    }
                }
            }

            if best.is_finite() {
                let min_possible = (ring as f32 * self.cell_size - self.cell_size * 0.5).max(0.0);
                if min_possible * min_possible > best {
                    break;
                }
            }
        }

        (best.is_finite() && best <= max_dist_sq).then_some(best)
    }
}

/// Rebuild alive player spatial index once per fixed tick.
pub fn sync_player_spatial_index(
    mut index: ResMut<PlayerSpatialIndex>,
    players: Query<
        (
            &Player,
            &Health,
            &PlayerPosition,
            Option<&crate::player::lifecycle::RespawnTimer>,
        ),
        Without<shared::components::Npc>,
    >,
) {
    index.alive_cells.clear();
    index.alive_positions.clear();

    for (player, health, position, respawn_timer) in players.iter() {
        if !is_player_alive(health, respawn_timer) {
            continue;
        }

        let key = cell_key(index.cell_size, position.0);
        index.alive_cells.entry(key).or_default().push(position.0);
        index.alive_positions.push((player.client_id, position.0));
    }

    index.version = index.version.wrapping_add(1);
}
