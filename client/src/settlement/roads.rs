//! One inexpensive, terrain-following ribbon for each builder-made village path.
//!
//! The server sends a compact ground-plane polyline and advances its built
//! prefix. Rebuilding this single mesh is cheaper than maintaining an entity
//! per paving stone, while vertex colour and a slight crown keep it from
//! reading as a flat brown rectangle.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use shared::components::{RoadSurface, VillageRoad};
use shared::terrain::WorldTerrain;

const ROAD_COLUMNS: [f32; 5] = [-1.0, -0.68, 0.0, 0.68, 1.0];
const ROAD_VISUAL_SAMPLE_SPACING: f32 = 0.45;
const ROAD_SURFACE_OFFSET: f32 = 0.055;
/// Road progress may replicate many times per second at high world speed. The
/// player cannot read sub-tenth-second ribbon growth, and rebuilding every
/// densely sampled vertex from the beginning on every packet causes avoidable
/// allocation/upload spikes.
const ROAD_VISUAL_UPDATE_INTERVAL_SECONDS: f64 = 0.10;

#[derive(Resource, Default)]
pub(super) struct VillageRoadAssets {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(super) struct VillageRoadVisual {
    mesh: Handle<Mesh>,
    dirty: bool,
    last_update_seconds: f64,
}

pub(super) fn update_village_road_visuals(
    mut commands: Commands,
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: ResMut<VillageRoadAssets>,
    mut roads: Query<(Entity, Ref<VillageRoad>, Option<&mut VillageRoadVisual>)>,
) {
    let Some(terrain) = terrain else { return };
    if assets.material == Handle::default() {
        assets.material = materials.add(StandardMaterial {
            // Vertex colours provide the worn centre and soft shoulders.
            base_color: Color::WHITE,
            perceptual_roughness: 0.98,
            metallic: 0.0,
            reflectance: 0.025,
            // A tiny renderer-side nudge complements the physical height
            // offset on steep triangles without making the road visibly float.
            depth_bias: 1.0,
            cull_mode: None,
            ..default()
        });
    }

    let now = time.elapsed_secs_f64();
    for (entity, road, visual) in roads.iter_mut() {
        let changed = road.is_changed();
        if let Some(mut visual) = visual {
            visual.dirty |= changed;
            if !visual.dirty {
                continue;
            }
            let due = now - visual.last_update_seconds >= ROAD_VISUAL_UPDATE_INTERVAL_SECONDS;
            if !due && !road.is_complete() {
                continue;
            }
            let Some(mesh) = build_village_road_mesh(&road, &terrain) else {
                continue;
            };
            if let Some(mut existing) = meshes.get_mut(&visual.mesh) {
                *existing = mesh;
            }
            visual.dirty = false;
            visual.last_update_seconds = now;
            continue;
        }

        let Some(mesh) = build_village_road_mesh(&road, &terrain) else {
            continue;
        };

        let mesh = meshes.add(mesh);
        commands.entity(entity).insert((
            Name::new(format!("{} path by {}", road.settlement, road.builder)),
            VillageRoadVisual {
                mesh: mesh.clone(),
                dirty: false,
                last_update_seconds: now,
            },
            Mesh3d(mesh),
            MeshMaterial3d(assets.material.clone()),
            Transform::default(),
            Visibility::Inherited,
        ));
    }
}

fn build_village_road_mesh(road: &VillageRoad, terrain: &WorldTerrain) -> Option<Mesh> {
    let built_points = road.built_points();
    if built_points.len() < 2 {
        return None;
    }
    // Replication keeps the route compact at roughly two-metre intervals, but
    // the terrain mesh can crest between those points. Densifying only the
    // visual ribbon makes every row resample the real ground at negligible
    // cost (still one mesh and one draw call per road).
    let points = densify_polyline(built_points, ROAD_VISUAL_SAMPLE_SPACING);

    let columns = ROAD_COLUMNS.len();
    let mut positions = Vec::with_capacity(points.len() * columns);
    let mut normals = Vec::with_capacity(points.len() * columns);
    let mut uvs = Vec::with_capacity(points.len() * columns);
    let mut colors = Vec::with_capacity(points.len() * columns);
    let mut distance_along = 0.0;

    for (row, point) in points.iter().copied().enumerate() {
        if row > 0 {
            distance_along += point.distance(points[row - 1]);
        }
        let previous = points[row.saturating_sub(1)];
        let next = points[(row + 1).min(points.len() - 1)];
        let direction = (next - previous).normalize_or_zero();
        let side = Vec2::new(-direction.y, direction.x);
        let width_noise = road_width_noise(point, row as u32);
        let half_width = road.width * 0.5 * width_noise;

        for fraction in ROAD_COLUMNS {
            let xz = point + side * half_width * fraction;
            let crown = (1.0 - fraction.abs()) * 0.035;
            positions.push([
                xz.x,
                terrain.get_height(xz.x, xz.y) + ROAD_SURFACE_OFFSET + crown,
                xz.y,
            ]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([(fraction + 1.0) * 0.5, distance_along * 0.28]);
            colors.push(road_color(road.surface, fraction, point, row as u32));
        }
    }

    let mut indices = Vec::with_capacity((points.len() - 1) * (columns - 1) * 6);
    for row in 0..(points.len() - 1) {
        for column in 0..(columns - 1) {
            let a = (row * columns + column) as u32;
            let b = ((row + 1) * columns + column) as u32;
            let c = b + 1;
            let d = a + 1;
            indices.extend_from_slice(&[a, c, b, a, d, c]);
        }
    }

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
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(colors),
    );
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn densify_polyline(points: &[Vec2], max_spacing: f32) -> Vec<Vec2> {
    let Some(first) = points.first().copied() else {
        return Vec::new();
    };
    let mut dense = vec![first];
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length <= 1e-4 {
            continue;
        }
        let steps = (length / max_spacing).ceil().max(1.0) as usize;
        for step in 1..=steps {
            dense.push(pair[0].lerp(pair[1], step as f32 / steps as f32));
        }
    }
    dense
}

fn road_width_noise(point: Vec2, row: u32) -> f32 {
    let hash = point.x.to_bits().rotate_left(9)
        ^ point.y.to_bits().rotate_left(21)
        ^ row.wrapping_mul(2_654_435_761);
    0.93 + (hash & 255) as f32 / 255.0 * 0.14
}

fn road_color(surface: RoadSurface, fraction: f32, point: Vec2, row: u32) -> [f32; 4] {
    let edge = fraction.abs().powf(1.7);
    let hash =
        point.x.to_bits() ^ point.y.to_bits().rotate_left(13) ^ row.wrapping_mul(2_246_822_519);
    let variation = (hash & 255) as f32 / 255.0 * 0.025 - 0.0125;
    let (packed, shoulder) = match surface {
        RoadSurface::Dirt => (
            Vec3::new(0.235, 0.145, 0.072),
            Vec3::new(0.34, 0.255, 0.145),
        ),
        RoadSurface::Stone => (Vec3::new(0.31, 0.30, 0.28), Vec3::new(0.43, 0.41, 0.37)),
    };
    let color = packed.lerp(shoulder, edge) + Vec3::splat(variation);
    [color.x, color.y, color.z, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_ribbon_has_five_columns_and_no_segment_entities() {
        let road = VillageRoad {
            settlement: "Oakmead".into(),
            builder: "Mara".into(),
            points: vec![Vec2::ZERO, Vec2::new(2.0, 0.5), Vec2::new(4.0, 0.5)],
            built_through: 3,
            width: 2.6,
            reserved_width: 4.0,
            surface: default(),
            class: default(),
            stone_committed: 0,
        };
        let mesh = build_village_road_mesh(&road, &WorldTerrain::default()).unwrap();
        let rows = mesh.count_vertices() / ROAD_COLUMNS.len();
        assert!(rows > road.points.len());
        assert_eq!(mesh.count_vertices(), rows * ROAD_COLUMNS.len());
        assert_eq!(mesh.indices().unwrap().len(), (rows - 1) * 4 * 6);
    }

    #[test]
    fn visual_sampling_preserves_endpoints_and_caps_spacing() {
        let points = [Vec2::ZERO, Vec2::new(2.0, 0.5), Vec2::new(4.0, 0.5)];
        let dense = densify_polyline(&points, ROAD_VISUAL_SAMPLE_SPACING);
        assert_eq!(dense.first(), points.first());
        assert_eq!(dense.last(), points.last());
        assert!(dense
            .windows(2)
            .all(|pair| pair[0].distance(pair[1]) <= ROAD_VISUAL_SAMPLE_SPACING + 1e-4));
    }

    #[test]
    fn replicated_progress_is_coalesced_then_drawn_at_the_visual_cadence() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<VillageRoadAssets>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, update_village_road_visuals);
        let road_entity = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Oakmead".into(),
                builder: "Mara".into(),
                points: vec![
                    Vec2::ZERO,
                    Vec2::new(2.0, 0.0),
                    Vec2::new(4.0, 0.0),
                    Vec2::new(6.0, 0.0),
                ],
                built_through: 2,
                width: 2.6,
                reserved_width: 4.0,
                surface: default(),
                class: default(),
                stone_committed: 0,
            })
            .id();
        app.update();
        let visual = app.world().get::<VillageRoadVisual>(road_entity).unwrap();
        let initial_vertices = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&visual.mesh)
            .unwrap()
            .count_vertices();

        app.world_mut()
            .get_mut::<VillageRoad>(road_entity)
            .unwrap()
            .built_through = 3;
        app.update();
        let visual = app.world().get::<VillageRoadVisual>(road_entity).unwrap();
        assert!(visual.dirty);
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&visual.mesh)
                .unwrap()
                .count_vertices(),
            initial_vertices
        );

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f64(
                ROAD_VISUAL_UPDATE_INTERVAL_SECONDS,
            ));
        app.update();
        let visual = app.world().get::<VillageRoadVisual>(road_entity).unwrap();
        assert!(!visual.dirty);
        assert!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&visual.mesh)
                .unwrap()
                .count_vertices()
                > initial_vertices
        );
    }
}
