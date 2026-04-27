use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;
use shared::props::PropKind;
use std::collections::HashMap;

use super::PropAssets;

#[derive(Resource, Default)]
pub struct SimplePropMeshCache {
    by_kind: HashMap<PropKind, SimplePropMeshState>,
}

#[derive(Clone)]
enum SimplePropMeshState {
    Ready(SimplePropMeshTemplate),
    Unsupported,
}

#[derive(Clone)]
struct SimplePropMeshTemplate {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
}

pub(crate) fn try_spawn_simple_prop_mesh(
    commands: &mut Commands,
    root: Entity,
    kind: PropKind,
    prop_assets: &PropAssets,
    cache: &mut SimplePropMeshCache,
    gltfs: Option<&Assets<Gltf>>,
    gltf_nodes: Option<&Assets<GltfNode>>,
    gltf_meshes: Option<&Assets<GltfMesh>>,
) -> bool {
    if let Some(state) = cache.by_kind.get(&kind).cloned() {
        return match state {
            SimplePropMeshState::Ready(template) => {
                spawn_simple_prop_mesh(commands, root, &template);
                true
            }
            SimplePropMeshState::Unsupported => false,
        };
    }

    let Some(template) =
        resolve_simple_prop_mesh(kind, prop_assets, gltfs, gltf_nodes, gltf_meshes)
    else {
        return false;
    };

    match template {
        Some(template) => {
            spawn_simple_prop_mesh(commands, root, &template);
            cache
                .by_kind
                .insert(kind, SimplePropMeshState::Ready(template));
            true
        }
        None => {
            cache.by_kind.insert(kind, SimplePropMeshState::Unsupported);
            false
        }
    }
}

fn resolve_simple_prop_mesh(
    kind: PropKind,
    prop_assets: &PropAssets,
    gltfs: Option<&Assets<Gltf>>,
    gltf_nodes: Option<&Assets<GltfNode>>,
    gltf_meshes: Option<&Assets<GltfMesh>>,
) -> Option<Option<SimplePropMeshTemplate>> {
    let gltfs = gltfs?;
    let gltf_nodes = gltf_nodes?;
    let gltf_meshes = gltf_meshes?;

    let gltf_handle = prop_assets.gltfs.get(&kind)?;
    let gltf = gltfs.get(gltf_handle)?;

    if gltf.nodes.len() != 1 {
        return Some(None);
    }

    let node = gltf_nodes.get(&gltf.nodes[0])?;
    if !node.children.is_empty() || node.skin.is_some() {
        return Some(None);
    }

    let Some(mesh_handle) = node.mesh.as_ref() else {
        return Some(None);
    };
    let mesh = gltf_meshes.get(mesh_handle)?;
    if mesh.primitives.len() != 1 {
        return Some(None);
    }

    let primitive = &mesh.primitives[0];
    let Some(material) = primitive.material.clone() else {
        return Some(None);
    };

    Some(Some(SimplePropMeshTemplate {
        mesh: primitive.mesh.clone(),
        material,
        transform: node.transform,
    }))
}

fn spawn_simple_prop_mesh(
    commands: &mut Commands,
    root: Entity,
    template: &SimplePropMeshTemplate,
) {
    if is_identity_transform(template.transform) {
        commands.entity(root).insert((
            Mesh3d(template.mesh.clone()),
            MeshMaterial3d(template.material.clone()),
        ));
        return;
    }

    commands.entity(root).with_children(|parent| {
        parent.spawn((
            Name::new("Mesh"),
            Mesh3d(template.mesh.clone()),
            MeshMaterial3d(template.material.clone()),
            template.transform,
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));
    });
}

fn is_identity_transform(transform: Transform) -> bool {
    transform.translation.length_squared() <= 1e-6
        && transform.scale.distance_squared(Vec3::ONE) <= 1e-6
        && (1.0 - transform.rotation.dot(Quat::IDENTITY).abs()) <= 1e-6
}
