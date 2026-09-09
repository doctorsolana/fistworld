use bevy::prelude::*;
use shared::components::{
    DefenseCircuit, FortificationKind, FortificationMaterial, FortificationSegment, SettlementId,
    SettlementWallStyle, DEFENSE_CORRIDOR_HALF_WIDTH, DEFENSE_GATE_MIN_WIDTH,
};

#[derive(Clone, Copy)]
pub(super) struct Plot {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub rotation: f32,
}

pub(super) struct RoadApproach {
    pub points: Vec<Vec2>,
    pub width: f32,
}

/// Preserve the charter's broad shape while bending individual corners around
/// accepted land. Uniformly enlarging a ring can hit a different farm on every
/// attempt; a short local detour can keep both old neighborhoods and roads intact.
fn clear_accepted_properties(center: Vec2, boundary: &mut [Vec2], occupied: &[Plot]) -> bool {
    let original = boundary.to_vec();
    let count = boundary.len();
    for _ in 0..32 {
        let mut changed = false;
        for edge in 0..count {
            let next = (edge + 1) % count;
            let vector = boundary[next] - boundary[edge];
            let rotation = -vector.y.atan2(vector.x);
            let midpoint = (boundary[edge] + boundary[next]) * 0.5;
            let half = Vec2::new(vector.length() * 0.5, DEFENSE_CORRIDOR_HALF_WIDTH + 0.6);
            if !occupied.iter().any(|plot| {
                shared::components::oriented_rects_overlap(
                    midpoint,
                    half,
                    rotation,
                    plot.center,
                    plot.half_extents,
                    plot.rotation,
                )
            }) {
                continue;
            }
            changed = true;
            for vertex in [edge, next] {
                let direction = (boundary[vertex] - center).normalize_or_zero();
                boundary[vertex] += direction * 4.0;
                // The survey remains bounded and cannot chase a distant farm
                // indefinitely or consume the whole map looking for a ring.
                if boundary[vertex].distance(original[vertex]) > 48.0 {
                    return false;
                }
            }
        }
        if !changed {
            return true;
        }
    }
    false
}

/// Shorter grounded sections follow convex/concave hillside changes. A single
/// eight-meter chord over a hollow otherwise leaves its foundation in the air.
fn append_grounded_section(
    section: FortificationSegment,
    ground: &mut impl FnMut(Vec2) -> Option<f32>,
    result: &mut Vec<FortificationSegment>,
    depth: u8,
) -> bool {
    if section.kind == FortificationKind::Gate {
        result.push(section);
        return true;
    }
    let midpoint = section.midpoint();
    let Some(height) = ground(midpoint.xz()) else {
        return false;
    };
    let mut curvature = (height - midpoint.y).abs();
    for t in [0.25, 0.75] {
        let sample = section.start.lerp(section.end, t);
        let Some(h) = ground(sample.xz()) else {
            return false;
        };
        curvature = curvature.max((h - sample.y).abs());
    }
    if curvature > 0.14 {
        if depth >= 3 {
            return false;
        }
        let grounded = Vec3::new(midpoint.x, height, midpoint.z);
        let mut first = section.clone();
        first.end = grounded;
        let mut second = section;
        second.start = grounded;
        return append_grounded_section(first, ground, result, depth + 1)
            && append_grounded_section(second, ground, result, depth + 1);
    }
    if (section.end.y - section.start.y).abs() > section.length() * 0.5 {
        return false;
    }
    result.push(section);
    true
}

/// Try a bounded family of nested outlines. Terrain and accepted land uses are
/// hard constraints; no existing property is moved to make the seed fit.
#[allow(clippy::too_many_arguments)]
pub(super) fn fit_circuit(
    settlement_id: SettlementId,
    id: u8,
    center: Vec2,
    minimum_radius: f32,
    style: SettlementWallStyle,
    seed: u64,
    core: &[Vec2],
    occupied: &[Plot],
    roads: &[RoadApproach],
    mut ground: impl FnMut(Vec2) -> Option<f32>,
) -> Option<DefenseCircuit> {
    let phase =
        (shared::worldgen::splitmix64(seed) & 0xffff) as f32 / 65535. * std::f32::consts::TAU;
    for expansion in 0..12 {
        let angle_offset = phase + (expansion % 4) as f32 * 0.047;
        let axis = Vec2::new(angle_offset.cos(), angle_offset.sin());
        let side = Vec2::new(-axis.y, axis.x);
        let core_floor = if id == 0 {
            minimum_radius * 0.55
        } else {
            minimum_radius
        };
        let core_half = core.iter().fold(Vec2::splat(core_floor), |bounds, point| {
            let relative = *point - center;
            bounds.max(Vec2::new(
                relative.dot(axis).abs() + 20.,
                relative.dot(side).abs() + 20.,
            ))
        });
        let radius = minimum_radius + expansion as f32 * 5.0;
        let count = if style == SettlementWallStyle::Square {
            8
        } else {
            24
        };
        let mut boundary: Vec<_> = (0..count)
            .map(|i| {
                let angle = angle_offset + i as f32 / count as f32 * std::f32::consts::TAU;
                let direction = Vec2::new(angle.cos(), angle.sin());
                let factor = match style {
                    SettlementWallStyle::Round => 1.0,
                    SettlementWallStyle::Square => {
                        1.0 / (angle - angle_offset)
                            .cos()
                            .abs()
                            .max((angle - angle_offset).sin().abs())
                    }
                    SettlementWallStyle::Organic => 1.08 + 0.06 * (angle * 3.0 + phase).sin(),
                    // A rounded ward envelope responds to the accepted residential
                    // core's length and breadth instead of treating this style as
                    // a differently named circle. Fourth-power corners leave room
                    // around houses at the ends of a long avenue.
                    SettlementWallStyle::DistrictFitted => {
                        let half = core_half + Vec2::splat(expansion as f32 * 5.0);
                        let local = Vec2::new(direction.dot(axis), direction.dot(side));
                        1.19 / ((local.x / half.x).powi(4) + (local.y / half.y).powi(4)).powf(0.25)
                            / radius
                    }
                };
                center + direction * radius * factor
            })
            .collect();
        if !clear_accepted_properties(center, &mut boundary, occupied) {
            continue;
        }
        let mut sections = Vec::new();
        let mut valid = true;
        for edge in 0..count {
            let a = boundary[edge];
            let b = boundary[(edge + 1) % count];
            let vector = b - a;
            let length = vector.length();
            let tangent = vector / length;
            let mut gates = Vec::new();
            // Four access points preserve future expansion even before all
            // arterial roads exist. Actual crossing roads add aligned gates.
            if edge % (count / 4) == 0 {
                gates.push((
                    length * 0.5 - DEFENSE_GATE_MIN_WIDTH * 0.5,
                    length * 0.5 + DEFENSE_GATE_MIN_WIDTH * 0.5,
                ));
            }
            for road in roads {
                for pair in road.points.windows(2) {
                    let route = pair[1] - pair[0];
                    let denominator = vector.perp_dot(route);
                    if denominator.abs() < 0.0001 {
                        continue;
                    }
                    let offset = pair[0] - a;
                    let t = offset.perp_dot(route) / denominator;
                    let u = offset.perp_dot(vector) / denominator;
                    if (-0.0001..=1.0001).contains(&t) && (-0.0001..=1.0001).contains(&u) {
                        let crossing = t * length;
                        let angle = tangent.perp_dot(route.normalize_or_zero()).abs().max(0.05);
                        let half =
                            ((road.width * 0.5 + 1.8) / angle).max(DEFENSE_GATE_MIN_WIDTH * 0.5);
                        if half > 12.0 || crossing - half < 1.0 || crossing + half > length - 1.0 {
                            valid = false;
                            break;
                        }
                        gates.push((crossing - half, crossing + half));
                    }
                }
                if !valid {
                    break;
                }
            }
            if !valid {
                break;
            }
            gates.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f32, f32)> = Vec::new();
            for (start, end) in gates {
                if start < 0.8 || end > length - 0.8 {
                    valid = false;
                    break;
                }
                if let Some(previous) = merged
                    .last_mut()
                    .filter(|previous| previous.1 + 1.0 >= start)
                {
                    previous.1 = previous.1.max(end);
                } else {
                    merged.push((start, end));
                }
            }
            if !valid {
                break;
            }
            let mut cursor = 0.0;
            let mut pieces = Vec::new();
            for (start, end) in merged {
                pieces.push((cursor, start, FortificationKind::Wall));
                pieces.push((start, end, FortificationKind::Gate));
                cursor = end;
            }
            pieces.push((cursor, length, FortificationKind::Wall));
            for (start, end, kind) in pieces {
                let steps = if kind == FortificationKind::Gate {
                    1
                } else {
                    ((end - start) / 8.0).ceil().max(1.) as usize
                };
                for step in 0..steps {
                    let from = a + tangent * (start + (end - start) * step as f32 / steps as f32);
                    let to =
                        a + tangent * (start + (end - start) * (step + 1) as f32 / steps as f32);
                    let Some(fh) = ground(from) else {
                        valid = false;
                        break;
                    };
                    let Some(th) = ground(to) else {
                        valid = false;
                        break;
                    };
                    let max_grade = if kind == FortificationKind::Gate {
                        0.30
                    } else {
                        0.50
                    };
                    if (fh - th).abs() > from.distance(to) * max_grade {
                        #[cfg(test)]
                        if std::env::var_os("FISTWORLD_DEFENSE_SNAPSHOT").is_some() {
                            println!("defense grade rejection: {from:?} -> {to:?}, heights {fh:.2}->{th:.2}, grade {:.3}",(fh-th).abs()/from.distance(to));
                        }
                        valid = false;
                        break;
                    }
                    let section = FortificationSegment {
                        settlement_id,
                        circuit: id,
                        start: Vec3::new(from.x, fh, from.y),
                        end: Vec3::new(to.x, th, to.y),
                        kind,
                        material: FortificationMaterial::Palisade,
                        complete: false,
                    };
                    if occupied.iter().any(|plot| {
                        section.intersects_footprint(
                            plot.center,
                            plot.half_extents,
                            plot.rotation,
                            DEFENSE_CORRIDOR_HALF_WIDTH + 0.5,
                        )
                    }) {
                        valid = false;
                        break;
                    }
                    // Survey the entire width, not only vertices. Low banks and
                    // river channels can lie between otherwise dry endpoints.
                    let samples = (from.distance(to) / 1.0).ceil().max(1.) as usize;
                    let normal = Vec2::new(-tangent.y, tangent.x);
                    for i in 0..=samples {
                        let point = from.lerp(to, i as f32 / samples as f32);
                        for side in [
                            -DEFENSE_CORRIDOR_HALF_WIDTH,
                            0.,
                            DEFENSE_CORRIDOR_HALF_WIDTH,
                        ] {
                            if ground(point + normal * side).is_none() {
                                valid = false;
                                break;
                            }
                        }
                        if !valid {
                            break;
                        }
                    }
                    if !valid {
                        break;
                    }
                    if !append_grounded_section(section, &mut ground, &mut sections, 0) {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    break;
                }
            }
            if !valid {
                break;
            }
        }
        if valid && !sections.is_empty() {
            return Some(DefenseCircuit {
                id,
                center,
                boundary,
                sections,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "diagnose a saved actual town via FISTWORLD_DEFENSE_SNAPSHOT"]
    fn defense_snapshot_fit_diagnostic() {
        let snapshot = shared::settlement_snapshot::TownSnapshot::read(
            std::env::var("FISTWORLD_DEFENSE_SNAPSHOT").unwrap(),
        )
        .unwrap();
        let town = &snapshot.settlements[0];
        let center = town.position.xz();
        let homes: Vec<_> = snapshot
            .buildings
            .iter()
            .filter(|b| {
                b.kind == shared::components::SettlementBuildingKind::House
                    && b.construction.is_none()
            })
            .map(|b| b.position.xz())
            .collect();
        let mut radii: Vec<_> = homes.iter().map(|p| p.distance(center)).collect();
        radii.sort_by(f32::total_cmp);
        let residential = radii[radii.len() * 3 / 4];
        let radius = (residential + 20.).max(60.);
        let core: Vec<_> = homes
            .iter()
            .copied()
            .filter(|p| p.distance(center) <= residential + 1.)
            .collect();
        let mut occupied: Vec<_> = snapshot
            .buildings
            .iter()
            .map(|b| Plot {
                center: b.footprint_center,
                half_extents: b.footprint * 0.5,
                rotation: b.rotation,
            })
            .collect();
        occupied.extend(snapshot.fields.iter().map(|b| Plot {
            center: b.footprint_center,
            half_extents: b.footprint * 0.5,
            rotation: b.rotation,
        }));
        occupied.extend(snapshot.pastures.iter().map(|b| Plot {
            center: b.footprint_center,
            half_extents: b.footprint * 0.5,
            rotation: b.rotation,
        }));
        let roads: Vec<_> = snapshot
            .roads
            .iter()
            .map(|r| RoadApproach {
                points: r.road.points.clone(),
                width: r.road.reservation_width(),
            })
            .collect();
        for (name, plots, routes) in [
            ("empty", &[][..], &[][..]),
            ("properties", &occupied[..], &[][..]),
            ("roads", &[][..], &roads[..]),
            ("both", &occupied[..], &roads[..]),
        ] {
            let result = fit_circuit(
                town.id,
                0,
                center,
                radius,
                town.development.inner_wall,
                town.development.plan_seed,
                &core,
                plots,
                routes,
                |_| Some(town.position.y),
            );
            println!(
                "defense snapshot {} day={} houses={} radius={radius} style={:?} fit={:?}",
                name,
                snapshot.day,
                homes.len(),
                town.development.inner_wall,
                result.map(|c| (c.sections.len(), c.boundary[0].distance(center)))
            );
        }
        if std::env::var("CITYSIM_MAP_ID").as_deref() == Ok(snapshot.map_id.as_str()) {
            let mut terrain = shared::terrain::WorldTerrain::default();
            terrain.replace_delta_chunks(
                snapshot
                    .terrain_deltas
                    .iter()
                    .map(|chunk| (chunk.coord, chunk.to_delta_data()))
                    .collect(),
            );
            let mut app = App::new();
            app.add_systems(Startup, crate::collision::library::setup_baked_colliders);
            app.update();
            let library = app
                .world()
                .resource::<crate::collision::library::DerivedColliderLibrary>();
            for with_props in [false, true] {
                let mut cache = std::collections::HashMap::new();
                let result = fit_circuit(
                    town.id,
                    0,
                    center,
                    radius,
                    town.development.inner_wall,
                    town.development.plan_seed,
                    &core,
                    &occupied,
                    &roads,
                    |p| {
                        let h = terrain.get_height(p.x, p.y);
                        if terrain
                            .water_surface_height(p.x, p.y)
                            .is_some_and(|water| h < water + 0.7)
                        {
                            return None;
                        }
                        if with_props {
                            let chunk =
                                shared::terrain::ChunkCoord::from_world_pos(Vec3::new(p.x, h, p.y));
                            for dx in -1..=1 {
                                for dz in -1..=1 {
                                    let chunk = shared::terrain::ChunkCoord::new(
                                        chunk.x + dx,
                                        chunk.z + dz,
                                    );
                                    let props = cache.entry(chunk).or_insert_with(|| {
                                        shared::props::generate_chunk_blocking_props(
                                            &terrain.generator,
                                            chunk,
                                        )
                                    });
                                    if props.iter().any(|prop| {
                                        library.by_kind.get(&prop.kind).is_some_and(|shape| {
                                            prop.position.distance(p)
                                                < shape.horizontal_radius * prop.scale + 0.6
                                        })
                                    }) {
                                        return None;
                                    }
                                }
                            }
                        }
                        Some(h)
                    },
                );
                println!(
                    "actual terrain, with_props={with_props}: fit={:?}",
                    result.map(|c| (c.sections.len(), c.boundary[0].distance(center)))
                );
            }
        }
    }
    #[test]
    fn circuit_is_deterministic_closed_and_keeps_dry_ground() {
        let build = || {
            fit_circuit(
                SettlementId(1),
                0,
                Vec2::ZERO,
                60.,
                SettlementWallStyle::Round,
                23,
                &[],
                &[],
                &[],
                |_| Some(2.),
            )
            .unwrap()
        };
        let circuit = build();
        assert_eq!(circuit, build());
        for pair in circuit.sections.windows(2) {
            assert!(pair[0].end.distance(pair[1].start) < 0.001);
        }
        assert!(
            circuit
                .sections
                .last()
                .unwrap()
                .end
                .distance(circuit.sections[0].start)
                < 0.001
        );
        assert_eq!(
            circuit
                .sections
                .iter()
                .filter(|s| s.kind == FortificationKind::Gate)
                .count(),
            4
        );
        assert!(fit_circuit(
            SettlementId(1),
            0,
            Vec2::ZERO,
            60.,
            SettlementWallStyle::Round,
            23,
            &[],
            &[],
            &[],
            |_| None
        )
        .is_none());
    }
    #[test]
    fn accepted_plots_and_crossing_roads_survive_enclosure() {
        let road = RoadApproach {
            points: vec![Vec2::ZERO, Vec2::new(180., 0.)],
            width: 6.,
        };
        let plot = Plot {
            center: Vec2::new(45., 20.),
            half_extents: Vec2::splat(10.),
            rotation: 0.4,
        };
        let circuit = fit_circuit(
            SettlementId(1),
            0,
            Vec2::ZERO,
            60.,
            SettlementWallStyle::Round,
            23,
            &[plot.center],
            &[plot],
            &[road],
            |_| Some(1.),
        )
        .unwrap();
        assert!(circuit.sections.iter().all(|s| !s.intersects_footprint(
            plot.center,
            plot.half_extents,
            plot.rotation,
            DEFENSE_CORRIDOR_HALF_WIDTH
        )));
        let mut grid = shared::spatial::SpatialObstacleGrid::default();
        for mut section in circuit.sections {
            section.complete = true;
            for obstacle in section.ground_obstacles() {
                grid.insert(obstacle);
            }
        }
        assert!(!grid.segment_blocked(Vec2::ZERO, Vec2::new(180., 0.)));
    }
    #[test]
    fn district_fitted_outline_responds_to_actual_residential_extent() {
        let core = [Vec2::new(-90., 0.), Vec2::new(90., 0.)];
        let plan = fit_circuit(
            SettlementId(1),
            0,
            Vec2::ZERO,
            100.,
            SettlementWallStyle::DistrictFitted,
            23,
            &core,
            &[],
            &[],
            |_| Some(0.),
        )
        .unwrap();
        let empty = fit_circuit(
            SettlementId(1),
            0,
            Vec2::ZERO,
            100.,
            SettlementWallStyle::DistrictFitted,
            23,
            &[],
            &[],
            &[],
            |_| Some(0.),
        )
        .unwrap();
        assert_ne!(plan.boundary, empty.boundary);
    }

    #[test]
    fn a_river_between_outline_vertices_rejects_the_entire_circuit() {
        let result = fit_circuit(
            SettlementId(1),
            0,
            Vec2::ZERO,
            60.,
            SettlementWallStyle::Round,
            23,
            &[],
            &[],
            &[],
            |point| (!(point.x.abs() < 1.0 && point.y > 20.0)).then_some(2.0),
        );
        assert!(
            result.is_none(),
            "endpoint-only dry checks must not bridge a river"
        );
    }
    #[test]
    fn secondary_circuit_cannot_intersect_the_inner_enclosure() {
        let inner = fit_circuit(
            SettlementId(1),
            0,
            Vec2::ZERO,
            60.,
            SettlementWallStyle::Square,
            23,
            &[],
            &[],
            &[],
            |_| Some(0.),
        )
        .unwrap();
        let outer_radius = inner.boundary.iter().map(|p| p.length()).fold(0., f32::max) + 35.;
        let outer = fit_circuit(
            SettlementId(1),
            1,
            Vec2::ZERO,
            outer_radius,
            SettlementWallStyle::Organic,
            41,
            &[],
            &[],
            &[],
            |_| Some(0.),
        )
        .unwrap();
        assert!(outer.boundary.iter().all(
            |p| p.length() > inner.boundary.iter().map(|p| p.length()).fold(0., f32::max) + 30.
        ));
        let fitted = fit_circuit(
            SettlementId(1),
            1,
            Vec2::ZERO,
            outer_radius,
            SettlementWallStyle::DistrictFitted,
            41,
            &[],
            &[],
            &[],
            |_| Some(0.),
        )
        .unwrap();
        assert!(fitted.boundary.iter().all(|p| p.length() > outer_radius));
    }
}
