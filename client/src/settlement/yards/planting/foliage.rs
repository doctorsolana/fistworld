//! Small opaque foliage silhouettes; every part stays inside its fitted disk.
use super::*;

impl Lobe {
    pub(super) fn draw(&self, mesh: &mut YardMesh, ground: &Ground, detail: bool) {
        let Self {
            position: p,
            radius,
            height,
            seed,
            kind,
            color,
        } = *self;
        let yaw = unit(seed) * std::f32::consts::TAU;
        match kind {
            PlantKind::Shrub => {
                // A rooted woody centre with unequal side growth, rather than
                // a row of identically sized single balls. Both LODs retain all
                // three masses so a camera zoom cannot change the arrangement.
                mesh.beam(
                    ground.at(p, -0.025),
                    ground.at(p, height * 0.68),
                    0.045,
                    0.045,
                    WOOD * 0.82,
                );
                for (i, offset) in [
                    Vec2::ZERO,
                    Vec2::from_angle(yaw) * radius * 0.42,
                    Vec2::from_angle(yaw + 2.65) * radius * 0.38,
                ]
                .into_iter()
                .enumerate()
                {
                    let scale = if i == 0 { 0.70 } else { 0.47 };
                    let h = height * [1.0, 0.73, 0.86][i];
                    let centre = ground.at(p + offset, h * 0.51);
                    let shape = Vec3::new(radius * scale, h * 0.54, radius * scale * 0.93);
                    let tone = color * [1.0, 1.09, 0.96][i];
                    if detail {
                        mesh.leafy_head(centre, shape, tone, yaw + i as f32);
                    } else {
                        mesh.crown(centre, shape, tone, yaw + i as f32);
                    }
                }
            }
            PlantKind::Perennial | PlantKind::Herbs => {
                let herb = kind == PlantKind::Herbs;
                let leaves = if detail { 7 } else { 5 };
                for i in 0..leaves {
                    let d = Vec2::from_angle(yaw + i as f32 * 2.4);
                    let reach = radius * (0.57 + unit(seed.wrapping_add(i + 11)) * 0.25);
                    mesh.leaf(
                        ground.at(p, -0.025),
                        ground.at(
                            p + d * reach,
                            height * (0.66 + unit(seed.wrapping_add(i + 17)) * 0.40),
                        ),
                        radius * if herb { 0.15 } else { 0.22 },
                        color * (0.94 + i as f32 * 0.017),
                    );
                }
                if herb && seed % 3 == 0 {
                    // Occasional grey-green flowering sage. A few muted spikes
                    // punctuate low herbs without covering every bush in blooms.
                    for i in 0..3 {
                        let q = p + Vec2::from_angle(yaw + i as f32 * 2.1) * radius * 0.35;
                        let h = height + 0.15 + unit(seed.wrapping_add(i + 53)) * 0.14;
                        mesh.leaf(ground.at(q, 0.02), ground.at(q, h), 0.012, color * 0.85);
                        mesh.crown(
                            ground.at(q, h),
                            Vec3::new(0.045, 0.11, 0.045),
                            Vec3::new(0.64, 0.56, 0.68),
                            yaw,
                        );
                    }
                }
            }
        }
    }
}
