//! Exact swept intersections with terrain-following wall prisms and gate lintels.
use bevy::prelude::*;
use shared::components::{FortificationKind, FortificationSegment};

pub(super) fn hit(section: &FortificationSegment, a: Vec3, b: Vec3) -> Option<f32> {
    if !section.complete {
        return None;
    }
    let length = section.length();
    if length <= 0.001 {
        return None;
    }
    let axis = (section.end.xz() - section.start.xz()) / length;
    let side = Vec2::new(-axis.y, axis.x);
    let local = |point: Vec3| {
        let delta = point.xz() - section.start.xz();
        let x = delta.dot(axis);
        let floor = if section.kind == FortificationKind::Gate {
            section.start.y.max(section.end.y)
        } else {
            section.start.y + x / length * (section.end.y - section.start.y)
        };
        Vec3::new(x, point.y - floor, delta.dot(side))
    };
    let start = local(a);
    let end = local(b);
    if section.kind == FortificationKind::Wall {
        let half = section.material.thickness() * 0.5;
        return box_hit(
            start,
            end,
            Vec3::new(0.0, -0.2, -half),
            Vec3::new(length, section.material.height(), half),
        );
    }
    let material = section.material;
    let overhang = material.gate_post_width() + 0.1;
    let half_depth = material.thickness().max(0.9) * 0.5;
    let mut closest = box_hit(
        start,
        end,
        Vec3::new(-overhang, material.gate_clear_height(), -half_depth),
        Vec3::new(
            length + overhang,
            material.gate_clear_height() + 0.4,
            half_depth,
        ),
    );
    for post in section.gate_post_centers() {
        let center = local(post);
        let half_width = material.gate_post_width() * 0.5;
        let half_depth = material.gate_post_depth() * 0.5;
        if let Some(hit) = box_hit(
            start,
            end,
            Vec3::new(center.x - half_width, center.y - 0.2, -half_depth),
            Vec3::new(
                center.x + half_width,
                material.gate_clear_height() + 0.4,
                half_depth,
            ),
        ) {
            if closest.is_none_or(|previous| hit < previous) {
                closest = Some(hit);
            }
        }
    }
    closest
}

fn box_hit(start: Vec3, end: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let direction = end - start;
    let mut enter = 0.0_f32;
    let mut leave = 1.0_f32;
    for axis in 0..3 {
        if direction[axis].abs() < 0.000001 {
            if start[axis] < min[axis] || start[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - start[axis]) / direction[axis];
            let b = (max[axis] - start[axis]) / direction[axis];
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
            if enter > leave {
                return None;
            }
        }
    }
    Some(enter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{FortificationMaterial, SettlementId};

    #[test]
    fn arrows_hit_solid_walls_but_can_arc_over_them_and_pass_through_gateways() {
        let mut wall = FortificationSegment {
            settlement_id: SettlementId(1),
            circuit: 0,
            start: Vec3::new(-4.0, 0.0, 0.0),
            end: Vec3::new(4.0, 1.0, 0.0),
            kind: FortificationKind::Wall,
            material: FortificationMaterial::Palisade,
            complete: true,
        };
        let crossing = |y| (Vec3::new(0.0, y, -4.0), Vec3::new(0.0, y, 4.0));
        let (a, b) = crossing(2.0);
        assert!(hit(&wall, a, b).is_some_and(|t| t > 0.4 && t < 0.6));
        assert!(hit(&wall, crossing(5.0).0, crossing(5.0).1).is_none());
        wall.kind = FortificationKind::Gate;
        assert!(hit(&wall, a, b).is_none());
        assert!(hit(&wall, crossing(4.75).0, crossing(4.75).1).is_some());
        for post in wall.gate_post_centers() {
            assert!(
                hit(
                    &wall,
                    post + Vec3::new(0.0, 1.0, -4.0),
                    post + Vec3::new(0.0, 1.0, 4.0)
                )
                .is_some(),
                "completed isolated jamb blocks arrows"
            );
        }
        wall.complete = false;
        assert!(hit(&wall, crossing(4.75).0, crossing(4.75).1).is_none());
    }
}
