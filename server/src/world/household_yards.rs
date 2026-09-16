//! Revocable household land that develops with real nearby street frontage.
//!
//! Planned roads and buildings retain priority. Built-road progress only wakes
//! nearby homes; fitting and body-safe publication are separately budgeted.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::*;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::library::DerivedColliderLibrary;

mod state;
use state::{HouseLocations, HouseSource, RoadLand, SITE_RADIUS, SiteContext};
#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct HouseholdYardState {
    building_version: Option<u64>,
    terrain_version: Option<u32>,
    roads: HashMap<Entity, RoadLand>,
    fields: HashMap<Entity, (Vec3, f32, Option<FarmFieldShape>)>,
    sites: HashMap<Entity, (SettlementBuildingKind, Vec3, f32)>,
    houses: HouseLocations,
    land: HouseholdYardLand,
    pending: VecDeque<Entity>,
    queued: HashSet<Entity>,
    released: HashSet<Entity>,
    evaluated: HashMap<Entity, SiteContext>,
    step: u64,
    props: HashMap<ChunkCoord, Vec<shared::props::BlockingPropSpawn>>,
    props_world_version: Option<u32>,
    validated: HashMap<Entity, YardValidation>,
    staged: Vec<YardGrant>,
}

struct YardGrant {
    entity: Entity,
    context: SiteContext,
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
    frontage: u64,
    appearance: HouseAppearance,
}

impl YardValidation {
    fn new(
        entity: Entity,
        yard: &HouseholdYard,
        origin: Vec3,
        yaw: f32,
        appearance: HouseAppearance,
        land: &HouseholdYardLand,
        terrain: &WorldTerrain,
    ) -> Self {
        let context = SiteContext::new(
            entity,
            &HouseSource {
                origin,
                yaw,
                appearance,
            },
            land,
            terrain,
        );
        Self {
            yard: yard.clone(),
            origin,
            yaw,
            land: context.land,
            terrain: context.terrain,
            frontage: context.frontage,
            appearance,
        }
    }
}

impl HouseholdYardState {
    fn reconsider(&mut self, entity: Entity, terrain: &WorldTerrain, force: bool) {
        let Some(source) = self.houses.sources.get(&entity) else {
            return;
        };
        let context = SiteContext::new(entity, source, &self.land, terrain);
        if !force && self.evaluated.get(&entity) == Some(&context) {
            return;
        }
        self.cancel_staged(entity);
        // Roadless homes stay dormant until their local road context changes.
        // Existing valid yards survive a road removal; only new fencing waits.
        if context.frontage == 0 {
            self.evaluated.insert(entity, context);
            return;
        }
        if self.queued.insert(entity) {
            self.pending.push_back(entity);
        }
    }

    fn release_land(
        &mut self,
        entity: Entity,
        old: &HouseholdYard,
        origin: Vec3,
        yaw: f32,
        replacement: Option<&HouseholdYard>,
    ) {
        if replacement.is_some_and(|next| claim_contains(next, old)) {
            return;
        }
        let (mut min, mut max) = old.world_bounds(origin, yaw, 0.3);
        if let Some((entry, approach)) = old.approach_path() {
            for point in [entry, approach] {
                let world = origin.xz() + shared::rotation::local_to_world_xz(point, yaw);
                // The approach reservation extends by its half-width along
                // and across the segment, including its end caps.
                let pad = Vec2::splat(YARD_APPROACH_HALF_WIDTH * std::f32::consts::SQRT_2);
                min = min.min(world - pad);
                max = max.max(world + pad);
            }
        }
        self.houses.around_bounds(min, max, &mut self.released);
        self.released.remove(&entity);
    }

    fn cancel_staged(&mut self, entity: Entity) {
        if let Some(index) = self.staged.iter().position(|grant| grant.entity == entity) {
            let grant = self.staged.remove(index);
            let retained = self
                .validated
                .get(&entity)
                .filter(|old| {
                    old.origin == grant.validation.origin && old.yaw == grant.validation.yaw
                })
                .map(|old| &old.yard);
            if !retained.is_some_and(|old| claim_contains(old, &grant.validation.yard)) {
                self.release_land(
                    entity,
                    &grant.validation.yard,
                    grant.validation.origin,
                    grant.validation.yaw,
                    None,
                );
            }
        }
        self.land.set_staged_yard(entity.to_bits(), None);
    }

    fn take_released_neighbours(&mut self) -> HashSet<Entity> {
        let mut released = std::mem::take(&mut self.released);
        // Released space cannot invalidate already-fitted work. Preserve it
        // rather than making neighbouring proposals cancel one another.
        released.retain(|entity| {
            !self.queued.contains(entity)
                && !self.staged.iter().any(|grant| grant.entity == *entity)
        });
        released
    }

    fn forget(&mut self, entity: Entity) {
        self.cancel_staged(entity);
        if let Some(old) = self.validated.remove(&entity) {
            self.release_land(entity, &old.yard, old.origin, old.yaw, None);
        }
        self.houses.remove(entity);
        self.evaluated.remove(&entity);
        self.queued.remove(&entity);
        self.released.remove(&entity);
        self.land.set_yard(entity.to_bits(), None);
        self.land.set_staged_yard(entity.to_bits(), None);
    }
}

/// A sufficient containment proof for releasing no land. Both parcels are
/// convex, so contained old corners imply contained old edges and interior.
/// Different approach endpoints conservatively trigger a local wake-up.
fn claim_contains(next: &HouseholdYard, old: &HouseholdYard) -> bool {
    old.boundary_points()
        .into_iter()
        .all(|p| next.contains_local_point(p, 0.0001))
        && (old.approach_path().is_none() || old.approach_path() == next.approach_path())
}

fn dry_height(terrain: &WorldTerrain, p: Vec2) -> Option<f32> {
    let h = terrain.get_height(p.x, p.y);
    (!terrain
        .water_surface_height(p.x, p.y)
        .is_some_and(|water| h < water + 0.35))
    .then_some(h)
}

fn prop_radius(
    derived: Option<&DerivedColliderLibrary>,
    prop: &shared::props::BlockingPropSpawn,
) -> f32 {
    derived
        .and_then(|d| d.by_kind.get(&prop.kind))
        .map_or(1.5, |d| d.horizontal_radius)
        * prop.scale
}

fn nearby_props(
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    cache: &mut HashMap<ChunkCoord, Vec<shared::props::BlockingPropSpawn>>,
    at: Vec3,
) -> Vec<shared::props::BlockingPropSpawn> {
    let chunk = ChunkCoord::from_world_pos(at);
    let mut result = Vec::new();
    for x in chunk.x - 1..=chunk.x + 1 {
        for z in chunk.z - 1..=chunk.z + 1 {
            let props = cache.entry(ChunkCoord::new(x, z)).or_insert_with(|| {
                shared::props::generate_chunk_blocking_props(
                    &terrain.generator,
                    ChunkCoord::new(x, z),
                )
            });
            result.extend(
                props
                    .iter()
                    .filter(|prop| {
                        prop.position.distance_squared(at.xz())
                            < (SITE_RADIUS + prop_radius(derived, prop) + 0.3).powi(2)
                    })
                    .copied(),
            );
        }
    }
    result
}

fn props_clear(
    derived: Option<&DerivedColliderLibrary>,
    props: &[shared::props::BlockingPropSpawn],
    point: Vec2,
    radius: f32,
) -> bool {
    !props.iter().any(|prop| {
        prop.position.distance_squared(point) < (prop_radius(derived, prop) + radius + 0.3).powi(2)
    })
}

fn accepted_yard_is_valid(
    entity: Entity,
    yard: &HouseholdYard,
    source: &HouseSource,
    land: &HouseholdYardLand,
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    props: &[shared::props::BlockingPropSpawn],
) -> bool {
    yard.fits_site(
        source.origin,
        source.yaw,
        |p, r| land.is_clear_for(entity.to_bits(), p, r) && props_clear(derived, props, p, r),
        |p| dry_height(terrain, p),
    ) && land.yard_access_is_clear_for(
        entity.to_bits(),
        yard,
        source.origin,
        source.yaw,
        |p, r| props_clear(derived, props, p, r),
        |p| dry_height(terrain, p),
    )
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

/// Stable frontage may offer a slightly different clipping solution after a
/// neighbour changes. Keep the lived-in layout unless useful area, the actual
/// house recipe or its access opening improves materially.
fn worthwhile_replacement(old: &HouseholdYard, next: &HouseholdYard) -> bool {
    if old == next {
        return false;
    }
    old.house != next.house
        || next.area() >= old.area() + (old.area() * 0.08).max(1.0)
        || match (old.entry, next.entry) {
            (None, Some(_)) => true,
            (Some(a), Some(b)) => a.distance_squared(b) > 0.75 * 0.75,
            _ => false,
        }
}

#[derive(SystemParam)]
pub struct YardEnvironment<'w, 's> {
    walls: Query<'w, 's, &'static FortificationSegment>,
    changed_walls: Query<'w, 's, (), Changed<FortificationSegment>>,
    removed_walls: RemovedComponents<'w, 's, FortificationSegment>,
    changed_squares: Query<'w, 's, (), Changed<SettlementCivicSquare>>,
    removed_squares: RemovedComponents<'w, 's, SettlementCivicSquare>,
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
    changed_houses: Query<
        'w,
        's,
        (
            Entity,
            &'static SettlementBuilding,
            &'static PlayerPosition,
            &'static PlayerRotation,
            Option<&'static HouseAppearance>,
        ),
        Or<(
            Changed<SettlementBuilding>,
            Changed<PlayerPosition>,
            Changed<PlayerRotation>,
            Changed<HouseAppearance>,
        )>,
    >,
    removed_buildings: RemovedComponents<'w, 's, SettlementBuilding>,
    removed_positions: RemovedComponents<'w, 's, PlayerPosition>,
    removed_rotations: RemovedComponents<'w, 's, PlayerRotation>,
    removed_appearances: RemovedComponents<'w, 's, HouseAppearance>,
}

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
    dirty |= environment.removed_squares.read().count() > 0;

    for entity in environment
        .removed_buildings
        .read()
        .chain(environment.removed_positions.read())
        .chain(environment.removed_rotations.read())
        .chain(environment.removed_appearances.read())
    {
        if let Ok((_, building, at, yaw, appearance, _)) = buildings.get(entity) {
            if building.kind == SettlementBuildingKind::House {
                dirty |= state.houses.update(
                    entity,
                    HouseSource {
                        origin: at.0,
                        yaw: yaw.0,
                        appearance: appearance.copied().unwrap_or_default(),
                    },
                );
                continue;
            }
        }
        dirty |= state.houses.sources.contains_key(&entity);
        state.forget(entity);
    }
    for (entity, building, at, yaw, appearance) in &environment.changed_houses {
        if building.kind == SettlementBuildingKind::House {
            dirty |= state.houses.update(
                entity,
                HouseSource {
                    origin: at.0,
                    yaw: yaw.0,
                    appearance: appearance.copied().unwrap_or_default(),
                },
            );
        } else {
            if state.houses.sources.contains_key(&entity) {
                state.forget(entity);
                dirty = true;
            }
            if buildings
                .get(entity)
                .is_ok_and(|(_, _, _, _, _, yard)| yard.is_some())
            {
                commands.entity(entity).remove::<HouseholdYard>();
                dirty = true;
            }
        }
    }
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
        state.land.remove_frontage(entity.to_bits());
    }
    let mut nearby_houses = state.take_released_neighbours();
    for (entity, road) in &changed_roads {
        if let Some(old) = state.roads.get(&entity) {
            if old.same_claim(road) {
                if !old.same_frontage(road) {
                    state.houses.changed_frontage(old, road, &mut nearby_houses);
                    state.land.replace_frontage(entity.to_bits(), road);
                    // Keep the unchanged full planned polyline allocation.
                    let cached = state.roads.get_mut(&entity).unwrap();
                    cached.width = road.width;
                    cached.built = road.built_points().len();
                    cached.surface = road.surface;
                    cached.class = road.class;
                }
                continue;
            }
        }
        state.roads.insert(entity, RoadLand::from_road(road));
        dirty = true;
    }
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
        state.building_version = Some(index.version);
        state.terrain_version = Some(terrain.modification_version());
        if state.props_world_version != Some(terrain.full_rebuild_version()) {
            state.props.clear();
            state.props_world_version = Some(terrain.full_rebuild_version());
        }
        let mut land = HouseholdYardLand::default();
        for building in index.snapshot() {
            let definition = building.building_type.definition();
            land.reserve_rect(
                definition.world_footprint_center(building.position, building.rotation),
                definition.footprint * 0.5,
                building.rotation,
            );
        }
        let field_owners: HashSet<_> = environment
            .fields
            .iter()
            .filter_map(|(_, _, _, owner)| owner.map(|owner| owner.0))
            .collect();
        let legacy_field_origins: HashSet<_> = environment
            .fields
            .iter()
            .filter(|(_, _, _, owner)| owner.is_none())
            .map(|(field, ..)| field.farmstead.to_array().map(f32::to_bits))
            .collect();
        for (entity, building, position, rotation, appearance, _) in &buildings {
            let explicit_fields = environment
                .building_ids
                .get(entity)
                .is_ok_and(|id| field_owners.contains(id))
                || legacy_field_origins.contains(&position.0.to_array().map(f32::to_bits));
            if building.kind == SettlementBuildingKind::House {
                land.reserve_house(
                    appearance.copied().unwrap_or_default(),
                    position.0,
                    rotation.0,
                );
            } else if building.kind == SettlementBuildingKind::Farmstead && explicit_fields {
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
        for (entity, road) in &roads {
            land.reserve_road_for(entity.to_bits(), road);
        }
        for (field, position, rotation, _) in &environment.fields {
            land.reserve_field(field, position.0, rotation.0);
        }
        for wall in &environment.walls {
            land.reserve_segment(wall.start.xz(), wall.end.xz(), DEFENSE_CORRIDOR_HALF_WIDTH);
        }
        // Protect every existing owner before assessing any replacement. An
        // early house in sorted order may not grow through a later one's yard.
        for (entity, building, position, rotation, _, yard) in &buildings {
            if let Some(yard) = yard.filter(|_| building.kind == SettlementBuildingKind::House) {
                land.set_yard(entity.to_bits(), Some((yard, position.0, rotation.0)));
            }
        }
        for grant in &state.staged {
            land.set_staged_yard(
                grant.entity.to_bits(),
                Some((
                    &grant.validation.yard,
                    grant.validation.origin,
                    grant.validation.yaw,
                )),
            );
        }
        let houses = state.houses.ordered(state.houses.sources.keys().copied());
        let mut revoked = HashSet::new();
        for &entity in &houses {
            let Ok((_, _, position, rotation, _, Some(yard))) = buildings.get(entity) else {
                continue;
            };
            let validation = YardValidation::new(
                entity,
                yard,
                position.0,
                rotation.0,
                state.houses.sources[&entity].appearance,
                &land,
                &terrain,
            );
            let valid = if state.validated.get(&entity) == Some(&validation) {
                true
            } else {
                let props =
                    nearby_props(&terrain, derived.as_deref(), &mut state.props, position.0);
                let source = state.houses.sources.get(&entity).unwrap();
                accepted_yard_is_valid(
                    entity,
                    yard,
                    source,
                    &land,
                    &terrain,
                    derived.as_deref(),
                    &props,
                )
            };
            if valid {
                state.validated.insert(entity, validation);
            } else {
                commands.entity(entity).remove::<HouseholdYard>();
                state.validated.remove(&entity);
                land.set_yard(entity.to_bits(), None);
                revoked.insert(entity);
            }
        }
        state.land = land;
        // A remote edit preserves a staged candidate and its occupied-body
        // retry. Only changed local source stamps cancel unpublished work.
        let stale: Vec<_> = state
            .staged
            .iter()
            .filter(|grant| {
                state
                    .houses
                    .sources
                    .get(&grant.entity)
                    .is_none_or(|source| {
                        grant.context
                            != SiteContext::new(grant.entity, source, &state.land, &terrain)
                    })
            })
            .map(|grant| grant.entity)
            .collect();
        for entity in stale {
            state.cancel_staged(entity);
            state.evaluated.remove(&entity);
        }
        for entity in houses {
            state.reconsider(entity, &terrain, revoked.contains(&entity));
        }
    } else {
        for entity in state.houses.ordered(nearby_houses) {
            let mut revoked = false;
            if let Ok((_, _, position, rotation, _, Some(yard))) = buildings.get(entity) {
                let validation = YardValidation::new(
                    entity,
                    yard,
                    position.0,
                    rotation.0,
                    state.houses.sources[&entity].appearance,
                    &state.land,
                    &terrain,
                );
                if state.validated.get(&entity) != Some(&validation) {
                    let props =
                        nearby_props(&terrain, derived.as_deref(), &mut state.props, position.0);
                    let source = state.houses.sources.get(&entity).unwrap();
                    if accepted_yard_is_valid(
                        entity,
                        yard,
                        source,
                        &state.land,
                        &terrain,
                        derived.as_deref(),
                        &props,
                    ) {
                        state.validated.insert(entity, validation);
                    } else {
                        commands.entity(entity).remove::<HouseholdYard>();
                        state.validated.remove(&entity);
                        state.land.set_yard(entity.to_bits(), None);
                        revoked = true;
                    }
                }
            }
            state.reconsider(entity, &terrain, revoked);
        }
    }

    for _ in 0..2 {
        let Some(entity) = state.pending.pop_front() else {
            break;
        };
        if !state.queued.remove(&entity) {
            continue;
        }
        let Some(source) = state.houses.sources.get(&entity).cloned() else {
            continue;
        };
        let context = SiteContext::new(entity, &source, &state.land, &terrain);
        state.evaluated.insert(entity, context.clone());
        if context.frontage == 0 {
            continue;
        }
        let nearby = nearby_props(
            &terrain,
            derived.as_deref(),
            &mut state.props,
            source.origin,
        );
        let yard = state.land.fit_yard_for(
            entity.to_bits(),
            source.appearance,
            source.origin,
            source.yaw,
            household_yard_seed(source.origin),
            |point, radius| props_clear(derived.as_deref(), &nearby, point, radius),
            |point| dry_height(&terrain, point),
        );
        if let Some(yard) = yard {
            let old = buildings
                .get(entity)
                .ok()
                .and_then(|(_, _, _, _, _, yard)| yard);
            // A revoked component may still be visible through this system's
            // query until deferred commands apply. Its old geometry is no
            // longer an accepted incumbent and must not veto the replacement.
            if state.validated.contains_key(&entity)
                && old.is_some_and(|old| !worthwhile_replacement(old, &yard))
            {
                continue;
            }
            let validation = YardValidation::new(
                entity,
                &yard,
                source.origin,
                source.yaw,
                source.appearance,
                &state.land,
                &terrain,
            );
            state
                .land
                .set_staged_yard(entity.to_bits(), Some((&yard, source.origin, source.yaw)));
            state.staged.push(YardGrant {
                entity,
                context,
                validation,
                retry_step: 0,
            });
        }
    }
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
        let Ok((_, building, position, rotation, appearance, incumbent)) =
            buildings.get(grant.entity)
        else {
            state.release_land(
                grant.entity,
                &grant.validation.yard,
                grant.validation.origin,
                grant.validation.yaw,
                None,
            );
            state.land.set_staged_yard(grant.entity.to_bits(), None);
            continue;
        };
        let source = &grant.context.source;
        if building.kind != SettlementBuildingKind::House
            || position.0 != source.origin
            || rotation.0 != source.yaw
            || appearance.copied().unwrap_or_default() != source.appearance
        {
            state.release_land(
                grant.entity,
                &grant.validation.yard,
                grant.validation.origin,
                grant.validation.yaw,
                None,
            );
            state.land.set_staged_yard(grant.entity.to_bits(), None);
            continue;
        }
        let obstacles = grant
            .validation
            .yard
            .ground_obstacles(source.origin, source.yaw);
        let (min, max) =
            grant
                .validation
                .yard
                .world_bounds(source.origin, source.yaw, HORSE_BODY_RADIUS + 0.5);
        if bodies.iter().any(|(p, horse, mounted)| {
            p.0.xz().cmpge(min).all()
                && p.0.xz().cmple(max).all()
                && overlaps_body(&obstacles, p.0.xz(), horse || mounted)
        }) {
            grant.retry_step = state.step.saturating_add(60);
            state.staged.push(grant);
            continue;
        }
        // Keep the incumbent until this single authoritative component swap;
        // renderer, navigation and collision all observe the accepted result.
        commands
            .entity(grant.entity)
            .insert(grant.validation.yard.clone());
        if let Some(old) = incumbent {
            state.release_land(
                grant.entity,
                old,
                source.origin,
                source.yaw,
                Some(&grant.validation.yard),
            );
        }
        state.land.set_yard(
            grant.entity.to_bits(),
            Some((&grant.validation.yard, source.origin, source.yaw)),
        );
        state.land.set_staged_yard(grant.entity.to_bits(), None);
        state.validated.insert(grant.entity, grant.validation);
        published += 1;
    }
}
