use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use bevy_mesh::VertexAttributeValues;

pub(crate) fn collect_scene_vertices(scene: &mut WorldAsset, meshes: &Assets<Mesh>) -> Vec<Vec3> {
    let mut out = Vec::new();

    let world = &mut scene.world;

    let mut query = world.query::<(Entity, &Mesh3d)>();
    for (entity, mesh3d) in query.iter(world) {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };

        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };

        let mat = world_matrix_for(entity, world);
        out.reserve(positions.len());
        for p in positions.iter() {
            let v = Vec3::new(p[0], p[1], p[2]);
            out.push(mat.transform_point3(v));
        }
    }

    out
}

pub(crate) fn collect_scene_mesh(
    scene: &mut WorldAsset,
    meshes: &Assets<Mesh>,
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    let world = &mut scene.world;

    let mut query = world.query::<(Entity, &Mesh3d)>();
    for (entity, mesh3d) in query.iter(world) {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };

        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            warn!("Skipping non-triangle mesh in scene (entity {:?})", entity);
            continue;
        }

        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };

        let base = vertices.len() as u32;
        let mat = world_matrix_for(entity, world);
        vertices.reserve(positions.len());
        for p in positions.iter() {
            let v = Vec3::new(p[0], p[1], p[2]);
            vertices.push(mat.transform_point3(v));
        }

        let local_indices: Vec<u32> = match mesh.indices() {
            Some(Indices::U32(idxs)) => idxs.clone(),
            Some(Indices::U16(idxs)) => idxs.iter().map(|i| *i as u32).collect(),
            None => (0..positions.len() as u32).collect(),
        };

        for chunk in local_indices.chunks(3) {
            if let [i0, i1, i2] = chunk {
                indices.push([base + *i0, base + *i1, base + *i2]);
            }
        }
    }

    (vertices, indices)
}

fn world_matrix_for(entity: Entity, world: &World) -> Mat4 {
    let mut mat = Mat4::IDENTITY;
    let mut current = entity;

    loop {
        if let Some(t) = world.get::<Transform>(current) {
            mat = t.to_matrix() * mat;
        }

        if let Some(parent) = world.get::<ChildOf>(current) {
            current = parent.parent();
        } else {
            break;
        }
    }

    mat
}
