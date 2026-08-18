//! Render plumbing for true GPU-instanced grass chunks.
//!
//! Bevy normally creates one ECS entity per transform and then batches those
//! entities on the GPU. Ground cover has tens of thousands of transforms, so
//! that still leaves extraction, visibility and storage work on the CPU. This
//! module gives each coarse terrain-sector/kind pair one entity and one compact
//! instance buffer.

use bevy::core_pipeline::core_3d::AlphaMask3d;
use bevy::ecs::{query::QueryItem, system::SystemParamItem};
use bevy::material::{labels::DrawFunctionLabel, MaterialProperties};
use bevy::mesh::VertexBufferLayout;
use bevy::pbr::{
    ExtendedMaterial, MainPassAlphaMaskDrawFunction, MaterialExtension, MaterialExtensionKey,
    MaterialExtensionPipeline, PreparedMaterial, RenderMeshInstances, SetMaterialBindGroup,
    SetMeshBindGroup, SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
};
use bevy::prelude::*;
use bevy::render::{
    erased_render_asset::ErasedRenderAssets,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    mesh::{allocator::MeshAllocator, RenderMesh, RenderMeshBufferInfo},
    render_asset::RenderAssets,
    render_phase::{
        AddRenderCommand, DrawFunctions, PhaseItem, RenderCommand, RenderCommandResult,
        SetItemPipeline, TrackedRenderPass,
    },
    render_resource::{
        AsBindGroup, Buffer, BufferInitDescriptor, BufferUsages, RenderPipelineDescriptor,
        SpecializedMeshPipelineError, VertexAttribute, VertexFormat, VertexStepMode,
    },
    renderer::{RenderDevice, RenderQueue},
    sync_component::SyncComponent,
    sync_world::MainEntity,
    Render, RenderApp, RenderSystems,
};
use bevy::shader::ShaderRef;
use bytemuck::{Pod, Zeroable};
use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

pub type InstancedGrassMaterial = ExtendedMaterial<StandardMaterial, InstancedGrassExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct InstancedGrassExtension {
    /// Same wind and climate layout as the retained foliage shader.
    #[uniform(100)]
    pub params: Vec4,
    #[uniform(101)]
    pub extra: Vec4,
}

impl MaterialExtension for InstancedGrassExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/instanced_grass.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/instanced_grass.wgsl".into()
    }

    // Grass does not cast shadows in the retained path and SSAO/prepass is an
    // optional camera feature. Disabling both here avoids drawing an uninstanced
    // source tuft through the stock prepass/shadow pipelines.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers.push(VertexBufferLayout {
            array_stride: size_of::<GrassInstance>() as u64,
            step_mode: VertexStepMode::Instance,
            attributes: vec![
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 8,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: VertexFormat::Float32x4.size(),
                    shader_location: 9,
                },
            ],
        });
        Ok(())
    }
}

/// 32 bytes per tuft. `position_height.w` is the precomputed height variation;
/// `rotation_scale.xy` is yaw sin/cos and `.z` is uniform scale.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct GrassInstance {
    pub position_height: [f32; 4],
    pub rotation_scale: [f32; 4],
}

#[derive(Component, Clone)]
pub struct GrassInstances {
    // Extraction runs every frame. Sharing this immutable array keeps that
    // transfer O(1) instead of cloning roughly 700 KiB of tuft records from
    // the main world into the render world every frame.
    instances: Arc<[GrassInstance]>,
    revision: u64,
}

impl GrassInstances {
    pub fn new(instances: Vec<GrassInstance>) -> Self {
        static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);
        Self {
            instances: Arc::from(instances),
            revision: NEXT_REVISION.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn as_slice(&self) -> &[GrassInstance] {
        &self.instances
    }

    fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    fn len(&self) -> usize {
        self.instances.len()
    }
}

impl SyncComponent for GrassInstances {
    type Target = Self;
}

impl ExtractComponent for GrassInstances {
    type QueryData = &'static GrassInstances;
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(item.clone())
    }
}

#[derive(Debug)]
struct GrassInstanceBuffer {
    buffer: Buffer,
    capacity: usize,
    length: usize,
    revision: u64,
}

#[derive(Resource, Default)]
struct GrassInstanceBuffers(HashMap<MainEntity, GrassInstanceBuffer>);

pub struct GroundCoverInstancingPlugin;

impl Plugin for GroundCoverInstancingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::pbr::MaterialPlugin::<InstancedGrassMaterial>::default());
        app.add_plugins(ExtractComponentPlugin::<GrassInstances>::default());

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<GrassInstanceBuffers>()
            .add_render_command::<AlphaMask3d, DrawInstancedGrass>()
            .add_systems(
                Render,
                override_instanced_grass_draw_function
                    .in_set(RenderSystems::PrepareMeshes)
                    .after(RenderSystems::PrepareAssets),
            )
            .add_systems(
                Render,
                prepare_grass_instance_buffers.in_set(RenderSystems::PrepareResources),
            );
    }
}

/// MaterialPlugin owns specialization and phase queuing. We only replace the
/// alpha-mask draw command for this material type so its extra instance buffer
/// is bound.
///
/// `PreparedMaterial::properties` is shared with Bevy's specialized-pipeline
/// cache. Waiting for `Arc::get_mut` therefore made this setup depend on which
/// frame the material first became visible: an offline capture could win the
/// race, while entering a connected big world after the menu left the stock
/// `DrawMesh` command installed. That command uses an indirect draw and never
/// binds our required instance vertex buffer (slot 1), which is a wgpu
/// validation error. Copy-on-write makes the override deterministic even when
/// Bevy has already retained the original properties.
fn override_instanced_grass_draw_function(
    draw_functions: Res<DrawFunctions<AlphaMask3d>>,
    mut prepared: ResMut<ErasedRenderAssets<PreparedMaterial>>,
) {
    let custom = draw_functions.read().id::<DrawInstancedGrass>();
    let wanted_type = TypeId::of::<InstancedGrassMaterial>();
    for (asset_id, material) in prepared.iter_mut() {
        if asset_id.type_id() != wanted_type {
            continue;
        }
        install_alpha_mask_draw_function(&mut material.properties, custom);
    }
}

fn install_alpha_mask_draw_function(
    properties: &mut Arc<MaterialProperties>,
    custom: bevy::material::labels::DrawFunctionId,
) {
    if properties.get_draw_function(MainPassAlphaMaskDrawFunction) == Some(custom) {
        return;
    }

    if Arc::get_mut(properties).is_none() {
        *properties = Arc::new(clone_material_properties(properties));
    }
    let properties = Arc::get_mut(properties)
        .expect("fresh material-properties copy must have unique ownership");
    let alpha_mask_label = MainPassAlphaMaskDrawFunction.intern();
    if let Some((_, draw)) = properties
        .draw_functions
        .iter_mut()
        .find(|(label, _)| *label == alpha_mask_label)
    {
        *draw = custom;
    } else {
        properties.add_draw_function(MainPassAlphaMaskDrawFunction, custom);
    }
}

/// Bevy intentionally keeps `MaterialProperties` behind an `Arc` and does not
/// implement `Clone` for the aggregate, although all of its public fields are
/// cloneable. Keep this explicit so adding a Bevy field becomes a compile-time
/// prompt to decide how the instanced material should carry it forward.
fn clone_material_properties(properties: &MaterialProperties) -> MaterialProperties {
    MaterialProperties {
        render_method: properties.render_method,
        alpha_mode: properties.alpha_mode,
        mesh_pipeline_key_bits: properties.mesh_pipeline_key_bits,
        depth_bias: properties.depth_bias,
        reads_view_transmission_texture: properties.reads_view_transmission_texture,
        render_phase_type: properties.render_phase_type,
        material_layout: properties.material_layout.clone(),
        draw_functions: properties.draw_functions.clone(),
        shaders: properties.shaders.clone(),
        bindless: properties.bindless,
        base_specialize: properties.base_specialize,
        prepass_specialize: properties.prepass_specialize,
        user_specialize: properties.user_specialize,
        material_key: properties.material_key.clone(),
        shadows_enabled: properties.shadows_enabled,
        prepass_enabled: properties.prepass_enabled,
    }
}

fn prepare_grass_instance_buffers(
    instances: Query<(&MainEntity, &GrassInstances)>,
    mut buffers: ResMut<GrassInstanceBuffers>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let mut active = HashSet::with_capacity(instances.iter().len());
    for (main_entity, instances) in instances.iter() {
        active.insert(*main_entity);
        if instances.is_empty() {
            continue;
        }
        if buffers
            .0
            .get(main_entity)
            .is_some_and(|buffer| buffer.revision == instances.revision)
        {
            continue;
        }
        let bytes = bytemuck::cast_slice(instances.as_slice());
        if let Some(current) = buffers
            .0
            .get_mut(main_entity)
            .filter(|buffer| buffer.capacity >= instances.len())
        {
            render_queue.write_buffer(&current.buffer, 0, bytes);
            current.length = instances.len();
            current.revision = instances.revision;
            continue;
        }
        let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("chunked grass instances"),
            contents: bytes,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });
        buffers.0.insert(
            *main_entity,
            GrassInstanceBuffer {
                buffer,
                capacity: instances.len(),
                length: instances.len(),
                revision: instances.revision,
            },
        );
    }
    buffers
        .0
        .retain(|main_entity, _| active.contains(main_entity));
}

type DrawInstancedGrass = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    SetMeshBindGroup<2>,
    SetMaterialBindGroup<3>,
    DrawGrassInstances,
);

struct DrawGrassInstances;

impl<P: PhaseItem> RenderCommand<P> for DrawGrassInstances {
    type Param = (
        bevy::ecs::system::lifetimeless::SRes<RenderAssets<RenderMesh>>,
        bevy::ecs::system::lifetimeless::SRes<RenderMeshInstances>,
        bevy::ecs::system::lifetimeless::SRes<MeshAllocator>,
        bevy::ecs::system::lifetimeless::SRes<GrassInstanceBuffers>,
    );
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        _view: (),
        _item_query: Option<()>,
        (meshes, render_mesh_instances, mesh_allocator, instance_buffers): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(item.main_entity())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(gpu_mesh) = meshes.into_inner().get(mesh_instance.mesh_asset_id()) else {
            return RenderCommandResult::Skip;
        };
        let Some(instance_buffer) = instance_buffers.into_inner().0.get(&item.main_entity()) else {
            return RenderCommandResult::Skip;
        };
        let mesh_allocator = mesh_allocator.into_inner();
        let Some(vertex_slice) = mesh_allocator.mesh_vertex_slice(&mesh_instance.mesh_asset_id())
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_vertex_buffer(0, vertex_slice.buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.buffer.slice(..));

        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                index_format,
                count,
            } => {
                let Some(index_slice) =
                    mesh_allocator.mesh_index_slice(&mesh_instance.mesh_asset_id())
                else {
                    return RenderCommandResult::Skip;
                };
                pass.set_index_buffer(index_slice.buffer.slice(..), *index_format);
                pass.draw_indexed(
                    index_slice.range.start..(index_slice.range.start + count),
                    vertex_slice.range.start as i32,
                    0..instance_buffer.length as u32,
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_slice.range, 0..instance_buffer.length as u32);
            }
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::material::labels::DrawFunctionId;
    use bevy::pbr::MainPassOpaqueDrawFunction;

    #[test]
    fn instanced_draw_override_is_label_specific_and_copy_on_write() {
        // The phase-specific registries can assign the same numeric id to the
        // stock opaque and alpha-mask commands. Matching by id (the former
        // implementation) could therefore modify the wrong phase.
        let stock = DrawFunctionId(7);
        let custom = DrawFunctionId(11);
        let mut initial = MaterialProperties::default();
        initial.add_draw_function(MainPassOpaqueDrawFunction, stock);
        initial.add_draw_function(MainPassAlphaMaskDrawFunction, stock);
        let retained_by_bevy = Arc::new(initial);
        let mut prepared = retained_by_bevy.clone();

        install_alpha_mask_draw_function(&mut prepared, custom);

        assert!(!Arc::ptr_eq(&prepared, &retained_by_bevy));
        assert_eq!(
            prepared.get_draw_function(MainPassOpaqueDrawFunction),
            Some(stock)
        );
        assert_eq!(
            prepared.get_draw_function(MainPassAlphaMaskDrawFunction),
            Some(custom)
        );
        assert_eq!(
            retained_by_bevy.get_draw_function(MainPassAlphaMaskDrawFunction),
            Some(stock),
            "the copy retained by an already-specialized Bevy pipeline stays valid"
        );
    }
}
