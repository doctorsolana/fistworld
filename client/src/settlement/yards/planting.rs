//! Workable planted rows and flowering edge groups inside accepted household land.
use super::{
    dressing::{Ground, LEAF, WOOD},
    mesh::YardMesh,
};
use bevy::prelude::*;
use shared::components::{HouseholdYard, YardSide, YardUse};

fn unit(seed: u64) -> f32 {
    (shared::worldgen::splitmix64(seed) % 1000) as f32 / 999.0
}

/// Reserve a continuous one-metre working strip on the homeward side even
/// when the accepted outer boundary is diagonal. Each crown/petal fits too.
fn plant_fits(yard: &HouseholdYard, p: Vec2, radius: f32) -> bool {
    if !yard.contains_local_point(p, -radius - 0.04) {
        return false;
    }
    match yard.side {
        YardSide::Left => p.x + radius <= yard.maximum.x - 1.0,
        YardSide::Right => p.x - radius >= yard.minimum.x + 1.0,
        YardSide::Rear => p.y - radius >= yard.minimum.y + 1.0,
    }
}

fn soil_strip(
    mesh: &mut YardMesh,
    ground: &Ground,
    yard: &HouseholdYard,
    a: Vec2,
    b: Vec2,
    side: Vec2,
    seed: u64,
) {
    let count = (a.distance(b) / 0.40).ceil().max(1.0) as usize;
    for i in 0..count {
        let p = a.lerp(b, i as f32 / count as f32);
        let q = a.lerp(b, (i + 1) as f32 / count as f32);
        let w = 0.27 * (0.95 + unit(seed + i as u64) * 0.08);
        let corners = [p + side * w, q + side * w, q - side * w, p - side * w];
        if !corners.iter().all(|p| plant_fits(yard, *p, 0.0)) {
            continue;
        }
        let color = Vec3::new(0.34, 0.235, 0.115) * (0.94 + unit(seed + i as u64 + 53) * 0.12);
        mesh.quad(corners.map(|p| ground.at(p, 0.045)), color);
    }
}

fn cabbage(mesh: &mut YardMesh, ground: &Ground, p: Vec2, seed: u64, color: Vec3, detail: bool) {
    // The distant crown keeps the entire plant's width instead of dropping
    // alternate cabbages and leaving bare holes along an otherwise full bed.
    mesh.crown(
        ground.at(p, 0.23),
        Vec3::new(
            if detail { 0.21 } else { 0.31 },
            0.20,
            if detail { 0.21 } else { 0.31 },
        ),
        color * 1.08,
        unit(seed) * std::f32::consts::TAU,
    );
    if detail {
        for i in 0..5 {
            let angle = unit(seed) * std::f32::consts::TAU + i as f32 * 2.4;
            let d = Vec2::from_angle(angle);
            mesh.leaf(
                ground.at(p + d * 0.02, 0.07),
                ground.at(p, 0.0) + Vec3::new(d.x * 0.29, 0.20 + unit(seed + i) * 0.06, d.y * 0.29),
                0.13,
                color * (0.91 + unit(seed + i + 7) * 0.16),
            );
        }
    }
}

fn herbs(mesh: &mut YardMesh, ground: &Ground, p: Vec2, seed: u64, color: Vec3, detail: bool) {
    for i in 0..if detail { 5 } else { 3 } {
        let d = Vec2::from_angle(i as f32 * 2.4 + unit(seed));
        mesh.leaf(
            ground.at(p, -0.01),
            ground.at(p, 0.0) + Vec3::new(d.x * 0.22, 0.38 + unit(seed + i + 7) * 0.13, d.y * 0.22),
            if detail { 0.09 } else { 0.13 },
            color * (0.95 + unit(seed + i) * 0.15),
        );
    }
}

pub(super) fn garden(
    mesh: &mut YardMesh,
    ground: &Ground,
    yard: &HouseholdYard,
    detail: bool,
    full: bool,
) {
    let (a, b) = yard.outer_edge();
    let length = a.distance(b);
    if length < 1.5 {
        return;
    }
    let along = (b - a).normalize();
    let inward = Vec2::new(-along.y, along.x);
    let max_depth = yard
        .boundary_points()
        .into_iter()
        .map(|p| (p - a).dot(inward))
        .fold(0., f32::max);
    let rows = (((max_depth - 1.0 - 0.34 - 0.64) / 0.68).floor() + 1.0).clamp(0., 4.) as usize;
    for row in 0..if full { rows } else { rows.min(2) } {
        let v = 0.64 + row as f32 * 0.68;
        let mut runs: Vec<Vec<Vec2>> = vec![Vec::new()];
        let count = ((length - 1.0) / 0.63).floor().max(1.0) as usize;
        for i in 0..=count {
            let t = 0.50 + (length - 1.0) * i as f32 / count as f32;
            // Include crown radii in the cross-path setback; leaves must not
            // close the apparent route between two otherwise separated beds.
            let cross_path =
                full && length > 5.5 && t > length * 0.5 - 0.34 && t < length * 0.5 + 1.04;
            let permitted_use = full || t < length * (0.43 + unit(yard.seed) * 0.10);
            let p = a + along * t + inward * v;
            if cross_path || !permitted_use || !plant_fits(yard, p, 0.34) {
                if !runs.last().unwrap().is_empty() {
                    runs.push(Vec::new());
                }
                continue;
            }
            runs.last_mut().unwrap().push(p);
        }
        for (r, run) in runs.into_iter().enumerate() {
            if run.len() < 2 {
                continue;
            }
            let seed = yard.seed + row as u64 * 37 + r as u64 * 103;
            soil_strip(
                mesh,
                ground,
                yard,
                run[0] - along * 0.28,
                *run.last().unwrap() + along * 0.28,
                inward,
                seed,
            );
            if detail && seed % 3 == 0 {
                let p = run[0] - along * 0.22;
                mesh.beam(
                    ground.at(p - inward * 0.25, 0.065),
                    ground.at(p + inward * 0.25, 0.065),
                    0.08,
                    0.10,
                    WOOD * 1.08,
                );
            }
            for (i, p) in run.into_iter().enumerate() {
                let seed = seed + i as u64 * 19;
                let color = if row % 3 == 2 && yard.seed % 3 == 0 {
                    Vec3::new(0.47, 0.34, 0.43)
                } else {
                    LEAF * (1.0 + unit(seed + 3) * 0.17)
                };
                if yard.use_kind == YardUse::Flowers && row % 2 == 0 {
                    blossom(mesh, ground, p, seed, false, detail);
                    herbs(mesh, ground, p, seed, LEAF, detail);
                } else if row % 3 == 1 && yard.seed % 2 == 0 {
                    herbs(mesh, ground, p, seed, color, detail);
                } else {
                    cabbage(mesh, ground, p, seed, color, detail);
                }
            }
        }
    }
}

fn blossom(mesh: &mut YardMesh, ground: &Ground, p: Vec2, seed: u64, golden: bool, detail: bool) {
    let height = if golden {
        0.86 + unit(seed) * 0.26
    } else {
        0.34 + unit(seed) * 0.18
    };
    let top = ground.at(p, height);
    mesh.leaf(
        ground.at(p, -0.025),
        top,
        if golden { 0.027 } else { 0.018 },
        LEAF * 0.85,
    );
    let golden_petals = golden || seed % 7 < 2;
    let color = if golden_petals {
        Vec3::new(1.0, 0.83, 0.18)
    } else {
        Vec3::new(1.0, 0.98, 0.87)
    };
    let radius = if golden {
        0.24
    } else {
        0.17 + unit(seed + 17) * 0.025
    };
    let petals = if detail { 5 } else { 4 };
    for i in 0..petals {
        let angle = i as f32 * std::f32::consts::TAU / petals as f32 + unit(seed) * 1.2;
        let d = Vec3::new(angle.cos(), 0.0, angle.sin());
        mesh.leaf(
            top + d * 0.025,
            top + d * radius + Vec3::Y * 0.018,
            radius * 0.42,
            color,
        );
    }
    mesh.crown(
        top + Vec3::Y * 0.025,
        Vec3::new(radius * 0.32, 0.035, radius * 0.32),
        if golden {
            Vec3::new(0.40, 0.24, 0.08)
        } else {
            Vec3::new(0.96, 0.70, 0.13)
        },
        0.0,
    );
    if detail || golden {
        for i in 0..2 {
            let d = Vec2::from_angle(unit(seed + i) * std::f32::consts::TAU);
            let base = ground.at(p, height * (0.32 + i as f32 * 0.20));
            mesh.leaf(
                base,
                base + Vec3::new(d.x * 0.18, 0.08, d.y * 0.18),
                0.065,
                LEAF * 1.04,
            );
        }
    }
}

pub(super) fn edges(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    let mut made = 0;
    for (edge, (a, b)) in yard.fence_segments().into_iter().enumerate() {
        let length = a.distance(b);
        if length < 1.2 {
            continue;
        }
        let tangent = (b - a).normalize();
        let inward = Vec2::new(-tangent.y, tangent.x);
        let clusters = if yard.use_kind == YardUse::Flowers {
            ((length / 1.8).round() as usize).clamp(1, 3)
        } else {
            1 + (yard.seed as usize + edge) % 2
        };
        for cluster in 0..clusters {
            if made == 6 {
                return;
            }
            let seed = yard.seed + edge as u64 * 79 + cluster as u64 * 131;
            let t = if clusters == 1 {
                0.27 + unit(seed) * 0.45
            } else {
                0.19 + cluster as f32 / (clusters - 1) as f32 * 0.62
            };
            let full_center = a.lerp(b, t) + inward * 0.47;
            let (center, compact) = if plant_fits(yard, full_center, 0.30) {
                (full_center, false)
            } else {
                // A 1.6 m fallback plot still has a useful narrow flower bed
                // beside its fence. Fit smaller crowns rather than consuming
                // the working strip or leaving only an empty fence outline.
                let p = a.lerp(b, t) + inward * 0.29;
                if !plant_fits(yard, p, 0.22) {
                    continue;
                }
                (p, true)
            };
            made += 1;
            // Broad, overlapping leaf groups give flowers a visible base at
            // town distance; each one beds into its own terrain sample.
            for i in 0..3 {
                let p = center + tangent * (i as f32 - 1.0) * 0.28;
                if !plant_fits(yard, p, if compact { 0.22 } else { 0.33 }) {
                    continue;
                }
                mesh.crown(
                    ground.at(p, 0.20),
                    if compact {
                        Vec3::new(0.21, 0.23, 0.20)
                    } else {
                        Vec3::new(0.32, 0.23, 0.30)
                    },
                    LEAF * (0.99 + unit(seed + i) * 0.18),
                    unit(seed + i) * std::f32::consts::TAU,
                );
                if detail {
                    let d = inward * if compact { 0.12 } else { 0.20 };
                    mesh.leaf(
                        ground.at(p, -0.01),
                        ground.at(p, 0.40) + Vec3::new(d.x, 0.0, d.y),
                        if compact { 0.07 } else { 0.10 },
                        LEAF * 1.15,
                    );
                }
            }
            let count = if detail { 9 } else { 6 };
            for i in 0..count {
                let s = seed + i * 23;
                let p = center
                    + tangent * ((unit(s) - 0.5) * 1.20)
                    + inward * ((unit(s + 7) - 0.5) * if compact { 0.06 } else { 0.38 });
                let golden = !compact && seed % 5 == 0 && i < 2;
                if plant_fits(yard, p, if golden { 0.27 } else { 0.22 }) {
                    blossom(mesh, ground, p, s, golden, detail);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planted_crowns_leave_a_continuous_work_strip_in_clipped_yards() {
        let yard = HouseholdYard {
            minimum: Vec2::new(4.5, -2.0),
            maximum: Vec2::new(8.5, 4.0),
            boundary: vec![
                Vec2::new(4.5, -2.0),
                Vec2::new(7.0, -2.0),
                Vec2::new(8.5, 4.0),
                Vec2::new(4.5, 4.0),
            ],
            side: YardSide::Right,
            use_kind: YardUse::Vegetables,
            seed: 291,
        };
        assert!(plant_fits(&yard, Vec2::new(7.2, 1.5), 0.34));
        assert!(
            !plant_fits(&yard, Vec2::new(5.7, 1.5), 0.34),
            "a plant radius must not intrude into the working strip"
        );
        assert!(
            !plant_fits(&yard, Vec2::new(7.5, -1.7), 0.34),
            "bounding rectangle alone cannot authorize planting beyond a clipped corner"
        );
        for side in [YardSide::Left, YardSide::Right, YardSide::Rear] {
            let simple = HouseholdYard {
                minimum: Vec2::ZERO,
                maximum: Vec2::new(4.0, 6.0),
                boundary: Vec::new(),
                side,
                ..yard.clone()
            };
            let p = match side {
                YardSide::Left => Vec2::new(2.9, 3.0),
                YardSide::Right => Vec2::new(1.1, 3.0),
                YardSide::Rear => Vec2::new(2.0, 1.1),
            };
            assert!(!plant_fits(&simple, p, 0.25));
        }
    }
}
