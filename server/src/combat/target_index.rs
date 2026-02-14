//! Spatial broadphase index for bullet-vs-character queries.

use bevy::prelude::*;
use std::collections::HashMap;

use shared::components::{Health, Npc, NpcPosition, Player, PlayerPosition};

use crate::player::lifecycle::{is_player_alive, RespawnTimer};

const HITTABLE_CELL_SIZE: f32 = 24.0;

#[derive(Resource)]
pub struct HittableSpatialIndex {
    cell_size: f32,
    player_cells: HashMap<(i32, i32), Vec<Entity>>,
    npc_cells: HashMap<(i32, i32), Vec<Entity>>,
    pub version: u64,
}

impl Default for HittableSpatialIndex {
    fn default() -> Self {
        Self {
            cell_size: HITTABLE_CELL_SIZE,
            player_cells: HashMap::new(),
            npc_cells: HashMap::new(),
            version: 0,
        }
    }
}

impl HittableSpatialIndex {
    #[inline]
    fn cell_key(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32,
        )
    }

    fn collect_cells_segment(
        &self,
        start: Vec3,
        end: Vec3,
        padding: f32,
        out_cells: &mut Vec<(i32, i32)>,
    ) {
        out_cells.clear();

        let min_x = start.x.min(end.x) - padding;
        let max_x = start.x.max(end.x) + padding;
        let min_z = start.z.min(end.z) - padding;
        let max_z = start.z.max(end.z) + padding;

        let min_cell_x = (min_x / self.cell_size).floor() as i32;
        let max_cell_x = (max_x / self.cell_size).floor() as i32;
        let min_cell_z = (min_z / self.cell_size).floor() as i32;
        let max_cell_z = (max_z / self.cell_size).floor() as i32;

        for cx in min_cell_x..=max_cell_x {
            for cz in min_cell_z..=max_cell_z {
                out_cells.push((cx, cz));
            }
        }
    }

    pub fn collect_player_candidates_segment(
        &self,
        start: Vec3,
        end: Vec3,
        padding: f32,
        out_cells: &mut Vec<(i32, i32)>,
        out_entities: &mut Vec<Entity>,
    ) {
        self.collect_cells_segment(start, end, padding, out_cells);
        out_entities.clear();

        for cell in out_cells.iter() {
            if let Some(entities) = self.player_cells.get(cell) {
                out_entities.extend(entities.iter().copied());
            }
        }
    }

    pub fn collect_npc_candidates_segment(
        &self,
        start: Vec3,
        end: Vec3,
        padding: f32,
        out_cells: &mut Vec<(i32, i32)>,
        out_entities: &mut Vec<Entity>,
    ) {
        self.collect_cells_segment(start, end, padding, out_cells);
        out_entities.clear();

        for cell in out_cells.iter() {
            if let Some(entities) = self.npc_cells.get(cell) {
                out_entities.extend(entities.iter().copied());
            }
        }
    }
}

/// Rebuild hittable target index once per fixed tick before combat hit checks.
pub fn sync_hittable_spatial_index(
    mut index: ResMut<HittableSpatialIndex>,
    players: Query<
        (Entity, &Health, &PlayerPosition, Option<&RespawnTimer>),
        (With<Player>, Without<Npc>),
    >,
    npcs: Query<(Entity, &Health, &NpcPosition), (With<Npc>, Without<Player>)>,
) {
    index.player_cells.clear();
    index.npc_cells.clear();

    for (entity, health, position, respawn_timer) in players.iter() {
        if !is_player_alive(health, respawn_timer) {
            continue;
        }
        let key = index.cell_key(position.0);
        index.player_cells.entry(key).or_default().push(entity);
    }

    for (entity, health, position) in npcs.iter() {
        if health.is_dead() {
            continue;
        }
        let key = index.cell_key(position.0);
        index.npc_cells.entry(key).or_default().push(entity);
    }

    index.version = index.version.wrapping_add(1);
}
