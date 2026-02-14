use bevy::asset::{embedded_asset, load_embedded_asset, AssetServer, Handle};
use bevy::camera::Camera;
use bevy::core_pipeline::{
    core_3d::graph::{Core3d, Node3d},
    FullscreenShader,
};
use bevy::ecs::{
    component::Component,
    entity::Entity,
    query::{QueryItem, With},
    resource::Resource,
    system::{Commands, Query, Res, ResMut},
    world::World,
};
use bevy::image::BevyDefault as _;
use bevy::prelude::*;
use bevy::render::{
    extract_component::{
        ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
        UniformComponentPlugin,
    },
    render_graph::{
        NodeRunError, RenderGraphContext, RenderGraphExt as _, RenderLabel, ViewNode,
        ViewNodeRunner,
    },
    render_resource::{
        binding_types::{sampler, texture_2d, uniform_buffer_sized},
        BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
        CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations,
        PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
        Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType,
        SpecializedRenderPipeline, SpecializedRenderPipelines, TextureFormat, TextureSampleType,
    },
    renderer::{RenderContext, RenderDevice},
    view::{ExtractedView, ViewTarget},
    Render, RenderApp, RenderStartup, RenderSystems,
};
use bevy::shader::Shader;

/// Runtime settings for the sniper fisheye post-process.
#[derive(Component, Reflect, Clone)]
#[reflect(Component, Default, Clone)]
pub struct SniperFisheye {
    /// Barrel distortion intensity. Zero disables the effect.
    pub strength: f32,
    /// Normalized radius where distortion starts (0..1).
    pub edge_start: f32,
}

impl Default for SniperFisheye {
    fn default() -> Self {
        Self {
            strength: 0.0,
            edge_start: 0.54,
        }
    }
}

impl ExtractComponent for SniperFisheye {
    type QueryData = &'static Self;
    type QueryFilter = With<Camera>;
    type Out = SniperFisheyeUniform;

    fn extract_component(item: QueryItem<Self::QueryData>) -> Option<Self::Out> {
        Some(SniperFisheyeUniform {
            strength: item.strength.max(0.0),
            edge_start: item.edge_start.clamp(0.0, 0.99),
            _padding: Vec2::ZERO,
        })
    }
}

#[derive(Component, ShaderType, Clone)]
pub struct SniperFisheyeUniform {
    strength: f32,
    edge_start: f32,
    _padding: Vec2,
}

#[derive(Resource)]
struct SniperFisheyePipeline {
    sampler: Sampler,
    layout: BindGroupLayoutDescriptor,
    fullscreen_shader: FullscreenShader,
    fragment_shader: Handle<Shader>,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct SniperFisheyePipelineKey {
    hdr: bool,
}

#[derive(Component)]
struct SniperFisheyePipelineId(CachedRenderPipelineId);

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
enum SniperFisheyeLabel {
    Pass,
}

#[derive(Default)]
struct SniperFisheyeNode;

/// Plugin that wires a sniper-only barrel distortion pass into the 3D render graph.
pub struct SniperFisheyePlugin;

impl Plugin for SniperFisheyePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "sniper_fisheye.wgsl");

        app.add_plugins((
            ExtractComponentPlugin::<SniperFisheye>::default(),
            UniformComponentPlugin::<SniperFisheyeUniform>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<SpecializedRenderPipelines<SniperFisheyePipeline>>()
            .add_systems(RenderStartup, init_sniper_fisheye_pipeline)
            .add_systems(
                Render,
                prepare_sniper_fisheye_pipelines.in_set(RenderSystems::Prepare),
            )
            .add_render_graph_node::<ViewNodeRunner<SniperFisheyeNode>>(
                Core3d,
                SniperFisheyeLabel::Pass,
            )
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::Tonemapping,
                    SniperFisheyeLabel::Pass,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }
}

fn init_sniper_fisheye_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "sniper_fisheye_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer_sized(true, Some(SniperFisheyeUniform::min_size())),
            ),
        ),
    );

    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let fragment_shader = load_embedded_asset!(asset_server.as_ref(), "sniper_fisheye.wgsl");

    commands.insert_resource(SniperFisheyePipeline {
        sampler,
        layout,
        fullscreen_shader: fullscreen_shader.clone(),
        fragment_shader,
    });
}

impl SpecializedRenderPipeline for SniperFisheyePipeline {
    type Key = SniperFisheyePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("sniper_fisheye_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.fragment_shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: if key.hdr {
                        ViewTarget::TEXTURE_FORMAT_HDR
                    } else {
                        TextureFormat::bevy_default()
                    },
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

fn prepare_sniper_fisheye_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<SniperFisheyePipeline>>,
    pipeline: Res<SniperFisheyePipeline>,
    views: Query<(Entity, &ExtractedView), With<SniperFisheyeUniform>>,
) {
    for (entity, view) in views.iter() {
        let pipeline_id = pipelines.specialize(
            &pipeline_cache,
            &pipeline,
            SniperFisheyePipelineKey { hdr: view.hdr },
        );
        commands
            .entity(entity)
            .insert(SniperFisheyePipelineId(pipeline_id));
    }
}

impl ViewNode for SniperFisheyeNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static SniperFisheyePipelineId,
        &'static SniperFisheyeUniform,
        &'static DynamicUniformIndex<SniperFisheyeUniform>,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (view_target, pipeline_id, settings, settings_offset): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        if settings.strength <= 0.0005 {
            return Ok(());
        }

        let pipeline_cache = world.resource::<PipelineCache>();
        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
            return Ok(());
        };

        let uniform_data = world.resource::<ComponentUniforms<SniperFisheyeUniform>>();
        let Some(settings_binding) = uniform_data.uniforms().binding() else {
            return Ok(());
        };

        let sniper_pipeline = world.resource::<SniperFisheyePipeline>();
        let post_process = view_target.post_process_write();

        let bind_group = render_context.render_device().create_bind_group(
            Some("sniper_fisheye_bind_group"),
            &pipeline_cache.get_bind_group_layout(&sniper_pipeline.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &sniper_pipeline.sampler,
                settings_binding.clone(),
            )),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("sniper_fisheye_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_render_pipeline(render_pipeline);
        render_pass.set_bind_group(0, &bind_group, &[settings_offset.index()]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}
