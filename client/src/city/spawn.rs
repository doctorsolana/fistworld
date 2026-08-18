use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use std::collections::HashMap;

use shared::{
    city::{
        build_road_render_segments, plot_rect, road_polyline_points, sample_polyline_strip,
        AuthoredCityLayout, OrientedRect, RoadClass, RoadRenderSegment,
    },
    terrain::{stylized_palette, ChunkCoord, WorldTerrain},
};

use crate::render::systems::ClientWorldRoot;

#[derive(Component)]
pub(crate) struct CityRoadVisual;

const ROAD_SURFACE_OFFSET: f32 = 0.02;
const SIDEWALK_BASE_OFFSET: f32 = 0.025;
const SIDEWALK_TOP_OFFSET: f32 = 0.12;
const ROAD_UV_SCALE: f32 = 0.32;
const SIDEWALK_UV_SCALE: f32 = 0.24;

pub fn spawn_city_layout_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldTerrain>,
    city_layout: Res<AuthoredCityLayout>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    existing_visuals: Query<Entity, With<CityRoadVisual>>,
) {
    if !existing_visuals.is_empty()
        || (city_layout.layout.road_segments().is_empty() && city_layout.layout.plots().is_empty())
    {
        return;
    }

    // No textures here on purpose.
    //
    // This block used to load four PNGs from `textures/terrain/optimized_1k/` -- the same
    // Cobblestone and Dirt images that are already layers 3 and 2 of `terrain_albedo_array`,
    // byte-identical to their mip 0. Roads are ordinary `StandardMaterial` strips and cannot
    // sample a `2d_array` the splat shader owns, so they re-loaded standalone copies: 16 MiB of
    // duplicate VRAM the moment any road existed, with `mip_level_count: 1` (Bevy 0.19 generates
    // no mips for PNGs) so they shimmered at the grazing angles an RTS camera always sees.
    //
    // The terrain itself renders as flat palette colour -- `stylize.x` is 1.0 -- so photographic
    // cobblestone beside it was off-model anyway. Roads now take their colour from the same
    // palette the ground does, which is why they no longer need an image at all.
    let palette = stylized_palette();
    let road_alley_material = materials.add(StandardMaterial {
        // Dirt, darkened: an alley is packed earth, not open ground.
        base_color: linear(palette.dirt * 0.62),
        perceptual_roughness: 0.96,
        metallic: 0.0,
        reflectance: 0.04,
        ..default()
    });
    let road_local_material = materials.add(StandardMaterial {
        base_color: linear(palette.cobble),
        perceptual_roughness: 0.94,
        metallic: 0.0,
        reflectance: 0.06,
        ..default()
    });
    let road_collector_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.105, 0.105, 0.10),
        perceptual_roughness: 0.93,
        metallic: 0.0,
        reflectance: 0.05,
        ..default()
    });
    let road_arterial_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.13, 0.13, 0.125),
        perceptual_roughness: 0.91,
        metallic: 0.0,
        reflectance: 0.055,
        ..default()
    });
    let sidewalk_material = materials.add(StandardMaterial {
        // A shade lighter than the road it borders, so the kerb line reads.
        base_color: linear(palette.cobble * 1.18),
        perceptual_roughness: 0.97,
        metallic: 0.0,
        reflectance: 0.035,
        cull_mode: None,
        ..default()
    });
    let plot_debug_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.24, 0.86, 0.36, 0.18),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });

    let show_plot_debug = std::env::var("CITYSIM_LAYOUT_DEBUG")
        .map(|value| value == "1")
        .unwrap_or(false);

    let mut sectors: HashMap<ChunkCoord, SectorGeometry> = HashMap::new();

    let render_segments = build_road_render_segments(city_layout.layout.roads());
    let render_segments_by_road = group_render_segments_by_road(&render_segments);

    for road in city_layout.layout.roads() {
        let Some(road_segments) = render_segments_by_road.get(&road.id) else {
            continue;
        };
        let (points, closed) = road_polyline_points(road);
        if points.len() < 2 {
            continue;
        }
        let (start_extension, end_extension, start_trim, end_trim) =
            road_endpoint_visuals(road_segments, closed);
        let coord = sector_coord_for_points(&points);
        let sector = sectors.entry(coord).or_default();
        let road_samples = sample_polyline_strip(
            &points,
            closed,
            road.width * 0.5,
            -road.width * 0.5,
            4.0,
            start_extension,
            end_extension,
        );
        append_strip_mesh(
            sector.roads.entry(road.road_class).or_default(),
            &world,
            &road_samples,
            ROAD_SURFACE_OFFSET,
            ROAD_UV_SCALE,
        );

        if road.sidewalk_left && road.sidewalk_width > 0.0 {
            let samples = sample_polyline_strip(
                &points,
                closed,
                road.width * 0.5 + road.sidewalk_width,
                road.width * 0.5,
                4.0,
                -start_trim,
                -end_trim,
            );
            append_extruded_strip_mesh(
                &mut sector.sidewalk,
                &world,
                &samples,
                SIDEWALK_TOP_OFFSET,
                SIDEWALK_BASE_OFFSET,
                SIDEWALK_UV_SCALE,
            );
        }
        if road.sidewalk_right && road.sidewalk_width > 0.0 {
            let samples = sample_polyline_strip(
                &points,
                closed,
                -road.width * 0.5,
                -(road.width * 0.5 + road.sidewalk_width),
                4.0,
                -start_trim,
                -end_trim,
            );
            append_extruded_strip_mesh(
                &mut sector.sidewalk,
                &world,
                &samples,
                SIDEWALK_TOP_OFFSET,
                SIDEWALK_BASE_OFFSET,
                SIDEWALK_UV_SCALE,
            );
        }
    }

    if show_plot_debug {
        for plot in city_layout.layout.plots() {
            let coord = sector_coord_for_rect(plot_rect(plot));
            append_rect_mesh(
                &mut sectors.entry(coord).or_default().plot_debug,
                &world,
                plot_rect(plot),
                0.03,
            );
        }
    }

    let world_root = world_root_query.single().ok();
    for (coord, sector) in sectors {
        for (road_class, buffers) in sector.roads {
            spawn_sector_visual(
                &mut commands,
                &mut meshes,
                world_root,
                coord,
                road_visual_kind_label(road_class),
                buffers,
                road_material_for_class(
                    road_class,
                    &road_alley_material,
                    &road_local_material,
                    &road_collector_material,
                    &road_arterial_material,
                ),
            );
        }
        spawn_sector_visual(
            &mut commands,
            &mut meshes,
            world_root,
            coord,
            "Sidewalk",
            sector.sidewalk,
            sidewalk_material.clone(),
        );
        spawn_sector_visual(
            &mut commands,
            &mut meshes,
            world_root,
            coord,
            "PlotDebug",
            sector.plot_debug,
            plot_debug_material.clone(),
        );
    }
}

pub fn cleanup_city_layout_visuals(
    mut commands: Commands,
    visuals: Query<Entity, With<CityRoadVisual>>,
) {
    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }
}

#[derive(Default)]
struct SectorGeometry {
    roads: HashMap<RoadClass, MeshBuffers>,
    sidewalk: MeshBuffers,
    plot_debug: MeshBuffers,
}

#[derive(Default)]
struct MeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

fn sector_coord_for_points(points: &[Vec2]) -> ChunkCoord {
    let center = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    ChunkCoord::from_world_pos(Vec3::new(center.x, 0.0, center.y))
}

fn sector_coord_for_rect(rect: OrientedRect) -> ChunkCoord {
    ChunkCoord::from_world_pos(Vec3::new(rect.center.x, 0.0, rect.center.y))
}

fn group_render_segments_by_road(
    render_segments: &[RoadRenderSegment],
) -> HashMap<u64, Vec<RoadRenderSegment>> {
    let mut by_road = HashMap::new();
    for render_segment in render_segments {
        by_road
            .entry(render_segment.segment.road_id)
            .or_insert_with(Vec::new)
            .push(*render_segment);
    }
    for segments in by_road.values_mut() {
        segments.sort_by_key(|render_segment| render_segment.segment.segment_index);
    }
    by_road
}

fn road_endpoint_visuals(
    render_segments: &[RoadRenderSegment],
    closed: bool,
) -> (f32, f32, f32, f32) {
    if closed || render_segments.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let start = render_segments[0].start_visual;
    let end = render_segments[render_segments.len() - 1].end_visual;
    (
        start.road_extension,
        end.road_extension,
        start.sidewalk_trim,
        end.sidewalk_trim,
    )
}

fn append_strip_mesh(
    buffers: &mut MeshBuffers,
    terrain: &WorldTerrain,
    samples: &[shared::city::StripSample],
    height_offset: f32,
    uv_scale: f32,
) {
    if samples.len() < 2 {
        return;
    }

    let base_index = buffers.positions.len() as u32;

    for sample in samples {
        buffers.positions.push([
            sample.left.x,
            terrain.get_height(sample.left.x, sample.left.y) + height_offset,
            sample.left.y,
        ]);
        buffers.positions.push([
            sample.right.x,
            terrain.get_height(sample.right.x, sample.right.y) + height_offset,
            sample.right.y,
        ]);
        buffers.normals.push([0.0, 1.0, 0.0]);
        buffers.normals.push([0.0, 1.0, 0.0]);
        let width = sample.left.distance(sample.right);
        buffers.uvs.push([0.0, sample.distance * uv_scale]);
        buffers
            .uvs
            .push([width * uv_scale, sample.distance * uv_scale]);
    }

    for index in 0..samples.len().saturating_sub(1) {
        let base = base_index + (index * 2) as u32;
        buffers.indices.extend_from_slice(&[
            base,
            base + 2,
            base + 1,
            base + 1,
            base + 2,
            base + 3,
        ]);
    }
}

fn append_extruded_strip_mesh(
    buffers: &mut MeshBuffers,
    terrain: &WorldTerrain,
    samples: &[shared::city::StripSample],
    top_offset: f32,
    bottom_offset: f32,
    uv_scale: f32,
) {
    if samples.len() < 2 {
        return;
    }

    let top_base = buffers.positions.len() as u32;
    for sample in samples {
        buffers.positions.push([
            sample.left.x,
            terrain.get_height(sample.left.x, sample.left.y) + top_offset,
            sample.left.y,
        ]);
        buffers.positions.push([
            sample.right.x,
            terrain.get_height(sample.right.x, sample.right.y) + top_offset,
            sample.right.y,
        ]);
        buffers.normals.push([0.0, 1.0, 0.0]);
        buffers.normals.push([0.0, 1.0, 0.0]);
        let width = sample.left.distance(sample.right);
        buffers.uvs.push([0.0, sample.distance * uv_scale]);
        buffers
            .uvs
            .push([width * uv_scale, sample.distance * uv_scale]);
    }
    for index in 0..samples.len().saturating_sub(1) {
        let base = top_base + (index * 2) as u32;
        buffers.indices.extend_from_slice(&[
            base,
            base + 2,
            base + 1,
            base + 1,
            base + 2,
            base + 3,
        ]);
    }

    for side in 0..2 {
        let wall_base = buffers.positions.len() as u32;
        for sample in samples {
            let point = if side == 0 { sample.left } else { sample.right };
            buffers.positions.push([
                point.x,
                terrain.get_height(point.x, point.y) + top_offset,
                point.y,
            ]);
            buffers.positions.push([
                point.x,
                terrain.get_height(point.x, point.y) + bottom_offset,
                point.y,
            ]);
            buffers.normals.push([0.0, 0.0, 1.0]);
            buffers.normals.push([0.0, 0.0, 1.0]);
            buffers.uvs.push([sample.distance * uv_scale, 0.0]);
            buffers.uvs.push([
                sample.distance * uv_scale,
                (top_offset - bottom_offset).abs() * 4.0,
            ]);
        }
        for index in 0..samples.len().saturating_sub(1) {
            let base = wall_base + (index * 2) as u32;
            buffers.indices.extend_from_slice(&[
                base,
                base + 1,
                base + 2,
                base + 1,
                base + 3,
                base + 2,
            ]);
        }
    }
}

fn append_rect_mesh(
    buffers: &mut MeshBuffers,
    terrain: &WorldTerrain,
    rect: OrientedRect,
    height_offset: f32,
) {
    let corners = rect.corners();
    let base_index = buffers.positions.len() as u32;
    for corner in corners {
        buffers.positions.push([
            corner.x,
            terrain.get_height(corner.x, corner.y) + height_offset,
            corner.y,
        ]);
    }
    buffers.normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
    buffers
        .uvs
        .extend_from_slice(&[[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]]);
    buffers.indices.extend_from_slice(&[
        base_index,
        base_index + 1,
        base_index + 2,
        base_index,
        base_index + 2,
        base_index + 3,
    ]);
}

fn spawn_sector_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    world_root: Option<Entity>,
    coord: ChunkCoord,
    kind: &str,
    buffers: MeshBuffers,
    material: Handle<StandardMaterial>,
) {
    let Some(mesh) = build_mesh(buffers) else {
        return;
    };
    let entity = commands
        .spawn((
            Name::new(format!("City{kind}Sector({}, {})", coord.x, coord.z)),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::default(),
            CityRoadVisual,
        ))
        .id();
    if let Some(world_root) = world_root {
        commands.entity(world_root).add_child(entity);
    }
}

fn road_material_for_class(
    road_class: RoadClass,
    road_alley: &Handle<StandardMaterial>,
    road_local: &Handle<StandardMaterial>,
    road_collector: &Handle<StandardMaterial>,
    road_arterial: &Handle<StandardMaterial>,
) -> Handle<StandardMaterial> {
    match road_class {
        RoadClass::Alley => road_alley.clone(),
        RoadClass::Local => road_local.clone(),
        RoadClass::Collector => road_collector.clone(),
        RoadClass::Arterial => road_arterial.clone(),
    }
}

fn road_visual_kind_label(road_class: RoadClass) -> &'static str {
    match road_class {
        RoadClass::Alley => "Alley",
        RoadClass::Local => "LocalRoad",
        RoadClass::Collector => "Collector",
        RoadClass::Arterial => "Arterial",
    }
}

fn build_mesh(buffers: MeshBuffers) -> Option<Mesh> {
    if buffers.indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(buffers.positions),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(buffers.normals),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        VertexAttributeValues::Float32x2(buffers.uvs),
    );
    mesh.insert_indices(Indices::U32(buffers.indices));
    let _ = mesh.generate_tangents();
    Some(mesh)
}

/// Palette entries are LINEAR (they are handed straight to the shader as uniforms), so they
/// must not go through `Color::srgb`, which would apply the transfer curve a second time and
/// wash every road out.
fn linear(v: Vec4) -> Color {
    Color::linear_rgb(v.x, v.y, v.z)
}
