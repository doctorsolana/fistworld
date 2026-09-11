//! Bounded presentation of accepted household outdoor space.
//!
//! Fences/yard decisions arrive on the existing house root. Two batched meshes
//! share one opaque material; every plank, leaf and garment is ordinary mesh
//! geometry with no per-object ECS update or simulation.

mod dressing;
mod fences;
mod ground;
mod laundry;
mod mesh;
mod planting;

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::components::{HouseholdYard, PlayerPosition, PlayerRotation};
use shared::terrain::WorldTerrain;

use crate::render::systems::ClientWorldRoot;
use crate::states::GameState;

pub(super) struct HouseholdYardsPlugin;

impl Plugin for HouseholdYardsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<YardGroundCover>();
        app.init_resource::<YardRenderState>();
        app.add_systems(Update, sync_yards.run_if(in_state(GameState::Playing)));
        app.add_systems(OnExit(GameState::Playing), clear_yards);
    }
}

#[derive(Component)]
pub(crate) struct YardVisual {
    placement: YardPlacement,
    terrain_signature: u64,
    /// Cached at mesh creation; captures never read back uploaded vertices.
    pub triangle_counts: [usize; 2],
}

pub(crate) fn visual_matches(
    yard: &HouseholdYard,
    origin: Vec3,
    yaw: f32,
    terrain: &WorldTerrain,
    visual: Option<&YardVisual>,
) -> bool {
    visual.is_some_and(|v| {
        v.placement.yard == *yard
            && v.placement.origin == origin
            && v.placement.yaw == yaw
            && v.terrain_signature == yard.terrain_signature(origin, yaw, terrain)
    })
}

#[derive(Clone, PartialEq)]
struct YardPlacement {
    yard: HouseholdYard,
    origin: Vec3,
    yaw: f32,
}

/// Spatial, changed-only interface for grass and roadside scatter. It excludes
/// native meadow tufts from accepted yards while their own beds/flowers draw.
#[derive(Resource, Default)]
pub(crate) struct YardGroundCover {
    placements: HashMap<Entity, YardPlacement>,
    cells: HashMap<(i32, i32), Vec<Entity>>,
    version: u64,
}

impl YardGroundCover {
    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    pub(crate) fn contains_world_point(&self, point: Vec2) -> bool {
        self.cells.get(&cell(point)).is_some_and(|entries| {
            entries.iter().any(|e| {
                self.placements
                    .get(e)
                    .is_some_and(|p| ground_cover_contains(&p.yard, point, p.origin, p.yaw, 0.10))
            })
        })
    }

    fn rebuild(&mut self) {
        self.cells.clear();
        for (&entity, p) in &self.placements {
            let (lo, hi) = ground_cover_bounds(&p.yard, p.origin, p.yaw, 0.10);
            let a = cell(lo);
            let b = cell(hi);
            for x in a.0..=b.0 {
                for z in a.1..=b.1 {
                    self.cells.entry((x, z)).or_default().push(entity);
                }
            }
        }
        self.version = self.version.wrapping_add(1);
    }
}

/// Presentation footprint includes the accepted external gate approach. This
/// shared client helper keeps grass, roadside scatter and ground caches aligned
/// without expanding the actual planted parcel or adding simulation obstacles.
pub(crate) fn ground_cover_bounds(
    yard: &HouseholdYard,
    origin: Vec3,
    yaw: f32,
    margin: f32,
) -> (Vec2, Vec2) {
    let (mut lo, mut hi) = yard.world_bounds(origin, yaw, margin);
    if let Some((a, b)) = yard.approach_path() {
        let a = origin.xz() + shared::rotation::local_to_world_xz(a, yaw);
        let b = origin.xz() + shared::rotation::local_to_world_xz(b, yaw);
        let radius = shared::components::YARD_APPROACH_HALF_WIDTH + margin;
        lo = lo.min(a.min(b) - Vec2::splat(radius));
        hi = hi.max(a.max(b) + Vec2::splat(radius));
    }
    (lo, hi)
}

pub(crate) fn ground_cover_contains(
    yard: &HouseholdYard,
    point: Vec2,
    origin: Vec3,
    yaw: f32,
    margin: f32,
) -> bool {
    if yard.contains_world_point(point, origin, yaw, margin) {
        return true;
    }
    yard.approach_path().is_some_and(|(a, b)| {
        let p = shared::rotation::world_to_local_xz(point - origin.xz(), yaw);
        let d = b - a;
        let nearest = a + d * ((p - a).dot(d) / d.length_squared()).clamp(0., 1.);
        p.distance_squared(nearest)
            <= (shared::components::YARD_APPROACH_HALF_WIDTH + margin).powi(2)
    })
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / 16.).floor() as i32, (p.y / 16.).floor() as i32)
}

#[derive(Resource, Default)]
struct YardRenderState {
    pending: VecDeque<Entity>,
    queued: HashSet<Entity>,
    roots: HashMap<Entity, [Entity; 2]>,
    material: Option<Handle<StandardMaterial>>,
    terrain_version: Option<u32>,
    terrain_signatures: HashMap<Entity, u64>,
}

impl YardRenderState {
    fn queue(&mut self, entity: Entity) {
        if self.queued.insert(entity) {
            self.pending.push_back(entity);
        }
    }
}

#[derive(Component)]
struct YardMeshRoot;

#[allow(clippy::too_many_arguments)]
fn sync_yards(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cover: ResMut<YardGroundCover>,
    mut state: ResMut<YardRenderState>,
    changed: Query<
        (Entity, &HouseholdYard, &PlayerPosition, &PlayerRotation),
        Or<(
            Changed<HouseholdYard>,
            Changed<PlayerPosition>,
            Changed<PlayerRotation>,
        )>,
    >,
    houses: Query<(&HouseholdYard, &PlayerPosition, &PlayerRotation)>,
    mut removed: RemovedComponents<HouseholdYard>,
    world_roots: Query<Entity, With<ClientWorldRoot>>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let mut index_changed = false;
    for entity in removed.read() {
        index_changed |= cover.placements.remove(&entity).is_some();
        state.terrain_signatures.remove(&entity);
        if let Some(roots) = state.roots.remove(&entity) {
            for root in roots {
                commands.entity(root).try_despawn();
            }
        }
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<YardVisual>();
        }
    }
    for (entity, yard, position, rotation) in &changed {
        let placement = YardPlacement {
            yard: yard.clone(),
            origin: position.0,
            yaw: rotation.0,
        };
        if cover.placements.get(&entity) != Some(&placement) {
            cover.placements.insert(entity, placement);
            index_changed = true;
            state.queue(entity);
        }
    }
    if index_changed {
        cover.rebuild();
    }
    if state.terrain_version != Some(terrain.modification_version()) {
        state.terrain_version = Some(terrain.modification_version());
        for (&entity, p) in &cover.placements {
            let signature = p.yard.terrain_signature(p.origin, p.yaw, &terrain);
            if state.terrain_signatures.get(&entity) != Some(&signature) {
                state.queue(entity);
            }
        }
    }
    let Ok(world_root) = world_roots.single() else {
        // Retain queued placements until the one streaming world root exists.
        return;
    };
    // At most four houses (eight meshes) per frame, including reconnects or
    // mass terrain changes. A capture waits for YardVisual, not an arbitrary delay.
    for _ in 0..4 {
        let Some(entity) = state.pending.pop_front() else {
            break;
        };
        state.queued.remove(&entity);
        let Ok((yard, position, rotation)) = houses.get(entity) else {
            continue;
        };
        let material = state
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.96,
                    reflectance: 0.22,
                    ..default()
                })
            })
            .clone();
        let mut next = Vec::with_capacity(2);
        let mut triangle_counts = [0; 2];
        for (lod, geometry) in dressing::build_lods(yard, position.0, rotation.0, &terrain)
            .into_iter()
            .enumerate()
        {
            debug_assert!(!geometry.is_empty());
            triangle_counts[lod] = geometry.triangle_count();
            let root = commands
                .spawn((
                    Name::new(if lod == 0 {
                        "Household yard"
                    } else {
                        "Household yard distant"
                    }),
                    YardMeshRoot,
                    ChildOf(world_root),
                    Mesh3d(meshes.add(geometry.finish())),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(position.0.x, 0., position.0.z)
                        .with_rotation(Quat::from_rotation_y(rotation.0)),
                    VisibilityRange {
                        start_margin: if lod == 0 { 0.0..0.0 } else { 220.0..220.0 },
                        end_margin: if lod == 0 { 220.0..220.0 } else { 480.0..540.0 },
                        use_aabb: false,
                    },
                ))
                .id();
            if lod == 1 {
                commands.entity(root).insert(NotShadowCaster);
            }
            next.push(root);
        }
        if let Some(old) = state.roots.insert(entity, [next[0], next[1]]) {
            for root in old {
                commands.entity(root).try_despawn();
            }
        }
        let terrain_signature = yard.terrain_signature(position.0, rotation.0, &terrain);
        state.terrain_signatures.insert(entity, terrain_signature);
        commands.entity(entity).insert(YardVisual {
            placement: YardPlacement {
                yard: yard.clone(),
                origin: position.0,
                yaw: rotation.0,
            },
            terrain_signature,
            triangle_counts,
        });
    }
}

fn clear_yards(
    mut commands: Commands,
    roots: Query<Entity, With<YardMeshRoot>>,
    mut cover: ResMut<YardGroundCover>,
    mut state: ResMut<YardRenderState>,
) {
    for root in &roots {
        commands.entity(root).try_despawn();
    }
    cover.placements.clear();
    cover.rebuild();
    *state = YardRenderState::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{YardSide, YardUse};

    #[test]
    fn clipped_yard_dressing_stays_inside_its_accepted_land_in_both_lods() {
        let terrain = WorldTerrain::default();
        for use_kind in [
            YardUse::Vegetables,
            YardUse::Laundry,
            YardUse::Flowers,
            YardUse::Firewood,
        ] {
            let yard = HouseholdYard {
                minimum: Vec2::new(4.5, -2.),
                maximum: Vec2::new(8., 4.),
                boundary: vec![
                    Vec2::new(4.5, -2.),
                    Vec2::new(6.8, -2.),
                    Vec2::new(8., 4.),
                    Vec2::new(4.5, 4.),
                ],
                side: YardSide::Right,
                use_kind,
                seed: 291,
                entry: None,
                approach: None,
                house: None,
            };
            for detail in [true, false] {
                let mesh = dressing::build(&yard, Vec3::ZERO, 0., &terrain, detail).finish();
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("yard needs positions");
                };
                assert!(!points.is_empty());
                assert!(points.iter().all(|p| Vec3::from_array(*p).is_finite()));
                assert!(
                    points
                        .iter()
                        .all(|p| yard.contains_local_point(Vec2::new(p[0], p[2]), 0.20)),
                    "{use_kind:?} detail={detail}: dressing escaped a clipped plot"
                );
            }
        }
    }

    #[test]
    fn yard_meshes_are_bounded_children_of_the_single_streaming_world_root() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<WorldTerrain>();
        app.init_resource::<YardGroundCover>();
        app.init_resource::<YardRenderState>();
        app.add_systems(Update, sync_yards);
        let houses: Vec<_> = (0..5)
            .map(|i| {
                app.world_mut()
                    .spawn((
                        HouseholdYard {
                            boundary: Vec::new(),
                            minimum: Vec2::new(5., -2.),
                            maximum: Vec2::new(7., 3.),
                            side: YardSide::Right,
                            use_kind: YardUse::Vegetables,
                            seed: i,
                            entry: None,
                            approach: None,
                            house: None,
                        },
                        PlayerPosition(Vec3::X * i as f32 * 20.),
                        PlayerRotation(0.),
                    ))
                    .id()
            })
            .collect();
        app.update();
        assert!(houses
            .iter()
            .all(|&e| app.world().get::<YardVisual>(e).is_none()));
        let root = app
            .world_mut()
            .spawn((ClientWorldRoot, Transform::default()))
            .id();
        app.update();
        assert_eq!(
            houses
                .iter()
                .filter(|&&e| app.world().get::<YardVisual>(e).is_some())
                .count(),
            4
        );
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<ClientWorldRoot>>()
                .iter(world)
                .count(),
            1
        );
        let parents: Vec<_> = world
            .query_filtered::<&ChildOf, With<YardMeshRoot>>()
            .iter(world)
            .map(ChildOf::parent)
            .collect();
        assert_eq!(parents.len(), 10);
        assert!(parents.into_iter().all(|parent| parent == root));
        world.despawn(root);
        assert_eq!(
            world
                .query_filtered::<Entity, With<YardMeshRoot>>()
                .iter(world)
                .count(),
            0
        );
    }

    #[test]
    fn readiness_rejects_a_stale_house_transform_or_terrain() {
        let yard = HouseholdYard {
            boundary: Vec::new(),
            minimum: Vec2::new(5., -2.),
            maximum: Vec2::new(7., 3.),
            side: YardSide::Right,
            use_kind: YardUse::Vegetables,
            seed: 7,
            entry: None,
            approach: None,
            house: None,
        };
        let origin = Vec3::new(30., 0., -15.);
        let mut terrain = WorldTerrain::default();
        let visual = YardVisual {
            placement: YardPlacement {
                yard: yard.clone(),
                origin,
                yaw: 0.5,
            },
            terrain_signature: yard.terrain_signature(origin, 0.5, &terrain),
            triangle_counts: [0; 2],
        };
        assert!(visual_matches(&yard, origin, 0.5, &terrain, Some(&visual)));
        assert!(!visual_matches(
            &yard,
            origin + Vec3::X,
            0.5,
            &terrain,
            Some(&visual)
        ));
        assert!(!visual_matches(&yard, origin, 0.6, &terrain, Some(&visual)));
        terrain.apply_flatten_rect(Vec3::new(1000., 80., 1000.), Vec2::splat(20.), 0., 2.);
        assert!(
            visual_matches(&yard, origin, 0.5, &terrain, Some(&visual)),
            "a remote terrace does not stale this yard"
        );
        terrain.apply_flatten_rect(origin + Vec3::Y * 80., Vec2::splat(20.), 0., 2.);
        assert!(!visual_matches(&yard, origin, 0.5, &terrain, Some(&visual)));
        assert!(!visual_matches(&yard, origin, 0.5, &terrain, None));
    }

    #[test]
    fn gate_approach_exclusion_crosses_cells_and_releases_with_its_yard() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let origin = Vec3::new(15.8, 0., 31.5);
        let yaw = 0.71;
        let yard = HouseholdYard {
            minimum: Vec2::new(5., 0.),
            maximum: Vec2::new(11., 8.),
            boundary: Vec::new(),
            side: YardSide::Right,
            use_kind: YardUse::Flowers,
            seed: 19,
            entry: Some(Vec2::new(7., 0.)),
            approach: Some(Vec2::new(4., -8.)),
            house: None,
        };
        let mut cover = YardGroundCover::default();
        cover.placements.insert(
            entity,
            YardPlacement {
                yard: yard.clone(),
                origin,
                yaw,
            },
        );
        cover.rebuild();
        let world = |p| origin.xz() + shared::rotation::local_to_world_xz(p, yaw);
        let (a, b) = yard.approach_path().unwrap();
        let side = (b - a).perp().normalize();
        for i in 0..=20 {
            let p = a.lerp(b, i as f32 / 20.);
            assert!(cover.contains_world_point(world(p)));
            assert!(cover.contains_world_point(world(p + side * 0.60)));
        }
        assert!(!cover.contains_world_point(world(a.lerp(b, 0.8) + side * 1.0)));
        cover.placements.remove(&entity);
        cover.rebuild();
        assert!(!cover.contains_world_point(world(a.lerp(b, 0.5))));
    }

    #[test]
    fn yard_grass_exclusion_rotates_and_releases_removed_land() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut cover = YardGroundCover::default();
        let yard = HouseholdYard {
            boundary: Vec::new(),
            minimum: Vec2::new(5., -2.),
            maximum: Vec2::new(7., 3.),
            side: YardSide::Right,
            use_kind: YardUse::Vegetables,
            seed: 7,
            entry: None,
            approach: None,
            house: None,
        };
        cover.placements.insert(
            entity,
            YardPlacement {
                yard,
                origin: Vec3::new(30., 0., -15.),
                yaw: std::f32::consts::FRAC_PI_2,
            },
        );
        cover.rebuild();
        assert!(cover.contains_world_point(Vec2::new(30., -21.)));
        assert!(!cover.contains_world_point(Vec2::new(36., -15.)));
        let version = cover.version();
        cover.placements.remove(&entity);
        cover.rebuild();
        assert!(!cover.contains_world_point(Vec2::new(30., -21.)));
        assert_ne!(cover.version(), version);
    }
}
