//! One closed, vertex-coloured mesh per regional timber bridge. No texture or
//! per-plank entities; rebuilds happen only when a replicated worksite changes.

use bevy::prelude::*;
use shared::components::RoadBridge;
use shared::terrain::WorldTerrain;

use super::structure_mesh::StructureMesh;
use crate::states::GameState;

pub(super) struct BridgePlugin;

impl Plugin for BridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach.run_if(in_state(GameState::Playing)));
    }
}

#[derive(Component)]
pub(crate) struct BridgeVisual(pub(crate) RoadBridge);

fn attach(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
    bridges: Query<(Entity, &RoadBridge, Option<&BridgeVisual>)>,
) {
    let Some(terrain) = terrain else { return };
    for (entity, bridge, _) in bridges
        .iter()
        .filter(|(_, bridge, old)| bridge.valid() && old.is_none_or(|old| old.0 != **bridge))
        .take(2)
    {
        let material = material.get_or_insert_with(|| {
            materials.add(StandardMaterial {
                perceptual_roughness: 0.95,
                ..default()
            })
        });
        let geometry = geometry(bridge, |p| terrain.get_height(p.x, p.y));
        commands.entity(entity).insert((
            BridgeVisual(bridge.clone()),
            Name::new(if bridge.built {
                "Timber road bridge"
            } else {
                "Surveyed bridge banks"
            }),
            Mesh3d(meshes.add(geometry)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(bridge.midpoint()),
            Visibility::Inherited,
        ));
    }
}

fn geometry(bridge: &RoadBridge, ground: impl Fn(Vec2) -> f32) -> Mesh {
    let mut mesh = StructureMesh::default();
    let origin = bridge.midpoint();
    let along = (bridge.end.xz() - bridge.start.xz()).normalize();
    let side = Vec3::new(-along.y, 0.0, along.x);
    let axis = Vec3::new(along.x, 0.0, along.y);
    let at = |d: f32| {
        let p = bridge.start.xz() + along * d;
        Vec3::new(p.x, bridge.surface_height(d), p.y) - origin
    };
    let timber = Vec3::new(0.38, 0.24, 0.12);
    let rail = Vec3::new(0.48, 0.31, 0.16);
    let length = bridge.length();
    if !bridge.built {
        for d in [0.0, length] {
            for sign in [-1.0, 1.0] {
                let p = at(d) + side * sign * bridge.width * 0.5;
                mesh.beam(p - Vec3::Y * 0.15, p + Vec3::Y * 0.8, 0.15, 0.15, timber);
            }
        }
        return mesh.finish();
    }
    // Closed deck boards overlap slightly; seams are colour variation, never
    // see-through slits. Their top follows the authoritative ramp profile.
    let boards = (length / 0.5).ceil() as usize;
    let step = length / boards as f32;
    for i in 0..boards {
        let d = (i as f32 + 0.5) * step;
        let p = at(d) - Vec3::Y * 0.13;
        let c = Vec3::new(0.59, 0.42, 0.23) * (0.94 + ((i * 37) % 11) as f32 * 0.012);
        // Each board runs across the deck. Very short plates keep the ramp
        // surface within 12cm of the continuous walking plane.
        mesh.beam(
            p - side * bridge.width * 0.5,
            p + side * bridge.width * 0.5,
            step + 0.025,
            0.26,
            c,
        );
    }
    // Break rails and bearers exactly at ramp joints, so posts and braces
    // meet their supported beams even on differently elevated riverbanks.
    let mut joints = vec![0.0, bridge.ramp_length, length - bridge.ramp_length, length];
    let supports = (length / 4.0).ceil() as usize;
    for i in 1..supports {
        joints.push(length * i as f32 / supports as f32);
    }
    joints.sort_by(f32::total_cmp);
    joints.dedup_by(|a, b| (*a - *b).abs() < 0.15);
    for sign in [-1.0, 1.0] {
        let offset = side * sign * (bridge.width * 0.5 - 0.06);
        for &d in &joints {
            let p = at(d) + offset;
            let absolute = p + origin;
            // Only bank supports enter terrain. The water channel stays clear
            // for ordinary boats, while the deep side trusses carry the span.
            let on_bank = d <= bridge.ramp_length || d >= length - bridge.ramp_length;
            let base_y = if on_bank {
                ground(absolute.xz()) - origin.y - 0.3
            } else {
                p.y - 1.0
            };
            let base = Vec3::new(p.x, base_y, p.z);
            mesh.beam(base, p + Vec3::Y * 1.05, 0.20, 0.20, timber);
            // Stone shoes seat on the riverbed/ground and wrap timber bases.
            if on_bank {
                mesh.beam(
                    base + Vec3::Y * 0.10,
                    base + Vec3::Y * 0.50,
                    0.42,
                    0.42,
                    Vec3::new(0.43, 0.43, 0.35),
                );
            }
        }
        for pair in joints.windows(2) {
            let a = at(pair[0]) + offset;
            let b = at(pair[1]) + offset;
            for height in [0.5, 0.98] {
                mesh.beam(a + Vec3::Y * height, b + Vec3::Y * height, 0.12, 0.14, rail);
            }
            mesh.beam(a - Vec3::Y * 0.28, b - Vec3::Y * 0.28, 0.25, 0.3, timber);
            if pair[0] >= bridge.ramp_length && pair[1] <= length - bridge.ramp_length {
                mesh.beam(a - Vec3::Y * 0.92, b - Vec3::Y * 0.92, 0.18, 0.18, timber);
                mesh.beam(a - Vec3::Y * 0.92, b - Vec3::Y * 0.28, 0.14, 0.14, timber);
            }
            if pair[1] - pair[0] > 1.0 {
                mesh.beam(
                    a + Vec3::Y * 0.16 + axis * 0.07,
                    b + Vec3::Y * 0.91 - axis * 0.07,
                    0.09,
                    0.1,
                    rail * 0.9,
                );
            }
        }
    }
    mesh.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn river_bridge_has_closed_bounded_geometry_and_grounded_supports() {
        let bridge = RoadBridge {
            start: Vec3::new(-20.0, 0.0, 0.0),
            end: Vec3::new(20.0, 0.0, 0.0),
            deck_height: 3.0,
            ramp_length: 8.0,
            width: 3.6,
            built: true,
        };
        let mesh = geometry(&bridge, |_| -2.0);
        let Some(VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        assert!(points.len() < 8_000);
        assert!(points.iter().all(|p| p.iter().all(|v| v.is_finite())));
        assert!(points.iter().any(|p| p[1] < -2.25));
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals")
        };
        assert!(
            normals.iter().any(|n| n[1] < -0.99),
            "deck has real undersides"
        );
        // Away from dry-bank ramps, no structural member may invade the
        // clearance used by the server's water-crossing survey.
        assert!(points.iter().filter(|p| p[0].abs() < 10.0).all(|p| p[1]
            >= bridge.deck_height
                - bridge.midpoint().y
                - shared::components::ROAD_BRIDGE_UNDERDECK_DEPTH));
    }
}
