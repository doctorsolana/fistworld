//! Direct segment/ray queries against terrain, props and buildings.
//!
//! These query `WorldTerrain` / prop / building data directly, so they work at any
//! distance — no dependency on physics colliders being streamed in around an anchor
//! (the rapier-based variant was removed with the physics engine). For a top-down
//! game whose camera can pan far from any unit, direct queries are the right shape.
//! Rescued from the deleted combat module for line-of-sight work.

#![allow(dead_code)]

use bevy::prelude::*;
use shared::building::{building_rotation_quat, BuildingPosition, PlacedBuilding};
use shared::terrain::WorldTerrain;

use crate::collision::library::{
    DerivedBuildingColliderLibrary, DerivedColliderLibrary, StaticColliders,
};

/// Returns hit point + outward normal for the nearest hit along `[0, ray_length]`.
fn ray_obb_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    ray_length: f32,
    box_center: Vec3,
    box_rotation: Quat,
    half_extents: Vec3,
) -> Option<(Vec3, Vec3)> {
    const EPS: f32 = 1e-6;

    let inv_rot = box_rotation.inverse();
    let local_origin = inv_rot * (ray_origin - box_center);
    let local_dir = inv_rot * ray_dir;

    let mins = -half_extents;
    let maxs = half_extents;
    let mut t_min = 0.0f32;
    let mut t_max = ray_length;

    let axis_test = |origin_axis: f32,
                     dir_axis: f32,
                     min_axis: f32,
                     max_axis: f32,
                     t_min: &mut f32,
                     t_max: &mut f32|
     -> bool {
        if dir_axis.abs() < EPS {
            origin_axis >= min_axis && origin_axis <= max_axis
        } else {
            let inv = 1.0 / dir_axis;
            let mut t1 = (min_axis - origin_axis) * inv;
            let mut t2 = (max_axis - origin_axis) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            *t_min = (*t_min).max(t1);
            *t_max = (*t_max).min(t2);
            *t_min <= *t_max
        }
    };

    if !axis_test(
        local_origin.x,
        local_dir.x,
        mins.x,
        maxs.x,
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }
    if !axis_test(
        local_origin.y,
        local_dir.y,
        mins.y,
        maxs.y,
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }
    if !axis_test(
        local_origin.z,
        local_dir.z,
        mins.z,
        maxs.z,
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }

    let hit_t = if t_min >= 0.0 { t_min } else { t_max };
    if !(0.0..=ray_length).contains(&hit_t) {
        return None;
    }

    let local_hit = local_origin + local_dir * hit_t;
    let hit_world = ray_origin + ray_dir * hit_t;

    // Pick dominant face at hit point.
    let nx = if half_extents.x > EPS {
        (local_hit.x / half_extents.x).abs()
    } else {
        0.0
    };
    let ny = if half_extents.y > EPS {
        (local_hit.y / half_extents.y).abs()
    } else {
        0.0
    };
    let nz = if half_extents.z > EPS {
        (local_hit.z / half_extents.z).abs()
    } else {
        0.0
    };

    let normal_local = if nx >= ny && nx >= nz {
        Vec3::new(local_hit.x.signum(), 0.0, 0.0)
    } else if ny >= nz {
        Vec3::new(0.0, local_hit.y.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, local_hit.z.signum())
    };
    let normal_world = (box_rotation * normal_local).normalize_or_zero();

    Some((hit_world, normal_world))
}

/// Segment vs terrain heightfield intersection.
/// Returns (distance_along_segment, hit_point, hit_normal) for the nearest hit.
pub fn segment_terrain_intersection(
    terrain: &WorldTerrain,
    start: Vec3,
    end: Vec3,
) -> Option<(f32, Vec3, Vec3)> {
    let dir = end - start;
    let length = dir.length();
    if length < 1e-3 {
        return None;
    }
    let ray_dir = dir / length;

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

            let t_hit = hi * length;
            let p_hit = start + ray_dir * t_hit;
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
pub fn segment_props_intersection(
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
pub fn segment_buildings_intersection(
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

        let rotation = building_rotation_quat(building.rotation);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_obb_intersection_hits_axis_aligned_box() {
        let origin = Vec3::new(0.0, 0.0, -5.0);
        let dir = Vec3::Z;
        let center = Vec3::ZERO;
        let rot = Quat::IDENTITY;
        let half = Vec3::splat(1.0);

        let hit = ray_obb_intersection(origin, dir, 20.0, center, rot, half);
        assert!(hit.is_some());
        let (point, normal) = hit.unwrap();
        assert!((point.z + 1.0).abs() < 1e-3);
        assert!(normal.z <= -0.99);
    }

    #[test]
    fn ray_obb_intersection_misses_when_parallel_outside_slab() {
        let origin = Vec3::new(2.5, 0.0, -5.0);
        let dir = Vec3::Z;
        let center = Vec3::ZERO;
        let rot = Quat::from_rotation_y(0.35);
        let half = Vec3::splat(1.0);

        let hit = ray_obb_intersection(origin, dir, 20.0, center, rot, half);
        assert!(hit.is_none());
    }

    #[test]
    fn segment_terrain_intersection_returns_world_distance() {
        let terrain = WorldTerrain::default();
        let x = 0.0;
        let z = 0.0;
        let ground_y = terrain.get_height(x, z);
        let start = Vec3::new(x, ground_y + 5.0, z);
        let end = Vec3::new(x, ground_y - 5.0, z);

        let hit = segment_terrain_intersection(&terrain, start, end).unwrap();
        let (distance, point, normal) = hit;

        assert!((distance - 5.0).abs() < 0.05);
        assert!((point.y - ground_y).abs() < 0.05);
        assert!(normal.y > 0.5);
    }
}
