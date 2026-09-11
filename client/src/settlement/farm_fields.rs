//! Cached, terrain-following wheat parcels from authoritative row boundaries.
//! Three batched meshes per worker area: soil, close stalks and coarse distant grain.
//! No per-stalk entities, textures, collisions or simulation. Standard Bevy
//! visibility ranges cross-fade the two crop representations on the GPU.

#[path = "farm_fields/ground.rs"]
mod ground;

use ground::FieldGround;

use crate::settlement::farm_wind::{CropWind, CropWindMaterial};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{primitives::MeshAabb, visibility::VisibilityRange};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use shared::components::{FarmField, FarmFieldShape, PlayerPosition, PlayerRotation};
use shared::terrain::{ChunkCoord, WorldTerrain};

/// Present only once the accepted field's procedural meshes have been created.
#[derive(Component)]
pub struct FarmFieldVisual {
    shape: Option<FarmFieldShape>,
    position: Vec3,
    rotation: f32,
    farmstead: Vec3,
    layout_version: u8,
    terrain: FieldTerrainStamp,
    /// Soil, near crop and distant crop counts for capture/performance evidence.
    pub triangle_counts: [usize; 3],
}

impl FarmFieldVisual {
    /// Readiness includes geometry freshness. Name/quality changes do not rebuild
    /// meshes, while accepted parcel edits and terrain earthworks do.
    pub fn matches(
        &self,
        field: &FarmField,
        position: Vec3,
        rotation: f32,
        terrain: &WorldTerrain,
    ) -> bool {
        self.shape == field.shape
            && self.position == position
            && self.rotation == rotation
            && self.farmstead == field.farmstead
            && self.layout_version == field.layout_version
            && self.terrain.is_current(terrain)
    }
}

/// Exact revisions for the few terrain chunks touched by this rotated parcel.
/// Bounds are computed once with geometry, never by walking every crop each
/// frame. Chunk revisions already include height-sampling boundary neighbours.
struct FieldTerrainStamp {
    full_rebuild: u32,
    chunks: Vec<(ChunkCoord, u32)>,
}

impl FieldTerrainStamp {
    fn new(shape: &FarmFieldShape, origin: Vec3, yaw: f32, terrain: &WorldTerrain) -> Self {
        let mut stamp = Self {
            full_rebuild: terrain.full_rebuild_version(),
            chunks: Vec::new(),
        };
        if !shape.is_valid() {
            return stamp;
        }
        let mut minimum = Vec2::splat(f32::INFINITY);
        let mut maximum = Vec2::splat(f32::NEG_INFINITY);
        for section in &shape.sections {
            for x in [section.left, section.right] {
                let point =
                    origin.xz() + shared::rotation::local_to_world_xz(Vec2::new(x, section.z), yaw);
                minimum = minimum.min(point);
                maximum = maximum.max(point);
            }
        }
        minimum -= Vec2::splat(0.5);
        maximum += Vec2::splat(0.5);
        let lo = ChunkCoord::from_world_pos(Vec3::new(minimum.x, 0., minimum.y));
        let hi = ChunkCoord::from_world_pos(Vec3::new(maximum.x, 0., maximum.y));
        for x in lo.x..=hi.x {
            for z in lo.z..=hi.z {
                let coord = ChunkCoord::new(x, z);
                stamp
                    .chunks
                    .push((coord, terrain.chunk_modification_version(coord)));
            }
        }
        stamp
    }

    fn is_current(&self, terrain: &WorldTerrain) -> bool {
        self.full_rebuild == terrain.full_rebuild_version()
            && self
                .chunks
                .iter()
                .all(|(coord, revision)| terrain.chunk_modification_version(*coord) == *revision)
    }
}

#[derive(Component)]
pub(crate) struct FieldMeshSet(Vec<Entity>);

const ROW_SPACING: f32 = 0.46;
const CLUMP_SPACING: f32 = 0.53;
const NEAR_CLUMP_RADIUS: f32 = 0.42;
const FAR_CLUMP_RADIUS: f32 = 0.42;
const CROP_INSET: f32 = 0.20;

pub(in crate::settlement) fn attach_farm_field_visuals(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
    mut crop_materials: ResMut<Assets<CropWindMaterial>>,
    mut crop_material: Local<Option<Handle<CropWindMaterial>>>,
    mut cursor: Local<usize>,
    fields: Query<(
        Entity,
        &FarmField,
        &PlayerPosition,
        &PlayerRotation,
        Option<&FieldMeshSet>,
        Option<&FarmFieldVisual>,
    )>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    if fields.is_empty() {
        return;
    }
    let material = material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                perceptual_roughness: 0.98,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        })
        .clone();
    let crop_material = crop_material
        .get_or_insert_with(|| {
            crop_materials.add(CropWindMaterial {
                base: StandardMaterial {
                    perceptual_roughness: 0.98,
                    cull_mode: None,
                    // Crop normals describe the top of a bunch of grain, not
                    // a dark upright card. Keep that canopy normal on both
                    // visible faces; geometry is still rendered double-sided.
                    double_sided: false,
                    ..default()
                },
                extension: CropWind::default(),
            })
        })
        .clone();
    let count = fields.iter().count();
    let start = *cursor % count;
    let mut rebuilt = 0;
    for (offset, (entity, field, position, rotation, previous, visual)) in fields
        .iter()
        .skip(start)
        .chain(fields.iter().take(start))
        .enumerate()
    {
        if visual.is_some_and(|visual| visual.matches(field, position.0, rotation.0, &terrain)) {
            continue;
        }
        if let Some(previous) = previous {
            for child in &previous.0 {
                commands.entity(*child).despawn();
            }
        }
        let shape = field
            .shape
            .clone()
            .unwrap_or_else(FarmFieldShape::legacy_rectangle);
        let mut children = Vec::with_capacity(3);
        let mut triangle_counts = [0; 3];
        if shape.is_valid() {
            let generated = build_meshes(&shape, &field, position.0, rotation.0, &terrain);
            for (index, mesh) in generated.into_iter().enumerate() {
                triangle_counts[index] = mesh.count_vertices() / 3;
                if mesh.count_vertices() == 0 {
                    continue;
                }
                let range = match index {
                    0 => VisibilityRange {
                        start_margin: 0.0..0.0,
                        end_margin: 950.0..1050.0,
                        use_aabb: false,
                    },
                    1 => VisibilityRange {
                        start_margin: 0.0..0.0,
                        end_margin: 115.0..150.0,
                        use_aabb: false,
                    },
                    _ => VisibilityRange {
                        start_margin: 115.0..150.0,
                        end_margin: 950.0..1050.0,
                        use_aabb: false,
                    },
                };
                let mut bounds = mesh.compute_aabb();
                if index != 0 {
                    if let Some(bounds) = &mut bounds {
                        bounds.half_extents += bevy::math::Vec3A::new(0.11, 0.005, 0.11);
                    }
                }
                let mut child = commands.spawn((
                    Name::new(["Field soil", "Wheat stalks", "Wheat rows (distant)"][index]),
                    Mesh3d(meshes.add(mesh)),
                    Transform::default(),
                    Visibility::Inherited,
                    range,
                    ChildOf(entity),
                ));
                if let Some(bounds) = bounds {
                    child.insert(bounds);
                }
                if index == 0 {
                    child.insert(MeshMaterial3d(material.clone()));
                } else {
                    child.insert(MeshMaterial3d(crop_material.clone()));
                }
                children.push(child.id());
            }
        }
        commands.entity(entity).insert((
            FarmFieldVisual {
                shape: field.shape.clone(),
                position: position.0,
                rotation: rotation.0,
                farmstead: field.farmstead,
                layout_version: field.layout_version,
                terrain: FieldTerrainStamp::new(&shape, position.0, rotation.0, &terrain),
                triangle_counts,
            },
            FieldMeshSet(children),
            Name::new(format!(
                "Wheat parcel {} ({})",
                field.plot_index + 1,
                field.settlement
            )),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
        rebuilt += 1;
        if rebuilt == 2 {
            *cursor = (start + offset + 1) % count;
            break;
        }
    }
}

#[derive(Clone, Default)]
struct FieldMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    wind_weights: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl FieldMesh {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let start = self.positions.len() as u32;
        let color = Color::srgb(color.x, color.y, color.z).to_linear();
        for p in [a, b, c] {
            self.positions.push(p.to_array());
            self.normals.push(normal.to_array());
            self.colors.push([color.red, color.green, color.blue, 1.0]);
            self.wind_weights.push([0.0, 0.0]);
        }
        self.indices.extend([start, start + 1, start + 2]);
    }
    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3) {
        self.triangle(a, b, c, color);
        self.triangle(a, c, d, color);
    }
    fn beam(&mut self, a: Vec3, b: Vec3, width: f32, depth: f32, color: Vec3) {
        let along = (b - a).normalize_or_zero();
        if along == Vec3::ZERO {
            return;
        }
        let reference = if along.y.abs() > 0.95 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let side = along.cross(reference).normalize() * width * 0.5;
        let up = side.normalize().cross(along) * depth * 0.5;
        let v = [
            a - side - up,
            a + side - up,
            a + side + up,
            a - side + up,
            b - side - up,
            b + side - up,
            b + side + up,
            b - side + up,
        ];
        let center = (a + b) * 0.5;
        for [ia, ib, ic, id] in [
            [0, 1, 2, 3],
            [4, 7, 6, 5],
            [0, 4, 5, 1],
            [1, 5, 6, 2],
            [2, 6, 7, 3],
            [3, 7, 4, 0],
        ] {
            let [a, b, c, d] = [v[ia], v[ib], v[ic], v[id]];
            if (b - a).cross(c - a).dot((a + b + c + d) * 0.25 - center) < 0. {
                self.quad(d, c, b, a, color);
            } else {
                self.quad(a, b, c, d, color);
            }
        }
    }
    fn weight_stalk(&mut self, first_vertex: usize, base: Vec3, height: f32, clearance: f32) {
        for normal in &mut self.normals[first_vertex..] {
            *normal = (Vec3::from_array(*normal) * 0.38 + Vec3::Y * 0.85)
                .normalize()
                .to_array();
        }
        for (position, weight) in self.positions[first_vertex..]
            .iter()
            .zip(&mut self.wind_weights[first_vertex..])
        {
            let h = ((position[1] - base.y) / height).clamp(0., 1.);
            weight[0] = h * h * clearance;
        }
    }

    fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            // The field marker keeps diagnostics; no CPU consumer needs the
            // generated vertices after upload. Avoid retaining a second copy
            // of every unique parcel mesh in a many-town world.
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.wind_weights)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn variation(x: f32, z: f32, salt: f32) -> f32 {
    // Immutable plant identity in farm-local space; adjoining worker halves
    // use the same row phase and palette instead of displaying a material seam.
    ((x * 12.9898 + z * 78.233 + salt).sin() * 43758.547)
        .fract()
        .abs()
}

fn build_meshes(
    shape: &FarmFieldShape,
    field: &FarmField,
    origin: Vec3,
    rotation: f32,
    terrain: &WorldTerrain,
) -> [Mesh; 3] {
    let ground = FieldGround::new(shape, origin, rotation, terrain);
    let farm_offset =
        shared::rotation::world_to_local_xz(origin.xz() - field.farmstead.xz(), rotation);
    let point = |p: Vec2, lift: f32| {
        let world = origin.xz() + shared::rotation::local_to_world_xz(p, rotation);
        Vec3::new(p.x, ground.height(world) - origin.y + lift, p.y)
    };
    // One flush soil mesh, clipped against the actual rendered terrain
    // triangles. No duplicate flat crop layer, raised tray or LOD ground seam.
    let mut soil = ground.soil(shape, origin, rotation, farm_offset);
    let mut far = FieldMesh::default();
    let mut near = FieldMesh::default();
    let first = shape.sections[0].z + CROP_INSET;
    let last = shape.sections.last().unwrap().z - CROP_INSET;
    let mut z = ((first + farm_offset.y) / ROW_SPACING).ceil() * ROW_SPACING - farm_offset.y;
    while z <= last {
        let Some((left, right)) = shape.span_at(z) else {
            break;
        };
        let row = ((z + farm_offset.y) / ROW_SPACING).round() as i32;
        let stagger = if row.rem_euclid(2) == 0 {
            0.
        } else {
            CLUMP_SPACING * 0.5
        };
        let mut x = ((left + CROP_INSET + farm_offset.x - stagger) / CLUMP_SPACING).ceil()
            * CLUMP_SPACING
            - farm_offset.x
            + stagger;
        while x <= right - CROP_INSET {
            let v = variation(x + farm_offset.x, z + farm_offset.y, 13.0);
            let sideways = variation(x + farm_offset.x, z + farm_offset.y, 29.0);
            let p = Vec2::new(x + (sideways - 0.5) * 0.22, z + (v - 0.5) * 0.20);
            if crop_clearance(shape, p, farm_offset) >= NEAR_CLUMP_RADIUS {
                let first_vertex = near.positions.len();
                let base = point(p, 0.010);
                let height = 1.04 + v * 0.27;
                wheat_cluster(
                    &mut near,
                    base,
                    height,
                    sideways * std::f32::consts::TAU,
                    Vec3::new(1.0, 0.855, 0.49) * (0.96 + v * 0.04),
                );
                near.weight_stalk(
                    first_vertex,
                    base,
                    height,
                    wind_clearance(shape, p, farm_offset, NEAR_CLUMP_RADIUS),
                );
            }
            x += CLUMP_SPACING;
        }
        z += ROW_SPACING;
    }
    let mut z = ((first + farm_offset.y) / 0.82).ceil() * 0.82 - farm_offset.y;
    while z <= last {
        let Some((left, right)) = shape.span_at(z) else {
            break;
        };
        let row = ((z + farm_offset.y) / 0.82).round() as i32;
        let stagger = if row.rem_euclid(2) == 0 { 0. } else { 0.4 };
        let mut x = ((left + FAR_CLUMP_RADIUS + farm_offset.x - stagger) / 0.80).ceil() * 0.80
            - farm_offset.x
            + stagger;
        while x <= right - FAR_CLUMP_RADIUS {
            let v = variation(x + farm_offset.x, z + farm_offset.y, 13.0);
            let sideways = variation(x + farm_offset.x, z + farm_offset.y, 29.0);
            let p = Vec2::new(x + (sideways - 0.5) * 0.18, z + (v - 0.5) * 0.16);
            if crop_clearance(shape, p, farm_offset) >= FAR_CLUMP_RADIUS {
                let first_vertex = far.positions.len();
                let base = point(p, 0.010);
                let height = 1.04 + v * 0.27;
                let gold = Vec3::new(1.0, 0.835, 0.455) * (0.96 + v * 0.04);
                distant_grain(
                    &mut far,
                    base,
                    height,
                    sideways * std::f32::consts::TAU,
                    gold,
                );
                far.weight_stalk(
                    first_vertex,
                    base,
                    height,
                    wind_clearance(shape, p, farm_offset, FAR_CLUMP_RADIUS),
                );
            }
            x += 0.80;
        }
        z += 0.82;
    }
    for (a, b) in field.fence_segments(origin, rotation) {
        let bays = (a.distance(b) / 2.4).ceil().max(1.) as usize;
        for i in 0..=bays {
            let p = a.lerp(b, i as f32 / bays as f32);
            let root = point(p, -0.08);
            let v = variation(p.x + farm_offset.x, p.y + farm_offset.y, 71.);
            let top = point(p, shared::components::FARM_FENCE_HEIGHT + v * 0.06);
            soil.beam(
                root,
                top,
                0.15,
                0.15,
                Vec3::new(0.39, 0.27, 0.145) * (0.92 + v * 0.13),
            );
            if i == bays {
                continue;
            }
            let next = a.lerp(b, (i + 1) as f32 / bays as f32);
            for h in [0.39, 0.76] {
                soil.beam(
                    point(p, h),
                    point(next, h),
                    0.105,
                    0.12,
                    Vec3::new(0.46, 0.335, 0.19) * (0.94 + v * 0.08),
                );
            }
        }
    }
    [soil.finish(), near.finish(), far.finish()]
}

/// Reserve the complete static clump radius plus the shader's maximum sway.
/// This is evaluated once per clump during a bounded mesh rebuild, not per frame.
fn wind_clearance(shape: &FarmFieldShape, p: Vec2, farm_offset: Vec2, radius: f32) -> f32 {
    ((crop_clearance(shape, p, farm_offset) - radius) / 0.11).clamp(0., 1.)
}

fn crop_clearance(shape: &FarmFieldShape, p: Vec2, farm_offset: Vec2) -> f32 {
    if !shape.contains_local_point(p, 0.) || !planted(p, farm_offset) {
        return 0.;
    }
    let distance_to = |a: Vec2, b: Vec2| {
        let edge = b - a;
        let t = ((p - a).dot(edge) / edge.length_squared().max(0.0001)).clamp(0., 1.);
        p.distance(a + edge * t)
    };
    let mut clearance = f32::INFINITY;
    for pair in shape.sections.windows(2) {
        for (a, b) in [(pair[0].left, pair[1].left), (pair[0].right, pair[1].right)] {
            clearance = clearance.min(distance_to(
                Vec2::new(a, pair[0].z),
                Vec2::new(b, pair[1].z),
            ));
        }
    }
    for end in [
        shape.sections.first().unwrap(),
        shape.sections.last().unwrap(),
    ] {
        clearance = clearance.min(distance_to(
            Vec2::new(end.left, end.z),
            Vec2::new(end.right, end.z),
        ));
    }
    let farm_p = p + farm_offset;
    let entrance_distance =
        Vec2::new((farm_p.x.abs() - 0.65).max(0.), (farm_p.y - 8.).max(0.)).length();
    clearance.min(entrance_distance)
}

fn planted(point: Vec2, farm_offset: Vec2) -> bool {
    (point.x + farm_offset.x).abs() > 0.65 || point.y + farm_offset.y > 8.0
}

/// Two bent grain heads and two folded straw leaves: twelve triangles per
/// clump. Slender five-point ears sit above the lower leaves, which preserve a
/// full planted silhouette without replacing the crop with a flat canopy.
fn wheat_cluster(mesh: &mut FieldMesh, base: Vec3, height: f32, angle: f32, color: Vec3) {
    let bend = Vec3::new((angle + 0.55).cos(), 0., (angle + 0.55).sin()) * 0.105;
    for (index, phase) in [0., std::f32::consts::FRAC_PI_2].into_iter().enumerate() {
        let side = Vec3::new((angle + phase).cos(), 0., (angle + phase).sin());
        let base = base + side * (index as f32 - 0.5) * 0.08;
        let height = height * (1. - index as f32 * 0.075);
        let lower = base + Vec3::Y * (height * 0.67) + bend * 0.62;
        let shoulder = base + Vec3::Y * (height * 0.96) + bend;
        let tip = base + Vec3::Y * (height * 1.08) + bend * 1.20;
        mesh.triangle(
            base - side * 0.017,
            base + side * 0.017,
            shoulder,
            color * Vec3::new(0.74, 0.79, 0.61),
        );
        // The blade rises out of the stem, folds across a narrow middle and
        // droops to a pointed tip. Width crosses the stem's vertical plane so
        // the individual angled leaves remain visible in steep RTS views;
        // they never join into a flat canopy over the field.
        let across = side.cross(Vec3::Y) * 0.07;
        let leaf_root = base + Vec3::Y * (height * 0.44) + bend * 0.20;
        let leaf_upper = base + Vec3::Y * (height * 0.65) - side * 0.16 + bend * 0.40 + across;
        let leaf_lower = base + Vec3::Y * (height * 0.60) - side * 0.16 + bend * 0.40 - across;
        let leaf_tip = base + Vec3::Y * (height * 0.54) - side * 0.36 + bend * 0.50;
        mesh.triangle(
            leaf_root,
            leaf_upper,
            leaf_lower,
            color * Vec3::new(0.94, 0.91, 0.82),
        );
        mesh.triangle(
            leaf_upper,
            leaf_tip,
            leaf_lower,
            color * Vec3::new(0.98, 0.94, 0.85),
        );
        let lo_left = lower - side * 0.036;
        let lo_right = lower + side * 0.036;
        let up_left = shoulder - side * 0.066;
        let up_right = shoulder + side * 0.066;
        mesh.triangle(lo_left, up_left, tip, color * 0.96);
        mesh.triangle(lo_left, tip, up_right, color);
        mesh.triangle(lo_left, up_right, lo_right, color * 0.98);
    }
}

/// Six triangles represent a small bunch at distance: two rooted stems and
/// two broad, uneven vertical grain ribbons. There is no horizontal opaque
/// canopy. The larger projected coverage preserves the gold mass at town zoom.
fn distant_grain(mesh: &mut FieldMesh, base: Vec3, height: f32, angle: f32, color: Vec3) {
    for (index, phase) in [0., std::f32::consts::FRAC_PI_2].into_iter().enumerate() {
        let side = Vec3::new((angle + phase).cos(), 0., (angle + phase).sin());
        let height = height * (1. - index as f32 * 0.055);
        let lean = side * 0.035;
        let shoulder = base + Vec3::Y * (height * 0.83) + lean;
        mesh.triangle(
            base - side * 0.035,
            base + side * 0.035,
            shoulder,
            color * 0.76,
        );
        mesh.quad(
            base + Vec3::Y * (height * 0.55) - side * 0.255,
            base + Vec3::Y * (height * 1.02) - side * 0.272 + lean,
            base + Vec3::Y * (height * 0.96) + side * 0.306 + lean,
            base + Vec3::Y * (height * 0.59) + side * 0.264,
            color * (1. - index as f32 * 0.025),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_readiness_tracks_geometry_but_ignores_production_quality() {
        let mut field = FarmField {
            settlement: "Test".into(),
            farmstead: Vec3::ZERO,
            plot_index: 0,
            layout_version: 0,
            quality: 1.,
            shape: Some(FarmFieldShape::legacy_rectangle()),
        };
        let terrain = WorldTerrain::default();
        let marker = FarmFieldVisual {
            shape: field.shape.clone(),
            position: Vec3::ZERO,
            rotation: 0.,
            farmstead: field.farmstead,
            layout_version: field.layout_version,
            terrain: FieldTerrainStamp::new(
                field.shape.as_ref().unwrap(),
                Vec3::ZERO,
                0.,
                &terrain,
            ),
            triangle_counts: [0; 3],
        };
        field.quality = 0.5;
        assert!(marker.matches(&field, Vec3::ZERO, 0., &terrain));
        assert!(!marker.matches(&field, Vec3::X, 0., &terrain));
        field.farmstead = Vec3::X;
        assert!(!marker.matches(&field, Vec3::ZERO, 0., &terrain));
        field.farmstead = Vec3::ZERO;
        field.shape.as_mut().unwrap().sections[0].right -= 1.;
        assert!(!marker.matches(&field, Vec3::ZERO, 0., &terrain));
    }

    #[test]
    fn explicit_crop_wind_masks_keep_roots_and_borders_fixed() {
        let shape = FarmFieldShape::legacy_rectangle();
        let offset = Vec2::new(0., 9.);
        assert_eq!(wind_clearance(&shape, Vec2::new(3.9, 2.), offset, 0.27), 0.);
        assert_eq!(
            wind_clearance(&shape, Vec2::new(0.7, -2.), offset, 0.27),
            0.
        );
        assert_eq!(wind_clearance(&shape, Vec2::new(2., 2.), offset, 0.27), 1.);
        let base = Vec3::new(2., 19., 2.);
        let mut mesh = FieldMesh::default();
        wheat_cluster(&mut mesh, base, 1.2, 0.7, Vec3::ONE);
        mesh.weight_stalk(0, base, 1.2, 1.);
        for (p, weight) in mesh.positions.iter().zip(&mesh.wind_weights) {
            assert!((0.0..=1.0).contains(&weight[0]));
            if (p[1] - base.y).abs() < 0.001 {
                assert_eq!(weight[0], 0.);
            }
        }
        assert!(mesh.wind_weights.iter().any(|w| w[0] > 0.9));
        assert_eq!(mesh.positions.len() / 3, 12);
        assert!(
            mesh.normals.iter().all(|n| n[1] > 0.8),
            "cheap crop cards must retain golden canopy lighting from both views"
        );
        let mut far = FieldMesh::default();
        distant_grain(&mut far, base, 1.2, 0.7, Vec3::ONE);
        far.weight_stalk(0, base, 1.2, 1.);
        assert_eq!(far.positions.len() / 3, 6);
        assert!(far.positions.iter().any(|p| (p[1] - base.y).abs() < 0.001));
        for (p, w) in far.positions.iter().zip(&far.wind_weights) {
            if (p[1] - base.y).abs() < 0.001 {
                assert_eq!(w[0], 0.);
            }
            assert!(Vec2::new(p[0] - base.x, p[2] - base.z).length() <= FAR_CLUMP_RADIUS);
        }
    }

    #[test]
    fn crop_terrain_stamp_ignores_remote_earthworks_and_tracks_local_replacement() {
        let shape = FarmFieldShape::legacy_rectangle();
        let origin = Vec3::new(64., 0., 64.);
        let yaw = 0.73;
        let mut terrain = WorldTerrain::default();
        let stamp = FieldTerrainStamp::new(&shape, origin, yaw, &terrain);
        assert!(stamp.is_current(&terrain));
        terrain.apply_flatten_rect(Vec3::new(1000., 80., 1000.), Vec2::splat(20.), 0., 2.);
        assert!(
            stamp.is_current(&terrain),
            "remote town grading must keep these crop meshes"
        );
        terrain.apply_flatten_rect(origin + Vec3::Y * 80., Vec2::splat(3.), 0., 2.);
        assert!(
            !stamp.is_current(&terrain),
            "local grading must refresh the ground-following crop roots"
        );
        let rebuilt = FieldTerrainStamp::new(&shape, origin, yaw, &terrain);
        assert!(rebuilt.is_current(&terrain));
        terrain.replace_delta_chunks(terrain.delta_chunks().clone());
        assert!(
            !rebuilt.is_current(&terrain),
            "full-world replacement invalidates even matching local revision numbers"
        );
    }

    #[test]
    fn detailed_heads_and_distant_bunches_keep_triangle_density_bounded() {
        let old_near_density = 8.0 / (0.42 * 0.44);
        assert!(12.0 / (ROW_SPACING * CLUMP_SPACING) < old_near_density * 1.15);
        let mut near = FieldMesh::default();
        wheat_cluster(&mut near, Vec3::ZERO, 1.2, 0., Vec3::ONE);
        let mut far = FieldMesh::default();
        distant_grain(&mut far, Vec3::ZERO, 1.2, 0., Vec3::ONE);
        assert_eq!(near.positions.len() / 3, 12);
        assert_eq!(far.positions.len() / 3, 6);
        for angle in 0..16 {
            let mut crop = FieldMesh::default();
            wheat_cluster(
                &mut crop,
                Vec3::ZERO,
                1.31,
                angle as f32 * std::f32::consts::TAU / 16.,
                Vec3::ONE,
            );
            assert!(crop
                .positions
                .iter()
                .all(|p| Vec2::new(p[0], p[2]).length() <= NEAR_CLUMP_RADIUS));
        }
        // Far crops must remain rooted vertical growth, never a filled flat slab.
        assert!(far.normals.iter().all(|n| n[1].abs() < 0.001));
    }
}
