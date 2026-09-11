//! Land/access and composition regressions for the common planting plan.

use super::*;
use bevy::mesh::VertexAttributeValues;
use shared::components::YardSide;
use shared::terrain::WorldTerrain;

fn yard(side: YardSide, narrow: bool, entry: bool, seed: u64) -> HouseholdYard {
    let maximum = match side {
        YardSide::Rear => Vec2::new(9., if narrow { 1.6 } else { 6. }),
        _ => Vec2::new(if narrow { 1.6 } else { 6. }, 9.),
    };
    HouseholdYard {
        minimum: Vec2::ZERO,
        maximum,
        boundary: if narrow {
            Vec::new()
        } else {
            vec![
                Vec2::ZERO,
                Vec2::new(maximum.x - 0.7, 0.),
                maximum,
                Vec2::new(0., maximum.y),
            ]
        },
        side,
        use_kind: YardUse::Flowers,
        seed,
        entry: entry.then_some(match side {
            YardSide::Rear => Vec2::new(0., maximum.y * 0.45),
            _ => Vec2::new(maximum.x * 0.45, 0.),
        }),
        approach: None,
        house: None,
    }
}

#[test]
fn household_uses_form_different_compositions_in_the_same_available_plot() {
    for seed in [19, 291, 805] {
        let base = yard(YardSide::Right, false, true, seed);
        let plan = |use_kind| {
            PlantingPlan::new(&HouseholdYard {
                use_kind,
                ..base.clone()
            })
        };
        let kitchen = plan(YardUse::Vegetables);
        let flowers = plan(YardUse::Flowers);
        let laundry = plan(YardUse::Laundry);
        let wood = plan(YardUse::Firewood);
        assert!(
            kitchen.beds.len() >= 2,
            "roomy kitchen gardens need usable grouped beds, seed={seed}"
        );
        assert!(kitchen.beds.iter().all(|b| b.points.len() >= 3));
        assert!(flowers.beds.is_empty() && wood.beds.is_empty());
        assert!(laundry.beds.len() <= 1);
        assert!(
            flowers.drifts.len() > wood.drifts.len(),
            "flower courts should have richer planted boundaries than wood yards"
        );
    }
}

#[test]
fn planted_meshes_fit_real_boundaries_and_leave_gate_and_home_paths_empty() {
    let terrain = WorldTerrain::default();
    for side in [YardSide::Left, YardSide::Right, YardSide::Rear] {
        for narrow in [false, true] {
            for entry in [false, true] {
                for use_kind in [
                    YardUse::Flowers,
                    YardUse::Vegetables,
                    YardUse::Laundry,
                    YardUse::Firewood,
                ] {
                    let yard = HouseholdYard {
                        use_kind,
                        ..yard(side, narrow, entry, 291)
                    };
                    let ground = Ground::new(&yard, Vec3::ZERO, 0., &terrain);
                    let plan = PlantingPlan::new(&yard);
                    for detail in [false, true] {
                        let mut geometry = YardMesh::default();
                        plan.draw(
                            &mut geometry,
                            &ground,
                            &yard,
                            detail,
                            if detail { 6500 } else { 4000 },
                        );
                        // Constrained plots may have no available planting;
                        // preserving their actual doorway is the priority.
                        if geometry.is_empty() {
                            continue;
                        }
                        let mesh = geometry.finish();
                        let Some(VertexAttributeValues::Float32x3(vertices)) =
                            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                        else {
                            panic!("missing planted vertices")
                        };
                        for [x, _, z] in vertices {
                            let p = Vec2::new(*x, *z);
                            assert!(yard.planting_clear(p, 0.), "{side:?} narrow={narrow} entry={entry} use={use_kind:?} detail={detail}: {p:?}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn large_street_plot_has_varied_planted_boundaries_and_open_working_space() {
    for seed in [19, 291, 711, 805] {
        let yard = yard(YardSide::Right, false, true, seed);
        let plan = PlantingPlan::new(&yard);
        assert!(plan.drifts.len() >= 2);
        assert!(
            plan.drifts.iter().any(|a| plan.drifts.iter().any(|b| a
                .tangent
                .perp_dot(b.tangent)
                .abs()
                > 0.2)),
            "planting should follow more than one parcel boundary"
        );
        assert!(
            plan.drifts.iter().map(|d| d.lobes.len()).sum::<usize>() <= 30 && plan.beds.is_empty()
        );
        assert!(plan
            .beds
            .iter()
            .flat_map(|bed| &bed.points)
            .all(|p| yard.planting_clear(*p, 0.34)));
    }
}
