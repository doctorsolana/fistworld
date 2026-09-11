//! Grounded low-poly household arrangements, in the accepted yard's local XZ.

use bevy::prelude::*;
use shared::components::{HouseholdYard, YardUse};
use shared::rotation::local_to_world_xz;
use shared::terrain::WorldTerrain;

use super::mesh::YardMesh;

pub(super) const WOOD: Vec3 = Vec3::new(0.43, 0.29, 0.145);
pub(super) const LEAF: Vec3 = Vec3::new(0.38, 0.53, 0.22);

pub(super) struct Ground<'a> {
    terrain: &'a WorldTerrain,
    origin: Vec3,
    yaw: f32,
}
impl Ground<'_> {
    pub(super) fn at(&self, p: Vec2, up: f32) -> Vec3 {
        let world = self.origin.xz() + local_to_world_xz(p, self.yaw);
        // Mesh transform owns only yaw and XZ translation, avoiding a second
        // terrain offset when the house root was graded before replication.
        Vec3::new(p.x, self.terrain.get_height(world.x, world.y) + up, p.y)
    }
}

fn clothesline(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    let (a, b) = yard.outer_edge();
    if a.distance(b) < 2.4 {
        return;
    }
    let a = a.lerp(b, 0.12);
    let b = a.lerp(b, 0.87);
    // Both uprights lie on the solid fence footprint, outside the working
    // strip. Rope, cloth and pegs all use this same supported hanging curve.
    let top = ground.at(a, 2.35).y.max(ground.at(b, 2.35).y);
    for p in [a, b] {
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
        Vec3::new(p.x, top - 0.16 * (std::f32::consts::PI * t).sin(), p.y)
    };
    let panels = if detail { 12 } else { 6 };
    for i in 0..panels {
        mesh.beam(
            rope(i as f32 / panels as f32),
            rope((i + 1) as f32 / panels as f32),
            0.024,
            0.024,
            Vec3::new(0.67, 0.58, 0.40),
        );
    }
    let side = (Vec3::new(b.x - a.x, 0., b.y - a.y)).normalize();
    let normal = Vec3::Y.cross(side);
    for (i, (left, right, length)) in [(0.08, 0.29, 0.67), (0.37, 0.63, 0.84), (0.74, 0.92, 0.58)]
        .into_iter()
        .enumerate()
    {
        let choice = shared::worldgen::splitmix64(yard.seed.wrapping_add(i as u64 * 41));
        let variation = (choice % 100) as f32 / 99.0;
        let color = if i == 2 && choice % 3 == 0 {
            // One faded garment among warm linen, rather than three identical
            // white rectangles on every household's line.
            Vec3::new(0.55, 0.66, 0.65)
        } else if i == 1 {
            Vec3::new(0.90, 0.86, 0.72)
        } else {
            Vec3::new(0.99, 0.97, 0.86)
        };
        let length = length * (0.94 + variation * 0.13);
        let strips = if detail { 5 } else { 2 };
        let rows = if detail { 2 } else { 1 };
        let cloth = |u: f32, v: f32| {
            let t = left + (right - left) * u;
            let fold = (u * std::f32::consts::TAU * 1.5 + variation * 1.6).sin();
            rope(t) - Vec3::Y * length * v * (0.97 + fold * 0.025)
                + normal * (fold * 0.075 * v + (v * std::f32::consts::PI).sin() * 0.035)
        };
        for j in 0..strips {
            let u0 = j as f32 / strips as f32;
            let u1 = (j + 1) as f32 / strips as f32;
            for row in 0..rows {
                let v0 = row as f32 / rows as f32;
                let v1 = (row + 1) as f32 / rows as f32;
                let q = [cloth(u0, v0), cloth(u0, v1), cloth(u1, v1), cloth(u1, v0)];
                mesh.quad(q, color);
                mesh.quad([q[3], q[2], q[1], q[0]], color * 0.93);
            }
        }
        if detail {
            for t in [left + 0.015, right - 0.015] {
                let at = rope(t);
                mesh.beam(
                    at - Vec3::Y * 0.065,
                    at + Vec3::Y * 0.045,
                    0.035,
                    0.035,
                    WOOD * 1.32,
                );
            }
        }
    }
}

fn firewood(mesh: &mut YardMesh, ground: &Ground, yard: &HouseholdYard, detail: bool) {
    let Some((center, tangent)) = yard.firewood_frame() else {
        return;
    };
    let inward = Vec2::new(-tangent.y, tangent.x);
    let corners = [
        center - tangent * 0.82 - inward * 0.30,
        center + tangent * 0.82 - inward * 0.30,
        center - tangent * 0.82 + inward * 0.30,
        center + tangent * 0.82 + inward * 0.30,
    ];
    let level = corners
        .into_iter()
        .map(|p| ground.at(p, 0.12).y)
        .fold(f32::NEG_INFINITY, f32::max);
    // Four grounded feet carry two level support rails. Logs rest on those
    // rails even when the soil is gently sloped; no floating bottom row.
    for p in corners {
        mesh.beam(
            ground.at(p, -0.08),
            Vec3::new(p.x, level, p.y),
            0.12,
            0.12,
            WOOD * 0.8,
        );
    }
    for t in [-0.82, 0.82] {
        let p = center + tangent * t;
        mesh.beam(
            Vec3::new(p.x - inward.x * 0.36, level, p.y - inward.y * 0.36),
            Vec3::new(p.x + inward.x * 0.36, level, p.y + inward.y * 0.36),
            0.13,
            0.12,
            WOOD,
        );
    }
    let rows = if detail { 3 } else { 2 };
    for row in 0..rows {
        let count = 5 - row;
        for i in 0..count {
            let p = center + tangent * ((i as f32 - (count - 1) as f32 * 0.5) * 0.32);
            let h = level + 0.20 + row as f32 * 0.277;
            mesh.log(
                Vec3::new(p.x - inward.x * 0.30, h, p.y - inward.y * 0.30),
                Vec3::new(p.x + inward.x * 0.30, h, p.y + inward.y * 0.30),
                0.16,
            );
        }
    }
}

pub(super) fn build(
    yard: &HouseholdYard,
    origin: Vec3,
    yaw: f32,
    terrain: &WorldTerrain,
    detail: bool,
) -> YardMesh {
    let ground = Ground {
        terrain,
        origin,
        yaw,
    };
    let mut mesh = YardMesh::default();
    super::fences::build(&mut mesh, &ground, yard, detail);
    match yard.use_kind {
        YardUse::Vegetables => super::planting::garden(&mut mesh, &ground, yard, detail, true),
        YardUse::Laundry => {
            clothesline(&mut mesh, &ground, yard, detail);
            super::planting::garden(&mut mesh, &ground, yard, detail, false);
        }
        YardUse::Firewood => firewood(&mut mesh, &ground, yard, detail),
        YardUse::Flowers => super::planting::garden(&mut mesh, &ground, yard, detail, true),
    }
    super::planting::edges(&mut mesh, &ground, yard, detail);
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use shared::components::YardSide;

    #[test]
    fn planted_geometry_keeps_narrow_and_clipped_yards_workable_in_both_lods() {
        let terrain = WorldTerrain::default();
        let ground = Ground {
            terrain: &terrain,
            origin: Vec3::ZERO,
            yaw: 0.0,
        };
        for side in [YardSide::Left, YardSide::Right, YardSide::Rear] {
            for narrow in [false, true] {
                let maximum = match side {
                    YardSide::Rear => Vec2::new(6.0, if narrow { 1.6 } else { 4.0 }),
                    _ => Vec2::new(if narrow { 1.6 } else { 4.0 }, 6.0),
                };
                let yard = HouseholdYard {
                    minimum: Vec2::ZERO,
                    maximum,
                    boundary: if narrow {
                        Vec::new()
                    } else {
                        vec![
                            Vec2::ZERO,
                            Vec2::new(maximum.x - 0.7, 0.0),
                            maximum,
                            Vec2::new(0.0, maximum.y),
                        ]
                    },
                    side,
                    use_kind: YardUse::Flowers,
                    seed: 291,
                };
                for detail in [false, true] {
                    let mut mesh = YardMesh::default();
                    super::super::planting::garden(&mut mesh, &ground, &yard, detail, true);
                    super::super::planting::edges(&mut mesh, &ground, &yard, detail);
                    assert!(!mesh.is_empty(), "narrow yards must retain planted edges");
                    let mesh = mesh.finish();
                    let Some(VertexAttributeValues::Float32x3(vertices)) =
                        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                    else {
                        panic!("missing yard positions");
                    };
                    for [x, _, z] in vertices {
                        let p = Vec2::new(*x, *z);
                        assert!(yard.contains_local_point(p, 0.001));
                        let clear = match side {
                            YardSide::Left => p.x <= yard.maximum.x - 0.999,
                            YardSide::Right => p.x >= yard.minimum.x + 0.999,
                            YardSide::Rear => p.y >= yard.minimum.y + 0.999,
                        };
                        assert!(clear, "{side:?} narrow={narrow} detail={detail}: {p:?}");
                    }
                }
            }
        }
    }
}
