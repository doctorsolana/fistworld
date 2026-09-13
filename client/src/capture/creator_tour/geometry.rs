//! Exact current-pose framing for the one offline preview rig. Dynamic skin
//! culling AABBs deliberately overestimate bounds after world/local rotations;
//! their corners are not rendered geometry and must not control UI framing.
use super::is_descendant;
use bevy::{
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    prelude::*,
};

pub(super) struct PreviewGeometry {
    pub bounds: Rect,
    pub scene_viewport: Rect,
    pub meshes: usize,
    pub vertices: usize,
}

fn skin_position(
    position: Vec3,
    indices: [u16; 4],
    weights: [f32; 4],
    joints: &[Mat4],
) -> Result<Vec3, String> {
    let mut point = Vec4::ZERO;
    for (joint, weight) in indices.into_iter().zip(weights) {
        if weight == 0.0 {
            continue;
        }
        let matrix = joints
            .get(joint as usize)
            .ok_or("preview vertex has an invalid skin joint")?;
        point += *matrix * position.extend(1.0) * weight;
    }
    if !point.is_finite() || point.w.abs() < 0.00001 {
        return Err("invalid skinned preview position".into());
    }
    Ok(point.truncate() / point.w)
}

pub(super) fn inspect(
    world: &mut World,
    rig: Entity,
    viewport: Rect,
) -> Result<PreviewGeometry, String> {
    let entities: Vec<_> = world
        .query::<(Entity, &Mesh3d, &InheritedVisibility)>()
        .iter(world)
        .filter(|(entity, _, visible)| visible.get() && is_descendant(world, *entity, rig))
        .map(|(entity, _, _)| entity)
        .collect();
    let (camera, view) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<crate::camera_rts::CommanderCamera>>()
        .iter(world)
        .next()
        .ok_or("preview main camera missing")?;
    let scene_viewport = camera
        .logical_viewport_rect()
        .ok_or("preview camera viewport missing")?;
    let scale = viewport.size() / scene_viewport.size();
    let assets = world.resource::<Assets<Mesh>>();
    let bindposes = world.resource::<Assets<SkinnedMeshInverseBindposes>>();
    let mut result = PreviewGeometry {
        bounds: Rect {
            min: Vec2::splat(f32::INFINITY),
            max: Vec2::splat(f32::NEG_INFINITY),
        },
        scene_viewport,
        meshes: entities.len(),
        vertices: 0,
    };
    for entity in entities {
        let handle = world.get::<Mesh3d>(entity).unwrap();
        let mesh = assets
            .get(handle)
            .ok_or("preview mesh CPU data is not loaded")?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("preview mesh has no Float32x3 positions".into());
        };
        result.vertices += positions.len();
        // Current full wardrobe is 14,448 vertices, with only selected parts
        // visible. A bounded offline observer must not silently become a whole
        // town CPU skinning pass after a fixture/asset regression.
        if result.vertices > 32_768 {
            return Err("preview capture exceeds its 32,768 vertex inspection budget".into());
        }
        let skin = world.get::<SkinnedMesh>(entity);
        let mut matrices = Vec::new();
        let influences = if let Some(skin) = skin {
            let inverse = bindposes
                .get(&skin.inverse_bindposes)
                .ok_or("preview inverse bind poses not loaded")?;
            for (index, joint) in skin.joints.iter().enumerate() {
                let pose = world
                    .get::<GlobalTransform>(*joint)
                    .ok_or("preview skin joint not propagated")?;
                matrices.push(
                    pose.to_matrix()
                        * *inverse.get(index).ok_or("preview skin bind pose missing")?,
                );
            }
            let Some(VertexAttributeValues::Uint16x4(indices)) =
                mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
            else {
                return Err("preview skin indices unavailable".into());
            };
            let Some(VertexAttributeValues::Float32x4(weights)) =
                mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
            else {
                return Err("preview skin weights unavailable".into());
            };
            if indices.len() != positions.len() || weights.len() != positions.len() {
                return Err("preview skin vertex attributes have mismatched lengths".into());
            }
            Some((indices, weights))
        } else {
            None
        };
        let pose = world
            .get::<GlobalTransform>(entity)
            .ok_or("preview mesh transform missing")?;
        for (index, position) in positions.iter().enumerate() {
            let position = Vec3::from_array(*position);
            // Exactly Bevy's skin_model contract: world joint * inverse bind,
            // weighted per vertex. The mesh root is not applied a second time.
            let point = if let Some((indices, weights)) = influences {
                skin_position(position, indices[index], weights[index], &matrices)?
            } else {
                pose.transform_point(position)
            };
            let projected = camera
                .world_to_viewport(view, point)
                .map_err(|_| "preview vertex projects outside camera depth")?;
            let pixel = viewport.min + (projected - scene_viewport.min) * scale;
            result.bounds.min = result.bounds.min.min(pixel);
            result.bounds.max = result.bounds.max.max(pixel);
        }
    }
    if result.vertices == 0 {
        return Err("preview has no visible mesh vertices".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posed_vertices_follow_weighted_world_joints_instead_of_culling_box_corners() {
        let root = Mat4::from_scale_rotation_translation(
            Vec3::splat(0.3),
            Quat::from_rotation_x(0.9) * Quat::from_rotation_y(2.7),
            Vec3::new(-9., 12., 7.),
        );
        let bind = Mat4::from_translation(Vec3::new(0., 1., 0.));
        let bend = Mat4::from_rotation_z(0.4);
        let matrices = [root, root * bind * bend * bind.inverse()];
        let p = Vec3::new(0.2, 1.4, 0.1);
        let actual = skin_position(p, [0, 1, 0, 0], [0.25, 0.75, 0., 0.], &matrices).unwrap();
        let expected = root
            .transform_point3(p * 0.25 + (bind * bend * bind.inverse()).transform_point3(p) * 0.75);
        assert!(actual.distance(expected) < 0.00001);
        assert!(
            actual.distance(root.transform_point3(expected)) > 1.0,
            "never apply the mesh root twice"
        );
        assert!(skin_position(p, [3, 0, 0, 0], [1., 0., 0., 0.], &matrices).is_err());
    }
}
