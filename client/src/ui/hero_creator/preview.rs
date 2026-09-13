//! The creator's local, front-facing character diorama.
//!
//! It shares the main PBR view deliberately: a secondary opaque camera has
//! incompatible atmosphere bindings in this renderer. Its screen position is
//! derived after UI layout, before spatial propagation, from the real cutout.

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::ui::{ComputedUiRenderTargetInfo, UiGlobalTransform, UiSystems};

use super::HeroCreatorOpen;
use crate::camera_rts::CommanderCamera;
use crate::hero::control::SelectedOutfit;
use crate::hero::{
    spawn_character_scene_child, HeroAssets, HeroDressed, HeroFullRig, HeroManifest,
    HeroPreviewRig, HeroVisual,
};
use crate::states::GameState;

const PREVIEW_DISTANCE: f32 = 3.2;
const BACKDROP_DEPTH: f32 = 1.2;
const SURROUND_DEPTH: f32 = 3.0;
// The complete left page includes breathing room and its inset bottom caption.
const FRAME_SIZE: Vec2 = Vec2::new(2.25, 3.25);
const PLINTH_TOP: f32 = -0.86;
const PLINTH_HEIGHT: f32 = 0.20;
/// Put this on the full transparent left page, behind its inset bottom caption.
#[derive(Component)]
pub(super) struct CreatorPreviewPane;

#[derive(Component)]
struct PreviewSet;

#[derive(Component)]
struct PreviewShadowsReady;

#[derive(Component)]
struct PreviewLight {
    illuminance: f32,
}

#[derive(Resource, Default)]
struct PreviewEntities {
    set: Option<Entity>,
    backdrop: Option<Entity>,
    surround: Option<Entity>,
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<PreviewEntities>()
        .add_systems(
            Update,
            (setup_preview, tag_preview_shadows).run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            PostUpdate,
            follow_preview_pane
                .after(UiSystems::Layout)
                .before(TransformSystems::Propagate)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(OnExit(GameState::Playing), clear_preview);
}

#[allow(clippy::too_many_arguments)]
fn setup_preview(
    mut commands: Commands,
    open: Res<HeroCreatorOpen>,
    mut preview: ResMut<PreviewEntities>,
    sets: Query<(), With<PreviewSet>>,
    asset_server: Res<AssetServer>,
    manifest: Res<HeroManifest>,
    mut hero_assets: ResMut<HeroAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selected: Res<SelectedOutfit>,
) {
    if !open.0 || preview.set.is_some_and(|set| sets.contains(set)) {
        return;
    }

    let backdrop_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });
    let surround_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.075, 0.065, 0.057),
        unlit: true,
        ..default()
    });
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.27, 0.25),
        perceptual_roughness: 0.96,
        reflectance: 0.05,
        ..default()
    });
    let top_stone = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.96,
        reflectance: 0.05,
        ..default()
    });
    let contact = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.14, 0.10),
        unlit: true,
        ..default()
    });

    let mut backdrop = None;
    let mut surround = None;
    let set = commands
        .spawn((
            Name::new("creator-preview-stage"),
            PreviewSet,
            Transform::IDENTITY,
            Visibility::Hidden,
        ))
        .with_children(|set| {
            // The preview shares the scene camera, so this deepest opaque
            // board supplies the modal's quiet full-screen surround without
            // a translucent UI scrim dulling the character itself.
            surround = Some(
                set.spawn((
                    Name::new("creator-preview-surround"),
                    Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
                    MeshMaterial3d(surround_material),
                    NotShadowCaster,
                    NotShadowReceiver,
                    Transform::IDENTITY,
                ))
                .id(),
            );
            backdrop = Some(
                set.spawn((
                    Name::new("creator-preview-backdrop"),
                    Mesh3d(meshes.add(backdrop_mesh())),
                    MeshMaterial3d(backdrop_material),
                    NotShadowCaster,
                    NotShadowReceiver,
                    Transform::IDENTITY,
                ))
                .id(),
            );
            // Closed beveled stone blocks share a level contact plane with
            // the rig's authored foot origin. A lower plinth tier supports
            // every block; tiny recessed seams reveal that tier, never a void.
            set.spawn((
                Name::new("creator-preview-plinth-base"),
                Mesh3d(meshes.add(Cylinder::new(0.79, 0.06).mesh().resolution(16))),
                MeshMaterial3d(stone),
                NotShadowCaster,
                NotShadowReceiver,
                Transform::from_xyz(0.0, PLINTH_TOP - PLINTH_HEIGHT + 0.03, 0.0),
            ));
            set.spawn((
                Name::new("creator-preview-plinth-top"),
                Mesh3d(meshes.add(plinth_stone_mesh())),
                MeshMaterial3d(top_stone),
                NotShadowCaster,
                NotShadowReceiver,
                Transform::from_xyz(0.0, PLINTH_TOP, 0.0),
            ));
            // A quiet contact stain grounds the shoes without another shadow
            // map. It is embedded in the plinth, separated by only 1 mm.
            set.spawn((
                Name::new("creator-preview-foot-contact"),
                Mesh3d(meshes.add(Cylinder::new(0.30, 0.002).mesh().resolution(24))),
                MeshMaterial3d(contact),
                NotShadowCaster,
                NotShadowReceiver,
                Transform::from_xyz(0.0, PLINTH_TOP, 0.0).with_scale(Vec3::new(1.0, 1.0, 0.65)),
            ));
            // A camera-aligned studio key and fill use ordinary PBR without
            // punctual-light clustering. They have no world sun/fill markers
            // and cast no shadow maps; follow_preview_pane powers them only
            // while the opaque creator and its valid preview pane are open.
            for (name, position, color, illuminance) in [
                (
                    "creator-preview-key",
                    Vec3::new(-1.7, 1.65, 2.1),
                    Color::srgb(1.0, 0.99, 0.97),
                    110_000.0,
                ),
                (
                    "creator-preview-fill",
                    Vec3::new(1.2, 0.7, 1.2),
                    Color::srgb(0.80, 0.89, 1.0),
                    12_000.0,
                ),
            ] {
                set.spawn((
                    Name::new(name),
                    PreviewLight { illuminance },
                    DirectionalLight {
                        color,
                        illuminance: 0.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(position)
                        .looking_at(Vec3::new(0.0, 0.3, 0.0), Vec3::Y),
                ));
            }
            set.spawn((
                Name::new("creator-preview-rig"),
                HeroPreviewRig,
                HeroFullRig,
                HeroVisual::idle(),
                selected.0,
                front_facing_rig_transform(),
                Visibility::Inherited,
            ))
            .with_children(|rig| {
                spawn_character_scene_child(rig, &asset_server, &mut hero_assets, &manifest);
            });
        })
        .id();

    preview.set = Some(set);
    preview.backdrop = backdrop;
    preview.surround = surround;
}

fn front_facing_rig_transform() -> Transform {
    // The canonical character faces -Z; the camera is on the set's +Z side.
    // This is a viewing transform, not a correction to the character asset.
    Transform::from_xyz(0.0, PLINTH_TOP, 0.0)
        .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
}

/// The UI target is native resolution; the scene target can be downscaled.
/// Normalising the actual physical UI bounds avoids mixing those pixel grids.
fn pane_ndc_bounds(size: Vec2, transform: &UiGlobalTransform, target: Vec2) -> Option<Rect> {
    if !size.is_finite()
        || size.min_element() <= 1.0
        || !target.is_finite()
        || target.min_element() <= 1.0
    {
        return None;
    }
    let half = size * 0.5;
    let mut bounds = Rect::from_corners(Vec2::splat(f32::INFINITY), Vec2::splat(f32::INFINITY));
    bounds.max = Vec2::splat(f32::NEG_INFINITY);
    for corner in [
        -half,
        Vec2::new(half.x, -half.y),
        half,
        Vec2::new(-half.x, half.y),
    ] {
        let point = transform.transform_point2(corner);
        let ndc = (point / target * 2.0 - Vec2::ONE) * Vec2::new(1.0, -1.0);
        if !ndc.is_finite() {
            return None;
        }
        bounds.min = bounds.min.min(ndc);
        bounds.max = bounds.max.max(ndc);
    }
    Some(bounds)
}

#[derive(Debug)]
struct PaneFrame {
    anchor: Vec3,
    scale: f32,
    backdrop: Transform,
    surround: Transform,
}

/// Intersect projected pane corners with a view-space plane. This uses the
/// live projection, so FOV, aspect ratio and orthographic scale are respected.
fn plane_bounds(clip_from_view: Mat4, bounds: Rect, distance: f32) -> Option<(Vec3, Vec2)> {
    if !clip_from_view.is_finite() || clip_from_view.determinant().abs() < 1e-8 {
        return None;
    }
    let clip_center = clip_from_view * Vec4::new(0.0, 0.0, -distance, 1.0);
    if clip_center.w.abs() < 1e-6 {
        return None;
    }
    let depth = clip_center.z / clip_center.w;
    let view_from_clip = clip_from_view.inverse();
    let project = |xy: Vec2| {
        let p = view_from_clip * Vec4::new(xy.x, xy.y, depth, 1.0);
        p.truncate() / p.w
    };
    let a = project(bounds.min);
    let b = project(bounds.max);
    let center = project(bounds.center());
    let size = (b - a).truncate().abs();
    (center.is_finite() && size.is_finite() && size.min_element() > 0.0).then_some((center, size))
}

fn frame_in_pane(clip_from_view: Mat4, bounds: Rect) -> Option<PaneFrame> {
    let (anchor, size) = plane_bounds(clip_from_view, bounds, PREVIEW_DISTANCE)?;
    let scale = (size / FRAME_SIZE).min_element();
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let (board_center, board_size) = plane_bounds(
        clip_from_view,
        bounds,
        PREVIEW_DISTANCE + BACKDROP_DEPTH * scale,
    )?;
    let (surround_center, surround_size) = plane_bounds(
        clip_from_view,
        Rect::from_corners(Vec2::NEG_ONE, Vec2::ONE),
        PREVIEW_DISTANCE + SURROUND_DEPTH * scale,
    )?;
    Some(PaneFrame {
        anchor,
        scale,
        // Backdrop is deeper than the character. Project its corners at that
        // depth too, otherwise an off-centre pane exposes the world at one edge.
        backdrop: Transform::from_translation((board_center - anchor) / scale)
            .with_scale((board_size / (2.0 * scale)).extend(1.0)),
        surround: Transform::from_translation((surround_center - anchor) / scale)
            .with_scale((surround_size / (2.0 * scale)).extend(1.0)),
    })
}

#[allow(clippy::type_complexity)]
fn follow_preview_pane(
    open: Res<HeroCreatorOpen>,
    preview: Res<PreviewEntities>,
    panes: Query<
        (
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            &ComputedUiRenderTargetInfo,
        ),
        With<CreatorPreviewPane>,
    >,
    // The live commander camera is a root entity. Reading its local Transform
    // here sees this frame's orbit/cinematic update before propagation.
    cameras: Query<(&Camera, &Transform), (With<CommanderCamera>, Without<ChildOf>)>,
    mut transforms: Query<&mut Transform, Without<CommanderCamera>>,
    mut visibility: Query<&mut Visibility, With<PreviewSet>>,
    mut lights: Query<(&PreviewLight, &mut DirectionalLight)>,
) {
    let Some(set) = preview.set else {
        set_studio_lighting(&mut lights, false);
        return;
    };
    let Ok(mut visible) = visibility.get_mut(set) else {
        set_studio_lighting(&mut lights, false);
        return;
    };
    let frame = if open.0 {
        panes
            .single()
            .ok()
            .and_then(|(node, computed, ui_transform, target)| {
                if node.display == Display::None {
                    return None;
                }
                let bounds = pane_ndc_bounds(
                    computed.size(),
                    ui_transform,
                    target.physical_size().as_vec2(),
                )?;
                let (camera, camera_transform) = cameras.single().ok()?;
                if !camera.is_active {
                    return None;
                }
                Some((
                    frame_in_pane(camera.clip_from_view(), bounds)?,
                    *camera_transform,
                ))
            })
    } else {
        None
    };
    let Some((frame, camera)) = frame else {
        set_studio_lighting(&mut lights, false);
        visible.set_if_neq(Visibility::Hidden);
        return;
    };
    if let Ok(mut transform) = transforms.get_mut(set) {
        transform.set_if_neq(Transform {
            translation: camera.translation + camera.rotation * frame.anchor,
            rotation: camera.rotation,
            scale: Vec3::splat(frame.scale),
        });
    }
    if let Some(backdrop) = preview.backdrop {
        if let Ok(mut transform) = transforms.get_mut(backdrop) {
            transform.set_if_neq(frame.backdrop);
        }
    }
    if let Some(surround) = preview.surround {
        if let Ok(mut transform) = transforms.get_mut(surround) {
            transform.set_if_neq(frame.surround);
        }
    }
    set_studio_lighting(&mut lights, true);
    visible.set_if_neq(Visibility::Inherited);
}

fn set_studio_lighting(lights: &mut Query<(&PreviewLight, &mut DirectionalLight)>, enabled: bool) {
    // Zero power independently of inherited visibility on every invalid or
    // closed path. Stable open frames leave the light components untouched.
    for (source, mut light) in lights {
        let target = if enabled { source.illuminance } else { 0.0 };
        if light.illuminance != target {
            light.illuminance = target;
        }
    }
}

/// Once the ordinary dresser has seen the complete scene, tag every mesh,
/// including hidden wardrobe alternatives. No fixed primitive count and no
/// perpetual subtree walk; a later session creates a fresh tagged rig.
fn tag_preview_shadows(
    mut commands: Commands,
    rigs: Query<
        Entity,
        (
            With<HeroPreviewRig>,
            With<HeroDressed>,
            Without<PreviewShadowsReady>,
        ),
    >,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
) {
    for rig in &rigs {
        for entity in children.iter_descendants(rig) {
            if meshes.contains(entity) {
                commands
                    .entity(entity)
                    .insert((NotShadowCaster, NotShadowReceiver));
            }
        }
        commands.entity(rig).insert(PreviewShadowsReady);
    }
}

fn clear_preview(
    mut commands: Commands,
    mut preview: ResMut<PreviewEntities>,
    sets: Query<Entity, With<PreviewSet>>,
) {
    for set in &sets {
        commands.entity(set).despawn();
    }
    *preview = PreviewEntities::default();
}

/// Sixteen individually beveled stones around a level central cap. All
/// pieces are closed; seams are geometry, not dark coplanar overlay strips.
fn plinth_stone_mesh() -> Mesh {
    let mut mesh = StoneMesh::default();
    let point =
        |radius: f32, angle: f32, y: f32| Vec3::new(radius * angle.cos(), y, radius * angle.sin());
    let boundary =
        |i: usize| i as f32 * std::f32::consts::TAU / 16.0 + ((i % 16) as f32 * 2.31).sin() * 0.014;
    for i in 0..16 {
        let a = boundary(i) + 0.016;
        let b = boundary(i + 1) - 0.016;
        let radius = 0.755 + (i as f32 * 2.399).sin() * 0.018;
        let color = Vec3::new(0.445, 0.45, 0.46) * (0.96 + (i as f32 * 1.73).sin() * 0.14);
        let mut rings = [[Vec3::ZERO; 4]; 3];
        for (ring, (inner, outer, y)) in rings.iter_mut().zip([
            (0.44, radius - 0.038, 0.0),
            (0.425, radius, -0.036),
            (0.425, radius - 0.008, -0.142),
        ]) {
            *ring = [
                point(inner, a, y),
                point(inner, b, y),
                point(outer, b, y),
                point(outer, a, y),
            ];
        }
        let center = point((0.425 + radius) * 0.5, (a + b) * 0.5, -0.07);
        mesh.face(&rings[0], center, color);
        mesh.face(&rings[2], center, color * 0.94);
        for band in 0..2 {
            for j in 0..4 {
                let next = (j + 1) % 4;
                mesh.face(
                    &[
                        rings[band][j],
                        rings[band + 1][j],
                        rings[band + 1][next],
                        rings[band][next],
                    ],
                    center,
                    color
                        * if band == 0 {
                            1.08
                        } else if j == 2 {
                            0.82
                        } else {
                            0.88
                        },
                );
            }
        }
    }
    let cap: Vec<_> = (0..16).map(|i| point(0.435, boundary(i), 0.0)).collect();
    let bottom: Vec<_> = cap.iter().map(|p| *p - Vec3::Y * 0.142).collect();
    let center = Vec3::new(0.0, -0.07, 0.0);
    let color = Vec3::new(0.435, 0.44, 0.45);
    mesh.face(&cap, center, color);
    mesh.face(&bottom, center, color);
    for i in 0..16 {
        let next = (i + 1) % 16;
        mesh.face(&[cap[i], bottom[i], bottom[next], cap[next]], center, color);
    }
    mesh.finish()
}

#[derive(Default)]
struct StoneMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl StoneMesh {
    fn face(&mut self, points: &[Vec3], interior: Vec3, rgb: Vec3) {
        let center = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize();
        let reverse = normal.dot(center - interior) < 0.0;
        let normal = if reverse { -normal } else { normal };
        let color = Color::srgb(rgb.x, rgb.y, rgb.z).to_linear().to_f32_array();
        let first = self.positions.len() as u32;
        for i in 0..points.len() {
            self.positions
                .push(points[if reverse { points.len() - 1 - i } else { i }].to_array());
            self.normals.push(normal.to_array());
            self.colors.push(color);
        }
        for i in 1..points.len() as u32 - 1 {
            self.indices.extend([first, first + i, first + i + 1]);
        }
    }

    fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// One opaque vertex-colour board: a soft warm halo, no bitmap or shader.
fn backdrop_mesh() -> Mesh {
    const X: usize = 12;
    const Y: usize = 16;
    let mut positions = Vec::with_capacity((X + 1) * (Y + 1));
    let mut colors = Vec::with_capacity(positions.capacity());
    let mut indices = Vec::with_capacity(X * Y * 6);
    for y in 0..=Y {
        for x in 0..=X {
            let p = Vec2::new(x as f32 / X as f32, y as f32 / Y as f32) * 2.0 - Vec2::ONE;
            let glow = (-((p.x * 1.55).powi(2) + ((p.y - 0.10) * 1.1).powi(2)) * 1.8).exp();
            let rgb = Vec3::new(0.115, 0.11, 0.085).lerp(Vec3::new(0.43, 0.35, 0.19), glow);
            positions.push([p.x, p.y, 0.0]);
            colors.push(Color::srgb(rgb.x, rgb.y, rgb.z).to_linear().to_f32_array());
            if x < X && y < Y {
                let a = (y * (X + 1) + x) as u32;
                let b = a + 1;
                let c = a + (X + 1) as u32;
                indices.extend([a, b, c, b, c + 1, c]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0.0, 0.0, 1.0]; positions.len()],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_tracks_real_pane_position_scale_and_camera_projection() {
        for (size, target, center) in [
            (
                Vec2::new(420.0, 390.0),
                Vec2::new(1600.0, 900.0),
                Vec2::new(505.0, 440.0),
            ),
            (
                Vec2::new(840.0, 780.0),
                Vec2::new(3200.0, 1800.0),
                Vec2::new(1010.0, 880.0),
            ),
            (
                Vec2::new(336.0, 312.0),
                Vec2::new(1280.0, 720.0),
                Vec2::new(404.0, 352.0),
            ),
        ] {
            let ui_transform = UiGlobalTransform::from_translation(center);
            let bounds = pane_ndc_bounds(size, &ui_transform, target).unwrap();
            for projection in [
                Mat4::perspective_infinite_reverse_rh(
                    45_f32.to_radians(),
                    target.x / target.y,
                    0.1,
                ),
                Mat4::perspective_infinite_reverse_rh(
                    70_f32.to_radians(),
                    target.x / target.y,
                    0.1,
                ),
                Mat4::orthographic_rh(-8.0, 8.0, -4.5, 4.5, 100.0, 0.1),
            ] {
                let frame = frame_in_pane(projection, bounds).unwrap();
                let clip = projection * frame.anchor.extend(1.0);
                assert!((clip.truncate().truncate() / clip.w - bounds.center()).length() < 1e-5);
                // The board covers this exact pane even when deeper and off-centre.
                for local in [Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 0.0)] {
                    let view = frame.anchor + frame.backdrop.transform_point(local) * frame.scale;
                    let clip = projection * view.extend(1.0);
                    let projected = clip.truncate().truncate() / clip.w;
                    let expected = if local.x < 0.0 {
                        bounds.min
                    } else {
                        bounds.max
                    };
                    assert!((projected - expected).length() < 1e-5);
                    let view = frame.anchor + frame.surround.transform_point(local) * frame.scale;
                    let clip = projection * view.extend(1.0);
                    assert!(
                        (clip.truncate().truncate() / clip.w - local.truncate()).length() < 1e-5
                    );
                }
            }
        }
    }

    #[test]
    fn idle_character_faces_the_viewer_and_stands_on_the_plinth() {
        let rig = front_facing_rig_transform();
        assert!((rig.rotation * Vec3::NEG_Z - Vec3::Z).length() < 1e-5);
        assert_eq!(rig.translation.y, PLINTH_TOP);
        assert_eq!(HeroVisual::idle().speed(), 0.0);
    }

    #[test]
    fn plinth_stones_are_closed_and_rest_on_the_supporting_tier() {
        use std::collections::BTreeMap;
        let mesh = plinth_stone_mesh();
        let vertices = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let indices: Vec<_> = mesh.indices().unwrap().iter().collect();
        assert!(indices.len() / 3 <= 400);
        let mut edges = BTreeMap::new();
        let mut top_area = 0.0;
        for triangle in indices.chunks_exact(3) {
            let p =
                [triangle[0], triangle[1], triangle[2]].map(|index| Vec3::from(vertices[index]));
            let cross = (p[1] - p[0]).cross(p[2] - p[0]);
            assert!(cross.is_finite() && cross.length() > 1e-7);
            if p.iter().all(|point| point.y == 0.0) {
                assert!(cross.y > 0.0, "level foot surface must face upwards");
                top_area += cross.y * 0.5;
            }
            for edge in [
                [triangle[0], triangle[1]],
                [triangle[1], triangle[2]],
                [triangle[2], triangle[0]],
            ] {
                let mut key = [
                    vertices[edge[0]].map(f32::to_bits),
                    vertices[edge[1]].map(f32::to_bits),
                ];
                key.sort();
                *edges.entry(key).or_insert(0) += 1;
            }
        }
        assert!(
            edges.values().all(|count| *count == 2),
            "stone shells must have no open edges"
        );
        assert!(top_area > 1.4, "retain a broad level standing surface");
        for point in vertices {
            let p = Vec3::from(*point);
            assert!(p.xz().length() <= 0.79);
            assert!(p.y <= 0.0 && p.y >= -PLINTH_HEIGHT);
        }
        let bottom = vertices.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        assert!(
            bottom <= -PLINTH_HEIGHT + 0.06,
            "stone blocks must meet the support tier"
        );
    }

    #[test]
    fn closed_or_unready_preview_cannot_leave_studio_lighting_on() {
        let mut app = App::new();
        app.insert_resource(HeroCreatorOpen(false))
            .init_resource::<PreviewEntities>()
            .add_systems(Update, follow_preview_pane);
        let studio = app
            .world_mut()
            .spawn((
                PreviewLight {
                    illuminance: 30_000.0,
                },
                DirectionalLight {
                    illuminance: 30_000.0,
                    ..default()
                },
                Visibility::Visible,
            ))
            .id();
        let world_sun = app
            .world_mut()
            .spawn(DirectionalLight {
                illuminance: 80_000.0,
                ..default()
            })
            .id();
        for open in [false, true] {
            app.world_mut().resource_mut::<HeroCreatorOpen>().0 = open;
            app.world_mut()
                .get_mut::<DirectionalLight>(studio)
                .unwrap()
                .illuminance = 30_000.0;
            app.update();
            assert_eq!(
                app.world()
                    .get::<DirectionalLight>(studio)
                    .unwrap()
                    .illuminance,
                0.0
            );
            assert_eq!(
                app.world()
                    .get::<DirectionalLight>(world_sun)
                    .unwrap()
                    .illuminance,
                80_000.0
            );
        }
    }

    #[test]
    fn unlaid_out_panes_and_invalid_projections_remain_hidden() {
        assert!(pane_ndc_bounds(
            Vec2::ZERO,
            &UiGlobalTransform::default(),
            Vec2::splat(1000.0)
        )
        .is_none());
        assert!(pane_ndc_bounds(
            Vec2::splat(400.0),
            &UiGlobalTransform::default(),
            Vec2::ZERO
        )
        .is_none());
        assert!(frame_in_pane(Mat4::ZERO, Rect::from_corners(Vec2::ZERO, Vec2::ONE)).is_none());
    }
}
