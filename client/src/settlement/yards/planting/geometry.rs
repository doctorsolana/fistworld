//! Mesh details for already fitted border lobes, flowers and kitchen beds.
use super::*;

impl Bed {
    pub(super) fn draw(
        &self,
        mesh: &mut YardMesh,
        ground: &Ground,
        yard: &HouseholdYard,
        detail: bool,
    ) {
        let inward = Vec2::new(-self.tangent.y, self.tangent.x);
        let a = self.points[0] - self.tangent * 0.27;
        let b = *self.points.last().unwrap() + self.tangent * 0.27;
        // Unequal clipped corners soften the bed outline without a raised
        // wooden tray. Its surface follows terrain and stays below foliage.
        let corners = [
            a - inward * 0.21,
            b - inward * 0.26,
            b + inward * 0.22,
            a + inward * 0.28,
        ];
        if soil_fits(yard, &corners) {
            ground.patch(
                mesh,
                &corners,
                Vec3::new(0.38, 0.285, 0.155) * (0.95 + unit(self.seed) * 0.10),
            );
        }
        for (i, &p) in self.points.iter().enumerate() {
            let seed = self.seed.wrapping_add(i as u64 * 19);
            match self.crop {
                BedCrop::Cabbage => cabbage(mesh, ground, p, seed, detail),
                BedCrop::Herbs => herbs(mesh, ground, p, seed, LEAF * 1.02, detail),
            }
        }

        if detail && self.seed % 3 == 0 {
            // Small edging offcuts are low planting detail, not new blockers.
            let p = self.points[0] - self.tangent * 0.23;
            if plant_fits(yard, p, 0.30) {
                mesh.beam(
                    ground.at(p - inward * 0.25, 0.06),
                    ground.at(p + inward * 0.25, 0.06),
                    0.07,
                    0.07,
                    WOOD * 1.10,
                );
            }
        }
    }
}

impl Drift {
    pub(super) fn draw(
        &self,
        mesh: &mut YardMesh,
        ground: &Ground,
        yard: &HouseholdYard,
        detail: bool,
    ) {
        let inward = Vec2::new(-self.tangent.y, self.tangent.x);
        for pair in self.lobes.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            if a.position.distance(b.position) > 1.0 {
                continue;
            }
            let corners = [
                a.position - inward * a.radius * 0.65,
                b.position - inward * b.radius * 0.65,
                b.position + inward * b.radius * 0.70,
                a.position + inward * a.radius * 0.70,
            ];
            if soil_fits(yard, &corners) {
                ground.patch(mesh, &corners, Vec3::new(0.40, 0.32, 0.185));
            }
        }
        for lobe in &self.lobes {
            lobe.draw(mesh, ground, detail);
        }
        for flower in &self.flowers {
            blossom(
                mesh,
                ground,
                flower.position,
                flower.seed,
                flower.golden,
                flower.yellow,
                detail,
                flower.height,
            );
        }
    }
}

fn cabbage(mesh: &mut YardMesh, ground: &Ground, p: Vec2, seed: u64, detail: bool) {
    let color = if seed % 7 == 0 {
        Vec3::new(0.47, 0.35, 0.43)
    } else {
        LEAF * (1.02 + unit(seed.wrapping_add(3)) * 0.16)
    };
    mesh.leafy_head(
        ground.at(p, 0.14),
        Vec3::new(0.265, 0.14, 0.25),
        color,
        unit(seed) * 6.28,
    );
    if detail {
        for i in 0..2 {
            let d = Vec2::from_angle(unit(seed) * 6.28 + i as f32 * 2.4);
            mesh.leaf(
                ground.at(p + d * 0.02, 0.06),
                ground.at(p + d * 0.27, 0.16),
                0.10,
                color * 0.94,
            );
        }
    }
}

fn herbs(mesh: &mut YardMesh, ground: &Ground, p: Vec2, seed: u64, color: Vec3, detail: bool) {
    for i in 0..if detail { 5 } else { 3 } {
        let d = Vec2::from_angle(i as f32 * 2.4 + unit(seed));
        mesh.leaf(
            ground.at(p, -0.01),
            ground.at(p + d * 0.21, 0.33 + unit(seed.wrapping_add(i + 7)) * 0.14),
            if detail { 0.075 } else { 0.12 },
            color * (0.95 + unit(seed.wrapping_add(i)) * 0.13),
        );
    }
}

fn blossom(
    mesh: &mut YardMesh,
    ground: &Ground,
    p: Vec2,
    seed: u64,
    golden: bool,
    yellow: bool,
    detail: bool,
    minimum_height: f32,
) {
    let height = if golden {
        0.95 + unit(seed) * 0.28
    } else {
        (0.39 + unit(seed) * 0.23).max(minimum_height)
    };
    let top = ground.at(p, height);
    mesh.leaf(
        ground.at(p, -0.025),
        top,
        if golden { 0.027 } else { 0.015 },
        LEAF * 0.82,
    );
    let color = if golden || yellow {
        Vec3::new(1.0, 0.83, 0.18)
    } else {
        Vec3::new(1.0, 0.98, 0.88)
    };
    let radius = if golden {
        0.24
    } else {
        0.165 + unit(seed.wrapping_add(17)) * 0.017
    };
    // Near flowers have five broad petals; far groups keep the same height,
    // footprint and light colour so the planted border does not vanish.
    let petals = if detail { 5 } else { 3 };
    for i in 0..petals {
        let angle = i as f32 * std::f32::consts::TAU / petals as f32 + unit(seed) * 1.2;
        let d = Vec3::new(angle.cos(), 0., angle.sin());
        mesh.leaf(
            top + d * 0.022,
            top + d * radius + Vec3::Y * 0.018,
            radius * 0.38,
            color,
        );
    }
    mesh.crown(
        top + Vec3::Y * 0.02,
        Vec3::new(radius * 0.29, 0.028, radius * 0.29),
        if golden {
            Vec3::new(0.40, 0.24, 0.08)
        } else {
            Vec3::new(0.96, 0.70, 0.13)
        },
        0.,
    );
    if golden {
        for i in 0..2 {
            let d = Vec2::from_angle(unit(seed.wrapping_add(i)) * 6.28);
            let base = ground.at(p, height * (0.32 + i as f32 * 0.2));
            mesh.leaf(
                base,
                base + Vec3::new(d.x * 0.18, 0.08, d.y * 0.18),
                0.06,
                LEAF * 1.04,
            );
        }
    }
}
