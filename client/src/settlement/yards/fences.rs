//! Uneven, joined timber construction along the authoritative parcel spans.
use super::{
    dressing::{Ground, WOOD},
    mesh::YardMesh,
};
use bevy::prelude::*;
use shared::components::{HouseholdYard, YARD_FENCE_HEIGHT};

fn variation(seed: u64) -> f32 {
    (shared::worldgen::splitmix64(seed) % 1000) as f32 / 999.0
}

pub(super) fn build(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    let mut made_posts: Vec<Vec2> = Vec::new();
    let rail_count = if yard.seed % 4 == 0 { 3 } else { 2 };
    for (segment, (a, b)) in yard.fence_segments().into_iter().enumerate() {
        let length = a.distance(b);
        if length < 0.15 {
            continue;
        }
        let count = (length / (1.35 + variation(yard.seed) * 0.4))
            .ceil()
            .max(1.0) as usize;
        let tangent = (b - a).normalize();
        let points: Vec<_> = (0..=count)
            .map(|i| {
                let t = if i == 0 {
                    0.0
                } else if i == count {
                    1.0
                } else {
                    (i as f32
                        + (variation(yard.seed + segment as u64 * 37 + i as u64) - 0.5) * 0.24)
                        / count as f32
                };
                a.lerp(b, t)
            })
            .collect();
        let post_height = |p: Vec2| {
            let seed = yard.seed
                ^ ((p.x * 100.).round() as i32 as u64).rotate_left(17)
                ^ (p.y * 100.).round() as i32 as u64;
            YARD_FENCE_HEIGHT - 0.08 + variation(seed) * 0.22
        };
        for &p in &points {
            if made_posts.iter().any(|q| q.distance_squared(p) < 0.0001) {
                continue;
            }
            made_posts.push(p);
            let height = post_height(p);
            // Subtle lean stays within the real fence width. The lower end
            // is buried; rails below use the same supported post positions.
            let tip = p + tangent * ((variation(yard.seed + p.x.to_bits() as u64) - 0.5) * 0.035);
            mesh.beam(
                ground.at(p, -0.12),
                ground.at(tip, height),
                0.13,
                0.13,
                WOOD * (0.86 + height * 0.12),
            );
        }
        for (i, pair) in points.windows(2).enumerate() {
            let seed = yard.seed.wrapping_add((segment * 37 + i) as u64);
            let tone = 0.86 + variation(seed) * 0.30;
            let levels: &[f32] = if rail_count == 3 {
                &[0.24, 0.46, 0.70]
            } else {
                &[0.30, 0.69]
            };
            for &level in levels {
                // Each rough rail seats into its two posts; differing post
                // heights give natural slopes without unsupported loose ends.
                mesh.beam(
                    ground.at(pair[0], level * post_height(pair[0]) / YARD_FENCE_HEIGHT),
                    ground.at(pair[1], level * post_height(pair[1]) / YARD_FENCE_HEIGHT),
                    0.075 + variation(seed + 9) * 0.025,
                    0.11,
                    WOOD * tone,
                );
            }
            if detail && seed % 7 == 0 {
                mesh.beam(
                    ground.at(pair[0], 0.30 * post_height(pair[0]) / YARD_FENCE_HEIGHT),
                    ground.at(pair[1], 0.69 * post_height(pair[1]) / YARD_FENCE_HEIGHT),
                    0.055,
                    0.07,
                    WOOD * tone * 0.94,
                );
            }
        }
    }
}
