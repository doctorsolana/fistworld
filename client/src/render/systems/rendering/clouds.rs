//! clouds systems.

use super::day_night::{lerp_color, lerp_f32, smoothstep, sun_yaw_from_phase};
use super::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};

const CLOUD_TEXTURE_PATH: &str = "sky_10_2k/sky_10_2k.png";
const CLOUD_LAYER_RADII: [f32; 1] = [1900.0];
const CLOUD_LAYER_SPEEDS: [Vec2; 1] = [Vec2::new(0.0006, 0.0)];
// Sky dome texture includes a baked-in sun. Keep it visible for now.
// If you want to remove the baked sun, the texture itself must be edited.
const CLOUD_LAYER_ALPHAS: [f32; 1] = [0.3];
const CLOUD_LAYER_UV_SCALES: [Vec2; 1] = [Vec2::splat(1.0)];
const CLOUD_LAYER_UV_ROTATIONS: [f32; 1] = [0.0];
const CLOUD_LAYER_YAW_OFFSETS: [f32; 1] = [0.0];
pub(super) const CLOUD_SUN_YAW_OFFSET: f32 =
    std::f32::consts::PI + std::f32::consts::FRAC_PI_2 + (170.0_f32.to_radians());
const CLOUD_BASE_PITCH: f32 = -std::f32::consts::FRAC_PI_2;

const CLOUD_CARD_COUNT: usize = 18;
const CLOUD_CARD_RADIUS: f32 = 650.0;
const CLOUD_CARD_MIN_HEIGHT: f32 = 160.0;
const CLOUD_CARD_MAX_HEIGHT: f32 = 260.0;
const CLOUD_CARD_MIN_SCALE: f32 = 180.0;
const CLOUD_CARD_MAX_SCALE: f32 = 320.0;
const CLOUD_CARD_MIN_SPEED: f32 = 0.4;
const CLOUD_CARD_MAX_SPEED: f32 = 0.9;
const CLOUD_CARD_TEXTURE_SIZE: u32 = 512;

const CLOUD_DAY_TINT: Color = Color::srgb(0.86, 0.92, 1.0);
const CLOUD_SUNSET_TINT: Color = Color::srgb(1.0, 0.74, 0.55);
const CLOUD_NIGHT_TINT: Color = Color::srgb(0.25, 0.3, 0.4);

const CLOUD_COVER_SEGMENT_SECS: f32 = 180.0;
const CLOUD_COVER_LERP_SPEED: f32 = 0.08;
const CLOUD_COVER_CLEAR_RANGE: (f32, f32) = (0.0, 0.25);
const CLOUD_COVER_CLOUDY_RANGE: (f32, f32) = (0.55, 1.0);

#[derive(Component, Clone, Copy)]
pub struct CloudLayer {
    pub uv_offset: Vec2,
    pub uv_speed: Vec2,
    pub uv_scale: Vec2,
    pub uv_rotation: f32,
    pub yaw_offset: f32,
    pub alpha: f32,
}

#[derive(Resource)]
pub struct CloudAssets {
    pub texture: Handle<Image>,
    pub sampler_configured: bool,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CloudCoverMode {
    #[default]
    Auto,
    Clear,
    Cloudy,
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct CloudCoverOverride {
    pub mode: CloudCoverMode,
}

impl Default for CloudCoverOverride {
    fn default() -> Self {
        Self {
            mode: CloudCoverMode::Auto,
        }
    }
}

#[derive(Resource, Clone, Copy)]
pub struct CloudCover {
    pub current: f32,
    pub target: f32,
    pub segment: i64,
}

impl Default for CloudCover {
    fn default() -> Self {
        Self {
            current: 1.0,
            target: 1.0,
            segment: -1,
        }
    }
}

#[derive(Component, Clone, Copy)]
pub struct CloudCard {
    pub offset: Vec3,
    pub velocity: Vec3,
    pub base_alpha: f32,
}

#[derive(Resource)]
pub struct CloudCardsSpawned;

#[derive(Resource)]
pub struct PendingCloudCardTexture {
    pub seed: u64,
    pub task: Task<Image>,
}

/// Cached cloud tint/visibility to avoid per-frame material mutations.
/// Only mutate GPU materials when the visual actually changes.
#[derive(Resource, Default)]
pub struct CloudMaterialCache {
    pub last_tint: Option<[f32; 3]>,
    pub last_visibility: f32,
}

pub(super) fn setup_cloud_layers(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let cloud_texture = asset_server.load(CLOUD_TEXTURE_PATH);
    let cloud_mesh = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));

    commands.insert_resource(CloudAssets {
        texture: cloud_texture.clone(),
        sampler_configured: false,
    });

    for i in 0..CLOUD_LAYER_RADII.len() {
        let alpha = CLOUD_LAYER_ALPHAS[i];
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 1.0, 1.0),
            base_color_texture: Some(cloud_texture.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        });

        commands.spawn((
            CloudLayer {
                uv_offset: Vec2::ZERO,
                uv_speed: CLOUD_LAYER_SPEEDS[i],
                uv_scale: CLOUD_LAYER_UV_SCALES[i],
                uv_rotation: CLOUD_LAYER_UV_ROTATIONS[i],
                yaw_offset: CLOUD_LAYER_YAW_OFFSETS[i],
                alpha,
            },
            NotShadowCaster,
            Mesh3d(cloud_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(CLOUD_LAYER_RADII[i])),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ));
    }
}

/// Spawn deterministic cloud cards once we receive the server seed.
pub fn spawn_cloud_cards(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    seed_query: Query<&shared::components::CloudSeed>,
    spawned: Option<Res<CloudCardsSpawned>>,
    pending_texture: Option<ResMut<PendingCloudCardTexture>>,
    settings: Res<GraphicsSettings>,
) {
    if spawned.is_some() {
        return;
    }
    if !settings.clouds_enabled {
        return;
    }

    let Some(seed) = seed_query.iter().next().map(|s| s.seed) else {
        return;
    };

    let Some(card_texture_image) =
        poll_or_start_cloud_card_texture(seed, pending_texture, &mut commands)
    else {
        return;
    };

    commands.remove_resource::<PendingCloudCardTexture>();
    commands.insert_resource(CloudCardsSpawned);

    let cloud_texture = images.add(card_texture_image);
    let cloud_mesh = meshes.add(Plane3d::default());

    let mut rng = StdRng::seed_from_u64(seed);
    for _ in 0..CLOUD_CARD_COUNT {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let radius = rng.gen_range(CLOUD_CARD_RADIUS * 0.35..CLOUD_CARD_RADIUS);
        let height = rng.gen_range(CLOUD_CARD_MIN_HEIGHT..CLOUD_CARD_MAX_HEIGHT);
        let offset = Vec3::new(angle.cos() * radius, height, angle.sin() * radius);

        let drift_angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = rng.gen_range(CLOUD_CARD_MIN_SPEED..CLOUD_CARD_MAX_SPEED);
        let velocity = Vec3::new(drift_angle.cos() * speed, 0.0, drift_angle.sin() * speed);

        let scale = rng.gen_range(CLOUD_CARD_MIN_SCALE..CLOUD_CARD_MAX_SCALE);
        let base_alpha = rng.gen_range(0.18..0.32);

        let material = materials.add(StandardMaterial {
            base_color: color_with_alpha(CLOUD_DAY_TINT, base_alpha),
            base_color_texture: Some(cloud_texture.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        });

        commands.spawn((
            CloudCard {
                offset,
                velocity,
                base_alpha,
            },
            NotShadowCaster,
            Mesh3d(cloud_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(offset).with_scale(Vec3::new(scale, 1.0, scale)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ));
    }
}

fn poll_or_start_cloud_card_texture(
    seed: u64,
    pending_texture: Option<ResMut<PendingCloudCardTexture>>,
    commands: &mut Commands,
) -> Option<Image> {
    if let Some(mut pending_texture) = pending_texture {
        if pending_texture.seed != seed {
            commands.remove_resource::<PendingCloudCardTexture>();
            return None;
        }
        return block_on(poll_once(&mut pending_texture.task));
    }

    let task = AsyncComputeTaskPool::get()
        .spawn(async move { generate_cloud_card_texture(seed, CLOUD_CARD_TEXTURE_SIZE) });
    commands.insert_resource(PendingCloudCardTexture { seed, task });
    None
}

fn hash_to_unit(seed: u64, salt: u64) -> f32 {
    let mut x = seed ^ salt;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x as f64 / u64::MAX as f64) as f32
}

/// Ensure cloud texture sampler repeats for UV scrolling.
pub fn apply_cloud_texture_sampler(
    mut cloud_assets: ResMut<CloudAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    if cloud_assets.sampler_configured {
        return;
    }

    let Some(image) = images.get_mut(&cloud_assets.texture) else {
        return;
    };

    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });

    cloud_assets.sampler_configured = true;
}

/// Scroll cloud layers and tint them by sun height.
pub fn update_cloud_cover(
    time: Res<Time>,
    world_time_query: Query<&shared::components::WorldTime>,
    seed_query: Query<&shared::components::CloudSeed>,
    override_mode: Res<CloudCoverOverride>,
    settings: Res<GraphicsSettings>,
    mut cover: ResMut<CloudCover>,
) {
    if !settings.clouds_enabled {
        return;
    }
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let seed = seed_query.iter().next().map(|s| s.seed).unwrap_or(0);

    let seconds = world_time.seconds_in_cycle.max(0.0);
    let segment = (seconds / CLOUD_COVER_SEGMENT_SECS).floor() as i64;
    match override_mode.mode {
        CloudCoverMode::Clear => {
            cover.segment = segment;
            cover.target = 0.0;
        }
        CloudCoverMode::Cloudy => {
            cover.segment = segment;
            cover.target = 1.0;
        }
        CloudCoverMode::Auto => {
            if cover.segment != segment {
                cover.segment = segment;

                let t = world_time.normalized_time();
                let phase = t * std::f32::consts::TAU;
                let elevation = -phase.cos();
                let dawn_dusk = 1.0 - smoothstep(0.25, 0.6, elevation.abs());
                let cloudy_chance = lerp_f32(0.4, 0.65, dawn_dusk);

                let pick = hash_to_unit(seed, segment as u64);
                let roll = hash_to_unit(seed ^ 0x9E37_79B9_7F4A_7C15, segment as u64);

                cover.target = if pick < cloudy_chance {
                    lerp_f32(CLOUD_COVER_CLOUDY_RANGE.0, CLOUD_COVER_CLOUDY_RANGE.1, roll)
                } else {
                    lerp_f32(CLOUD_COVER_CLEAR_RANGE.0, CLOUD_COVER_CLEAR_RANGE.1, roll)
                };
            }
        }
    }

    let dt = time.delta_secs().max(0.0);
    let blend = 1.0 - (-CLOUD_COVER_LERP_SPEED * dt).exp();
    cover.current = lerp_f32(cover.current, cover.target, blend).clamp(0.0, 1.0);
}

/// Scroll cloud layers and tint them by sun height.
pub fn update_cloud_layers(
    time: Res<Time>,
    world_time_query: Query<&shared::components::WorldTime>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    cover: Res<CloudCover>,
    settings: Res<GraphicsSettings>,
    mut layers: Query<(
        &mut CloudLayer,
        &MeshMaterial3d<StandardMaterial>,
        &mut Transform,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<CloudMaterialCache>,
) {
    if !settings.clouds_enabled {
        return;
    }
    let Ok(camera_tf) = camera.single() else {
        return;
    };

    let t = world_time_query
        .iter()
        .next()
        .map(|wt| wt.normalized_time())
        .unwrap_or(0.5);

    let phase = t * std::f32::consts::TAU;
    let elevation = -phase.cos();
    let sun_yaw = sun_yaw_from_phase(phase);
    let day_factor = smoothstep(-0.05, 0.15, elevation);
    let twilight = 1.0 - smoothstep(0.12, 0.35, elevation.max(0.0));
    let base_tint = lerp_color(CLOUD_NIGHT_TINT, CLOUD_DAY_TINT, day_factor);
    let tint = lerp_color(base_tint, CLOUD_SUNSET_TINT, twilight);
    let visibility = (0.2 + 0.8 * day_factor) * cover.current;

    // Check if tint/visibility actually changed (avoid GPU re-upload when steady)
    let tint_rgba = tint.to_srgba();
    let tint_arr = [tint_rgba.red, tint_rgba.green, tint_rgba.blue];
    let tint_changed = cache.last_tint.is_none_or(|prev| {
        (prev[0] - tint_arr[0]).abs() > 0.005
            || (prev[1] - tint_arr[1]).abs() > 0.005
            || (prev[2] - tint_arr[2]).abs() > 0.005
            || (cache.last_visibility - visibility).abs() > 0.005
    });

    for (mut layer, material_handle, mut transform) in layers.iter_mut() {
        // Keep clouds centered on the camera.
        transform.translation = camera_tf.translation();
        transform.rotation = Quat::from_rotation_y(sun_yaw + layer.yaw_offset)
            * Quat::from_rotation_x(CLOUD_BASE_PITCH);

        // Scroll UVs slowly for parallax.
        let delta = layer.uv_speed * time.delta_secs();
        layer.uv_offset = Vec2::new(
            (layer.uv_offset.x + delta.x).rem_euclid(1.0),
            (layer.uv_offset.y + delta.y).rem_euclid(1.0),
        );

        // Only mutate material when tint/visibility changed or UV scrolled
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.uv_transform = bevy::math::Affine2::from_scale_angle_translation(
                layer.uv_scale,
                layer.uv_rotation,
                layer.uv_offset,
            );
            if tint_changed {
                material.base_color =
                    color_with_alpha(tint, (layer.alpha * visibility).clamp(0.0, 1.0));
            }
        }
    }

    if tint_changed {
        cache.last_tint = Some(tint_arr);
        cache.last_visibility = visibility;
    }
}

/// Update cloud card motion and tinting.
pub fn update_cloud_cards(
    time: Res<Time>,
    world_time_query: Query<&shared::components::WorldTime>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    cover: Res<CloudCover>,
    settings: Res<GraphicsSettings>,
    mut cards: Query<(
        &mut CloudCard,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cache: Res<CloudMaterialCache>,
) {
    if !settings.clouds_enabled {
        return;
    }
    let Ok(camera_tf) = camera.single() else {
        return;
    };

    let t = world_time_query
        .iter()
        .next()
        .map(|wt| wt.normalized_time())
        .unwrap_or(0.5);
    let phase = t * std::f32::consts::TAU;
    let elevation = -phase.cos();
    let day_factor = smoothstep(-0.05, 0.15, elevation);
    let twilight = 1.0 - smoothstep(0.12, 0.35, elevation.max(0.0));
    let base_tint = lerp_color(CLOUD_NIGHT_TINT, CLOUD_DAY_TINT, day_factor);
    let tint = lerp_color(base_tint, CLOUD_SUNSET_TINT, twilight);
    let visibility = (0.2 + 0.8 * day_factor) * cover.current;
    let sun_yaw = sun_yaw_from_phase(phase);

    // Check if tint actually changed (reuse cache from update_cloud_layers)
    let tint_rgba = tint.to_srgba();
    let tint_arr = [tint_rgba.red, tint_rgba.green, tint_rgba.blue];
    let tint_changed = cache.last_tint.is_none_or(|prev| {
        (prev[0] - tint_arr[0]).abs() > 0.005
            || (prev[1] - tint_arr[1]).abs() > 0.005
            || (prev[2] - tint_arr[2]).abs() > 0.005
            || (cache.last_visibility - visibility).abs() > 0.005
    });

    let dt = time.delta_secs();
    for (mut card, mut transform, material_handle) in cards.iter_mut() {
        let velocity = card.velocity;
        card.offset += velocity * dt;

        // Wrap within a square to keep cards around the player.
        if card.offset.x > CLOUD_CARD_RADIUS {
            card.offset.x = -CLOUD_CARD_RADIUS;
        } else if card.offset.x < -CLOUD_CARD_RADIUS {
            card.offset.x = CLOUD_CARD_RADIUS;
        }
        if card.offset.z > CLOUD_CARD_RADIUS {
            card.offset.z = -CLOUD_CARD_RADIUS;
        } else if card.offset.z < -CLOUD_CARD_RADIUS {
            card.offset.z = CLOUD_CARD_RADIUS;
        }

        let rotated = Quat::from_rotation_y(sun_yaw) * Vec3::new(card.offset.x, 0.0, card.offset.z);
        transform.translation =
            camera_tf.translation() + Vec3::new(rotated.x, card.offset.y, rotated.z);

        // Only mutate material when tint/visibility actually changed
        if tint_changed {
            if let Some(material) = materials.get_mut(&material_handle.0) {
                material.base_color =
                    color_with_alpha(tint, (card.base_alpha * visibility).clamp(0.0, 1.0));
            }
        }
    }
}

fn color_with_alpha(color: Color, alpha: f32) -> Color {
    let rgba = color.to_srgba();
    Color::srgba(rgba.red, rgba.green, rgba.blue, alpha)
}

fn generate_cloud_card_texture(seed: u64, size: u32) -> Image {
    let fbm: Fbm<Perlin> = Fbm::new((seed as u32).wrapping_add(777))
        .set_octaves(4)
        .set_frequency(1.6)
        .set_persistence(0.55);

    let mut data = Vec::with_capacity((size * size * 4) as usize);

    for y in 0..size {
        for x in 0..size {
            let nx = x as f32 / size as f32 * 2.0 - 1.0;
            let ny = y as f32 / size as f32 * 2.0 - 1.0;
            let r = (nx * nx + ny * ny).sqrt();

            let noise = fbm.get([nx as f64 * 1.8, ny as f64 * 1.8]) as f32 * 0.5 + 0.5;
            let puff = smoothstep(0.35, 0.65, noise);
            let edge_falloff = 1.0 - smoothstep(0.55, 1.0, r);
            let alpha = (puff * edge_falloff).powf(1.1).clamp(0.0, 1.0);
            let a = (alpha * 255.0) as u8;

            data.push(255);
            data.push(255);
            data.push(255);
            data.push(a);
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );

    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });

    image
}
