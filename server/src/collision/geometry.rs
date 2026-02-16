//! Collision geometry internals shared by resolver systems.

use bevy::prelude::*;

use shared::physics::WALKABLE_THRESHOLD;

use crate::ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex};
use crate::collision::library::{DerivedColliderLibrary, DerivedHull, HullFace, StaticColliders};

const COLLIDER_CELL_SIZE: f32 = 16.0;

fn cell_key(x: f32, z: f32) -> (i32, i32) {
    (
        (x / COLLIDER_CELL_SIZE).floor() as i32,
        (z / COLLIDER_CELL_SIZE).floor() as i32,
    )
}

/// Support contact info returned by collision resolution.
#[derive(Default, Clone, Copy)]
pub struct SupportContact {
    /// True if bottom sphere made contact with a walkable surface.
    pub has_support: bool,
    /// The best support normal encountered.
    pub support_normal: Vec3,
}

/// Vehicle-specific collision that can skip small obstacles (hover over rocks).
pub fn handle_vehicle_vs_static(
    derived: &DerivedColliderLibrary,
    colliders: &StaticColliders,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
    min_obstacle_radius: f32,
    candidate_ids: &mut Vec<u32>,
) {
    let half_h = height * 0.5;

    for _ in 0..4 {
        let mut moved = false;
        collect_nearby_instance_ids(colliders, *pos, radius + 6.0, candidate_ids);

        for id in candidate_ids.iter() {
            let Some(inst) = colliders.instances.get(id) else {
                continue;
            };
            let Some(shape) = derived.by_kind.get(&inst.kind) else {
                continue;
            };

            let obstacle_size = shape.bounding_radius * inst.scale;
            if obstacle_size < min_obstacle_radius {
                continue;
            }

            let bounding_r = obstacle_size.max(0.05);
            let to_prop = *pos - inst.position;
            let dist2 = to_prop.length_squared();
            let max_dist = radius + bounding_r + half_h;
            if dist2 >= max_dist * max_dist {
                continue;
            }

            let sphere_positions = [
                *pos - Vec3::Y * (half_h - radius).max(0.0),
                *pos,
                *pos + Vec3::Y * (half_h - radius).max(0.0),
            ];

            let mut best_penetration = 0.0f32;
            let mut best_normal = Vec3::ZERO;

            for sphere_pos in sphere_positions {
                if let Some((pen, normal)) = sphere_vs_compound_hulls(
                    sphere_pos,
                    radius,
                    &shape.hulls,
                    inst.position,
                    inst.rotation,
                    inst.scale,
                ) {
                    if pen > best_penetration {
                        best_penetration = pen;
                        best_normal = normal;
                    }
                }
            }

            if best_penetration <= 0.0 {
                continue;
            }

            let push = best_normal * best_penetration;
            pos.x += push.x;
            pos.y += push.y.max(0.0);
            pos.z += push.z;

            if let Some(v) = velocity.as_deref_mut() {
                let vn = v.dot(best_normal);
                if vn < 0.0 {
                    *v -= best_normal * vn;
                }
            }

            moved = true;
        }

        if !moved {
            break;
        }
    }
}

pub fn handle_capsule_vs_static(
    derived: &DerivedColliderLibrary,
    colliders: &StaticColliders,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
    step_up_height: f32,
    candidate_ids: &mut Vec<u32>,
) -> SupportContact {
    let half_h = height * 0.5;
    let step_up_height = step_up_height.max(0.0);
    let sphere_offset = (half_h - radius).max(0.0);
    let mut support = SupportContact::default();

    for _ in 0..4 {
        let mut moved = false;
        collect_nearby_instance_ids(colliders, *pos, radius + 6.0, candidate_ids);

        for id in candidate_ids.iter() {
            let Some(inst) = colliders.instances.get(id) else {
                continue;
            };
            let Some(shape) = derived.by_kind.get(&inst.kind) else {
                continue;
            };

            let bounding_r = (shape.bounding_radius * inst.scale).max(0.05);
            let to_prop = *pos - inst.position;
            let dist2 = to_prop.length_squared();
            let max_dist = radius + bounding_r + half_h;
            if dist2 >= max_dist * max_dist {
                continue;
            }

            let sphere_positions = [
                *pos - Vec3::Y * sphere_offset,
                *pos,
                *pos + Vec3::Y * sphere_offset,
            ];

            let mut best_penetration = 0.0f32;
            let mut best_normal = Vec3::ZERO;
            let mut best_sphere_idx: usize = 1;
            let mut bottom_support_normal = Vec3::ZERO;
            let mut bottom_has_support = false;

            for (sphere_idx, sphere_pos) in sphere_positions.iter().copied().enumerate() {
                if let Some((pen, normal)) = sphere_vs_compound_hulls(
                    sphere_pos,
                    radius,
                    &shape.hulls,
                    inst.position,
                    inst.rotation,
                    inst.scale,
                ) {
                    if sphere_idx == 0 && normal.y > WALKABLE_THRESHOLD {
                        bottom_has_support = true;
                        if normal.y > bottom_support_normal.y {
                            bottom_support_normal = normal;
                        }
                    }
                    if pen > best_penetration {
                        best_penetration = pen;
                        best_normal = normal;
                        best_sphere_idx = sphere_idx;
                    }
                }
            }

            if best_penetration <= 0.0 {
                continue;
            }

            let is_walkable_slope = best_normal.y > WALKABLE_THRESHOLD;

            if bottom_has_support {
                support.has_support = true;
                if bottom_support_normal.y > support.support_normal.y {
                    support.support_normal = bottom_support_normal;
                }
            }

            let vel_y = velocity.as_deref().map(|v| v.y).unwrap_or(0.0);
            let can_step = step_up_height > 0.0
                && !is_walkable_slope
                && best_sphere_idx == 0
                && best_normal.y.abs() < 0.2
                && vel_y <= 0.0;

            if can_step {
                let mut test_pos = *pos;
                test_pos.y += step_up_height;

                let test_sphere_positions = [
                    test_pos - Vec3::Y * sphere_offset,
                    test_pos,
                    test_pos + Vec3::Y * sphere_offset,
                ];

                let mut still_colliding = false;
                for test_sphere_pos in test_sphere_positions.iter().copied() {
                    if sphere_vs_compound_hulls(
                        test_sphere_pos,
                        radius,
                        &shape.hulls,
                        inst.position,
                        inst.rotation,
                        inst.scale,
                    )
                    .is_some()
                    {
                        still_colliding = true;
                        break;
                    }
                }

                if !still_colliding {
                    pos.y = test_pos.y;
                    if let Some(v) = velocity.as_deref_mut() {
                        v.y = v.y.max(0.0);
                    }
                    support.has_support = true;
                    moved = true;
                    continue;
                }
            }

            let push = best_normal * best_penetration;
            pos.x += push.x;
            pos.y += if is_walkable_slope {
                push.y
            } else {
                push.y.max(0.0)
            };
            pos.z += push.z;

            if let Some(v) = velocity.as_deref_mut() {
                let vn = v.dot(best_normal);
                if vn < 0.0 {
                    *v -= best_normal * vn;
                }
            }

            moved = true;
        }

        if !moved {
            break;
        }
    }

    support
}

pub fn handle_capsule_vs_corpse_spheres(
    corpse_index: &CorpseCollisionIndex,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
    candidates: &mut Vec<CorpseBodyPoint>,
) {
    let half_h = height * 0.5;
    let sphere_offset = (half_h - radius).max(0.0);
    let search_radius = radius + half_h + 2.0;
    corpse_index.collect_nearby(*pos, search_radius, candidates);

    for _ in 0..2 {
        let mut moved = false;
        for corpse in candidates.iter().copied() {
            let sphere_positions = [
                *pos - Vec3::Y * sphere_offset,
                *pos,
                *pos + Vec3::Y * sphere_offset,
            ];
            let mut best_pen = 0.0f32;
            let mut best_normal = Vec3::ZERO;
            for sphere_pos in sphere_positions {
                let delta = sphere_pos - corpse.position;
                let dist_sq = delta.length_squared();
                let min_dist = radius + corpse.radius;
                if dist_sq >= min_dist * min_dist || dist_sq <= 1.0e-8 {
                    continue;
                }
                let dist = dist_sq.sqrt();
                let pen = min_dist - dist;
                if pen > best_pen {
                    best_pen = pen;
                    best_normal = delta / dist;
                }
            }

            if best_pen <= 0.0 {
                continue;
            }

            pos.x += best_normal.x * best_pen;
            pos.y += (best_normal.y * best_pen).max(0.0);
            pos.z += best_normal.z * best_pen;

            if let Some(v) = velocity.as_deref_mut() {
                let vn = v.dot(best_normal);
                if vn < 0.0 {
                    *v -= best_normal * vn;
                }
            }
            moved = true;
        }
        if !moved {
            break;
        }
        corpse_index.collect_nearby(*pos, search_radius, candidates);
    }
}

pub fn handle_vehicle_proxy_vs_corpse_spheres(
    corpse_index: &CorpseCollisionIndex,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    proxy_radius: f32,
    candidates: &mut Vec<CorpseBodyPoint>,
) {
    let search_radius = proxy_radius + 2.5;
    corpse_index.collect_nearby(*pos, search_radius, candidates);
    for corpse in candidates.iter().copied() {
        let delta = *pos - corpse.position;
        let dist_sq = delta.length_squared();
        let min_dist = proxy_radius + corpse.radius;
        if dist_sq >= min_dist * min_dist || dist_sq <= 1.0e-8 {
            continue;
        }
        let dist = dist_sq.sqrt();
        let pen = min_dist - dist;
        let normal = delta / dist;
        pos.x += normal.x * pen;
        pos.y += (normal.y * pen).max(0.0);
        pos.z += normal.z * pen;
        if let Some(v) = velocity.as_deref_mut() {
            let vn = v.dot(normal);
            if vn < 0.0 {
                *v -= normal * vn;
            }
        }
    }
}

fn sphere_vs_convex_hull_3d(
    sphere_center: Vec3,
    radius: f32,
    faces: &[HullFace],
    hull_origin: Vec3,
    hull_rotation: Quat,
    scale: f32,
) -> Option<(f32, Vec3)> {
    if faces.is_empty() {
        return None;
    }

    let inv_rotation = hull_rotation.inverse();
    let local_center = inv_rotation * (sphere_center - hull_origin) / scale;
    let local_radius = radius / scale;

    let mut min_dist = f32::INFINITY;
    let mut closest_normal = Vec3::ZERO;
    let mut inside_all_faces = true;

    for face in faces {
        let dist_to_plane = face.normal.dot(local_center) - face.d;

        if dist_to_plane > local_radius {
            return None;
        }

        if dist_to_plane > 0.0 {
            inside_all_faces = false;
        }

        let projected = local_center - face.normal * dist_to_plane;
        let closest_on_face = closest_point_on_triangle(projected, &face.vertices);
        let to_sphere = local_center - closest_on_face;
        let dist = to_sphere.length();

        if dist < min_dist {
            min_dist = dist;
            if dist > 1e-6 {
                closest_normal = to_sphere / dist;
            } else {
                closest_normal = face.normal;
            }
        }
    }

    if inside_all_faces {
        let mut closest_face_dist = f32::INFINITY;
        for face in faces {
            let dist = (face.normal.dot(local_center) - face.d).abs();
            if dist < closest_face_dist {
                closest_face_dist = dist;
                closest_normal = face.normal;
            }
        }
        let penetration = (closest_face_dist + local_radius) * scale;
        let world_normal = hull_rotation * closest_normal;
        return Some((penetration, world_normal));
    }

    if min_dist < local_radius {
        let penetration = (local_radius - min_dist) * scale;
        let world_normal = hull_rotation * closest_normal;
        Some((penetration, world_normal))
    } else {
        None
    }
}

pub(crate) fn sphere_vs_compound_hulls(
    sphere_center: Vec3,
    radius: f32,
    hulls: &[DerivedHull],
    hull_origin: Vec3,
    hull_rotation: Quat,
    scale: f32,
) -> Option<(f32, Vec3)> {
    let mut best_pen = 0.0f32;
    let mut best_normal = Vec3::ZERO;
    let mut hit = false;

    for hull in hulls {
        if let Some((pen, normal)) = sphere_vs_convex_hull_3d(
            sphere_center,
            radius,
            &hull.hull_faces,
            hull_origin,
            hull_rotation,
            scale,
        ) {
            if !hit || pen > best_pen {
                best_pen = pen;
                best_normal = normal;
                hit = true;
            }
        }
    }

    hit.then_some((best_pen, best_normal))
}

fn closest_point_on_triangle(p: Vec3, tri: &[Vec3; 3]) -> Vec3 {
    let a = tri[0];
    let b = tri[1];
    let c = tri[2];

    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }

    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

pub(crate) fn collect_nearby_instance_ids(
    colliders: &StaticColliders,
    pos: Vec3,
    radius: f32,
    out: &mut Vec<u32>,
) {
    let (cx, cz) = cell_key(pos.x, pos.z);
    let cells = (radius / COLLIDER_CELL_SIZE).ceil() as i32 + 1;
    out.clear();

    for dx in -cells..=cells {
        for dz in -cells..=cells {
            if let Some(list) = colliders.cells.get(&(cx + dx, cz + dz)) {
                out.extend(list.iter().copied());
            }
        }
    }
}
