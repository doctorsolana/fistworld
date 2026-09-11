//! Geometry-only fence snapshots shared by navigation consumers. Production
//! quality changes cannot flush path caches in an otherwise stationary town.

use bevy::{ecs::system::SystemParam, prelude::*};
use shared::components::{FarmField, FarmFieldShape, PlayerPosition, PlayerRotation};
use shared::spatial::ObstacleEntry;
use std::collections::HashMap;

#[derive(Default)]
struct FenceCache {
    entries: HashMap<Entity, FenceEntry>,
}
struct FenceEntry {
    shape: Option<FarmFieldShape>,
    farm: Vec3,
    position: Vec3,
    rotation: f32,
    revision: u8,
    obstacles: Vec<ObstacleEntry>,
}

#[derive(SystemParam)]
pub(crate) struct FarmBoundarySource<'w, 's> {
    changed: Query<
        'w,
        's,
        (
            Entity,
            &'static FarmField,
            &'static PlayerPosition,
            &'static PlayerRotation,
        ),
        Or<(
            Changed<FarmField>,
            Changed<PlayerPosition>,
            Changed<PlayerRotation>,
        )>,
    >,
    removed: RemovedComponents<'w, 's, FarmField>,
    cache: Local<'s, FenceCache>,
}
impl FarmBoundarySource<'_, '_> {
    pub fn refresh(&mut self) -> bool {
        let mut dirty = false;
        for entity in self.removed.read() {
            dirty |= self.cache.entries.remove(&entity).is_some();
        }
        for (entity, field, position, rotation) in &self.changed {
            if self.cache.entries.get(&entity).is_some_and(|entry| {
                entry.shape == field.shape
                    && entry.farm == field.farmstead
                    && entry.position == position.0
                    && entry.rotation == rotation.0
                    && entry.revision == field.layout_version
            }) {
                continue;
            }
            self.cache.entries.insert(
                entity,
                FenceEntry {
                    shape: field.shape.clone(),
                    farm: field.farmstead,
                    position: position.0,
                    rotation: rotation.0,
                    revision: field.layout_version,
                    obstacles: field.ground_obstacles(position.0, rotation.0),
                },
            );
            dirty = true;
        }
        dirty
    }
    pub fn obstacles(&self) -> impl Iterator<Item = ObstacleEntry> + '_ {
        self.cache
            .entries
            .values()
            .flat_map(|entry| entry.obstacles.iter().cloned())
    }
}
