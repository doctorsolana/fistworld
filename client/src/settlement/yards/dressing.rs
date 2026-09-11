//! Grounded low-poly household arrangements, in the accepted yard's local XZ.

use bevy::prelude::*;
use shared::components::{HouseholdYard, YardUse};
use shared::terrain::WorldTerrain;

use super::{ground::Ground, mesh::YardMesh, planting::PlantingPlan};

pub(super) const WOOD: Vec3 = Vec3::new(0.43, 0.29, 0.145);
pub(super) const LEAF: Vec3 = Vec3::new(0.38, 0.53, 0.22);

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
        YardUse::Laundry => super::laundry::build(&mut mesh, ground, yard, detail),
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
        for seed in [0, 19, 291, 711, 805, u64::MAX] {
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
