use super::*;
use bevy::mesh::VertexAttributeValues;
use shared::components::{distance_squared_to_segment, YardSide, YardUse, YARD_FENCE_THICKNESS};
use shared::terrain::WorldTerrain;

fn yard(seed: u64) -> HouseholdYard {
    HouseholdYard {
        minimum: Vec2::new(4.0, -4.0),
        maximum: Vec2::new(10.0, 5.0),
        side: YardSide::Right,
        use_kind: YardUse::Laundry,
        seed,
        boundary: vec![
            Vec2::new(4., -4.),
            Vec2::new(8., -4.),
            Vec2::new(10., 3.),
            Vec2::new(7., 5.),
            Vec2::new(4., 5.),
        ],
        entry: Some(Vec2::new(6., -4.)),
        approach: Some(Vec2::new(6., -6.)),
        house: None,
    }
}

#[test]
fn seeded_loads_vary_real_silhouettes_and_fit_without_overlapping() {
    let mut seen_kinds = std::collections::HashSet::new();
    let mut seen_counts = std::collections::HashSet::new();
    let mut first = None;
    let mut different = false;
    for seed in (0..128).chain([u64::MAX]) {
        for length in [1.68, 2.8, 4.9] {
            let load = household_load(seed, length);
            assert_eq!(load, household_load(seed, length));
            assert!(!load.is_empty() && load.len() <= 5);
            seen_counts.insert(load.len());
            let mut end = 0.0;
            for garment in &load {
                seen_kinds.insert(garment.kind as u8);
                assert!(garment.left >= end + 0.119);
                end = garment.left + garment.width;
                assert!(end <= length - 0.179);
                assert!(garment.height > 0.5 && garment.height < 1.25);
            }
            if let Some(previous) = &first {
                different |= previous != &load;
            } else {
                first = Some(load);
            }
        }
    }
    assert_eq!(seen_kinds.len(), 4);
    assert!(seen_counts.len() >= 3 && different);
}

#[test]
fn supports_stay_within_a_real_intact_fence_and_unfenced_plots_have_no_line() {
    for seed in (0..64).chain([u64::MAX]) {
        let yard = yard(seed);
        let wash = Wash::for_yard(&yard).unwrap();
        let height = hanging_post_height(&wash);
        assert!(height <= 2.40);
        for garment in &wash.garments {
            assert!(height - wash.sag - garment.height * 1.018 >= YARD_FENCE_HEIGHT + 0.199);
        }
        let supported = yard.fence_segments().iter().any(|&(a, b)| {
            wash.posts.iter().all(|&p| {
                distance_squared_to_segment(p, a, b) < 0.000001
                    && p.distance(a) > 0.10
                    && p.distance(b) > 0.10
            })
        });
        assert!(
            supported,
            "line cannot bridge a gate or create off-fence posts"
        );
        assert_eq!(wash, Wash::for_yard(&yard).unwrap());
    }
    let mut tiny = yard(1);
    tiny.boundary.clear();
    tiny.maximum.x = tiny.minimum.x + 1.8;
    assert!(tiny.fence_segments().is_empty());
    assert!(Wash::for_yard(&tiny).is_none());
}

fn panel_coverage(kind: GarmentKind, point: Vec2) -> usize {
    panels(kind)
        .iter()
        .filter(|panel| {
            let p = panel.map(Vec2::from_array);
            (0..4).all(|i| (p[(i + 1) % 4] - p[i]).perp_dot(point - p[i]) > 0.000001)
        })
        .count()
}

#[test]
fn tunic_neck_and_trouser_legs_are_open_geometry_not_sheet_recolouring() {
    use GarmentKind::*;
    assert_eq!(panel_coverage(Tunic, Vec2::new(0.5, 0.07)), 0);
    assert_eq!(panel_coverage(Tunic, Vec2::new(0.08, 0.23)), 1);
    assert_eq!(panel_coverage(Tunic, Vec2::new(0.08, 0.8)), 0);
    assert_eq!(panel_coverage(Tunic, Vec2::new(0.5, 0.8)), 1);
    assert_eq!(panel_coverage(Trousers, Vec2::new(0.5, 0.8)), 0);
    assert_eq!(panel_coverage(Trousers, Vec2::new(0.28, 0.8)), 1);
    assert_eq!(panel_coverage(Trousers, Vec2::new(0.72, 0.8)), 1);
    for kind in [Tunic, Trousers, Towel, Sheet] {
        for x in 0..31 {
            for y in 0..31 {
                assert!(
                    panel_coverage(
                        kind,
                        Vec2::new((x as f32 + 0.37) / 31., (y as f32 + 0.29) / 31.)
                    ) <= 1
                );
            }
        }
    }
}

#[test]
fn both_lods_keep_attached_two_sided_cloth_inside_the_fence_strip() {
    for kind in [
        GarmentKind::Tunic,
        GarmentKind::Trousers,
        GarmentKind::Towel,
        GarmentKind::Sheet,
    ] {
        let garment = Garment {
            kind,
            left: 0.3,
            width: 1.,
            height: 1.,
            color: Vec3::splat(0.7),
            fold: 0.7,
        };
        for detail in [true, false] {
            let mut mesh = YardMesh::default();
            let rope =
                |t: f32| Vec3::new(t * 3., 2.2 - 0.16 * (t * std::f32::consts::PI).sin(), 0.);
            draw_garment(&mut mesh, &garment, rope, 3., Vec3::Z, detail);
            let mesh = mesh.finish();
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            let cloth_vertices = panels(kind).len() * if detail { 4 * 2 } else { 2 } * 12;
            // Cloth emits each triangle immediately followed by its exact
            // reverse; paired quads could choose different fold diagonals.
            for pair in positions[..cloth_vertices].chunks_exact(6) {
                assert_eq!(pair[0], pair[5]);
                assert_eq!(pair[1], pair[4]);
                assert_eq!(pair[2], pair[3]);
            }
            let Some(VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("normals");
            };
            for pair in normals[..cloth_vertices].chunks_exact(6) {
                let front = Vec3::from_array(pair[0]);
                let back = Vec3::from_array(pair[3]);
                assert!(front.dot(back) < -0.9999);
            }
            let mut attached = 0;
            for &position in &positions[..cloth_vertices] {
                let p = Vec3::from_array(position);
                assert!(p.z.abs() < YARD_FENCE_THICKNESS * 0.5);
                assert!(p.y > 0.95 && p.x >= 0.299 && p.x <= 1.301);
                if p.z.abs() < 0.000001 && (p.y - rope(p.x / 3.).y).abs() < 0.000001 {
                    attached += 1;
                }
            }
            assert!(
                attached >= 4,
                "every garment is attached to the supported rope in both LODs"
            );
        }
    }
}

#[test]
fn complete_washes_have_bounded_cost_and_grounded_posts() {
    let terrain = WorldTerrain::default();
    for seed in 0..64 {
        let yard = yard(seed);
        let ground = Ground::new(&yard, Vec3::ZERO, 0.37, &terrain);
        let wash = Wash::for_yard(&yard).unwrap();
        for detail in [true, false] {
            let mut mesh = YardMesh::default();
            build(&mut mesh, &ground, &yard, detail);
            assert!(mesh.triangle_count() <= if detail { 1100 } else { 320 });
            let mesh = mesh.finish();
            let Some(VertexAttributeValues::Float32x3(points)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            for (i, &post) in wash.posts.iter().enumerate() {
                let minimum = points[i * 36..(i + 1) * 36]
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::INFINITY, f32::min);
                assert!((minimum - ground.at(post, -0.12).y).abs() < 0.00001);
            }
        }
    }
}
