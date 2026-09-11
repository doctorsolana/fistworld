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
                            assert!(plant_fits(&yard, p, 0.), "{side:?} narrow={narrow} entry={entry} use={use_kind:?} detail={detail}: {p:?}");
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

#[test]
fn household_planting_is_stable_but_varies_species_size_density_and_colour() {
    let plans: Vec<_> = (0..24)
        .map(|seed| {
            let yard = yard(YardSide::Right, false, true, seed);
            let plan = PlantingPlan::new(&yard);
            assert_eq!(
                plan,
                PlantingPlan::new(&yard),
                "rebuild must preserve the household"
            );
            plan
        })
        .collect();
    for kind in [PlantKind::Shrub, PlantKind::Perennial, PlantKind::Herbs] {
        assert!(
            plans
                .iter()
                .flat_map(|p| &p.drifts)
                .flat_map(|d| &d.lobes)
                .any(|l| l.kind == kind),
            "missing {kind:?}"
        );
    }
    let counts: std::collections::HashSet<_> = plans
        .iter()
        .map(|p| p.drifts.iter().map(|d| d.lobes.len()).sum::<usize>())
        .collect();
    assert!(
        counts.len() >= 5,
        "all households have the same planting density"
    );
    let flowers: Vec<_> = plans
        .iter()
        .flat_map(|p| &p.drifts)
        .flat_map(|d| &d.flowers)
        .collect();
    assert!(flowers.iter().any(|f| f.yellow) && flowers.iter().any(|f| !f.yellow));
    assert!(plans.windows(2).all(|p| p[0] != p[1]));
}

#[test]
fn every_foliage_silhouette_stays_in_its_fitted_disk_at_both_lods() {
    let terrain = WorldTerrain::default();
    let yard = yard(YardSide::Right, false, true, 0);
    let ground = Ground::new(&yard, Vec3::ZERO, 0., &terrain);
    for seed in [0, 19, 291, 805, u64::MAX] {
        for radius in [0.132, 0.28, 0.52, 0.65] {
            for kind in [PlantKind::Shrub, PlantKind::Perennial, PlantKind::Herbs] {
                for detail in [false, true] {
                    let lobe = Lobe {
                        position: Vec2::new(3., 3.),
                        radius,
                        height: 0.7,
                        seed,
                        kind,
                        color: Vec3::splat(0.5),
                    };
                    let mut geometry = YardMesh::default();
                    lobe.draw(&mut geometry, &ground, detail);
                    let mesh = geometry.finish();
                    let Some(VertexAttributeValues::Float32x3(vertices)) =
                        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                    else {
                        panic!("missing vertices");
                    };
                    assert!(vertices.iter().all(|[x, y, z]| y.is_finite()
                        && Vec2::new(*x, *z).distance(lobe.position) <= radius),
                        "{kind:?} seed={seed} radius={radius} detail={detail} exceeds accepted footprint");
                }
            }
        }
        assert_eq!(
            PlantingPlan::new(&HouseholdYard {
                seed,
                ..yard.clone()
            }),
            PlantingPlan::new(&HouseholdYard {
                seed,
                ..yard.clone()
            })
        );
    }
}
