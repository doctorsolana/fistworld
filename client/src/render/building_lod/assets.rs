//! Shared, lazily loaded mesh libraries. The authored scene remains the owner of TRS/materials.
use bevy::{gltf::Gltf, prelude::*};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Deserialize)]
pub(super) struct Definition {
    pub source: String,
    pub library: String,
    pub center: [f32; 3],
    pub radius: f32,
    pub primitives: Vec<PrimitiveDefinition>,
    pub triangles: [usize; super::LOD_COUNT],
}

#[derive(Clone, Deserialize)]
pub(super) struct PrimitiveDefinition {
    source: String,
    levels: [String; super::LOD_COUNT - 1],
}

pub(super) fn definitions() -> Vec<Definition> {
    serde_json::from_str(include_str!(
        "../../../assets/game_assets/buildings/lod/manifest.json"
    ))
    .expect("generated building LOD catalog")
}

pub(super) struct Library {
    pub gltf: Handle<Gltf>,
    pub meshes: Vec<[Handle<Mesh>; super::LOD_COUNT]>,
    pub center: Vec3,
    pub radius: f32,
    pub triangles: [usize; super::LOD_COUNT],
}

#[derive(Resource)]
pub(super) struct Catalog {
    definitions: HashMap<String, Definition>,
    loaded: HashMap<String, Arc<Library>>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            definitions: definitions()
                .into_iter()
                .map(|d| (d.source.clone(), d))
                .collect(),
            loaded: HashMap::new(),
        }
    }
}

impl Catalog {
    pub fn get(
        &mut self,
        scene: &Handle<WorldAsset>,
        assets: &AssetServer,
    ) -> Option<Arc<Library>> {
        let path = scene.path()?.path().to_str()?;
        if let Some(library) = self.loaded.get(path) {
            return Some(library.clone());
        }
        let definition = self.definitions.get(path)?;
        let library = Arc::new(Library {
            gltf: assets.load(definition.library.clone()),
            meshes: definition
                .primitives
                .iter()
                .map(|p| {
                    [
                        assets.load(format!("{}#{}", definition.source, p.source)),
                        assets.load(format!("{}#{}", definition.library, p.levels[0])),
                    ]
                })
                .collect(),
            center: Vec3::from_array(definition.center),
            radius: definition.radius,
            triangles: definition.triangles,
        });
        self.loaded.insert(path.to_owned(), library.clone());
        Some(library)
    }
}
