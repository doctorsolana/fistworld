//! assets systems.

use super::*;

/// Setup shared visual assets for procedural vehicles.
pub fn setup_vehicle_visual_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let def = shared::vehicle::vehicle_def(VehicleType::Car);

    // Materials match the previous per-car setup.
    let body_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.25, 0.45),
        metallic: 0.7,
        perceptual_roughness: 0.3,
        ..default()
    });
    let front_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.9, 0.2),
        emissive: bevy::color::LinearRgba::new(1.0, 1.2, 0.3, 1.0),
        ..default()
    });
    let rear_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.1, 0.1),
        emissive: bevy::color::LinearRgba::new(1.5, 0.2, 0.1, 1.0),
        ..default()
    });
    let wheel_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.1, 0.1),
        metallic: 0.0,
        perceptual_roughness: 0.9,
        ..default()
    });

    let body_length = def.wheel_base * 1.1;
    let body_width = def.track_width * 0.9;
    let body_height = 0.5;
    let wheel_radius = def.wheel_radius;

    let body_mesh = meshes.add(Cuboid::new(body_width, body_height, body_length));
    let front_mesh = meshes.add(Cuboid::new(body_width * 0.8, 0.15, 0.3));
    let rear_mesh = meshes.add(Cuboid::new(body_width * 0.9, 0.2, 0.15));
    let wheel_mesh = meshes.add(Cylinder::new(wheel_radius, 0.15));

    commands.insert_resource(CarVisualAssets {
        body_mesh,
        front_mesh,
        rear_mesh,
        wheel_mesh,
        body_material,
        front_material,
        rear_material,
        wheel_material,
    });
}
