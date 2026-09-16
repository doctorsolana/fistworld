//! Exact accepted crop land shared by visual and collision prop streaming.
//! Chunk rectangles are only an index; final clearing follows the replicated
//! row boundary. Neither production quality nor observer presence changes it.

use std::collections::{HashMap, HashSet};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::components::{FarmField, FarmFieldShape, PlayerPosition, PlayerRotation};
use crate::terrain::ChunkCoord;

use super::{BuildZoneEntry, BuildingType, clearance_zones_for_building};

/// Small soil/fence-edge clearance, not the rectangular planning/terrace envelope.
pub const FARM_VEGETATION_MARGIN: f32 = 0.25;

#[derive(Clone, Debug, PartialEq)]
struct FieldClaim {
    farm: Vec3,
    origin: Vec3,
    yaw: f32,
    shape: FarmFieldShape,
}

impl FieldClaim {
    fn new(field: &FarmField, position: Vec3, yaw: f32) -> Self {
        Self {
            farm: field.farmstead,
            origin: position,
            yaw,
            // Some(empty) is an accepted absence of crop land, never a legacy box.
            shape: field
                .shape
                .clone()
                .unwrap_or_else(FarmFieldShape::legacy_rectangle),
        }
    }

    fn chunks(&self) -> HashSet<ChunkCoord> {
        let mut chunks = HashSet::new();
        let mut include = |zone: BuildZoneEntry| {
            let (x0, x1, z0, z1) = zone.chunk_bounds();
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let coord = ChunkCoord::new(x, z);
                    // Index the finite accepted footprint in full. The
                    // terrain owner clips streaming against its own map.
                    chunks.insert(coord);
                }
            }
        };
        // First/last accepted record also replaces/restores the old inferred
        // Farmstead rectangles, so their chunks must share this revision.
        for zone in clearance_zones_for_building(self.farm, BuildingType::Farmstead, self.yaw) {
            include(zone);
        }
        if self.shape.is_valid() {
            let minimum = Vec2::new(
                self.shape
                    .sections
                    .iter()
                    .map(|s| s.left)
                    .fold(f32::INFINITY, f32::min),
                self.shape.sections[0].z,
            );
            let maximum = Vec2::new(
                self.shape
                    .sections
                    .iter()
                    .map(|s| s.right)
                    .fold(f32::NEG_INFINITY, f32::max),
                self.shape.sections.last().unwrap().z,
            );
            include(BuildZoneEntry::from_rotated_rect(
                self.origin.xz()
                    + crate::rotation::local_to_world_xz((minimum + maximum) * 0.5, self.yaw),
                (maximum - minimum) * 0.5 + Vec2::splat(FARM_VEGETATION_MARGIN),
                self.yaw,
            ));
        }
        chunks
    }
}

fn farm_key(position: Vec3) -> (u32, u32) {
    let bits = |value: f32| if value == 0.0 { 0 } else { value.to_bits() };
    (bits(position.x), bits(position.z))
}

#[derive(Default)]
struct ClaimChunk {
    sources: HashSet<Entity>,
    revision: u64,
}

/// At most one immutable geometry snapshot per field, plus touched chunk buckets.
/// Empty buckets are removed; there is no growing history/dirty tombstone list.
#[derive(Default)]
pub struct FarmFieldClaimIndex {
    sources: HashMap<Entity, FieldClaim>,
    chunks: HashMap<ChunkCoord, ClaimChunk>,
    farms: HashMap<(u32, u32), usize>,
    version: u64,
    initialized: bool,
}

impl FarmFieldClaimIndex {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn chunk_revision(&self, coord: ChunkCoord) -> u64 {
        self.chunks.get(&coord).map_or(0, |chunk| chunk.revision)
    }

    pub fn has_farm(&self, position: Vec3) -> bool {
        self.farms.contains_key(&farm_key(position))
    }

    pub fn contains_point(&self, point: Vec2) -> bool {
        let coord = ChunkCoord::from_world_pos(Vec3::new(point.x, 0.0, point.y));
        self.chunks.get(&coord).is_some_and(|chunk| {
            chunk.sources.iter().any(|entity| {
                self.sources.get(entity).is_some_and(|claim| {
                    claim.shape.is_valid()
                        && claim.shape.contains_world_point(
                            point,
                            claim.origin,
                            claim.yaw,
                            FARM_VEGETATION_MARGIN,
                        )
                })
            })
        })
    }

    /// A completed farm with explicit worker records clears exactly those
    /// records. Unmigrated/no-record buildings retain the historical contract.
    pub fn building_zones(
        &self,
        position: Vec3,
        kind: BuildingType,
        yaw: f32,
    ) -> Vec<BuildZoneEntry> {
        if kind == BuildingType::Farmstead && self.has_farm(position) {
            vec![BuildZoneEntry::from_building(position, kind, yaw)]
        } else {
            clearance_zones_for_building(position, kind, yaw)
        }
    }

    pub fn building_zones_by_chunk(
        &self,
        buildings: &[(Vec3, BuildingType, f32)],
    ) -> HashMap<ChunkCoord, Vec<BuildZoneEntry>> {
        let mut chunks: HashMap<ChunkCoord, Vec<BuildZoneEntry>> = HashMap::new();
        for &(position, kind, yaw) in buildings {
            for zone in self.building_zones(position, kind, yaw) {
                let (x0, x1, z0, z1) = zone.chunk_bounds();
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        chunks.entry(ChunkCoord::new(x, z)).or_default().push(zone);
                    }
                }
            }
        }
        chunks
    }

    fn replace(&mut self, entity: Entity, next: Option<FieldClaim>) -> Vec<ChunkCoord> {
        if self.sources.get(&entity) == next.as_ref() {
            return Vec::new();
        }
        let mut touched = HashSet::new();
        if let Some(old) = self.sources.remove(&entity) {
            for coord in old.chunks() {
                if let Some(chunk) = self.chunks.get_mut(&coord) {
                    chunk.sources.remove(&entity);
                }
                touched.insert(coord);
            }
            let key = farm_key(old.farm);
            if let Some(count) = self.farms.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    self.farms.remove(&key);
                }
            }
        }
        if let Some(next) = next {
            for coord in next.chunks() {
                self.chunks.entry(coord).or_default().sources.insert(entity);
                touched.insert(coord);
            }
            *self.farms.entry(farm_key(next.farm)).or_default() += 1;
            self.sources.insert(entity, next);
        }
        self.version = self.version.wrapping_add(1).max(1);
        for coord in &touched {
            if self
                .chunks
                .get(coord)
                .is_some_and(|chunk| chunk.sources.is_empty())
            {
                self.chunks.remove(coord);
            } else if let Some(chunk) = self.chunks.get_mut(coord) {
                chunk.revision = self.version;
            }
        }
        let mut touched: Vec<_> = touched.into_iter().collect();
        touched.sort_unstable_by_key(|c| (c.x, c.z));
        touched
    }
}

type FieldGeometry = (
    Entity,
    &'static FarmField,
    &'static PlayerPosition,
    &'static PlayerRotation,
);

/// Each host embeds the index in its existing streaming resource. This reader
/// observes changed sources only after initialization and ignores quality edits.
#[derive(SystemParam)]
pub struct FarmFieldClaimChanges<'w, 's> {
    all: Query<'w, 's, FieldGeometry>,
    changed: Query<
        'w,
        's,
        FieldGeometry,
        Or<(
            Changed<FarmField>,
            Changed<PlayerPosition>,
            Changed<PlayerRotation>,
        )>,
    >,
    removed_fields: RemovedComponents<'w, 's, FarmField>,
    removed_positions: RemovedComponents<'w, 's, PlayerPosition>,
    removed_rotations: RemovedComponents<'w, 's, PlayerRotation>,
}

impl FarmFieldClaimChanges<'_, '_> {
    pub fn sync(&mut self, index: &mut FarmFieldClaimIndex) -> Vec<ChunkCoord> {
        let mut touched = HashSet::new();
        for entity in self
            .removed_fields
            .read()
            .chain(self.removed_positions.read())
            .chain(self.removed_rotations.read())
        {
            // Remove + reinsert during one frame uses the final complete source.
            if self.all.get(entity).is_err() {
                touched.extend(index.replace(entity, None));
            }
        }
        let initialized = index.initialized;
        let mut update = |(entity, field, position, rotation): (
            Entity,
            &FarmField,
            &PlayerPosition,
            &PlayerRotation,
        )| {
            let next =
                (field.farmstead.is_finite() && position.0.is_finite() && rotation.0.is_finite())
                    .then(|| FieldClaim::new(field, position.0, rotation.0));
            touched.extend(index.replace(entity, next));
        };
        if initialized {
            for source in &self.changed {
                update(source);
            }
        } else {
            for source in &self.all {
                update(source);
            }
        }
        index.initialized = true;
        let mut touched: Vec<_> = touched.into_iter().collect();
        touched.sort_unstable_by_key(|c| (c.x, c.z));
        touched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::FarmFieldSection;

    fn field(farm: Vec3, shape: Option<FarmFieldShape>) -> FarmField {
        FarmField {
            settlement: "Claim test".into(),
            farmstead: farm,
            plot_index: 0,
            quality: 1.0,
            layout_version: 0,
            shape,
        }
    }

    #[derive(Resource, Default)]
    struct Observed {
        index: FarmFieldClaimIndex,
        touched: Vec<ChunkCoord>,
    }

    fn synchronize(mut changes: FarmFieldClaimChanges, mut observed: ResMut<Observed>) {
        observed.touched = changes.sync(&mut observed.index);
    }

    #[test]
    fn rotated_taper_clears_exact_ground_not_its_bounding_rectangle() {
        let farm = Vec3::new(32.0, 0.0, 32.0);
        let origin = farm + Vec3::new(5.0, 0.0, 9.0);
        let yaw = 0.73;
        let shape = FarmFieldShape {
            sections: vec![
                FarmFieldSection {
                    z: -4.0,
                    left: -5.0,
                    right: 5.0,
                },
                FarmFieldSection {
                    z: 4.0,
                    left: -1.0,
                    right: 1.0,
                },
            ],
        };
        let mut index = FarmFieldClaimIndex::default();
        index.replace(
            Entity::from_bits(1),
            Some(FieldClaim::new(&field(farm, Some(shape)), origin, yaw)),
        );
        let world = |p| origin.xz() + crate::rotation::local_to_world_xz(p, yaw);
        assert!(index.contains_point(world(Vec2::new(0.0, 3.0))));
        assert!(
            !index.contains_point(world(Vec2::new(4.0, 3.0))),
            "clipped corner must retain its vegetation/collision"
        );
        assert_eq!(
            index
                .building_zones(farm, BuildingType::Farmstead, yaw)
                .len(),
            1
        );
    }

    #[test]
    fn empty_accepted_land_and_legacy_missing_shape_are_distinct() {
        let at = Vec3::new(20.0, 0.0, 20.0);
        let mut index = FarmFieldClaimIndex::default();
        let entity = Entity::from_bits(1);
        index.replace(
            entity,
            Some(FieldClaim::new(
                &field(at, Some(FarmFieldShape::default())),
                at,
                0.0,
            )),
        );
        assert!(!index.contains_point(at.xz()));
        assert!(
            index.has_farm(at),
            "an empty explicit claim still replaces inferred fields"
        );
        index.replace(entity, Some(FieldClaim::new(&field(at, None), at, 0.0)));
        assert!(index.contains_point(at.xz() + Vec2::new(3.9, 5.4)));
        assert!(!index.contains_point(at.xz() + Vec2::new(6.0, 0.0)));
    }

    #[test]
    fn source_changes_invalidate_old_and_new_ground_without_quality_or_remote_churn() {
        let mut app = App::new();
        app.init_resource::<Observed>()
            .add_systems(Update, synchronize);
        let at = Vec3::new(30.0, 0.0, 30.0);
        let entity = app
            .world_mut()
            .spawn((field(at, None), PlayerPosition(at), PlayerRotation(0.0)))
            .id();
        app.update();
        let home = ChunkCoord::from_world_pos(at);
        let version = app.world().resource::<Observed>().index.version();
        let revision = app
            .world()
            .resource::<Observed>()
            .index
            .chunk_revision(home);
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<FarmField>()
            .unwrap()
            .quality = 0.3;
        app.update();
        let observed = app.world().resource::<Observed>();
        assert_eq!(observed.index.version(), version);
        assert!(observed.touched.is_empty());

        let remote = Vec3::new(720.0, 0.0, 720.0);
        app.world_mut().spawn((
            field(remote, None),
            PlayerPosition(remote),
            PlayerRotation(0.0),
        ));
        app.update();
        assert_eq!(
            app.world()
                .resource::<Observed>()
                .index
                .chunk_revision(home),
            revision
        );
        assert!(!app.world().resource::<Observed>().touched.contains(&home));

        let moved = Vec3::new(190.0, 0.0, 190.0);
        app.world_mut()
            .entity_mut(entity)
            .insert((PlayerPosition(moved), PlayerRotation(0.8)));
        app.update();
        let observed = app.world().resource::<Observed>();
        assert!(observed.touched.contains(&home));
        assert!(
            observed
                .touched
                .contains(&ChunkCoord::from_world_pos(moved))
        );
        assert!(!observed.index.contains_point(at.xz()));
        assert!(observed.index.contains_point(moved.xz()));

        app.world_mut()
            .entity_mut(entity)
            .remove::<PlayerPosition>();
        app.update();
        assert!(
            !app.world()
                .resource::<Observed>()
                .index
                .contains_point(moved.xz())
        );
        app.world_mut()
            .entity_mut(entity)
            .insert(PlayerPosition(moved));
        app.update();
        assert!(
            app.world()
                .resource::<Observed>()
                .index
                .contains_point(moved.xz())
        );

        app.world_mut().entity_mut(entity).remove::<FarmField>();
        app.update();
        let observed = app.world().resource::<Observed>();
        assert!(!observed.index.has_farm(at));
        assert_eq!(observed.index.chunk_revision(home), 0);
        assert_eq!(
            observed
                .index
                .building_zones(at, BuildingType::Farmstead, 0.0)
                .len(),
            3
        );
    }
}
