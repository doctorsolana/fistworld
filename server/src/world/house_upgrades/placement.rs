//! Validate the real enlarged building, retaining its existing road/door.

use bevy::prelude::*;
use shared::building::PlacedBuilding;
use shared::components::*;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::world::village::UnderConstruction;

pub(super) fn geometry_is_safe(
    world: &mut World,
    home: Entity,
    settlement: SettlementId,
    appearance: HouseAppearance,
    position: Vec3,
    rotation: f32,
    hall_position: Vec3,
    hall_rotation: f32,
) -> Result<Vec3, String> {
    let definition = appearance.building_type().definition();
    let center = definition.world_footprint_center(position, rotation);
    let half = definition.footprint * 0.5;
    if !center.is_finite() || !half.is_finite() || !rotation.is_finite() {
        return Err("This house has invalid placement data.".into());
    }
    if let (Some(colliders), Some(derived)) = (
        world.get_resource::<crate::collision::library::StaticColliders>(),
        world.get_resource::<crate::collision::library::DerivedColliderLibrary>(),
    ) {
        if !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
            center, half, rotation, 0.2, colliders, derived,
        ) {
            return Err("The larger foundation would overlap a permanent rock or prop.".into());
        }
    }
    let mut buildings = world.query::<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&HouseAppearance>,
        Option<&PlacedBuilding>,
    )>();
    for (entity, building, origin, yaw, house, placed) in buildings.iter(world) {
        if entity == home {
            continue;
        }
        let yaw = yaw.map_or(0.0, |yaw| yaw.0);
        let art = placed.map_or_else(
            || building.kind.art_with_house(house),
            |placed| placed.building_type,
        );
        let other = art.definition();
        if oriented_rects_overlap(
            center,
            half + Vec2::splat(0.2),
            rotation,
            other.world_footprint_center(origin.0, yaw),
            other.footprint * 0.5,
            yaw,
        ) {
            return Err("The larger house would overlap another building.".into());
        }
    }
    let mut halls = world.query::<(&Settlement, &PlayerPosition, Option<&PlayerRotation>)>();
    for (_, origin, yaw) in halls.iter(world) {
        let yaw = yaw.map_or(0.0, |yaw| yaw.0);
        if oriented_rects_overlap(
            center,
            half,
            rotation,
            CivicHallLevel::reserved_world_center(origin.0, yaw),
            CivicHallLevel::reserved_half_extents(),
            yaw,
        ) {
            return Err("The extension overlaps the civic Hall reservation.".into());
        }
    }
    let mut pending = world.query::<&UnderConstruction>();
    for site in pending.iter(world) {
        let other = site.kind.placement_definition();
        if oriented_rects_overlap(
            center,
            half,
            rotation,
            other.world_footprint_center(site.position, site.rotation),
            other.footprint * 0.5,
            site.rotation,
        ) {
            return Err("The extension overlaps an approved construction site.".into());
        }
    }
    let mut walls = world.query::<&FortificationSegment>();
    if walls
        .iter(world)
        .any(|wall| wall.intersects_footprint(center, half, rotation, 0.3))
    {
        return Err("The extension overlaps a wall or gate corridor.".into());
    }
    let mut squares = world.query::<&SettlementCivicSquare>();
    if squares
        .iter(world)
        .any(|square| square.intersects_rect(center, half, rotation))
    {
        return Err("The extension overlaps the civic square.".into());
    }
    let original = world
        .get::<HouseAppearance>(home)
        .copied()
        .unwrap_or(appearance);
    let mut roads = world.query::<(&VillageRoad, &RoadOf)>();
    if roads
        .iter(world)
        .any(|(road, _)| road_blocks_extension(road, original, appearance, position, rotation))
    {
        return Err("The larger house would block a road.".into());
    }
    let town_roads: Vec<_> = roads
        .iter(world)
        .filter_map(|(road, of)| (of.0 == settlement).then_some(road))
        .collect();
    if (!town_roads.is_empty()
        || world.contains_resource::<crate::world::village_roads::VillageRoadGraph>())
        && !crate::world::village_roads::building_has_connected_road(
            SettlementBuildingKind::House,
            position,
            rotation,
            hall_position,
            hall_rotation,
            &town_roads,
        )
    {
        return Err("The house needs a connected road before it can be extended.".into());
    }
    let mut fields = world.query::<(&FarmField, &PlayerPosition, &PlayerRotation)>();
    for (field, origin, yaw) in fields.iter(world) {
        // A conservative box per short crop strip protects even crossings
        // where neither field endpoints nor house centre lie inside the other.
        for strips in field.accepted_shape().sections.windows(2) {
            let left = strips[0].left.min(strips[1].left);
            let right = strips[0].right.max(strips[1].right);
            let z = (strips[0].z + strips[1].z) * 0.5;
            let strip_center =
                shared::rotation::local_to_world_xz(Vec2::new((left + right) * 0.5, z), yaw.0)
                    + origin.0.xz();
            let strip_half = Vec2::new(right - left, strips[1].z - strips[0].z) * 0.5;
            if oriented_rects_overlap(
                center,
                half + Vec2::splat(0.2),
                rotation,
                strip_center,
                strip_half,
                yaw.0,
            ) {
                return Err("The extension would take occupied crop land.".into());
            }
        }
    }
    if let Some(terrain) = world.get_resource::<WorldTerrain>() {
        let bounds = terrain.generator.active_map_bounds();
        for local in [
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, half.y),
        ] {
            let point = center + shared::rotation::local_to_world_xz(local, rotation);
            if point.cmplt(bounds.min_vec2()).any() || point.cmpgt(bounds.max_vec2()).any() {
                return Err("The extension would leave the playable world.".into());
            }
            let height = terrain.get_height(point.x, point.y);
            if terrain
                .get_water_height(point.x, point.y)
                .is_some_and(|water| height <= water + 0.2)
                || height > position.y + 1.0
                || height < position.y - 2.0
            {
                return Err(
                    "The larger foundation needs unsuitable earthworks or crosses water.".into(),
                );
            }
        }
    }
    let door = SettlementBuildingKind::House.entrance_position(position, rotation);
    let obstacles = world.get_resource::<SpatialObstacleGrid>();
    for local in [
        Vec2::new(-1.8, -1.2),
        Vec2::new(1.8, -1.2),
        Vec2::new(0.0, -1.8),
    ] {
        let offset = shared::rotation::local_to_world_xz(local, rotation);
        let mut stand = door + Vec3::new(offset.x, 0.0, offset.y);
        if let Some(terrain) = world.get_resource::<WorldTerrain>() {
            stand.y = terrain.get_height(stand.x, stand.z);
            if terrain
                .get_water_height(stand.x, stand.z)
                .is_some_and(|water| stand.y <= water + 0.2)
            {
                continue;
            }
        }
        if obstacles.is_none_or(|grid| {
            !grid.point_blocked(stand.xz()) && !grid.segment_blocked(stand.xz(), door.xz())
        }) {
            return Ok(stand);
        }
    }
    Err("The house has no safe work stand beside its doorway.".into())
}

fn in_rect(point: Vec2, center: Vec2, half: Vec2, rotation: f32) -> bool {
    shared::rotation::world_to_local_xz(point - center, rotation)
        .abs()
        .cmple(half)
        .all()
}

/// Occupants inside the old house are safe; only actors in newly solid space
/// defer the final swap. This never teleports a bystander out of construction.
pub(super) fn expansion_is_occupied(
    world: &mut World,
    old: HouseAppearance,
    target: HouseAppearance,
    origin: Vec3,
    rotation: f32,
) -> bool {
    let old = old.building_type().definition();
    let new = target.building_type().definition();
    let mut bodies = world.query_filtered::<&PlayerPosition, (
        Or<(With<CharacterKind>, With<Horse>)>,
        Without<crate::world::village::strategic::StrategicPerson>,
        Without<AboardBoat>,
    )>();
    bodies.iter(world).any(|body| {
        in_rect(
            body.0.xz(),
            new.world_footprint_center(origin, rotation),
            new.footprint * 0.5 + Vec2::splat(0.3),
            rotation,
        ) && !in_rect(
            body.0.xz(),
            old.world_footprint_center(origin, rotation),
            old.footprint * 0.5,
            rotation,
        )
    })
}

/// A road's broad future reservation already touches its own doorway apron.
/// Keep that accepted entrance, but never let an extension cover the route's
/// centreline or a different road's public reservation. The exemption ends
/// after one shoulder-width of outward travel, rather than exempting the whole
/// connector merely because one endpoint happens to be this house's door.
pub(super) fn road_blocks_extension(
    road: &VillageRoad,
    original: HouseAppearance,
    target: HouseAppearance,
    position: Vec3,
    rotation: f32,
) -> bool {
    let new = target.building_type().definition();
    let center = new.world_footprint_center(position, rotation);
    let half = new.footprint * 0.5;
    if !road.intersects_rotated_rect(center, half, rotation, 0.15) {
        return false;
    }
    let door = SettlementBuildingKind::House
        .entrance_position(position, rotation)
        .xz();
    let reverse = if road
        .points
        .first()
        .is_some_and(|point| point.distance_squared(door) < 0.25)
    {
        false
    } else if road
        .points
        .last()
        .is_some_and(|point| point.distance_squared(door) < 0.25)
    {
        true
    } else {
        return true;
    };
    let old = original.building_type().definition();
    if !road.intersects_rotated_rect(
        old.world_footprint_center(position, rotation),
        old.footprint * 0.5,
        rotation,
        0.15,
    ) {
        return true;
    }
    let radius = road.reservation_width() * 0.5 + 0.15;
    let apron = radius + 1.0;
    let mut travelled = 0.0;
    for index in 0..road.points.len().saturating_sub(1) {
        let (a, b) = if reverse {
            (
                road.points[road.points.len() - 1 - index],
                road.points[road.points.len() - 2 - index],
            )
        } else {
            (road.points[index], road.points[index + 1])
        };
        // A lawful doorway corridor approaches from outside the solid model;
        // even its first segment cannot be swallowed by a larger extension.
        if segment_hits_rect(a, b, center, half + Vec2::splat(0.2), rotation) {
            return true;
        }
        let length = a.distance(b);
        if travelled + length > apron {
            let visible_start = if travelled < apron && length > 0.0 {
                a.lerp(b, (apron - travelled) / length)
            } else {
                a
            };
            if segment_hits_rect(
                visible_start,
                b,
                center,
                half + Vec2::splat(radius),
                rotation,
            ) {
                return true;
            }
        }
        travelled += length;
    }
    false
}

fn segment_hits_rect(a: Vec2, b: Vec2, center: Vec2, half: Vec2, rotation: f32) -> bool {
    let start = shared::rotation::world_to_local_xz(a - center, rotation);
    let delta = shared::rotation::world_to_local_xz(b - a, rotation);
    let mut enter = 0.0f32;
    let mut leave = 1.0f32;
    for (origin, direction, extent) in [(start.x, delta.x, half.x), (start.y, delta.y, half.y)] {
        if direction.abs() < 1e-6 {
            if origin.abs() > extent {
                return false;
            }
        } else {
            let a = (-extent - origin) / direction;
            let b = (extent - origin) / direction;
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
            if enter > leave {
                return false;
            }
        }
    }
    true
}
