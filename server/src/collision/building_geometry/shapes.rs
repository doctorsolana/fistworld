use bevy::prelude::*;

use shared::structures::StructureCollider;

pub(super) fn get_structure_bounding_radius(collider: &StructureCollider, scale: f32) -> f32 {
    match collider {
        StructureCollider::Dome { radius, height } => {
            ((radius * radius) + (height * height)).sqrt() * scale
        }
        StructureCollider::Cylinder { radius, height } => {
            ((radius * radius) + (height * 0.5 * height * 0.5)).sqrt() * scale
        }
        StructureCollider::Box { half_extents } => half_extents.length() * scale,
        StructureCollider::Arch {
            width,
            height,
            depth,
            ..
        } => Vec3::new(width * 0.5, *height, depth * 0.5).length() * scale,
        StructureCollider::BakedProp(_) => 0.0,
    }
}

pub(super) fn sphere_vs_structure(
    sphere_center: Vec3,
    sphere_radius: f32,
    collider: &StructureCollider,
    structure_pos: Vec3,
    structure_rot: Quat,
    scale: f32,
) -> Option<(f32, Vec3)> {
    let inv_rot = structure_rot.inverse();
    let local_center = inv_rot * (sphere_center - structure_pos) / scale;
    let local_radius = sphere_radius / scale;

    let result = match collider {
        StructureCollider::Dome { radius, height } => {
            sphere_vs_dome(local_center, local_radius, *radius, *height)
        }
        StructureCollider::Cylinder { radius, height } => {
            sphere_vs_cylinder(local_center, local_radius, *radius, *height)
        }
        StructureCollider::Box { half_extents } => {
            sphere_vs_box(local_center, local_radius, *half_extents)
        }
        StructureCollider::Arch {
            width,
            height,
            depth,
            thickness,
        } => sphere_vs_arch(
            local_center,
            local_radius,
            *width,
            *height,
            *depth,
            *thickness,
        ),
        StructureCollider::BakedProp(_) => None,
    };

    result.map(|(pen, normal)| (pen * scale, structure_rot * normal))
}

fn sphere_vs_dome(
    center: Vec3,
    radius: f32,
    dome_radius: f32,
    dome_height: f32,
) -> Option<(f32, Vec3)> {
    if center.y < -radius {
        return None;
    }

    let effective_height = dome_height * 0.7;

    let xz_dist = (center.x * center.x + center.z * center.z).sqrt();
    if xz_dist > dome_radius + radius && center.y > effective_height + radius {
        return None;
    }

    let height_ratio = effective_height / dome_radius;

    let scaled_center = Vec3::new(center.x, center.y / height_ratio.max(0.01), center.z);
    let dist_to_origin = scaled_center.length();

    if dist_to_origin < dome_radius + radius {
        if dist_to_origin < 0.001 {
            return Some((effective_height + radius - center.y, Vec3::Y));
        }

        let scaled_normal = scaled_center / dist_to_origin;
        let normal = Vec3::new(
            scaled_normal.x,
            scaled_normal.y / height_ratio.max(0.01),
            scaled_normal.z,
        )
        .normalize();

        let penetration = (dome_radius - dist_to_origin + radius).max(0.0);

        if penetration > 0.0 && center.y >= 0.0 {
            return Some((penetration, normal));
        }
    }

    None
}

fn sphere_vs_cylinder(
    center: Vec3,
    radius: f32,
    cyl_radius: f32,
    cyl_height: f32,
) -> Option<(f32, Vec3)> {
    let cyl_half_h = cyl_height * 0.5;
    let cyl_center_y = cyl_half_h;

    let dy = center.y - cyl_center_y;
    if dy.abs() > cyl_half_h + radius {
        return None;
    }

    let xz_dist = (center.x * center.x + center.z * center.z).sqrt();

    if xz_dist < cyl_radius + radius {
        if center.y > 0.0 && center.y < cyl_height {
            let penetration = cyl_radius + radius - xz_dist;
            if penetration > 0.0 {
                let normal = if xz_dist > 0.001 {
                    Vec3::new(center.x / xz_dist, 0.0, center.z / xz_dist)
                } else {
                    Vec3::X
                };
                return Some((penetration, normal));
            }
        }

        if center.y > cyl_height - radius && xz_dist < cyl_radius {
            let pen_top = center.y + radius - cyl_height;
            if pen_top > 0.0 {
                return Some((pen_top, Vec3::Y));
            }
        }
    }

    None
}

fn sphere_vs_box(center: Vec3, radius: f32, half_extents: Vec3) -> Option<(f32, Vec3)> {
    let closest = Vec3::new(
        center.x.clamp(-half_extents.x, half_extents.x),
        center.y.clamp(0.0, half_extents.y * 2.0),
        center.z.clamp(-half_extents.z, half_extents.z),
    );

    let to_sphere = center - closest;
    let dist = to_sphere.length();

    if dist < radius && dist > 0.001 {
        let normal = to_sphere / dist;
        let penetration = radius - dist;
        return Some((penetration, normal));
    } else if dist <= 0.001 {
        let dx = half_extents.x - center.x.abs();
        let dy_bottom = center.y;
        let dy_top = half_extents.y * 2.0 - center.y;
        let dz = half_extents.z - center.z.abs();

        let min_dist = dx.min(dy_bottom).min(dy_top).min(dz);

        let normal = if min_dist == dx {
            Vec3::new(center.x.signum(), 0.0, 0.0)
        } else if min_dist == dy_bottom {
            -Vec3::Y
        } else if min_dist == dy_top {
            Vec3::Y
        } else {
            Vec3::new(0.0, 0.0, center.z.signum())
        };

        return Some((min_dist + radius, normal));
    }

    None
}

fn sphere_vs_arch(
    center: Vec3,
    radius: f32,
    width: f32,
    height: f32,
    depth: f32,
    thickness: f32,
) -> Option<(f32, Vec3)> {
    let half_width = width * 0.5;
    let half_depth = depth * 0.5;
    let pillar_height = height * 0.6;

    let left_center = Vec3::new(-half_width + thickness * 0.5, pillar_height * 0.5, 0.0);
    let left_half = Vec3::new(thickness * 0.5, pillar_height * 0.5, half_depth);
    if let Some(result) = sphere_vs_box(
        center - left_center + Vec3::new(0.0, left_half.y, 0.0),
        radius,
        left_half,
    ) {
        return Some(result);
    }

    let right_center = Vec3::new(half_width - thickness * 0.5, pillar_height * 0.5, 0.0);
    let right_half = Vec3::new(thickness * 0.5, pillar_height * 0.5, half_depth);
    if let Some(result) = sphere_vs_box(
        center - right_center + Vec3::new(0.0, right_half.y, 0.0),
        radius,
        right_half,
    ) {
        return Some(result);
    }

    if center.y > pillar_height - radius && center.y < height + radius {
        let xz_dist = (center.x * center.x + center.z * center.z).sqrt();
        if xz_dist < half_width && center.z.abs() < half_depth {
            let pen = center.y + radius - height;
            if pen > 0.0 && pen < radius * 2.0 {
                return Some((pen, Vec3::Y));
            }
        }
    }

    None
}
