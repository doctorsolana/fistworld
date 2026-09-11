//! Immutable source preparation. Meshes and texture pixels are copied only when
//! the canonical GLB revision changes; outfits clone small Arc-backed draw lists.

use std::sync::Arc;

use bevy::{
    ecs::system::SystemParam,
    gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode},
    mesh::VertexAttributeValues,
    platform::collections::{HashMap, HashSet},
    prelude::*,
};
use image::RgbaImage;
use shared::{character::CharacterManifest, components::HeroOutfit};

use super::raster::{Geometry, Part, Vertex};
use crate::hero::HeroManifest;

#[derive(SystemParam)]
pub(super) struct SourceAssets<'w> {
    pub manifest: Res<'w, HeroManifest>,
    pub server: Res<'w, AssetServer>,
    gltfs: Res<'w, Assets<Gltf>>,
    nodes: Res<'w, Assets<GltfNode>>,
    gltf_meshes: Res<'w, Assets<GltfMesh>>,
    meshes: Res<'w, Assets<Mesh>>,
    materials: Res<'w, Assets<GltfMaterial>>,
}

struct SourcePart {
    node_name: String,
    skin: bool,
    draw: Part,
}

pub(super) struct PreparedSource {
    manifest: CharacterManifest,
    parts: Vec<SourcePart>,
    pub background: Arc<RgbaImage>,
    node_ids: HashSet<AssetId<GltfNode>>,
    gltf_mesh_ids: HashSet<AssetId<GltfMesh>>,
    mesh_ids: HashSet<AssetId<Mesh>>,
    material_ids: HashSet<AssetId<GltfMaterial>>,
    texture_ids: HashSet<AssetId<Image>>,
}

impl PreparedSource {
    pub fn dress(&self, outfit: HeroOutfit) -> Vec<Part> {
        self.parts
            .iter()
            .filter(|part| !outfit.hides_node(&self.manifest, &part.node_name))
            .map(|part| {
                let mut draw = part.draw.clone();
                if part.skin {
                    let [r, g, b] = self.manifest.skin_color(outfit.skin);
                    draw.color = Vec4::new(r, g, b, 1.0);
                }
                draw
            })
            .collect()
    }
}

pub(super) fn prepare(
    handle: &Handle<Gltf>,
    background: &Handle<Image>,
    source: &SourceAssets,
    images: &Assets<Image>,
) -> Option<Arc<PreparedSource>> {
    if !source.server.is_loaded_with_dependencies(handle.id()) {
        return None;
    }
    let background = Arc::new(
        images
            .get(background)?
            .clone()
            .try_into_dynamic()
            .ok()?
            .to_rgba8(),
    );
    let gltf = source.gltfs.get(handle)?;
    let skin = gltf
        .named_materials
        .get(source.manifest.skin.material.as_str());
    let mut parts = Vec::new();
    let mut gltf_mesh_ids = HashSet::default();
    let mut mesh_ids = HashSet::default();
    let mut material_ids = HashSet::default();
    let mut textures: HashMap<AssetId<Image>, Arc<RgbaImage>> = HashMap::default();
    // The authored hierarchy is prepared once, rather than rescanned for every outfit.
    let mut parents: HashMap<AssetId<GltfNode>, &Handle<GltfNode>> = HashMap::default();
    for handle in &gltf.nodes {
        let node = source.nodes.get(handle)?;
        for child in &node.children {
            parents.insert(child.id(), handle);
        }
    }
    for handle in &gltf.nodes {
        let node = source.nodes.get(handle)?;
        let Some(mesh_handle) = &node.mesh else {
            continue;
        };
        let mut transform = node.transform.to_matrix();
        let mut child = handle;
        for _ in 0..gltf.nodes.len() {
            let Some(parent) = parents.get(&child.id()) else {
                break;
            };
            transform = source.nodes.get(*parent)?.transform.to_matrix() * transform;
            child = parent;
        }
        let normal_transform = transform.inverse().transpose();
        gltf_mesh_ids.insert(mesh_handle.id());
        let mesh = source.gltf_meshes.get(mesh_handle)?;
        for primitive in &mesh.primitives {
            mesh_ids.insert(primitive.mesh.id());
            if let Some(material) = &primitive.material {
                material_ids.insert(material.id());
            }
            let mesh = source.meshes.get(&primitive.mesh)?;
            let VertexAttributeValues::Float32x3(positions) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
            else {
                return None;
            };
            let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
                Some(VertexAttributeValues::Float32x3(v)) => Some(v),
                _ => None,
            };
            let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
                Some(VertexAttributeValues::Float32x4(v)) => Some(v),
                _ => None,
            };
            let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                Some(VertexAttributeValues::Float32x2(v)) => Some(v),
                _ => None,
            };
            let vertices = positions
                .iter()
                .enumerate()
                .map(|(i, position)| Vertex {
                    position: transform.transform_point3(Vec3::from(*position)),
                    normal: normals.map_or(Vec3::ZERO, |v| {
                        normal_transform
                            .transform_vector3(Vec3::from(v[i]))
                            .normalize_or_zero()
                    }),
                    color: colors.map_or(Vec4::ONE, |v| Vec4::from(v[i])),
                    uv: uvs.map_or(Vec2::ZERO, |v| Vec2::from(v[i])),
                })
                .collect();
            let indices = mesh.indices().map_or_else(
                || (0..positions.len()).collect(),
                |indices| indices.iter().collect(),
            );
            let material = primitive
                .material
                .as_ref()
                .and_then(|h| source.materials.get(h));
            let color = material.map_or(Vec4::ONE, |m| {
                Vec4::from_array(m.base_color.to_linear().to_f32_array())
            });
            let texture = match material.and_then(|m| m.base_color_texture.as_ref()) {
                Some(handle) => {
                    if let Some(texture) = textures.get(&handle.id()) {
                        Some(texture.clone())
                    } else {
                        let texture = Arc::new(
                            images
                                .get(handle)?
                                .clone()
                                .try_into_dynamic()
                                .ok()?
                                .to_rgba8(),
                        );
                        textures.insert(handle.id(), texture.clone());
                        Some(texture)
                    }
                }
                None => None,
            };
            parts.push(SourcePart {
                node_name: node.name.clone(),
                skin: skin.is_some() && primitive.material.as_ref() == skin,
                draw: Part {
                    geometry: Arc::new(Geometry { vertices, indices }),
                    color,
                    texture,
                },
            });
        }
    }
    (!parts.is_empty()).then_some(Arc::new(PreparedSource {
        manifest: source.manifest.0.clone(),
        parts,
        background,
        node_ids: gltf.nodes.iter().map(Handle::id).collect(),
        gltf_mesh_ids,
        mesh_ids,
        material_ids,
        texture_ids: textures.keys().copied().collect(),
    }))
}

/// Watch only assets used by the prepared source. Unrelated image uploads (such
/// as newly completed portraits) never invalidate the source or fan out work.
#[derive(SystemParam)]
pub(super) struct SourceChanges<'w, 's> {
    gltfs: MessageReader<'w, 's, AssetEvent<Gltf>>,
    nodes: MessageReader<'w, 's, AssetEvent<GltfNode>>,
    gltf_meshes: MessageReader<'w, 's, AssetEvent<GltfMesh>>,
    meshes: MessageReader<'w, 's, AssetEvent<Mesh>>,
    materials: MessageReader<'w, 's, AssetEvent<GltfMaterial>>,
    images: MessageReader<'w, 's, AssetEvent<Image>>,
}

impl SourceChanges<'_, '_> {
    pub fn affects(
        &mut self,
        prepared: Option<&PreparedSource>,
        gltf: Option<&Handle<Gltf>>,
        background: Option<&Handle<Image>>,
    ) -> bool {
        // Consume every message even after the first match, so a multi-asset
        // reload creates one revision and cannot invalidate on following frames.
        let mut changed = modified(&mut self.gltfs, |id| gltf.is_some_and(|h| h.id() == id));
        changed |= modified(&mut self.nodes, |id| {
            prepared.is_some_and(|p| p.node_ids.contains(&id))
        });
        changed |= modified(&mut self.gltf_meshes, |id| {
            prepared.is_some_and(|p| p.gltf_mesh_ids.contains(&id))
        });
        changed |= modified(&mut self.meshes, |id| {
            prepared.is_some_and(|p| p.mesh_ids.contains(&id))
        });
        changed |= modified(&mut self.materials, |id| {
            prepared.is_some_and(|p| p.material_ids.contains(&id))
        });
        changed |= modified(&mut self.images, |id| {
            background.is_some_and(|h| h.id() == id)
                || prepared.is_some_and(|p| p.texture_ids.contains(&id))
        });
        changed
    }
}

fn modified<A: Asset>(
    reader: &mut MessageReader<AssetEvent<A>>,
    matches: impl Fn(AssetId<A>) -> bool,
) -> bool {
    reader.read().fold(false, |changed, event| {
        changed
            | match event {
                AssetEvent::Modified { id } => matches(*id),
                _ => false,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wardrobe_dressing_reuses_source_pixels_and_geometry_and_obeys_coverage() {
        let manifest: CharacterManifest =
            ron::from_str(include_str!("../../../assets/characters/Humanoid.ron")).unwrap();
        let geometry = Arc::new(Geometry {
            vertices: vec![],
            indices: vec![],
        });
        let texture = Arc::new(RgbaImage::new(1, 1));
        let draw = Part {
            geometry: geometry.clone(),
            color: Vec4::ONE,
            texture: Some(texture.clone()),
        };
        let source = PreparedSource {
            parts: vec![
                SourcePart {
                    node_name: manifest.body.clone(),
                    skin: true,
                    draw: draw.clone(),
                },
                SourcePart {
                    node_name: "Hair_Tousled".into(),
                    skin: false,
                    draw: draw.clone(),
                },
                SourcePart {
                    node_name: "Headgear_NasalHelmet".into(),
                    skin: false,
                    draw,
                },
            ],
            manifest,
            background: texture.clone(),
            node_ids: default(),
            gltf_mesh_ids: default(),
            mesh_ids: default(),
            material_ids: default(),
            texture_ids: default(),
        };
        let base = HeroOutfit::from_manifest(&source.manifest);
        let portrait = source.dress(base);
        assert_eq!(portrait.len(), 2); // Body and observed hair, no helmet.
        assert!(Arc::ptr_eq(&portrait[0].geometry, &geometry));
        assert!(Arc::ptr_eq(portrait[0].texture.as_ref().unwrap(), &texture));
        let mut helmet = base;
        helmet.slots[3] = 1;
        helmet.skin = 5;
        let protected = source.dress(helmet);
        assert_eq!(protected.len(), 2); // Helmet hides the hair instead of intersecting it.
        assert_ne!(protected[0].color, portrait[0].color);
        assert!(Arc::ptr_eq(&protected[0].geometry, &portrait[0].geometry));
    }
}
