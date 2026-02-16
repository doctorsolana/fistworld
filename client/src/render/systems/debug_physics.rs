//! Client-side visuals for replicated debug physics boxes.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::primitives::Cuboid;
use bevy::prelude::*;
use shared::components::{DebugPhysicsBox, DebugPhysicsBoxPosition, DebugPhysicsBoxRotation};

#[derive(Resource, Clone)]
pub struct DebugPhysicsBoxAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub fn setup_debug_physics_box_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.55, 0.2, 0.8),
        emissive: Color::srgb(0.35, 0.18, 0.05).into(),
        metallic: 0.0,
        perceptual_roughness: 0.8,
        alpha_mode: AlphaMode::Opaque,
        ..default()
    });
    commands.insert_resource(DebugPhysicsBoxAssets { mesh, material });
}

pub fn spawn_debug_physics_box_visuals(
    mut commands: Commands,
    assets: Option<Res<DebugPhysicsBoxAssets>>,
    boxes: Query<
        (
            Entity,
            &DebugPhysicsBox,
            Option<&DebugPhysicsBoxPosition>,
            Option<&DebugPhysicsBoxRotation>,
        ),
        (With<DebugPhysicsBox>, Without<Mesh3d>),
    >,
) {
    let Some(assets) = assets else { return };
    for (entity, box_data, box_pos, box_rot) in boxes.iter() {
        let Some(box_pos) = box_pos else { continue };
        let Some(box_rot) = box_rot else { continue };
        commands.entity(entity).insert((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(box_pos.0)
                .with_rotation(box_rot.0)
                .with_scale(box_data.half_extents * 2.0),
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
            NoFrustumCulling,
        ));
        info!("Spawned debug box visual at {:?}", box_pos.0);
    }
}

pub fn sync_debug_physics_box_transforms(
    mut boxes: Query<(
        &DebugPhysicsBox,
        &DebugPhysicsBoxPosition,
        &DebugPhysicsBoxRotation,
        &mut Transform,
    )>,
) {
    for (box_data, box_pos, box_rot, mut transform) in boxes.iter_mut() {
        transform.translation = box_pos.0;
        transform.rotation = box_rot.0;
        transform.scale = box_data.half_extents * 2.0;
    }
}
