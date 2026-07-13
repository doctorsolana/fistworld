//! assets systems.

use super::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

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
    mut images: ResMut<Assets<Image>>,
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

    // ==========================================================================
    // Blood visuals. Realism rules: blood is DARK (near-black in shadow, never
    // luminous — no emissive!), mist is a soft textured puff (not a sphere),
    // and ground blood is an irregular textured decal that dries over time.
    // ==========================================================================
    let blood_droplet_mesh = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());
    let blood_mist_mesh = meshes.add(Plane3d::default());
    let blood_splat_mesh = meshes.add(Plane3d::default());

    // Flying droplets: small dark specks, no glow.
    let blood_droplet_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.30, 0.012, 0.010, 0.96),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // Mist puffs: the white Kenney smoke flipbook tinted dark red. The
    // texture's own alpha dissipates across frames, so the flipbook handles
    // the fade without any material mutation.
    let blood_mist_materials: Vec<Handle<StandardMaterial>> = (0..=24)
        .map(|i| {
            let texture = asset_server.load(format!(
                "VFX/kenney_smoke-particles/PNG/White puff/whitePuff{i:02}.png"
            ));
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.34, 0.014, 0.012, 0.92),
                base_color_texture: Some(texture),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })
        })
        .collect();

    // Ground decals: procedural splatter textures (irregular blob + satellite
    // droplets + drips), LIT so they sit in the scene's sun/shadow, with a
    // wet gloss that dries to dark matte brown.
    let blood_splat_variants: Vec<BloodSplatVariant> = (0..3)
        .map(|variant| {
            let image = images.add(generate_blood_splat_image(0xB10D + variant as u64 * 7919));
            let mut stage = |color: Color, alpha_texture: &Handle<Image>, rough: f32, refl: f32| {
                materials.add(StandardMaterial {
                    base_color: color,
                    base_color_texture: Some(alpha_texture.clone()),
                    perceptual_roughness: rough,
                    reflectance: refl,
                    metallic: 0.0,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })
            };
            BloodSplatVariant {
                fresh: stage(Color::srgba(0.45, 0.022, 0.016, 0.96), &image, 0.30, 0.5),
                drying: stage(Color::srgba(0.30, 0.016, 0.011, 0.92), &image, 0.60, 0.35),
                dried: stage(Color::srgba(0.16, 0.012, 0.008, 0.80), &image, 0.92, 0.2),
            }
        })
        .collect();

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
        blood_droplet_mesh,
        blood_droplet_material,
        blood_mist_mesh,
        blood_mist_materials,
        blood_splat_mesh,
        blood_splat_variants,
        impact_terrain_material,
        impact_wall_material,
        smoke_mesh,
        smoke_materials,
        flash_mesh,
        flash_materials,
    });
}

/// Generate an irregular blood-splatter alpha texture: a noisy central blob,
/// satellite droplets, and a few directional drips. White RGB so the material
/// `base_color` provides the tint.
fn generate_blood_splat_image(seed: u64) -> Image {
    use rand::{Rng, SeedableRng};
    const SIZE: usize = 192;

    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let center = SIZE as f32 * 0.5;

    // Metaball field: one main blob, a handful of medium lobes, small satellites.
    let mut blobs: Vec<(f32, f32, f32)> = Vec::new();
    blobs.push((center, center, SIZE as f32 * 0.16));
    for _ in 0..rng.gen_range(4..8) {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(0.05..0.22) * SIZE as f32;
        blobs.push((
            center + angle.cos() * dist,
            center + angle.sin() * dist,
            rng.gen_range(0.05..0.11) * SIZE as f32,
        ));
    }
    for _ in 0..rng.gen_range(10..18) {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(0.2..0.46) * SIZE as f32;
        blobs.push((
            center + angle.cos() * dist,
            center + angle.sin() * dist,
            rng.gen_range(0.012..0.045) * SIZE as f32,
        ));
    }
    // Directional drips: chains of shrinking blobs.
    for _ in 0..rng.gen_range(2..4) {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let steps = rng.gen_range(4..8);
        let step_len = rng.gen_range(0.035..0.06) * SIZE as f32;
        let start_r = rng.gen_range(0.045..0.07) * SIZE as f32;
        for s in 0..steps {
            let d = SIZE as f32 * 0.12 + step_len * s as f32;
            blobs.push((
                center + angle.cos() * d,
                center + angle.sin() * d,
                start_r * (1.0 - s as f32 / steps as f32).max(0.25),
            ));
        }
    }

    let fbm: Fbm<Perlin> = Fbm::new(seed as u32)
        .set_octaves(3)
        .set_frequency(7.0)
        .set_persistence(0.55);

    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32;
            let py = y as f32;
            let mut field = 0.0_f32;
            for (bx, by, r) in &blobs {
                let d2 = (px - bx) * (px - bx) + (py - by) * (py - by);
                field += (r * r) / (d2 + 1.0);
            }
            // Noisy threshold gives ragged, organic edges instead of circles.
            let nx = px / SIZE as f32;
            let ny = py / SIZE as f32;
            let noise = fbm.get([nx as f64, ny as f64]) as f32;
            let threshold = 0.85 + noise * 0.45;
            let alpha = ((field - threshold) * 6.0).clamp(0.0, 1.0);
            // Slight in-blob density variation so big pools aren't flat.
            let body = 0.82 + 0.18 * ((field * 0.35).min(1.0));
            let a = (alpha * body * 255.0) as u8;
            data.extend_from_slice(&[255, 255, 255, a]);
        }
    }

    Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
