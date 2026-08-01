use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use std::collections::HashMap;

use shared::{
    building::{BuildingPosition, PlacedBuilding},
    city::{
        build_road_render_segments, build_road_segments, plot_building_front_direction,
        plot_building_ground_position, plot_building_rect, plot_building_scene_transform,
        plot_rect, road_polyline_points, sample_polyline_strip, sample_strip_extended,
        AuthoredCityBuilding, MapPlot, MapRoad, OrientedRect, RoadClass, RoadRenderSegment,
        RoadSegment,
    },
    terrain::{stylized_palette, WorldTerrain},
};

use crate::{
    session::{CursorTerrainHit, EditorSession, EditorUiState, ToolMode},
    tools::VisualRefreshFlags,
};

use super::{
    planned_plot_placements, snap_road_point, CityEditorState, EditorCityPreviewVisual,
    EditorPlotBuildingVisual, EditorPlotVisual, EditorRoadVisual,
};

#[derive(Resource, Clone)]
pub(crate) struct CityEditorMaterials {
    road_alley: Handle<StandardMaterial>,
    road_local: Handle<StandardMaterial>,
    road_collector: Handle<StandardMaterial>,
    road_arterial: Handle<StandardMaterial>,
    sidewalk: Handle<StandardMaterial>,
    plot_residential: Handle<StandardMaterial>,
    plot_commercial: Handle<StandardMaterial>,
    plot_industrial: Handle<StandardMaterial>,
    plot_civic: Handle<StandardMaterial>,
    plot_park: Handle<StandardMaterial>,
    road_preview: Handle<StandardMaterial>,
    plot_preview: Handle<StandardMaterial>,
    building_preview: Handle<StandardMaterial>,
    front_arrow: Handle<StandardMaterial>,
}

const ROAD_SURFACE_OFFSET: f32 = 0.02;
const SIDEWALK_BASE_OFFSET: f32 = 0.025;
const SIDEWALK_TOP_OFFSET: f32 = 0.12;
const ROAD_UV_SCALE: f32 = 0.32;
const SIDEWALK_UV_SCALE: f32 = 0.24;

pub fn setup_city_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldTerrain>,
    session: Res<EditorSession>,
) {
    // No textures here on purpose -- and for a second reason the client does not have.
    //
    // This block loaded two PNGs that this pipeline deletes, so leaving it would have been a
    // missing-asset failure at editor startup rather than a wasted megabyte. It is the same
    // duplicate-VRAM mistake `client/src/city/spawn.rs` had (see the note there), copied into
    // the editor, and it survived the client's fix because nothing connects the two files.
    // That is the editor's whole failure mode: a second copy of the renderer that nobody
    // compiles against the first.
    let palette = stylized_palette();
    let city_materials = CityEditorMaterials {
        road_alley: materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.07, 0.08),
            perceptual_roughness: 0.98,
            ..default()
        }),
        road_local: materials.add(StandardMaterial {
            base_color: linear(palette.cobble),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.08,
            ..default()
        }),
        road_collector: materials.add(StandardMaterial {
            base_color: Color::srgb(0.14, 0.14, 0.15),
            perceptual_roughness: 0.95,
            ..default()
        }),
        road_arterial: materials.add(StandardMaterial {
            base_color: Color::srgb(0.17, 0.17, 0.18),
            perceptual_roughness: 0.92,
            ..default()
        }),
        sidewalk: materials.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.72, 0.69),
            perceptual_roughness: 1.0,
            cull_mode: None,
            ..default()
        }),
        plot_residential: materials.add(plot_material(Color::srgba(0.18, 0.72, 0.32, 0.24))),
        plot_commercial: materials.add(plot_material(Color::srgba(0.18, 0.50, 0.84, 0.24))),
        plot_industrial: materials.add(plot_material(Color::srgba(0.80, 0.55, 0.18, 0.24))),
        plot_civic: materials.add(plot_material(Color::srgba(0.72, 0.24, 0.24, 0.24))),
        plot_park: materials.add(plot_material(Color::srgba(0.12, 0.58, 0.44, 0.24))),
        road_preview: materials.add(StandardMaterial {
            base_color: Color::srgba(0.08, 0.78, 0.92, 0.72),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        plot_preview: materials.add(StandardMaterial {
            base_color: Color::srgba(0.95, 0.86, 0.20, 0.26),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
        building_preview: materials.add(StandardMaterial {
            base_color: Color::srgba(0.95, 0.38, 0.16, 0.38),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
        front_arrow: materials.add(StandardMaterial {
            base_color: Color::srgba(0.98, 0.16, 0.12, 0.92),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
    };

    commands.insert_resource(city_materials.clone());
    spawn_authored_city_visuals(
        &mut commands,
        &asset_server,
        &mut meshes,
        &city_materials,
        &world,
        &session.map_edits.roads,
        &session.map_edits.plots,
    );
}

pub fn apply_city_visual_refresh(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<CityEditorMaterials>,
    world: Res<WorldTerrain>,
    session: Res<EditorSession>,
    mut flags: ResMut<VisualRefreshFlags>,
    visuals: Query<
        Entity,
        Or<(
            With<EditorRoadVisual>,
            With<EditorPlotVisual>,
            With<EditorPlotBuildingVisual>,
        )>,
    >,
) {
    if !flags.city_layout {
        return;
    }

    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }

    spawn_authored_city_visuals(
        &mut commands,
        &asset_server,
        &mut meshes,
        &materials,
        &world,
        &session.map_edits.roads,
        &session.map_edits.plots,
    );

    flags.city_layout = false;
}

pub fn update_city_preview_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Option<Res<CityEditorMaterials>>,
    world: Res<WorldTerrain>,
    session: Res<EditorSession>,
    city_state: Res<CityEditorState>,
    cursor_hit: Res<CursorTerrainHit>,
    ui_state: Res<EditorUiState>,
    preview_entities: Query<Entity, With<EditorCityPreviewVisual>>,
) {
    for entity in preview_entities.iter() {
        commands.entity(entity).despawn();
    }

    let Some(materials) = materials else {
        return;
    };

    match ui_state.tool {
        ToolMode::Road => {
            if city_state.draft_road_points.is_empty() {
                return;
            }

            let mut points = city_state.draft_road_points.clone();
            if let Some(hit) = cursor_hit.0 {
                points.push(snap_road_point(
                    Vec2::new(hit.x, hit.z),
                    &session,
                    &city_state,
                    &ui_state,
                ));
            }
            if points.len() < 2 {
                return;
            }

            let preview_road = MapRoad {
                id: 0,
                points: points.iter().map(|point| [point.x, point.y]).collect(),
                width: ui_state.road.width.max(1.0),
                road_class: ui_state.road.road_class,
                lane_count: ui_state.road.lane_count.max(1),
                sidewalk_left: ui_state.road.sidewalk_left,
                sidewalk_right: ui_state.road.sidewalk_right,
                sidewalk_width: ui_state.road.sidewalk_width.max(0.0),
                parking_left: false,
                parking_right: false,
                district: None,
            };

            for segment in build_road_segments(&preview_road) {
                if let Some(mesh) = build_road_mesh(
                    &world,
                    &segment,
                    segment.road_width * 0.5,
                    ROAD_SURFACE_OFFSET + 0.02,
                    ROAD_UV_SCALE,
                    0.0,
                    0.0,
                ) {
                    commands.spawn((
                        Name::new("RoadPreview"),
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(materials.road_preview.clone()),
                        Transform::default(),
                        EditorCityPreviewVisual,
                    ));
                }
            }
        }
        ToolMode::Plot => {
            let Some(hit) = cursor_hit.0 else {
                return;
            };
            for plot in planned_plot_placements(Vec2::new(hit.x, hit.z), &ui_state.plot, &session) {
                if let Some(mesh) = build_plot_mesh(&world, plot_rect(&plot), 0.07) {
                    commands.spawn((
                        Name::new("PlotPreview"),
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(materials.plot_preview.clone()),
                        Transform::default(),
                        EditorCityPreviewVisual,
                    ));
                }
                if let Some(kind) = plot.building_kind {
                    if let Some(mesh) =
                        build_plot_mesh(&world, plot_building_rect(&plot, kind), 0.1)
                    {
                        commands.spawn((
                            Name::new("PlotBuildingPreview"),
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(materials.building_preview.clone()),
                            Transform::default(),
                            EditorCityPreviewVisual,
                        ));
                    }
                    let rect = plot_building_rect(&plot, kind);
                    if let Some(mesh) = build_front_arrow_mesh(
                        &world,
                        rect.center,
                        plot_building_front_direction(&plot, kind),
                        rect.half_extents.y + 1.2,
                        0.18,
                        0.65,
                        0.12,
                    ) {
                        commands.spawn((
                            Name::new("PlotFrontPreview"),
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(materials.front_arrow.clone()),
                            Transform::default(),
                            EditorCityPreviewVisual,
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

fn spawn_authored_city_visuals(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &CityEditorMaterials,
    world: &WorldTerrain,
    roads: &[MapRoad],
    plots: &[MapPlot],
) {
    let render_segments = build_road_render_segments(roads);
    let render_segments_by_road = group_render_segments_by_road(&render_segments);

    for road in roads {
        let Some(road_segments) = render_segments_by_road.get(&road.id) else {
            continue;
        };
        let (points, closed) = road_polyline_points(road);
        if points.len() < 2 {
            continue;
        }
        let (start_extension, end_extension, start_trim, end_trim) =
            road_endpoint_visuals(road_segments, closed);

        let road_samples = sample_polyline_strip(
            &points,
            closed,
            road.width * 0.5,
            -road.width * 0.5,
            4.0,
            start_extension,
            end_extension,
        );
        if let Some(mesh) =
            build_strip_mesh(world, &road_samples, ROAD_SURFACE_OFFSET, ROAD_UV_SCALE)
        {
            commands.spawn((
                Name::new(format!("Road({})", road.id)),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(road_material_for_class(materials, road.road_class)),
                Transform::default(),
                EditorRoadVisual,
            ));
        }

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
            if let Some(mesh) = build_extruded_strip_mesh(
                world,
                &samples,
                SIDEWALK_TOP_OFFSET,
                SIDEWALK_BASE_OFFSET,
                SIDEWALK_UV_SCALE,
            ) {
                commands.spawn((
                    Name::new(format!("Sidewalk({}:Left)", road.id)),
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.sidewalk.clone()),
                    Transform::default(),
                    EditorRoadVisual,
                ));
            }
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
            if let Some(mesh) = build_extruded_strip_mesh(
                world,
                &samples,
                SIDEWALK_TOP_OFFSET,
                SIDEWALK_BASE_OFFSET,
                SIDEWALK_UV_SCALE,
            ) {
                commands.spawn((
                    Name::new(format!("Sidewalk({}:Right)", road.id)),
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.sidewalk.clone()),
                    Transform::default(),
                    EditorRoadVisual,
                ));
            }
        }
    }

    for plot in plots {
        if let Some(mesh) = build_plot_mesh(world, plot_rect(plot), 0.06) {
            commands.spawn((
                Name::new(format!("Plot({})", plot.id)),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(plot_material_for_zone(materials, plot.zone)),
                Transform::default(),
                EditorPlotVisual,
            ));
        }

        let Some(kind) = plot.building_kind else {
            continue;
        };
        let scene = asset_server.load(kind.spec().scene_path);
        let footprint = plot_building_rect(plot, kind);
        let ground_y = world.get_height(footprint.center.x, footprint.center.y);
        commands.spawn((
            Name::new(format!("PlotBuilding({}:{})", plot.id, kind.display_name())),
            WorldAssetRoot(scene),
            plot_building_scene_transform(plot, kind, ground_y),
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
            AuthoredCityBuilding { plot_id: plot.id },
            PlacedBuilding {
                building_type: kind.spec().building_type,
                rotation: footprint.rotation_y,
            },
            BuildingPosition(plot_building_ground_position(plot, kind, ground_y)),
            EditorPlotBuildingVisual,
        ));

        if let Some(mesh) = build_front_arrow_mesh(
            world,
            footprint.center,
            plot_building_front_direction(plot, kind),
            footprint.half_extents.y + 1.2,
            0.16,
            0.6,
            0.1,
        ) {
            commands.spawn((
                Name::new(format!("PlotFront({})", plot.id)),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.front_arrow.clone()),
                Transform::default(),
                EditorPlotVisual,
            ));
        }
    }
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

fn plot_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    }
}

fn plot_material_for_zone(
    materials: &CityEditorMaterials,
    zone: shared::city::PlotZone,
) -> Handle<StandardMaterial> {
    match zone {
        shared::city::PlotZone::Residential | shared::city::PlotZone::MixedUse => {
            materials.plot_residential.clone()
        }
        shared::city::PlotZone::Commercial => materials.plot_commercial.clone(),
        shared::city::PlotZone::Industrial => materials.plot_industrial.clone(),
        shared::city::PlotZone::Civic => materials.plot_civic.clone(),
        shared::city::PlotZone::Park => materials.plot_park.clone(),
    }
}

fn road_material_for_class(
    materials: &CityEditorMaterials,
    road_class: RoadClass,
) -> Handle<StandardMaterial> {
    match road_class {
        RoadClass::Alley => materials.road_alley.clone(),
        RoadClass::Local => materials.road_local.clone(),
        RoadClass::Collector => materials.road_collector.clone(),
        RoadClass::Arterial => materials.road_arterial.clone(),
    }
}

fn build_road_mesh(
    terrain: &WorldTerrain,
    segment: &RoadSegment,
    half_width: f32,
    height_offset: f32,
    uv_scale: f32,
    start_extension: f32,
    end_extension: f32,
) -> Option<Mesh> {
    let samples = sample_strip_extended(segment, half_width, 4.0, start_extension, end_extension);
    build_strip_mesh(terrain, &samples, height_offset, uv_scale)
}

fn build_strip_mesh(
    terrain: &WorldTerrain,
    samples: &[shared::city::StripSample],
    height_offset: f32,
    uv_scale: f32,
) -> Option<Mesh> {
    if samples.len() < 2 {
        return None;
    }

    let mut positions = Vec::with_capacity(samples.len() * 2);
    let mut normals = Vec::with_capacity(samples.len() * 2);
    let mut uvs = Vec::with_capacity(samples.len() * 2);
    let mut indices = Vec::with_capacity((samples.len().saturating_sub(1)) * 6);

    for sample in samples {
        positions.push([
            sample.left.x,
            terrain.get_height(sample.left.x, sample.left.y) + height_offset,
            sample.left.y,
        ]);
        positions.push([
            sample.right.x,
            terrain.get_height(sample.right.x, sample.right.y) + height_offset,
            sample.right.y,
        ]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        let width = sample.left.distance(sample.right);
        uvs.push([0.0, sample.distance * uv_scale]);
        uvs.push([width * uv_scale, sample.distance * uv_scale]);
    }

    for index in 0..samples.len().saturating_sub(1) {
        let base = (index * 2) as u32;
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }

    Some(build_mesh(positions, normals, uvs, indices))
}

fn build_extruded_strip_mesh(
    terrain: &WorldTerrain,
    samples: &[shared::city::StripSample],
    top_offset: f32,
    bottom_offset: f32,
    uv_scale: f32,
) -> Option<Mesh> {
    if samples.len() < 2 {
        return None;
    }

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    let top_base = 0u32;
    for sample in samples {
        positions.push([
            sample.left.x,
            terrain.get_height(sample.left.x, sample.left.y) + top_offset,
            sample.left.y,
        ]);
        positions.push([
            sample.right.x,
            terrain.get_height(sample.right.x, sample.right.y) + top_offset,
            sample.right.y,
        ]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        let width = sample.left.distance(sample.right);
        uvs.push([0.0, sample.distance * uv_scale]);
        uvs.push([width * uv_scale, sample.distance * uv_scale]);
    }
    for index in 0..samples.len().saturating_sub(1) {
        let base = top_base + (index * 2) as u32;
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }

    for side in 0..2 {
        let wall_base = positions.len() as u32;
        for sample in samples {
            let point = if side == 0 { sample.left } else { sample.right };
            positions.push([
                point.x,
                terrain.get_height(point.x, point.y) + top_offset,
                point.y,
            ]);
            positions.push([
                point.x,
                terrain.get_height(point.x, point.y) + bottom_offset,
                point.y,
            ]);

            let next_index = if samples.len() > 1 {
                1.min(samples.len() - 1)
            } else {
                0
            };
            let tangent = if side == 0 {
                (samples[next_index].left - samples[0].left).normalize_or_zero()
            } else {
                (samples[next_index].right - samples[0].right).normalize_or_zero()
            };
            let mut normal = Vec3::new(tangent.y, 0.0, -tangent.x);
            if side == 1 {
                normal = -normal;
            }
            let normal = normal.normalize_or_zero().to_array();
            normals.push(normal);
            normals.push(normal);
            uvs.push([sample.distance * uv_scale, 0.0]);
            uvs.push([
                sample.distance * uv_scale,
                (top_offset - bottom_offset).abs() * 4.0,
            ]);
        }
        for index in 0..samples.len().saturating_sub(1) {
            let base = wall_base + (index * 2) as u32;
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    Some(build_mesh(positions, normals, uvs, indices))
}

fn build_extruded_rect_mesh(
    terrain: &WorldTerrain,
    rect: OrientedRect,
    top_offset: f32,
    bottom_offset: f32,
    uv_scale: f32,
) -> Option<Mesh> {
    let corners = rect.corners();
    let mut positions = Vec::with_capacity(20);
    let mut normals = Vec::with_capacity(20);
    let mut uvs = Vec::with_capacity(20);
    let mut indices = Vec::with_capacity(30);

    let top_positions = corners.map(|corner| {
        [
            corner.x,
            terrain.get_height(corner.x, corner.y) + top_offset,
            corner.y,
        ]
    });
    let bottom_positions = corners.map(|corner| {
        [
            corner.x,
            terrain.get_height(corner.x, corner.y) + bottom_offset,
            corner.y,
        ]
    });

    positions.extend_from_slice(&top_positions);
    normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
    uvs.extend_from_slice(&[
        [
            rect.half_extents.x * 2.0 * uv_scale,
            rect.half_extents.y * 2.0 * uv_scale,
        ],
        [0.0, rect.half_extents.y * 2.0 * uv_scale],
        [0.0, 0.0],
        [rect.half_extents.x * 2.0 * uv_scale, 0.0],
    ]);
    indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);

    for edge_index in 0..4 {
        let next = (edge_index + 1) % 4;
        let top_a = Vec3::from(top_positions[edge_index]);
        let top_b = Vec3::from(top_positions[next]);
        let bottom_b = Vec3::from(bottom_positions[next]);
        let bottom_a = Vec3::from(bottom_positions[edge_index]);
        let edge = top_b - top_a;
        let edge_length = edge.xz().length().max(0.001);
        let height = (top_a.y - bottom_a.y).abs().max(0.001);
        let normal = Vec3::new(edge.z, 0.0, -edge.x).normalize_or_zero();
        let base = positions.len() as u32;

        positions.extend_from_slice(&[
            top_a.to_array(),
            top_b.to_array(),
            bottom_b.to_array(),
            bottom_a.to_array(),
        ]);
        normals.extend_from_slice(&[normal.to_array(); 4]);
        uvs.extend_from_slice(&[
            [0.0, 0.0],
            [edge_length * uv_scale, 0.0],
            [edge_length * uv_scale, height * 4.0],
            [0.0, height * 4.0],
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    Some(build_mesh(positions, normals, uvs, indices))
}

fn build_plot_mesh(terrain: &WorldTerrain, rect: OrientedRect, height_offset: f32) -> Option<Mesh> {
    let corners = rect.corners();
    let positions = corners
        .iter()
        .map(|corner| {
            [
                corner.x,
                terrain.get_height(corner.x, corner.y) + height_offset,
                corner.y,
            ]
        })
        .collect::<Vec<_>>();
    let normals = vec![[0.0, 1.0, 0.0]; 4];
    let uvs = vec![[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]];
    let indices = vec![0, 1, 2, 0, 2, 3];
    Some(build_mesh(positions, normals, uvs, indices))
}

fn build_front_arrow_mesh(
    terrain: &WorldTerrain,
    origin: Vec2,
    direction: Vec2,
    length: f32,
    half_width: f32,
    head_length: f32,
    height_offset: f32,
) -> Option<Mesh> {
    let dir = direction.normalize_or_zero();
    if dir.length_squared() <= 1.0e-6 {
        return None;
    }

    let length = length.max(head_length + 0.25);
    let shaft_length = (length - head_length).max(0.2);
    let shaft_end = origin + dir * shaft_length;
    let tip = origin + dir * length;
    let perp = Vec2::new(-dir.y, dir.x);
    let head_half_width = half_width * 2.2;

    let positions = vec![
        arrow_vertex(terrain, origin - perp * half_width, height_offset),
        arrow_vertex(terrain, origin + perp * half_width, height_offset),
        arrow_vertex(terrain, shaft_end + perp * half_width, height_offset),
        arrow_vertex(terrain, shaft_end - perp * half_width, height_offset),
        arrow_vertex(terrain, shaft_end - perp * head_half_width, height_offset),
        arrow_vertex(terrain, shaft_end + perp * head_half_width, height_offset),
        arrow_vertex(terrain, tip, height_offset),
    ];
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [0.5, 1.0],
    ];
    let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6];
    Some(build_mesh(positions, normals, uvs, indices))
}

fn arrow_vertex(terrain: &WorldTerrain, point: Vec2, height_offset: f32) -> [f32; 3] {
    [
        point.x,
        terrain.get_height(point.x, point.y) + height_offset,
        point.y,
    ]
}

fn build_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(positions),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(normals),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, VertexAttributeValues::Float32x2(uvs));
    mesh.insert_indices(Indices::U32(indices));
    let _ = mesh.generate_tangents();
    mesh
}

/// Palette entries are LINEAR -- they go to the shader as uniforms -- so they must not pass
/// through `Color::srgb`, which would apply the transfer curve twice and wash the roads out.
fn linear(v: Vec4) -> Color {
    Color::linear_rgb(v.x, v.y, v.z)
}
