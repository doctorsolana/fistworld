//! Revocable household landscaping fitted to authoritative village land.
//!
//! Permits and roads take priority. A new reservation removes an incompatible
//! yard before the navigation grid updates; remaining homes are fitted under a
//! small work budget. There is no independent yard economy or per-prop simulation.

use std::collections::{HashMap, VecDeque};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::*;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::library::DerivedColliderLibrary;

#[derive(Clone, PartialEq)]
struct RoadLand {
    points: Vec<Vec2>,
    width: f32,
}

#[derive(Default)]
pub struct HouseholdYardState {
    building_version: Option<u64>,
    terrain_version: Option<u32>,
    roads: HashMap<Entity, RoadLand>,
    fields: HashMap<Entity, (Vec3, f32, Option<FarmFieldShape>)>,
    sites: HashMap<Entity, (SettlementBuildingKind, Vec3, f32)>,
    land: HouseholdYardLand,
    pending: VecDeque<Entity>,
    step: u64,
    props: HashMap<ChunkCoord, Vec<shared::props::BlockingPropSpawn>>,
    props_world_version: Option<u32>,
    validated: HashMap<Entity, YardValidation>,
    staged: Vec<YardGrant>,
}

struct YardGrant {
    entity: Entity,
    validation: YardValidation,
    retry_step: u64,
}

#[derive(PartialEq)]
struct YardValidation {
    yard: HouseholdYard,
    origin: Vec3,
    yaw: f32,
    land: u64,
    terrain: u64,
}

impl YardValidation {
    fn new(
        yard: &HouseholdYard,
        origin: Vec3,
        yaw: f32,
        land: &HouseholdYardLand,
        terrain: &WorldTerrain,
    ) -> Self {
        Self {
            yard: yard.clone(),
            origin,
            yaw,
            land: land.signature_for_yard(yard, origin, yaw),
            terrain: yard.terrain_signature(origin, yaw, terrain),
        }
    }
}

fn dry_height(terrain: &WorldTerrain, p: Vec2) -> Option<f32> {
    let h = terrain.get_height(p.x, p.y);
    (!terrain
        .water_surface_height(p.x, p.y)
        .is_some_and(|water| h < water + 0.35))
    .then_some(h)
}

fn overlaps_body(obstacles: &[shared::spatial::ObstacleEntry], point: Vec2, horse: bool) -> bool {
    let extra = if horse {
        HORSE_BODY_RADIUS - shared::physics::CHARACTER_NAV_RADIUS
    } else {
        0.0
    };
    obstacles.iter().any(|obstacle| {
        let local = shared::rotation::world_to_local_xz(point - obstacle.center, obstacle.rotation);
        local
            .abs()
            .cmple(obstacle.half_extents + Vec2::splat(extra))
            .all()
    })
}

#[derive(SystemParam)]
pub struct YardEnvironment<'w, 's> {
    walls: Query<'w, 's, &'static FortificationSegment>,
    changed_walls: Query<'w, 's, (), Changed<FortificationSegment>>,
    removed_walls: RemovedComponents<'w, 's, FortificationSegment>,
    changed_squares: Query<'w, 's, (), Changed<SettlementCivicSquare>>,
    fields: Query<
        'w,
        's,
        (
            &'static FarmField,
            &'static PlayerPosition,
            &'static PlayerRotation,
            Option<&'static AttachedTo>,
        ),
    >,
    changed_fields: Query<
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
    removed_fields: RemovedComponents<'w, 's, FarmField>,
    building_ids: Query<'w, 's, &'static BuildingId>,
}

/// Geometry-bearing source changes are the only trigger. Worker, inventory and
/// household-account churn does not rebuild the garden catalogue.
#[allow(clippy::too_many_arguments)]
pub fn refresh_household_yards(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    index: Option<Res<BuildingSpatialIndex>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    mut state: Local<HouseholdYardState>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&HouseAppearance>,
        Option<&HouseholdYard>,
    )>,
    halls: Query<(
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&SettlementCivicSquare>,
    )>,
    sites: Query<(Entity, &ConstructionSite, &PlayerPosition)>,
    roads: Query<(Entity, &VillageRoad)>,
    changed_roads: Query<(Entity, &VillageRoad), Changed<VillageRoad>>,
    mut removed_roads: RemovedComponents<VillageRoad>,
    mut environment: YardEnvironment,
    bodies: Query<
        (&PlayerPosition, Has<Horse>, Has<Mounted>),
        (
            Or<(With<CharacterKind>, With<Horse>)>,
            Without<crate::player::hero::OfflineHero>,
            Without<crate::world::village::strategic::StrategicPerson>,
            Without<AboardBoat>,
        ),
    >,
) {
    let (Some(terrain), Some(index)) = (terrain, index) else {
        return;
    };
    state.step = state.step.wrapping_add(1);
    let mut dirty = state.building_version != Some(index.version)
        || state.terrain_version != Some(terrain.modification_version())
        || !environment.changed_walls.is_empty()
        || !environment.changed_squares.is_empty();
    dirty |= environment.removed_walls.read().count() > 0;
    for entity in environment.removed_fields.read() {
        dirty |= state.fields.remove(&entity).is_some();
    }
    for (entity, field, position, rotation) in &environment.changed_fields {
        if !state.fields.get(&entity).is_some_and(|(p, r, shape)| {
            *p == position.0 && *r == rotation.0 && *shape == field.shape
        }) {
            state
                .fields
                .insert(entity, (position.0, rotation.0, field.shape.clone()));
            dirty = true;
        }
    }
    for entity in removed_roads.read() {
        dirty |= state.roads.remove(&entity).is_some();
    }
    for (entity, road) in &changed_roads {
        let width = road.width.max(road.reserved_width);
        if !state
            .roads
            .get(&entity)
            .is_some_and(|r| r.width == width && r.points == road.points)
        {
            state.roads.insert(
                entity,
                RoadLand {
                    points: road.points.clone(),
                    width,
                },
            );
            dirty = true;
        }
    }
    // Only pending worksites are surveyed here, never every actor in the world.
    // Their progress changes often but their land signature normally does not.
    let sites_changed = state.sites.len() != sites.iter().len()
        || sites
            .iter()
            .any(|(e, s, p)| state.sites.get(&e) != Some(&(s.kind, p.0, s.rotation)));
    if sites_changed {
        state.sites.clear();
        state
            .sites
            .extend(sites.iter().map(|(e, s, p)| (e, (s.kind, p.0, s.rotation))));
        dirty = true;
    }
    if dirty {
        // Unpublished fits have no navigation/visual lifetime yet. Refit them
        // against the new priority source rather than publishing stale land.
        state.staged.clear();
        state.building_version = Some(index.version);
        if state.props_world_version != Some(terrain.full_rebuild_version()) {
            state.props.clear();
            state.props_world_version = Some(terrain.full_rebuild_version());
        }
        state.terrain_version = Some(terrain.modification_version());
        let mut land = HouseholdYardLand::default();
        // Authored city plots and other plain PlacedBuilding entities also
        // own ground, even without settlement/household metadata. The same
        // index is authoritative for navigation; never fit a fence inside it.
        for building in index.snapshot() {
            let definition = building.building_type.definition();
            land.reserve_rect(
                definition.world_footprint_center(building.position, building.rotation),
                definition.footprint * 0.5,
                building.rotation,
            );
        }
        let field_owners: std::collections::HashSet<_> = environment
            .fields
            .iter()
            .filter_map(|(_, _, _, owner)| owner.map(|owner| owner.0))
            .collect();
        let legacy_field_origins: std::collections::HashSet<_> = environment
            .fields
            .iter()
            .filter(|(_, _, _, owner)| owner.is_none())
            .map(|(field, ..)| field.farmstead.to_array().map(f32::to_bits))
            .collect();
        for (entity, building, position, rotation, _, _) in &buildings {
            let explicit_fields = environment
                .building_ids
                .get(entity)
                .is_ok_and(|id| field_owners.contains(id))
                || legacy_field_origins.contains(&position.0.to_array().map(f32::to_bits));
            if building.kind == SettlementBuildingKind::Farmstead && explicit_fields {
                land.reserve_building_without_inferred_fields(
                    building.kind,
                    position.0,
                    rotation.0,
                );
            } else {
                land.reserve_building(building.kind, position.0, rotation.0);
            }
        }
        for (_, position, rotation, square) in &halls {
            land.reserve_building(
                SettlementBuildingKind::Hall,
                position.0,
                rotation.map_or(0., |r| r.0),
            );
            if let Some(square) = square {
                land.reserve_rect(square.center.xz(), square.half_extents, square.rotation);
            }
        }
        for (_, site, position) in &sites {
            land.reserve_new_building(site.kind, position.0, site.rotation);
        }
        for (_, road) in &roads {
            land.reserve_road(road);
        }
        for (field, position, rotation, _) in &environment.fields {
            land.reserve_field(field, position.0, rotation.0);
        }
        for wall in &environment.walls {
            // Planned walls already reserve this passage; do not make the
            // builders work around a newly granted garden in their corridor.
            land.reserve_segment(wall.start.xz(), wall.end.xz(), DEFENSE_CORRIDOR_HALF_WIDTH);
        }
        let mut houses: Vec<_> = buildings
            .iter()
            .filter(|(_, b, ..)| b.kind == SettlementBuildingKind::House)
            .collect();
        houses.sort_by(|a, b| {
            a.2 .0
                .x
                .total_cmp(&b.2 .0.x)
                .then(a.2 .0.z.total_cmp(&b.2 .0.z))
        });
        state.pending.clear();
        for (entity, _, position, rotation, _, yard) in houses {
            // Retain valid existing land before considering new grants. This
            // avoids gardens oscillating when unrelated houses are completed.
            let valid = yard.is_some_and(|yard| {
                let validation = YardValidation::new(yard, position.0, rotation.0, &land, &terrain);
                if state.validated.get(&entity) == Some(&validation) {
                    return true;
                }
                let valid = yard.fits_site(
                    position.0,
                    rotation.0,
                    |p, r| land.is_clear(p, r),
                    |p| dry_height(&terrain, p),
                );
                if valid {
                    state.validated.insert(entity, validation);
                }
                valid
            });
            if let Some(yard) = yard.filter(|_| valid) {
                land.reserve_yard(yard, position.0, rotation.0);
            } else {
                state.validated.remove(&entity);
                if yard.is_some() {
                    commands.entity(entity).remove::<HouseholdYard>();
                }
                state.pending.push_back(entity);
            }
        }
        for (entity, building, _, _, _, yard) in &buildings {
            if building.kind != SettlementBuildingKind::House && yard.is_some() {
                commands.entity(entity).remove::<HouseholdYard>();
            }
        }
        state.validated.retain(|entity, _| {
            buildings
                .get(*entity)
                .is_ok_and(|(_, building, ..)| building.kind == SettlementBuildingKind::House)
        });
        state.land = land;
    }
    for _ in 0..2 {
        let Some(entity) = state.pending.pop_front() else {
            break;
        };
        let Ok((_, building, position, rotation, appearance, _)) = buildings.get(entity) else {
            continue;
        };
        if building.kind != SettlementBuildingKind::House {
            continue;
        }
        let at = position.0;
        let chunk = ChunkCoord::from_world_pos(at);
        // The immutable world recipe, not observed collider streaming, decides
        // whether a tree or rock occupies the new yard. Cache surrounding chunks.
        let mut nearby = Vec::new();
        for x in chunk.x - 1..=chunk.x + 1 {
            for z in chunk.z - 1..=chunk.z + 1 {
                let props = state.props.entry(ChunkCoord::new(x, z)).or_insert_with(|| {
                    shared::props::generate_chunk_blocking_props(
                        &terrain.generator,
                        ChunkCoord::new(x, z),
                    )
                });
                nearby.extend(
                    props
                        .iter()
                        .filter(|p| p.position.distance_squared(at.xz()) < 18.0 * 18.0)
                        .copied(),
                );
            }
        }
        let seed = household_yard_seed(at);
        let yard = state.land.fit_yard(
            appearance.copied().unwrap_or_default(),
            at,
            rotation.0,
            seed,
            |point, radius| {
                !nearby.iter().any(|prop| {
                    let size = derived
                        .as_ref()
                        .and_then(|d| d.by_kind.get(&prop.kind))
                        .map_or(1.5, |d| d.horizontal_radius)
                        * prop.scale;
                    prop.position.distance_squared(point) < (size + radius + 0.3).powi(2)
                })
            },
            |point| dry_height(&terrain, point),
        );
        if let Some(yard) = yard {
            let validation = YardValidation::new(&yard, at, rotation.0, &state.land, &terrain);
            state.land.reserve_yard(&yard, at, rotation.0);
            state.staged.push(YardGrant {
                entity,
                validation,
                retry_step: 0,
            });
        }
    }
    // Fitting stays at two homes/update, but initial town dressing publishes
    // up to sixteen together: one shared navigation/cache revision per group,
    // instead of invalidating the road graph for every pair of homes.
    let ready = state
        .staged
        .iter()
        .filter(|g| g.retry_step <= state.step)
        .count();
    if ready == 0 || (ready < 16 && !state.pending.is_empty() && state.step % 60 != 0) {
        return;
    }
    let staged = std::mem::take(&mut state.staged);
    let mut published = 0;
    for mut grant in staged {
        if grant.retry_step > state.step || published == 16 {
            state.staged.push(grant);
            continue;
        }
        let at = grant.validation.origin;
        let yaw = grant.validation.yaw;
        let Ok((_, building, position, rotation, _, _)) = buildings.get(grant.entity) else {
            continue;
        };
        if building.kind != SettlementBuildingKind::House || position.0 != at || rotation.0 != yaw {
            continue;
        }
        // Check at publication, because a person may have entered while the
        // other homes in this batch were being fitted. A deferred grant keeps
        // its already reserved land and retries without repeating the survey.
        let obstacles = grant.validation.yard.ground_obstacles(at, yaw);
        if bodies.iter().any(|(p, horse, mounted)| {
            p.0.xz().distance_squared(at.xz()) < 18.0 * 18.0
                && overlaps_body(&obstacles, p.0.xz(), horse || mounted)
        }) {
            grant.retry_step = state.step.saturating_add(60);
            state.staged.push(grant);
            continue;
        }
        commands
            .entity(grant.entity)
            .insert(grant.validation.yard.clone());
        state.validated.insert(grant.entity, grant.validation);
        published += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::building_index::sync_building_spatial_index;
    use crate::world::navgrid::{sync_obstacle_grid, ObstacleGridState};
    use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
    use shared::spatial::SpatialObstacleGrid;

    fn yard_app() -> (App, Entity, HouseholdYard, Vec3) {
        let at = Vec3::new(1700., 80., 0.);
        let source = WorldTerrain::default();
        let mut map = source.generator.loaded_map().clone();
        map.objects_by_chunk.clear();
        if let Some(recipe) = &mut map.definition.generated {
            recipe.scatter_vegetation = false;
        }
        let mut terrain = WorldTerrain::from_loaded_map(map);
        terrain.apply_flatten_rect(at, Vec2::splat(30.), 0., 4.);
        let mut land = HouseholdYardLand::default();
        land.reserve_building(SettlementBuildingKind::House, at, 0.);
        let yard = fit_household_yard(
            HouseAppearance::default(),
            at,
            0.,
            0,
            |p, r| land.is_clear(p, r),
            |_| Some(at.y),
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(terrain);
        app.init_resource::<BuildingSpatialIndex>();
        app.init_resource::<SpatialObstacleGrid>();
        app.init_resource::<ObstacleGridState>();
        app.add_systems(
            Update,
            (
                sync_building_spatial_index,
                refresh_household_yards,
                sync_obstacle_grid,
            )
                .chain(),
        );
        let entity = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Yard test".into(),
                    owner: None,
                    quality: 1.,
                    workers: vec![],
                },
                HouseAppearance::default(),
                PlayerPosition(at),
                PlayerRotation(0.),
                PlacedBuilding {
                    building_type: BuildingType::LogCabin,
                    rotation: 0.,
                },
                BuildingPosition(at),
                yard.clone(),
            ))
            .id();
        (app, entity, yard, at)
    }

    #[test]
    fn later_reserved_road_releases_the_old_yard_and_its_navigation() {
        let (mut app, house, yard, at) = yard_app();
        app.update();
        assert_eq!(app.world().get::<HouseholdYard>(house), Some(&yard));
        let outer = (yard.outer_edge().0 + yard.outer_edge().1) * 0.5 + at.xz();
        assert!(app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(outer));
        let center = yard.center() + at.xz();
        let road = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Yard test".into(),
                builder: "Builder".into(),
                points: vec![center - Vec2::Y * 9., center + Vec2::Y * 9.],
                built_through: 0,
                width: 1.,
                reserved_width: 3.,
                surface: RoadSurface::Dirt,
                class: RoadClass::Lane,
                stone_committed: 0,
            })
            .id();
        app.update();
        assert_ne!(
            app.world().get::<HouseholdYard>(house),
            Some(&yard),
            "the unbuilt road reservation takes priority"
        );
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(outer),
            "revocation and navigation update occur in the same schedule"
        );
        let accepted = app.world().get::<HouseholdYard>(house).cloned();
        let version = app.world().resource::<SpatialObstacleGrid>().version;
        app.world_mut()
            .get_mut::<VillageRoad>(road)
            .unwrap()
            .built_through = 2;
        app.update();
        assert_eq!(app.world().get::<HouseholdYard>(house), accepted.as_ref());
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version,
            "ordinary road progress must not rebuild yard navigation"
        );
    }

    #[test]
    fn house_upgrade_preserves_future_clearance_and_demolition_removes_fences() {
        let (mut app, house, yard, at) = yard_app();
        app.update();
        app.world_mut().entity_mut(house).insert(PlacedBuilding {
            building_type: BuildingType::LongCabinL2,
            rotation: 0.,
        });
        app.update();
        assert_eq!(app.world().get::<HouseholdYard>(house), Some(&yard));
        let outer = (yard.outer_edge().0 + yard.outer_edge().1) * 0.5 + at.xz();
        assert!(app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(outer));
        app.world_mut().despawn(house);
        app.update();
        assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
    }

    #[test]
    fn a_plain_placed_building_revokes_conflicting_yard_land() {
        let (mut app, house, yard, at) = yard_app();
        app.update();
        let point = at + Vec3::new(yard.center().x, 0.0, yard.center().y);
        let building = BuildingType::LogCabin;
        app.world_mut().spawn((
            PlacedBuilding {
                building_type: building,
                rotation: 0.0,
            },
            BuildingPosition(point),
        ));
        app.update();
        assert_ne!(app.world().get::<HouseholdYard>(house), Some(&yard));
        if let Some(next) = app.world().get::<HouseholdYard>(house) {
            let mut land = HouseholdYardLand::default();
            let definition = building.definition();
            land.reserve_rect(
                definition.world_footprint_center(point, 0.0),
                definition.footprint * 0.5,
                0.0,
            );
            assert!(next.fits_site(at, 0.0, |p, r| land.is_clear(p, r), |_| Some(at.y)));
        }
    }

    #[test]
    fn live_grants_never_embed_a_person_or_horse_in_a_new_fence() {
        let yard = fit_household_yard(
            HouseAppearance::default(),
            Vec3::ZERO,
            0.7,
            0,
            |_, _| true,
            |_| Some(0.),
        )
        .unwrap();
        let obstacles = yard.ground_obstacles(Vec3::ZERO, 0.7);
        let fence = &obstacles[1];
        assert!(overlaps_body(&obstacles, fence.center, false));
        let beside =
            fence.center + shared::rotation::local_to_world_xz(Vec2::new(0., 0.8), fence.rotation);
        assert!(!overlaps_body(&obstacles, beside, false));
        assert!(overlaps_body(&obstacles, beside, true));
        assert!(!overlaps_body(&obstacles, Vec2::splat(100.), true));
    }

    #[test]
    fn fitting_a_town_publishes_navigation_in_groups_not_every_pair_of_houses() {
        let (mut app, first, _, at) = yard_app();
        app.world_mut().entity_mut(first).remove::<HouseholdYard>();
        let building = app
            .world()
            .get::<SettlementBuilding>(first)
            .unwrap()
            .clone();
        app.world_mut()
            .resource_mut::<WorldTerrain>()
            .apply_flatten_rect(at + Vec3::new(48., 0., 36.), Vec2::splat(120.), 0., 4.);
        for i in 1..18 {
            let point = at + Vec3::new((i % 5) as f32 * 24., 0., (i / 5) as f32 * 24.);
            app.world_mut().spawn((
                building.clone(),
                HouseAppearance::default(),
                PlayerPosition(point),
                PlayerRotation(0.),
                PlacedBuilding {
                    building_type: BuildingType::LogCabin,
                    rotation: 0.,
                },
                BuildingPosition(point),
            ));
        }
        app.update();
        let count = |world: &mut World| world.query::<&HouseholdYard>().iter(world).count();
        assert_eq!(count(app.world_mut()), 0);
        let initial_version = app.world().resource::<SpatialObstacleGrid>().version;
        for _ in 1..7 {
            app.update();
            assert_eq!(count(app.world_mut()), 0);
            assert_eq!(
                app.world().resource::<SpatialObstacleGrid>().version,
                initial_version
            );
        }
        app.update();
        assert_eq!(count(app.world_mut()), 16);
        let batch_version = app.world().resource::<SpatialObstacleGrid>().version;
        assert_ne!(batch_version, initial_version);
        app.update();
        assert_eq!(count(app.world_mut()), 18);
        assert_ne!(
            app.world().resource::<SpatialObstacleGrid>().version,
            batch_version
        );
    }

    #[test]
    fn a_staged_fence_waits_for_a_standing_body_then_publishes_without_a_refit() {
        let (mut app, house, _, at) = yard_app();
        app.world_mut().entity_mut(house).remove::<HouseholdYard>();
        let mut land = HouseholdYardLand::default();
        land.reserve_building(SettlementBuildingKind::House, at, 0.);
        let yard = land
            .fit_yard(
                HouseAppearance::default(),
                at,
                0.,
                household_yard_seed(at),
                |_, _| true,
                |_| Some(at.y),
            )
            .unwrap();
        let point = yard.ground_obstacles(at, 0.)[1].center;
        let person = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(point.x, at.y, point.y)),
            ))
            .id();
        app.update();
        assert!(app.world().get::<HouseholdYard>(house).is_none());
        assert!(!app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(point));
        app.world_mut().get_mut::<PlayerPosition>(person).unwrap().0 += Vec3::splat(100.);
        for _ in 0..59 {
            app.update();
        }
        assert!(app.world().get::<HouseholdYard>(house).is_none());
        app.update();
        assert_eq!(app.world().get::<HouseholdYard>(house), Some(&yard));
        assert!(app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(point));
    }
}
