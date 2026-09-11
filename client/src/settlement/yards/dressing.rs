//! Grounded low-poly household arrangements, in the accepted yard's local XZ.

use bevy::prelude::*;
use shared::components::{HouseholdYard, YardUse};
use shared::terrain::WorldTerrain;

use super::{ground::Ground, mesh::YardMesh, planting::PlantingPlan};

pub(super) const WOOD: Vec3 = Vec3::new(0.43, 0.29, 0.145);
pub(super) const LEAF: Vec3 = Vec3::new(0.38, 0.53, 0.22);

fn clothesline(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    // Tiny plots can be unfenced planted borders. Their outer-edge fallback
    // is valid planting land, but cannot authorize two new solid line posts.
    if yard.fence_segments().is_empty() {
        return;
    }
    let (a, b) = yard.outer_edge();
    if a.distance(b) < 2.4 {
        return;
    }
    let tangent = (b - a).normalize();
    // A longer road-shaped boundary does not imply a gigantic bedsheet. Fit
    // a household-sized line within one intact authoritative fence span.
    let length = (a.distance(b) * 0.76).min(4.2 + super::planting::unit(yard.seed) * 0.7);
    let center = a.lerp(b, 0.43 + super::planting::unit(yard.seed + 13) * 0.14);
    let a = center - tangent * length * 0.5;
    let b = center + tangent * length * 0.5;
    // Both uprights lie on the solid fence footprint, outside the working
    // strip. Rope, cloth and pegs all use this same supported hanging curve.
    let post_height = 2.12 + super::planting::unit(yard.seed + 41) * 0.18;
    let top = ground.at(a, post_height).y.max(ground.at(b, post_height).y);
    for p in [a, b] {
        mesh.beam(
            ground.at(p, -0.12),
            Vec3::new(p.x, top, p.y),
            0.11,
            0.11,
            WOOD,
        );
    }
    let rope = |t: f32| {
        let p = a.lerp(b, t);
        Vec3::new(p.x, top - 0.16 * (std::f32::consts::PI * t).sin(), p.y)
    };
    let panels = if detail { 12 } else { 6 };
    for i in 0..panels {
        mesh.beam(
            rope(i as f32 / panels as f32),
            rope((i + 1) as f32 / panels as f32),
            0.024,
            0.024,
            Vec3::new(0.67, 0.58, 0.40),
        );
    }
    let side = (Vec3::new(b.x - a.x, 0., b.y - a.y)).normalize();
    let normal = Vec3::Y.cross(side);
    let garments: &[(f32, f32, f32)] = match yard.seed % 3 {
        0 => &[(0.09, 0.40, 0.85), (0.58, 0.85, 0.63)],
        1 => &[(0.07, 0.23, 0.57), (0.32, 0.57, 0.79), (0.70, 0.91, 0.63)],
        _ => &[
            (0.07, 0.19, 0.55),
            (0.29, 0.43, 0.70),
            (0.54, 0.69, 0.52),
            (0.78, 0.92, 0.62),
        ],
    };
    for (i, &(left, right, length)) in garments.iter().enumerate() {
        let choice = shared::worldgen::splitmix64(yard.seed.wrapping_add(i as u64 * 41));
        let variation = (choice % 100) as f32 / 99.0;
        let color = if i == 2 && choice % 3 == 0 {
            // One faded garment among warm linen, rather than three identical
            // white rectangles on every household's line.
            Vec3::new(0.55, 0.66, 0.65)
        } else if i == 1 {
            Vec3::new(0.90, 0.86, 0.72)
        } else {
            Vec3::new(0.99, 0.97, 0.86)
        };
        let length = length * (0.94 + variation * 0.13);
        let strips = if detail { 5 } else { 2 };
        let rows = if detail { 2 } else { 1 };
        let cloth = |u: f32, v: f32| {
            let t = left + (right - left) * u;
            let fold = (u * std::f32::consts::TAU * 1.5 + variation * 1.6).sin();
            rope(t) - Vec3::Y * length * v * (0.97 + fold * 0.025)
                + normal * (fold * 0.075 * v + (v * std::f32::consts::PI).sin() * 0.035)
        };
        for j in 0..strips {
            let u0 = j as f32 / strips as f32;
            let u1 = (j + 1) as f32 / strips as f32;
            for row in 0..rows {
                let v0 = row as f32 / rows as f32;
                let v1 = (row + 1) as f32 / rows as f32;
                let q = [cloth(u0, v0), cloth(u0, v1), cloth(u1, v1), cloth(u1, v0)];
                mesh.quad(q, color);
                mesh.quad([q[3], q[2], q[1], q[0]], color * 0.93);
            }
        }
        if detail {
            for t in [left + 0.015, right - 0.015] {
                let at = rope(t);
                mesh.beam(
                    at - Vec3::Y * 0.065,
                    at + Vec3::Y * 0.045,
                    0.035,
                    0.035,
                    WOOD * 1.32,
                );
            }
        }
    }
}

fn firewood(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    let Some((center, tangent)) = yard.firewood_frame() else {
        return;
    };
    let inward = Vec2::new(-tangent.y, tangent.x);
    let corners = [
        center - tangent * 0.82 - inward * 0.30,
        center + tangent * 0.82 - inward * 0.30,
        center - tangent * 0.82 + inward * 0.30,
        center + tangent * 0.82 + inward * 0.30,
    ];
    let level = corners
        .into_iter()
        .map(|p| ground.at(p, 0.12).y)
        .fold(f32::NEG_INFINITY, f32::max);
    // Four grounded feet carry two level support rails. Logs rest on those
    // rails even when the soil is gently sloped; no floating bottom row.
    for p in corners {
        mesh.beam(
            ground.at(p, -0.08),
            Vec3::new(p.x, level, p.y),
            0.12,
            0.12,
            WOOD * 0.8,
        );
    }
    for t in [-0.82, 0.82] {
        let p = center + tangent * t;
        mesh.beam(
            Vec3::new(p.x - inward.x * 0.36, level, p.y - inward.y * 0.36),
            Vec3::new(p.x + inward.x * 0.36, level, p.y + inward.y * 0.36),
            0.13,
            0.12,
            WOOD,
        );
    }
    let rows = if detail { 3 } else { 2 };
    for row in 0..rows {
        let count = 5 - row;
        for i in 0..count {
            let p = center + tangent * ((i as f32 - (count - 1) as f32 * 0.5) * 0.32);
            let h = level + 0.20 + row as f32 * 0.277;
            mesh.log(
                Vec3::new(p.x - inward.x * 0.30, h, p.y - inward.y * 0.30),
                Vec3::new(p.x + inward.x * 0.30, h, p.y + inward.y * 0.30),
                0.16,
            );
        }
    }
}

pub(super) const NEAR_TRIANGLE_BUDGET: usize = 6500;
pub(super) const FAR_TRIANGLE_BUDGET: usize = 4000;

fn build_with_plan(
    yard: &HouseholdYard,
    ground: &Ground,
    plan: &PlantingPlan,
    detail: bool,
) -> YardMesh {
    let budget = if detail {
        NEAR_TRIANGLE_BUDGET
    } else {
        FAR_TRIANGLE_BUDGET
    };
    let mut mesh = YardMesh::default();
    // Structural meshes always stay complete and agree with the authoritative
    // blockers. The bounded decorative groups use the remaining budget.
    super::fences::build(&mut mesh, ground, yard, detail);
    match yard.use_kind {
        YardUse::Laundry => clothesline(&mut mesh, ground, yard, detail),
        YardUse::Firewood => firewood(&mut mesh, ground, yard, detail),
        _ => {}
    }
    plan.draw(&mut mesh, ground, yard, detail, budget);
    debug_assert!(mesh.triangle_count() <= budget);
    mesh
}

pub(super) fn build_lods(
    yard: &HouseholdYard,
    origin: Vec3,
    yaw: f32,
    terrain: &WorldTerrain,
) -> [YardMesh; 2] {
    let ground = Ground::new(yard, origin, yaw, terrain);
    let plan = PlantingPlan::new(yard);
    [
        build_with_plan(yard, &ground, &plan, true),
        build_with_plan(yard, &ground, &plan, false),
    ]
}

#[cfg(test)]
pub(super) fn build(
    yard: &HouseholdYard,
    origin: Vec3,
    yaw: f32,
    terrain: &WorldTerrain,
    detail: bool,
) -> YardMesh {
    let ground = Ground::new(yard, origin, yaw, terrain);
    build_with_plan(yard, &ground, &PlantingPlan::new(yard), detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::YardSide;

    #[test]
    fn full_household_compositions_have_bounded_lod_cost_in_large_and_clipped_plots() {
        let terrain = WorldTerrain::default();
        for seed in [0, 19, 291, 711, 805] {
            for side in [YardSide::Left, YardSide::Right, YardSide::Rear] {
                for use_kind in [
                    YardUse::Vegetables,
                    YardUse::Flowers,
                    YardUse::Laundry,
                    YardUse::Firewood,
                ] {
                    for clipped in [false, true] {
                        let yard = HouseholdYard {
                            minimum: Vec2::ZERO,
                            maximum: Vec2::new(11.8, 11.8),
                            side,
                            use_kind,
                            seed,
                            boundary: if clipped {
                                vec![
                                    Vec2::ZERO,
                                    Vec2::new(9.0, 0.),
                                    Vec2::new(11.8, 7.2),
                                    Vec2::new(8., 11.8),
                                    Vec2::new(0., 11.8),
                                ]
                            } else {
                                Vec::new()
                            },
                            entry: Some(Vec2::new(0., 5.0)),
                            approach: None,
                            house: None,
                        };
                        let [near, far] = build_lods(&yard, Vec3::ZERO, 0.37, &terrain);
                        assert!(!near.is_empty() && !far.is_empty());
                        assert!(
                            near.triangle_count() <= NEAR_TRIANGLE_BUDGET,
                            "seed={seed} side={side:?} use={use_kind:?}"
                        );
                        assert!(
                            far.triangle_count() <= FAR_TRIANGLE_BUDGET,
                            "seed={seed} side={side:?} use={use_kind:?}"
                        );
                        assert!(near.triangle_count() >= far.triangle_count());
                    }
                }
            }
        }
    }
}
