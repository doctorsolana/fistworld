//! Deliberately composed household planting fitted to the accepted street parcel.
//!
//! A single deterministic plan drives both LODs: mixed planted drifts soften
//! several boundary runs, vegetable beds occupy selected remaining pockets,
//! and the actual entrance/house working strip stays empty. No plant entities.
mod geometry;
use super::{
    dressing::{LEAF, WOOD},
    ground::Ground,
    mesh::YardMesh,
};
use bevy::prelude::*;
use shared::components::{HouseholdYard, YardUse};

pub(super) fn unit(seed: u64) -> f32 {
    (shared::worldgen::splitmix64(seed) % 1000) as f32 / 999.0
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    p.distance(a + d * ((p - a).dot(d) / d.length_squared().max(0.00001)).clamp(0., 1.))
}

fn plant_fits(yard: &HouseholdYard, p: Vec2, radius: f32) -> bool {
    if !yard.planting_clear(p, radius) {
        return false;
    }
    // Firewood is an authoritative solid fixture. Its access apron is useful
    // outdoor working space, not another planting pocket.
    if let Some((center, tangent)) = yard.firewood_frame() {
        let inward = Vec2::new(-tangent.y, tangent.x);
        let delta = p - center;
        if delta.dot(tangent).abs() < 1.05 + radius
            && delta.dot(inward) > -0.42 - radius
            && delta.dot(inward) < 2.02 + radius
        {
            return false;
        }
    }
    true
}

/// Corner checks alone can bridge the rounded access capsule. Soil patches
/// must keep every edge outside it too, and may not enclose either endpoint.
fn soil_fits(yard: &HouseholdYard, polygon: &[Vec2]) -> bool {
    if !polygon.iter().all(|p| plant_fits(yard, *p, 0.)) {
        return false;
    }
    let Some((a, b)) = yard.entry_path() else {
        return true;
    };
    let inside = |p: Vec2| {
        (0..polygon.len())
            .all(|i| (polygon[(i + 1) % polygon.len()] - polygon[i]).perp_dot(p - polygon[i]) >= 0.)
    };
    if inside(a) || inside(b) {
        return false;
    }
    (0..polygon.len()).all(|i| {
        let c = polygon[i];
        let d = polygon[(i + 1) % polygon.len()];
        let cross = (b - a).perp_dot(d - c);
        if cross.abs() > 0.00001 {
            let t = (c - a).perp_dot(d - c) / cross;
            let u = (c - a).perp_dot(b - a) / cross;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                return false;
            }
        }
        segment_distance(a, c, d)
            .min(segment_distance(b, c, d))
            .min(segment_distance(c, a, b))
            .min(segment_distance(d, a, b))
            >= 0.75
    })
}

#[derive(Clone, Copy)]
struct Lobe {
    position: Vec2,
    radius: f32,
    height: f32,
    seed: u64,
}
struct Flower {
    position: Vec2,
    height: f32,
    golden: bool,
    seed: u64,
}
struct Drift {
    tangent: Vec2,
    lobes: Vec<Lobe>,
    flowers: Vec<Flower>,
}
enum BedCrop {
    Cabbage,
    Herbs,
}

struct Bed {
    points: Vec<Vec2>,
    tangent: Vec2,
    seed: u64,
    crop: BedCrop,
}

pub(super) struct PlantingPlan {
    drifts: Vec<Drift>,
    beds: Vec<Bed>,
}
impl PlantingPlan {
    pub(super) fn new(yard: &HouseholdYard) -> Self {
        let mut plan = Self {
            drifts: Vec::new(),
            beds: Vec::new(),
        };
        let boundary = yard.boundary_points();
        // Planting is land use, not a consequence of how many individual
        // fence pieces survive the gate/house cuts. A border can continue
        // along accepted unfenced land while access remains authoritatively clear.
        let mut edges: Vec<_> = boundary
            .iter()
            .copied()
            .zip(boundary.iter().copied().cycle().skip(1))
            .take(boundary.len())
            .collect();
        if let Some(entry) = yard.entry {
            edges.sort_by(|(a, b), (c, d)| {
                segment_distance(entry, *a, *b).total_cmp(&segment_distance(entry, *c, *d))
            });
        } else {
            let start = yard.seed as usize % edges.len().max(1);
            edges.rotate_left(start);
        }
        let max_lobes = match yard.use_kind {
            YardUse::Flowers => 30,
            YardUse::Vegetables => 18,
            YardUse::Laundry => 12,
            YardUse::Firewood => 10,
        };
        let mut lobes = 0;
        let mut dressed_edges = 0;
        for (edge, &(a, b)) in edges.iter().enumerate() {
            if lobes >= max_lobes {
                break;
            }
            let length = a.distance(b);
            if length < 0.65 {
                continue;
            }
            let tangent = (b - a) / length;
            let inward = Vec2::new(-tangent.y, tangent.x);
            let count = (length / 0.58).ceil().max(1.) as usize;
            let mut drift = Drift {
                tangent,
                lobes: Vec::new(),
                flowers: Vec::new(),
            };
            let before = lobes;
            for i in 0..count {
                if lobes >= max_lobes {
                    break;
                }
                let t = (i as f32 + 0.5) / count as f32;
                let along = length * t;
                let seed = yard.seed.wrapping_add(edge as u64 * 197 + i as u64 * 71);
                // Coherent lengths of border, with the remaining lawn left
                // useful. Kitchen plots keep a quiet bed area; work yards
                // receive a pair of corner borders instead of a floral ring.
                let length_limit = match yard.use_kind {
                    YardUse::Flowers => length,
                    YardUse::Vegetables => {
                        if dressed_edges == 0 {
                            3.5
                        } else {
                            2.8
                        }
                    }
                    YardUse::Laundry => {
                        if dressed_edges == 0 {
                            3.0
                        } else {
                            2.0
                        }
                    }
                    YardUse::Firewood => 2.6,
                };
                let far_end = yard.seed % 2 == 1 && dressed_edges > 0;
                let permitted = if far_end {
                    length - along <= length_limit
                } else {
                    along <= length_limit
                };
                if !permitted {
                    continue;
                }
                let edge_point = a.lerp(b, t);
                let mut fitted = None;
                // Fit each visible lobe once, adapting to a wedge's actual
                // depth. Rendering no longer rejects most of a nominal group.
                for radius in [0.52, 0.44, 0.36, 0.28, 0.20] {
                    let p = edge_point + inward * (radius + 0.10 + unit(seed) * 0.09);
                    if plant_fits(yard, p, radius + 0.012) {
                        fitted = Some((p, radius));
                        break;
                    }
                }
                let Some((p, radius)) = fitted else {
                    if !drift.lobes.is_empty() {
                        plan.drifts.push(drift);
                        drift = Drift {
                            tangent,
                            lobes: Vec::new(),
                            flowers: Vec::new(),
                        };
                    }
                    continue;
                };
                if plan
                    .drifts
                    .iter()
                    .flat_map(|d| &d.lobes)
                    .any(|l| p.distance(l.position) < (radius + l.radius) * 0.62)
                {
                    continue;
                }
                let height = if radius > 0.35 {
                    0.43 + unit(seed + 3) * 0.40
                } else {
                    0.23 + unit(seed + 3) * 0.20
                };
                drift.lobes.push(Lobe {
                    position: p,
                    radius,
                    height,
                    seed,
                });
                lobes += 1;
                let flowers = match yard.use_kind {
                    YardUse::Flowers => 3,
                    YardUse::Vegetables | YardUse::Laundry => 2,
                    YardUse::Firewood => usize::from(seed % 3 == 0),
                };
                for j in 0..flowers {
                    let flower_seed = seed + j as u64 * 23;
                    let golden = radius > 0.35 && j == 0 && seed % 7 == 0;
                    let flower_radius = if golden { 0.27 } else { 0.20 };
                    // Search a small band at the front of the leaf mass.
                    // Every accepted flower has its complete footprint clear.
                    let sideways = (j as f32 - (flowers - 1) as f32 * 0.5) * 0.25;
                    for depth in [radius * 0.63, radius * 0.30, 0.] {
                        let flower = p + tangent * sideways + inward * depth;
                        if plant_fits(yard, flower, flower_radius) {
                            drift.flowers.push(Flower {
                                position: flower,
                                height: height * 0.80 + 0.18,
                                golden,
                                seed: flower_seed,
                            });
                            break;
                        }
                    }
                }
                // Several complete medium-sized groups keep budget omissions
                // local and prevent a distant LOD dropping an entire long edge.
                if drift.lobes.len() >= 6 {
                    plan.drifts.push(drift);
                    drift = Drift {
                        tangent,
                        lobes: Vec::new(),
                        flowers: Vec::new(),
                    };
                }
            }
            if !drift.lobes.is_empty() {
                plan.drifts.push(drift);
            }
            if lobes > before {
                dressed_edges += 1;
            }
            if dressed_edges >= 2 && matches!(yard.use_kind, YardUse::Laundry | YardUse::Firewood) {
                break;
            }
        }
        // Beds derive axes from several actual parcel edges, rather than
        // extending one fixed house-aligned rectangle into clipped corners.
        let dimensions = yard.maximum - yard.minimum;
        let max_beds = if dimensions.min_element() < 2.6 {
            0 // narrow accepted strips are planted borders, not squeezed rows
        } else {
            match yard.use_kind {
                YardUse::Vegetables => {
                    if yard.area() > 38. {
                        8
                    } else {
                        5
                    }
                }
                YardUse::Flowers | YardUse::Firewood => 0,
                YardUse::Laundry => usize::from(yard.seed % 3 == 0),
            }
        };
        let mut plants = 0;
        let max_plants = if yard.use_kind == YardUse::Vegetables {
            58
        } else {
            32
        };
        for (edge, &(a, b)) in edges.iter().enumerate() {
            if plan.beds.len() >= max_beds || plants >= max_plants {
                break;
            }
            let length = a.distance(b);
            if length < 2.0 {
                continue;
            }
            let tangent = (b - a).normalize();
            let inward = Vec2::new(-tangent.y, tangent.x);
            let seed = yard.seed.wrapping_add(edge as u64 * 197 + 501);
            let row_count = if yard.use_kind == YardUse::Vegetables {
                if dimensions.min_element() > 5.0 {
                    4
                } else {
                    3
                }
            } else {
                1
            };
            for row in 0..row_count {
                let depth = 1.42 + row as f32 * 0.76;
                let count = ((length - 1.0) / 0.63).floor() as usize;
                let mut run = Vec::new();
                for i in 0..=count {
                    let t = 0.50 + i as f32 * 0.63;
                    let p = a + tangent * t + inward * depth;
                    // Every household leaves some open lawn/work space. Bed
                    // lengths differ, including the end nearest the street.
                    let end = length * (0.62 + unit(seed + row as u64) * 0.27);
                    let available = t < end
                        && plant_fits(yard, p, 0.34)
                        && !plan.drifts.iter().any(|d| {
                            d.lobes
                                .iter()
                                .any(|l| p.distance(l.position) < l.radius + 0.40)
                                || d.flowers.iter().any(|f| p.distance(f.position) < 0.55)
                        })
                        && !plan.beds.iter().any(|bed| {
                            bed.points
                                .iter()
                                .any(|q| p.distance_squared(*q) < 0.70 * 0.70)
                        });
                    if available && plants + run.len() < max_plants && run.len() < 7 {
                        run.push(p);
                    } else if !run.is_empty() {
                        Self::keep_bed(
                            &mut plan.beds,
                            &mut plants,
                            std::mem::take(&mut run),
                            tangent,
                            seed + row as u64 * 37,
                            yard.use_kind,
                        );
                    }
                    if plan.beds.len() >= max_beds || plants >= max_plants {
                        break;
                    }
                }
                if plan.beds.len() < max_beds {
                    Self::keep_bed(
                        &mut plan.beds,
                        &mut plants,
                        run,
                        tangent,
                        seed + row as u64 * 37,
                        yard.use_kind,
                    );
                }
                if plan.beds.len() >= max_beds || plants >= max_plants {
                    break;
                }
            }
        }
        plan
    }

    fn keep_bed(
        beds: &mut Vec<Bed>,
        plants: &mut usize,
        points: Vec<Vec2>,
        tangent: Vec2,
        seed: u64,
        use_kind: YardUse,
    ) {
        if points.len()
            < if use_kind == YardUse::Vegetables {
                3
            } else {
                2
            }
        {
            return;
        }
        *plants += points.len();
        let crop = if use_kind == YardUse::Laundry || seed % 2 != 0 {
            BedCrop::Herbs
        } else {
            BedCrop::Cabbage
        };
        beds.push(Bed {
            points,
            tangent,
            seed,
            crop,
        });
    }

    pub(super) fn draw(
        &self,
        mesh: &mut YardMesh,
        ground: &Ground,
        yard: &HouseholdYard,
        detail: bool,
        budget: usize,
    ) {
        // Border masses are the main street silhouette and get budget before
        // secondary vegetable rows. Complete groups are omitted when capped.
        for drift in &self.drifts {
            let mut group = YardMesh::default();
            drift.draw(&mut group, ground, yard, detail);
            mesh.append_with_budget(group, budget);
        }
        for bed in &self.beds {
            let mut group = YardMesh::default();
            bed.draw(&mut group, ground, yard, detail);
            mesh.append_with_budget(group, budget);
        }
    }
}

#[cfg(test)]
mod tests;
