//! Bind once after scene loading, and rebind after upgrades or asset hot reload.
use super::{assets::Catalog, BuildingLod};
use bevy::{
    prelude::*,
    world_serialization::{WorldInstance, WorldInstanceReady},
};

pub(super) fn scene_ready(event: On<WorldInstanceReady>, mut roots: Query<&mut BuildingLod>) {
    if let Ok(mut lod) = roots.get_mut(event.entity) {
        // Bevy respawns child entities on source hot reload, even when its root handle is unchanged.
        lod.ready = false;
        lod.failed = false;
        lod.bindings.clear();
        lod.needs_refresh = true;
    }
}

pub(super) fn register(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut catalog: ResMut<Catalog>,
    mut roots: Query<
        (
            Entity,
            &WorldAssetRoot,
            Option<&BuildingLod>,
            &mut Visibility,
        ),
        Changed<WorldAssetRoot>,
    >,
) {
    for (entity, scene, previous, mut visibility) in &mut roots {
        if let Some(original) = previous.and_then(|lod| lod.visibility_before_hide) {
            *visibility = original;
        }
        if let Some(library) = catalog.get(&scene.0, &assets) {
            commands.entity(entity).insert(BuildingLod {
                library,
                bindings: Vec::new(),
                level: 0,
                ready: false,
                failed: false,
                needs_refresh: true,
                visibility_before_hide: None,
            });
        } else {
            // An art/preview replacement may cease to be a building.
            commands.entity(entity).remove::<BuildingLod>();
        }
    }
}

pub(super) fn bind(
    assets: Res<AssetServer>,
    meshes: Res<Assets<Mesh>>,
    spawner: Res<WorldInstanceSpawner>,
    mut roots: Query<(
        Entity,
        &WorldAssetRoot,
        Option<&WorldInstance>,
        &mut BuildingLod,
    )>,
    children: Query<&Children>,
    primitives: Query<&Mesh3d>,
    mut stack: Local<Vec<Entity>>,
) {
    for (root, scene, instance, mut lod) in &mut roots {
        if lod.ready || lod.failed {
            continue;
        }
        use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState};
        if matches!(
            assets.get_load_states(lod.library.gltf.id()),
            Some((LoadState::Failed(_), _, _))
                | Some((_, DependencyLoadState::Failed(_), _))
                | Some((_, _, RecursiveDependencyLoadState::Failed(_)))
        ) {
            warn!(
                "Building LOD library failed for {:?}; retaining authored detail",
                scene.0
            );
            lod.failed = true;
            continue;
        }
        if !assets.is_loaded_with_dependencies(scene.id())
            || !assets.is_loaded_with_dependencies(lod.library.gltf.id())
            || !instance.is_some_and(|i| spawner.instance_is_ready(**i))
            || !lod
                .library
                .meshes
                .iter()
                .flatten()
                .all(|mesh| meshes.contains(mesh))
        {
            continue;
        }
        stack.clear();
        stack.push(root);
        lod.bindings.clear();
        while let Some(entity) = stack.pop() {
            if let Ok(mesh) = primitives.get(entity) {
                if let Some(index) = lod
                    .library
                    .meshes
                    .iter()
                    .position(|levels| levels.contains(&mesh.0))
                {
                    lod.bindings.push((entity, index));
                }
            }
            if let Ok(children) = children.get(entity) {
                stack.extend(children.iter());
            }
        }
        // A replacement WorldAssetRoot can still have its previous instance this frame.
        lod.ready = !lod.bindings.is_empty()
            && (0..lod.library.meshes.len()).all(|i| lod.bindings.iter().any(|(_, p)| *p == i));
        if lod.ready {
            // A refreshed scene may mix surviving LOD meshes with new source meshes.
            // Force the next selection pass to put every bound primitive on one level.
            lod.needs_refresh = true;
        }
    }
}
