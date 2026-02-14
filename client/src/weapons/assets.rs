//! assets systems.

use super::*;

/// Load weapon audio assets at startup
pub fn setup_weapon_audio_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(WeaponAudioAssets {
        assault_shot: asset_server.load(paths::SFX_ASSAULT_SHOT),
        revolver_shot: asset_server.load(paths::SFX_REVOLVER_SHOT),
        shotgun_shot: asset_server.load(paths::SFX_SHOTGUN_SHOT),
        sniper_shot: asset_server.load(paths::SFX_SNIPER_SHOT),
        out_of_ammo: asset_server.load(paths::SFX_OUT_OF_AMMO),
        gun_reload: asset_server.load(paths::SFX_GUN_RELOAD),
        assault_reload: asset_server.load(paths::SFX_ASSAULT_RELOAD),
        revolver_reload: asset_server.load(paths::SFX_REVOLVER_RELOAD),
        shotgun_reload: asset_server.load(paths::SFX_SHOTGUN_RELOAD),
        sniper_reload: asset_server.load(paths::SFX_SNIPER_RELOAD),
    });
}

/// Create shared meshes/materials for weapon visuals.
pub fn setup_weapon_visual_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Bullet tracer visual - a glowing elongated capsule
    let tracer_mesh = meshes.add(Capsule3d::new(0.08, 2.0));
    let tracer_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.2),
        emissive: LinearRgba::new(20.0, 15.0, 5.0, 1.0),
        unlit: true,
        ..default()
    });

    // Impact marker disk - unit radius, scaled per marker.
    let impact_disk_mesh_unit = meshes.add(Cylinder::new(1.0, 0.03));

    // Blood splatter visuals
    let blood_splatter_mesh = meshes.add(Cylinder::new(1.0, 0.02)); // Thin disk for ground splats
    let blood_droplet_mesh = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()); // Small sphere for flying droplets
    let blood_burst_mesh = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()); // Larger sphere for burst

    // Dark red blood droplet material (for flying particles)
    let blood_droplet_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.02, 0.02, 0.95),
        emissive: LinearRgba::new(0.4, 0.0, 0.0, 1.0), // Red glow so visible in air
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // BRIGHT blood burst material for instant hit feedback (PUBG-style splat)
    // Very bright emissive so it's visible from distance
    let blood_burst_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.1, 0.1, 0.9),
        emissive: LinearRgba::new(2.0, 0.2, 0.2, 1.0), // Strong red glow!
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // Shared impact marker materials (avoids per-hit allocation)
    let impact_terrain_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.5, 0.0, 0.85),
        emissive: LinearRgba::new(2.0, 1.0, 0.0, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let impact_wall_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.0, 0.0, 0.9),
        emissive: LinearRgba::new(2.0, 0.0, 0.0, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // Shared blood ground splat materials (avoids per-droplet allocation)
    let blood_splat_shared_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.01, 0.01, 0.85),
        emissive: LinearRgba::new(0.2, 0.0, 0.0, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let blood_ring_shared_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.42, 0.01, 0.01, 0.55),
        emissive: LinearRgba::new(0.25, 0.0, 0.0, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // Muzzle smoke + flash assets (Kenney smoke particles)
    let smoke_mesh = meshes.add(Plane3d::default());
    let flash_mesh = meshes.add(Plane3d::default());

    let smoke_frames: Vec<_> = (0..=24)
        .map(|i| {
            asset_server.load(format!(
                "VFX/kenney_smoke-particles/PNG/White puff/whitePuff{:02}.png",
                i
            ))
        })
        .collect();
    let smoke_materials: Vec<_> = smoke_frames
        .iter()
        .map(|texture| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.85, 0.85),
                base_color_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                perceptual_roughness: 1.0,
                metallic: 0.0,
                ..default()
            })
        })
        .collect();

    let flash_frames: Vec<_> = (0..=8)
        .map(|i| {
            asset_server.load(format!(
                "VFX/kenney_smoke-particles/PNG/Flash/flash{:02}.png",
                i
            ))
        })
        .collect();
    let flash_materials: Vec<_> = flash_frames
        .iter()
        .map(|texture| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.9, 0.6),
                emissive: LinearRgba::new(3.0, 2.2, 1.2, 1.0),
                base_color_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                perceptual_roughness: 1.0,
                metallic: 0.0,
                ..default()
            })
        })
        .collect();

    commands.insert_resource(WeaponVisualAssets {
        tracer_mesh,
        tracer_material,
        impact_disk_mesh_unit,
        blood_splatter_mesh,
        blood_droplet_mesh,
        blood_burst_mesh,
        blood_droplet_material,
        blood_burst_material,
        impact_terrain_material,
        impact_wall_material,
        blood_splat_shared_material,
        blood_ring_shared_material,
        smoke_mesh,
        smoke_materials,
        flash_mesh,
        flash_materials,
    });
}
