//! Procedural sword and shield models, registered as Scene assets so the
//! existing first-person and third-person weapon pipelines render them like
//! any GLB weapon. Built from primitives at real-world scale (blade ~1m,
//! shield ~0.7m diameter); the holders apply their usual view scales.

use bevy::prelude::*;

fn steel(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.72, 0.75, 0.80),
        metallic: 0.9,
        perceptual_roughness: 0.35,
        ..default()
    })
}

fn dark_steel(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.36, 0.40),
        metallic: 0.8,
        perceptual_roughness: 0.5,
        ..default()
    })
}

fn leather(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.2, 0.12),
        metallic: 0.0,
        perceptual_roughness: 0.9,
        ..default()
    })
}

fn wood(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.3, 0.17),
        metallic: 0.0,
        perceptual_roughness: 0.85,
        ..default()
    })
}

/// Sword modeled along -Z (blade forward), grip at the origin — the same
/// orientation convention as the gun GLBs (barrel toward -Z).
pub fn build_sword_scene(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Scene {
    let mut world = World::new();
    let steel = steel(materials);
    let dark = dark_steel(materials);
    let leather = leather(materials);

    // Grip
    world.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.024, 0.24))),
        MeshMaterial3d(leather.clone()),
        Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
    ));
    // Pommel
    world.spawn((
        Mesh3d(meshes.add(Sphere::new(0.036))),
        MeshMaterial3d(dark.clone()),
        Transform::from_xyz(0.0, 0.0, 0.145),
    ));
    // Crossguard
    world.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.30, 0.045, 0.05))),
        MeshMaterial3d(dark),
        Transform::from_xyz(0.0, 0.0, -0.14),
    ));
    // Blade: a long thin cuboid with a narrower, brighter edge core.
    world.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.056, 0.014, 0.92))),
        MeshMaterial3d(steel.clone()),
        Transform::from_xyz(0.0, 0.0, -0.63),
    ));
    // Tip: tapered wedge suggested by a rotated, smaller cuboid.
    world.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.04, 0.012, 0.12))),
        MeshMaterial3d(steel),
        Transform::from_xyz(0.0, 0.0, -1.13),
    ));

    Scene::new(world)
}

/// Round shield facing -Z (the boss points away from the wielder).
pub fn build_shield_scene(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Scene {
    let mut world = World::new();
    let steel = steel(materials);
    let dark = dark_steel(materials);
    let wood = wood(materials);

    let face_rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);

    // Wooden face
    world.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.34, 0.035))),
        MeshMaterial3d(wood),
        Transform::from_rotation(face_rot),
    ));
    // Steel rim
    world.spawn((
        Mesh3d(meshes.add(Torus::new(0.325, 0.355))),
        MeshMaterial3d(dark),
        Transform::from_rotation(face_rot),
    ));
    // Center boss
    world.spawn((
        Mesh3d(meshes.add(Sphere::new(0.085))),
        MeshMaterial3d(steel),
        Transform::from_xyz(0.0, 0.0, -0.03).with_scale(Vec3::new(1.0, 1.0, 0.55)),
    ));

    Scene::new(world)
}
