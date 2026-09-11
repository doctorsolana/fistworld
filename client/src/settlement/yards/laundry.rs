//! A stable household wash, hung on an existing solid fence span.
//!
//! Both mesh LODs retain garment silhouettes and the same load. Only folds,
//! rope subdivisions and pegs simplify; nothing animates or spawns per item.

use bevy::prelude::*;
use shared::components::{HouseholdYard, YARD_FENCE_HEIGHT};

use super::{dressing::WOOD, ground::Ground, mesh::YardMesh};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GarmentKind {
    Tunic,
    Trousers,
    Towel,
    Sheet,
}

#[derive(Clone, Debug, PartialEq)]
struct Garment {
    kind: GarmentKind,
    left: f32,
    width: f32,
    height: f32,
    color: Vec3,
    fold: f32,
}

#[derive(Debug, PartialEq)]
struct Wash {
    posts: [Vec2; 2],
    post_height: f32,
    sag: f32,
    garments: Vec<Garment>,
}

fn noise(seed: u64, salt: u64) -> f32 {
    (shared::worldgen::splitmix64(seed.wrapping_add(salt)) & 0xffff) as f32 / 65535.
}

impl Wash {
    fn for_yard(yard: &HouseholdYard) -> Option<Self> {
        // An unfenced border cannot authorize two new solid uprights. The
        // outer edge selects one intact shared fence span, never a gate chord.
        if yard.fence_segments().is_empty() {
            return None;
        }
        let (a, b) = yard.outer_edge();
        let edge_length = a.distance(b);
        if edge_length < 2.4 {
            return None;
        }
        let tangent = (b - a) / edge_length;
        let length = (edge_length * (0.70 + noise(yard.seed, 71) * 0.10))
            .min(4.1 + noise(yard.seed, 9) * 0.8);
        let margin = (edge_length - length) * 0.5;
        let shift = (noise(yard.seed, 13) - 0.5) * (margin - 0.12).max(0.0);
        let center = (a + b) * 0.5 + tangent * shift;
        let posts = [
            center - tangent * length * 0.5,
            center + tangent * length * 0.5,
        ];
        Some(Self {
            posts,
            post_height: 2.20 + noise(yard.seed, 41) * 0.18,
            sag: 0.10 + noise(yard.seed, 53) * 0.09,
            garments: household_load(yard.seed, length),
        })
    }
}

fn household_load(seed: u64, length: f32) -> Vec<Garment> {
    use GarmentKind::*;
    // Different tasks produce coherent loads, rather than recolouring one
    // repeating row of sheets. The available fence length limits the count.
    const LOADS: [[GarmentKind; 5]; 4] = [
        [Sheet, Towel, Tunic, Towel, Trousers],
        [Tunic, Trousers, Towel, Sheet, Towel],
        [Towel, Towel, Sheet, Tunic, Trousers],
        [Trousers, Tunic, Tunic, Towel, Sheet],
    ];
    const PALETTE: [Vec3; 6] = [
        Vec3::new(0.90, 0.87, 0.75), // Unbleached linen.
        Vec3::new(0.76, 0.72, 0.60),
        Vec3::new(0.49, 0.59, 0.62), // Faded woad.
        Vec3::new(0.65, 0.40, 0.32), // Soft madder.
        Vec3::new(0.69, 0.59, 0.36),
        Vec3::new(0.52, 0.56, 0.43),
    ];
    let hash = shared::worldgen::splitmix64(seed ^ 0x5741_5348);
    let load = &LOADS[(hash % LOADS.len() as u64) as usize];
    let count = 2 + ((hash >> 8) % 4) as usize;
    let available = length - 0.36;
    let mut used = 0.0;
    let mut garments = Vec::with_capacity(count);
    for (i, &kind) in load.iter().take(count).enumerate() {
        let salt = i as u64 * 31 + 101;
        let size = 0.90 + noise(seed, salt) * 0.20;
        let (width, height) = match kind {
            Tunic => (0.82, 0.82),
            Trousers => (0.57, 0.94),
            Towel => (0.42, 0.65 + noise(seed, salt + 1) * 0.16),
            Sheet => (1.10 + noise(seed, salt + 2) * 0.22, 0.94),
        };
        // A very short intact span can still carry a smaller linen sheet.
        let size = if i == 0 {
            size.min(available / width)
        } else {
            size
        };
        let width = (width * size).min(available);
        let gap = if i == 0 {
            0.0
        } else {
            0.12 + noise(seed, salt + 3) * 0.15
        };
        if used + gap + width > available {
            break;
        }
        used += gap;
        let choice = shared::worldgen::splitmix64(seed.wrapping_add(salt + 7));
        let color = match kind {
            Sheet => PALETTE[(choice % 2) as usize],
            Towel => PALETTE[(choice % 3) as usize],
            _ => PALETTE[(choice % PALETTE.len() as u64) as usize],
        };
        garments.push(Garment {
            kind,
            left: used,
            width,
            height: height * size,
            color,
            fold: noise(seed, salt + 11) * std::f32::consts::TAU,
        });
        used += width;
    }
    let start = 0.18 + (available - used).max(0.0) * (0.25 + noise(seed, 197) * 0.50);
    for garment in &mut garments {
        garment.left += start;
    }
    garments
}

// Non-overlapping sewn panels, in garment UVs: X is across the line and Y
// hangs down. The neck and the space between trouser legs are real openings.
fn panels(kind: GarmentKind) -> &'static [[[f32; 2]; 4]] {
    match kind {
        GarmentKind::Tunic => &[
            [[0.16, 0.], [0.43, 0.], [0.43, 0.14], [0., 0.14]],
            [[0.57, 0.], [0.84, 0.], [1., 0.14], [0.57, 0.14]],
            [[0., 0.14], [1., 0.14], [1., 0.33], [0., 0.33]],
            [[0.24, 0.33], [0.76, 0.33], [0.81, 1.], [0.19, 1.]],
        ],
        GarmentKind::Trousers => &[
            [[0., 0.], [1., 0.], [0.98, 0.25], [0.02, 0.25]],
            [[0.02, 0.25], [0.50, 0.25], [0.43, 1.], [0.10, 1.]],
            [[0.50, 0.25], [0.98, 0.25], [0.90, 1.], [0.57, 1.]],
        ],
        GarmentKind::Towel | GarmentKind::Sheet => &[[[0., 0.], [1., 0.], [1., 1.], [0., 1.]]],
    }
}

fn draw_garment(
    mesh: &mut YardMesh,
    garment: &Garment,
    rope: impl Fn(f32) -> Vec3,
    line_length: f32,
    normal: Vec3,
    detail: bool,
) {
    let cloth = |uv: Vec2| {
        let t = (garment.left + uv.x * garment.width) / line_length;
        let fold = (uv.x * std::f32::consts::TAU * 1.5 + garment.fold).sin();
        // Keep the full cloth displacement within the existing 0.13 m fence
        // thickness. Its hem stays well above the rails and pedestrian ground.
        rope(t) - Vec3::Y * garment.height * uv.y * (1.0 + fold * 0.018)
            + normal * (fold * 0.045 * uv.y)
    };
    for corners in panels(garment.kind) {
        let [a, b, c, d] = corners.map(Vec2::from_array);
        let strips = if detail { 4 } else { 2 };
        let rows = if detail { 2 } else { 1 };
        for x in 0..strips {
            for y in 0..rows {
                let uv = |x: usize, y: usize| {
                    let u = x as f32 / strips as f32;
                    a.lerp(b, u).lerp(d.lerp(c, u), y as f32 / rows as f32)
                };
                let points = [uv(x, y), uv(x + 1, y), uv(x + 1, y + 1), uv(x, y + 1)].map(cloth);
                for [a, b, c] in [
                    [points[0], points[1], points[2]],
                    [points[0], points[2], points[3]],
                ] {
                    mesh.triangle(a, b, c, garment.color);
                    mesh.triangle(c, b, a, garment.color * 0.94);
                }
            }
        }
    }
    if detail {
        let pegs = if garment.kind == GarmentKind::Tunic {
            [0.22, 0.78]
        } else {
            [0.06, 0.94]
        };
        for u in pegs {
            let at = rope((garment.left + garment.width * u) / line_length);
            mesh.beam(
                at - Vec3::Y * 0.055,
                at + Vec3::Y * 0.045,
                0.030,
                0.030,
                WOOD * 1.32,
            );
        }
    }
}

fn hanging_post_height(wash: &Wash) -> f32 {
    let longest = wash
        .garments
        .iter()
        .map(|g| g.height * 1.018)
        .fold(0.0, f32::max);
    // The irregular fence uprights reach 0.14 m above the nominal height;
    // retain another 0.06 m above them, including the deepest cloth fold/sag.
    wash.post_height
        .max(YARD_FENCE_HEIGHT + 0.20 + longest + wash.sag)
}

pub(super) fn build(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    let Some(wash) = Wash::for_yard(yard) else {
        return;
    };
    let [a, b] = wash.posts;
    // A shared level line is carried by two individually grounded posts, even
    // on sloping soil. Sample beneath the wash as well as at its supports.
    let post_height = hanging_post_height(&wash);
    let top = (0..=8)
        .map(|i| ground.at(a.lerp(b, i as f32 / 8.), post_height).y)
        .fold(f32::NEG_INFINITY, f32::max);
    for p in wash.posts {
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
        Vec3::new(p.x, top - wash.sag * (std::f32::consts::PI * t).sin(), p.y)
    };
    let segments = if detail { 12 } else { 6 };
    for i in 0..segments {
        mesh.beam(
            rope(i as f32 / segments as f32),
            rope((i + 1) as f32 / segments as f32),
            0.024,
            0.024,
            Vec3::new(0.67, 0.58, 0.40),
        );
    }
    let delta = b - a;
    let side = Vec3::new(delta.x, 0., delta.y).normalize();
    let normal = Vec3::Y.cross(side);
    for garment in &wash.garments {
        draw_garment(mesh, garment, rope, a.distance(b), normal, detail);
    }
}

#[cfg(test)]
mod tests;
