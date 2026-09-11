//! Selected-character thumbnails from the canonical GLB, generated only on demand.
//!
//! A small software raster job uses the authored bind-pose geometry, wardrobe
//! coverage, skin palette, vertex colours and textures. The result is an ordinary
//! UI image: there is no second PBR view, animation rig or streaming dependency.

mod raster;

use std::collections::VecDeque;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use shared::components::{CharacterKind, HeroOutfit};

use crate::hero::HeroManifest;
use crate::selection::Selection;
use crate::states::GameState;

const CACHE_LIMIT: usize = 16;
const PORTRAIT_SIZE: u32 = 192;

/// Attach to the image inside the HUD's circular portrait frame. A transparent
/// image means unsupported selection/loading; a white tint means ready to draw.
#[derive(Component)]
pub(super) struct PortraitImage;

/// Semantic readiness for capture fixtures; no frame-count wait is necessary.
#[derive(Resource, Default)]
pub(crate) struct PortraitReadiness(pub bool);

pub(super) fn install(app: &mut App) {
    app.init_resource::<PortraitCache>()
        .init_resource::<PortraitReadiness>()
        .add_systems(
            Update,
            sync_portrait
                .after(crate::selection::SelectionGestureSet)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(OnExit(GameState::Playing), clear_portraits);
}

#[derive(Resource, Default)]
struct PortraitCache {
    source: Option<Handle<Gltf>>,
    desired: Option<HeroOutfit>,
    entries: VecDeque<(HeroOutfit, Handle<Image>)>,
    /// One worker at most, even when selection changes during rasterization.
    pending: Option<(HeroOutfit, Task<Vec<u8>>)>,
}

impl PortraitCache {
    fn finish(&mut self, outfit: HeroOutfit, pixels: Vec<u8>, images: &mut Assets<Image>) {
        if Some(outfit) != self.desired {
            return;
        }
        let handle = images.add(Image::new(
            Extent3d {
                width: PORTRAIT_SIZE,
                height: PORTRAIT_SIZE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        ));
        self.entries.push_back((outfit, handle));
        while self.entries.len() > CACHE_LIMIT {
            if let Some((_, image)) = self.entries.pop_front() {
                images.remove(image.id());
            }
        }
    }

    fn release(&mut self, images: &mut Assets<Image>) {
        for (_, image) in self.entries.drain(..) {
            images.remove(image.id());
        }
        *self = Self::default();
    }
}

#[derive(SystemParam)]
struct PortraitSource<'w> {
    manifest: Res<'w, HeroManifest>,
    server: Res<'w, AssetServer>,
    gltfs: Res<'w, Assets<Gltf>>,
    nodes: Res<'w, Assets<GltfNode>>,
    gltf_meshes: Res<'w, Assets<GltfMesh>>,
    meshes: Res<'w, Assets<Mesh>>,
    materials: Res<'w, Assets<GltfMaterial>>,
}

fn sync_portrait(
    selection: Res<Selection>,
    people: Query<&HeroOutfit, With<CharacterKind>>,
    source: PortraitSource,
    mut cache: ResMut<PortraitCache>,
    mut images: ResMut<Assets<Image>>,
    mut widgets: Query<&mut ImageNode, With<PortraitImage>>,
    mut ready: ResMut<PortraitReadiness>,
) {
    let desired = selection
        .primary()
        .filter(|_| selection.len() == 1)
        .and_then(|entity| people.get(entity).ok())
        .copied();
    if cache.desired != desired {
        cache.desired = desired;
        // Keep the existing worker until completion, then discard its result
        // when stale. Dropping a running synchronous raster job cannot interrupt
        // it halfway through a poll and would allow two workers to overlap.
    }

    let completed = cache
        .pending
        .as_mut()
        .and_then(|(outfit, task)| block_on(poll_once(task)).map(|pixels| (*outfit, pixels)));
    if let Some((outfit, pixels)) = completed {
        cache.pending = None;
        cache.finish(outfit, pixels, &mut images);
    }

    let cached = desired.and_then(|outfit| {
        cache
            .entries
            .iter()
            .find(|(key, _)| *key == outfit)
            .map(|(_, image)| image.clone())
    });
    if ready.0 != cached.is_some() {
        ready.0 = cached.is_some();
    }
    for mut widget in &mut widgets {
        let tint = if cached.is_some() {
            Color::WHITE
        } else {
            Color::NONE
        };
        if widget.color != tint {
            widget.color = tint;
        }
        if let Some(image) = &cached {
            if widget.image != *image {
                widget.image = image.clone();
            }
        }
    }
    if cached.is_some() || cache.pending.is_some() || widgets.is_empty() {
        return;
    }
    let Some(outfit) = desired else { return };
    let gltf_handle = cache.source.get_or_insert_with(|| {
        source.server.load(
            source
                .manifest
                .scene
                .split('#')
                .next()
                .unwrap_or(&source.manifest.scene)
                .to_owned(),
        )
    });
    if !source.server.is_loaded_with_dependencies(gltf_handle.id()) {
        return;
    }
    let Some(gltf) = source.gltfs.get(gltf_handle) else {
        return;
    };
    let Some(parts) = collect_parts(gltf, &source, &images, outfit) else {
        return;
    };
    cache.pending = Some((
        outfit,
        AsyncComputeTaskPool::get().spawn(async move { raster::render(parts, PORTRAIT_SIZE) }),
    ));
}

/// All source handles remain shared with the normal character renderer. Copy
/// only the selected wardrobe's geometry, once per uncached appearance.
fn collect_parts(
    gltf: &Gltf,
    source: &PortraitSource,
    images: &Assets<Image>,
    outfit: HeroOutfit,
) -> Option<Vec<raster::Part>> {
    let mut result = Vec::new();
    let skin = gltf
        .named_materials
        .get(source.manifest.skin.material.as_str());
    for handle in &gltf.nodes {
        let node = source.nodes.get(handle)?;
        let Some(mesh_handle) = &node.mesh else {
            continue;
        };
        if outfit.hides_node(&source.manifest, &node.name) {
            continue;
        }
        let mut transform = node.transform.to_matrix();
        let mut child = handle;
        // Authored node transforms are static in a neutral portrait. Meshes are
        // already in their bind pose; no simulation/animation state is sampled.
        for _ in 0..gltf.nodes.len() {
            let parent = gltf.nodes.iter().find_map(|candidate| {
                let parent = source.nodes.get(candidate)?;
                parent
                    .children
                    .contains(child)
                    .then_some((candidate, parent))
            });
            let Some((parent_handle, parent)) = parent else {
                break;
            };
            transform = parent.transform.to_matrix() * transform;
            child = parent_handle;
        }
        let mesh = source.gltf_meshes.get(mesh_handle)?;
        for primitive in &mesh.primitives {
            let mesh = source.meshes.get(&primitive.mesh)?;
            let VertexAttributeValues::Float32x3(positions) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
            else {
                return None;
            };
            let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
                Some(VertexAttributeValues::Float32x3(values)) => Some(values),
                _ => None,
            };
            let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
                Some(VertexAttributeValues::Float32x4(values)) => Some(values),
                _ => None,
            };
            let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                Some(VertexAttributeValues::Float32x2(values)) => Some(values),
                _ => None,
            };
            let normal_transform = transform.inverse().transpose();
            let vertices = positions
                .iter()
                .enumerate()
                .map(|(i, position)| raster::Vertex {
                    position: transform.transform_point3(Vec3::from(*position)),
                    normal: normals.map_or(Vec3::ZERO, |values| {
                        normal_transform
                            .transform_vector3(Vec3::from(values[i]))
                            .normalize_or_zero()
                    }),
                    color: colors.map_or(Vec4::ONE, |values| Vec4::from(values[i])),
                    uv: uvs.map_or(Vec2::ZERO, |values| Vec2::from(values[i])),
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
            let color = if skin.is_some() && primitive.material.as_ref() == skin {
                let rgb = source.manifest.skin_color(outfit.skin);
                Vec4::new(rgb[0], rgb[1], rgb[2], 1.0)
            } else {
                material.map_or(Vec4::ONE, |material| {
                    Vec4::from_array(material.base_color.to_linear().to_f32_array())
                })
            };
            let texture = match material.and_then(|material| material.base_color_texture.as_ref()) {
                Some(handle) => Some(images.get(handle)?.clone()),
                None => None,
            };
            result.push(raster::Part {
                vertices,
                indices,
                color,
                texture,
            });
        }
    }
    (!result.is_empty()).then_some(result)
}

fn clear_portraits(
    mut cache: ResMut<PortraitCache>,
    mut images: ResMut<Assets<Image>>,
    mut ready: ResMut<PortraitReadiness>,
) {
    cache.release(&mut images);
    ready.0 = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels() -> Vec<u8> {
        vec![0; (PORTRAIT_SIZE * PORTRAIT_SIZE * 4) as usize]
    }

    #[test]
    fn stale_worker_output_never_allocates_or_replaces_a_portrait() {
        let mut images = Assets::<Image>::default();
        let mut cache = PortraitCache::default();
        cache.desired = Some(HeroOutfit {
            skin: 1,
            ..default()
        });
        cache.finish(HeroOutfit::default(), pixels(), &mut images);
        assert!(cache.entries.is_empty());
        assert_eq!(images.len(), 0);
    }

    #[test]
    fn thumbnail_cache_evicts_images_and_releases_every_owned_asset() {
        let mut images = Assets::<Image>::default();
        let unrelated = images.add(Image::default());
        let mut cache = PortraitCache::default();
        for skin in 0..(CACHE_LIMIT + 4) as u8 {
            let outfit = HeroOutfit { skin, ..default() };
            cache.desired = Some(outfit);
            cache.finish(outfit, pixels(), &mut images);
        }
        assert_eq!(cache.entries.len(), CACHE_LIMIT);
        assert_eq!(images.len(), CACHE_LIMIT + 1);
        assert_eq!(cache.entries.front().unwrap().0.skin, 4);
        cache.release(&mut images);
        assert!(cache.entries.is_empty());
        assert_eq!(images.len(), 1);
        assert!(images.contains(unrelated.id()));
    }
}
