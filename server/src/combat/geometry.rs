//! Internal combat geometry helpers.

use bevy::prelude::*;
use shared::building::{BuildingPosition, PlacedBuilding};
use shared::terrain::WorldTerrain;

use crate::collision::library::{
    DerivedBuildingColliderLibrary, DerivedColliderLibrary, StaticColliders,
};

/// Ray-sphere intersection (approx): returns closest point along ray segment if within radius.
pub(super) fn ray_sphere_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    ray_length: f32,
    sphere_center: Vec3,
    sphere_radius: f32,
) -> Option<Vec3> {
    let to_center = sphere_center - ray_origin;
    let t = to_center.dot(ray_dir).clamp(0.0, ray_length);
    let p = ray_origin + ray_dir * t;
    let d = (p - sphere_center).length();
    let effective = sphere_radius * 1.25;
    (d <= effective).then_some(p)
}

/// Ray-capsule intersection test.
pub(super) fn ray_capsule_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    ray_length: f32,
    capsule_a: Vec3,
    capsule_b: Vec3,
    capsule_radius: f32,
) -> Option<Vec3> {
    let capsule_center = (capsule_a + capsule_b) * 0.5;
    let capsule_half_height = (capsule_b.y - capsule_a.y) * 0.5;

    let to_center = capsule_center - ray_origin;
    let closest_t = to_center.dot(ray_dir).clamp(0.0, ray_length);
    let closest_point = ray_origin + ray_dir * closest_t;

    let effective_radius = capsule_radius * 1.5;

    let horizontal_dist = Vec2::new(
        closest_point.x - capsule_center.x,
        closest_point.z - capsule_center.z,
    )
    .length();

    if horizontal_dist > effective_radius {
        return None;
    }

    let height_diff = closest_point.y - capsule_center.y;
    if height_diff.abs() > capsule_half_height + effective_radius {
        return None;
    }

    Some(closest_point)
}

/// Segment vs terrain heightfield intersection.
pub(super) fn segment_terrain_intersection(
    terrain: &WorldTerrain,
    start: Vec3,
    end: Vec3,
) -> Option<(f32, Vec3, Vec3)> {
    let dir = end - start;
    let length = dir.length();
    if length < 1e-3 {
        return None;
    }

    let f = |p: Vec3| -> f32 { p.y - terrain.get_height(p.x, p.z) };

    if f(start) <= 0.0 {
        let ground_y = terrain.get_height(start.x, start.z);
        let hit_pos = Vec3::new(start.x, ground_y, start.z);
        let normal = terrain.get_normal(hit_pos.x, hit_pos.z);
        return Some((0.0, hit_pos, normal));
    }

    let step_size = 0.5_f32;
    let steps = (length / step_size).ceil().clamp(1.0, 200.0) as u32;

    let mut prev_t = 0.0_f32;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let p = start + dir * t;
        if f(p) <= 0.0 {
            let mut lo = prev_t;
            let mut hi = t;
            for _ in 0..12 {
                let mid = (lo + hi) * 0.5;
                let pmid = start + dir * mid;
                if f(pmid) > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }

            let t_hit = hi;
            let p_hit = start + dir * t_hit;
            let ground_y = terrain.get_height(p_hit.x, p_hit.z);
            let hit_pos = Vec3::new(p_hit.x, ground_y, p_hit.z);
            let normal = terrain.get_normal(hit_pos.x, hit_pos.z);
            return Some((t_hit, hit_pos, normal));
        }
        prev_t = t;
    }

    None
}

/// Test ray segment against static props (trees, rocks, etc.).
/// Returns (t, hit_point, hit_normal) for the closest hit.
pub(super) fn segment_props_intersection(
    start: Vec3,
    end: Vec3,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> Option<(f32, Vec3, Vec3)> {
    let dir = end - start;
    let length = dir.length();
    if length < 1e-4 {
        return None;
    }
    let ray_dir = dir / length;

    let mut best_hit: Option<(f32, Vec3, Vec3)> = None;

    let mid = (start + end) * 0.5;
    let query_radius = length * 0.5 + 5.0;

    // Must match collision cell size.
    let cell_size = 16.0;
    let cx = (mid.x / cell_size).floor() as i32;
    let cz = (mid.z / cell_size).floor() as i32;
    let cells = (query_radius / cell_size).ceil() as i32 + 1;

    for dx in -cells..=cells {
        for dz in -cells..=cells {
            let Some(ids) = colliders.cells.get(&(cx + dx, cz + dz)) else {
                continue;
            };

            for &id in ids {
                let Some(inst) = colliders.instances.get(&id) else {
                    continue;
                };
                let Some(shape) = derived.by_kind.get(&inst.kind) else {
                    continue;
                };

                let bounding_r = shape.bounding_radius * inst.scale;
                let to_prop = mid - inst.position;
                if to_prop.length() > query_radius + bounding_r {
                    continue;
                }

                for hull in &shape.hulls {
                    for face in &hull.hull_faces {
                        let v0 = inst.position + inst.rotation * (face.vertices[0] * inst.scale);
                        let v1 = inst.position + inst.rotation * (face.vertices[1] * inst.scale);
                        let v2 = inst.position + inst.rotation * (face.vertices[2] * inst.scale);
                        let world_normal = inst.rotation * face.normal;

                        if let Some((t, hit_point)) =
                            ray_triangle_intersection(start, ray_dir, length, v0, v1, v2)
                        {
                            match best_hit {
                                Some((best_t, _, _)) if best_t <= t => {}
                                _ => best_hit = Some((t, hit_point, world_normal)),
                            }
                        }
                    }
                }
            }
        }
    }

    best_hit
}

/// Test ray segment against placed buildings (baked convex hulls).
pub(super) fn segment_buildings_intersection(
    start: Vec3,
    end: Vec3,
    buildings: &Query<(&PlacedBuilding, &BuildingPosition)>,
    building_lib: Option<&DerivedBuildingColliderLibrary>,
) -> Option<(f32, Vec3, Vec3)> {
    let building_lib = building_lib?;

    let dir = end - start;
    let length = dir.length();
    if length < 1e-4 {
        return None;
    }
    let ray_dir = dir / length;

    let mut best_hit: Option<(f32, Vec3, Vec3)> = None;

    let mid = (start + end) * 0.5;
    let query_radius = length * 0.5 + 10.0;

    for (building, position) in buildings.iter() {
        let Some(shape) = building_lib.by_type.get(&building.building_type) else {
            continue;
        };

        let bounding_r = shape.bounding_radius;
        let to_building = mid - position.0;
        if to_building.length() > query_radius + bounding_r {
            continue;
        }

        let rotation = Quat::from_rotation_y(building.rotation);

        for hull in &shape.hulls {
            for face in &hull.hull_faces {
                let v0 = position.0 + rotation * face.vertices[0];
                let v1 = position.0 + rotation * face.vertices[1];
                let v2 = position.0 + rotation * face.vertices[2];
                let world_normal = rotation * face.normal;

                if let Some((t, hit_point)) =
                    ray_triangle_intersection(start, ray_dir, length, v0, v1, v2)
                {
                    match best_hit {
                        Some((best_t, _, _)) if best_t <= t => {}
                        _ => best_hit = Some((t, hit_point, world_normal)),
                    }
                }
            }
        }
    }

    best_hit
}

/// Möller–Trumbore ray-triangle intersection.
fn ray_triangle_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    max_t: f32,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
) -> Option<(f32, Vec3)> {
    const EPSILON: f32 = 1e-6;

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = ray_dir.cross(edge2);
    let a = edge1.dot(h);

    if a.abs() < EPSILON {
        return None;
    }

    let f = 1.0 / a;
    let s = ray_origin - v0;
    let u = f * s.dot(h);

    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = s.cross(edge1);
    let v = f * ray_dir.dot(q);

    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * edge2.dot(q);

    if t > EPSILON && t <= max_t {
        let hit_point = ray_origin + ray_dir * t;
        Some((t, hit_point))
    } else {
        None
    }
}
