//! assets systems.

use super::*;
use shared::worldgen::WorldBiome;

pub(super) fn ensure_map_texture(
    map_open: Res<MapOpen>,
    map_config: Res<MapUiConfig>,
    mut map_texture: ResMut<MapTexture>,
    mut images: ResMut<Assets<Image>>,
    terrain: Res<shared::terrain::WorldTerrain>,
) {
    if !map_open.0 || map_texture.handle.is_some() {
        return;
    }

    // Always render from the LIVE terrain (heights + edits + water level).
    // The authored minimap.png goes stale the moment the world is sculpted
    // or generated, so it is no longer used.
    let bounds = map_config
        .bounds
        .unwrap_or_else(|| terrain.generator.active_map_bounds());
    let image = build_live_map_image(MAP_TEX_SIZE, bounds, &terrain);
    map_texture.handle = Some(images.add(image));
}

/// Hillshaded elevation map from the real heightfield: deep-to-shallow
/// water blues, sandy shores, greens rising through the hills, rocky gray
/// peaks — with a simple NW-light hillshade so the relief reads.
fn build_live_map_image(
    size: u32,
    bounds: MapBounds,
    terrain: &shared::terrain::WorldTerrain,
) -> Image {
    let min_x = bounds.min[0];
    let min_z = bounds.min[1];
    let width = bounds.width();
    let depth = bounds.depth();
    let water_level = terrain.water_level();
    let biome_field = terrain.generator.loaded_map().biome_field.clone();
    let climate_seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map(|g| g.seed)
        .unwrap_or(0);

    // Sample heights once; reuse for color + hillshade.
    let n = size as usize;
    let mut heights = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let world_x = min_x + (x as f32 / (size - 1) as f32) * width;
            let world_z = min_z + (y as f32 / (size - 1) as f32) * depth;
            heights[y * n + x] = terrain.get_height(world_x, world_z);
        }
    }
    let step_world = width / size as f32;

    let lerp3 = |a: [f32; 3], b: [f32; 3], t: f32| {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };

    let mut pixels = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let h = heights[y * n + x];

            // Hillshade from central differences (NW light).
            let xl = heights[y * n + x.saturating_sub(1)];
            let xr = heights[y * n + (x + 1).min(n - 1)];
            let zu = heights[y.saturating_sub(1) * n + x];
            let zd = heights[(y + 1).min(n - 1) * n + x];
            let dx = (xr - xl) / (2.0 * step_world);
            let dz = (zd - zu) / (2.0 * step_world);
            let shade = (1.0 - (dx * 0.7 + dz * 0.7) * 1.6).clamp(0.55, 1.35);

            // Soft waterline: the binary h < wl test pixelated every coast
            // into a 16m staircase. Blend across a ±0.35m height band so the
            // edge anti-aliases along the bank ramp instead.
            let water_t = water_level
                .map(|wl| ((wl - h) / 0.35 + 0.5).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let underwater = water_t >= 1.0;
            let rgb = if underwater {
                let wl = water_level.unwrap_or(0.0);
                let depth_t = ((wl - h) / 8.0).clamp(0.0, 1.0);
                lerp3([0.36, 0.66, 0.80], [0.05, 0.20, 0.38], depth_t)
            } else {
                let wl = water_level.unwrap_or(0.0);
                let above = h - wl;
                let base = if above < 2.2 {
                    // Beach sand.
                    [0.82, 0.74, 0.54]
                } else if let Some(biomes) = biome_field.as_deref() {
                    // Biome-coloured land: the map is how you read where the
                    // resources are. Iron veins show as rust specks.
                    let world_x = min_x + (x as f32 / (size - 1) as f32) * width;
                    let world_z = min_z + (y as f32 / (size - 1) as f32) * depth;
                    let slope = (dx * dx + dz * dz).sqrt();
                    let biome = biomes.biome(world_x, world_z, h, slope);
                    // Colours blend with the same smooth field the ground
                    // textures use, so map borders feather where the world's
                    // do; the discrete biome is still read for the iron-vein
                    // overlay below.
                    let blend = biomes.biome_blend(world_x, world_z, h, slope);
                    let mountain_base = lerp3(
                        [0.58, 0.56, 0.52],
                        [0.78, 0.78, 0.80],
                        (above - 28.0) / 18.0,
                    );
                    let mut base = [0.0f32; 3];
                    let parts: [([f32; 3], f32); 6] = [
                        ([0.48, 0.61, 0.30], blend.meadow()),
                        ([0.21, 0.41, 0.19], blend.forest),
                        ([0.56, 0.49, 0.32], blend.highlands),
                        (mountain_base, blend.mountains),
                        // The climate tint below paints the actual snow and
                        // sand; these bases keep the biome readable through
                        // it (cold rocky ground, dune sand).
                        ([0.55, 0.58, 0.60], blend.snow),
                        ([0.74, 0.64, 0.44], blend.desert),
                    ];
                    for (color, weight) in parts {
                        base[0] += color[0] * weight;
                        base[1] += color[1] * weight;
                        base[2] += color[2] * weight;
                    }
                    if matches!(biome, WorldBiome::Highlands | WorldBiome::Mountains) {
                        let vein = biomes.iron_vein(world_x, world_z);
                        if vein > 0.55 {
                            base = lerp3(base, [0.47, 0.25, 0.13], 0.65);
                        }
                    }
                    base
                } else if above < 14.0 {
                    // Legacy maps: elevation greens.
                    lerp3([0.32, 0.52, 0.26], [0.45, 0.58, 0.30], (above - 2.2) / 11.8)
                } else if above < 28.0 {
                    lerp3(
                        [0.45, 0.58, 0.30],
                        [0.52, 0.48, 0.38],
                        (above - 14.0) / 14.0,
                    )
                } else {
                    lerp3(
                        [0.52, 0.48, 0.38],
                        [0.72, 0.72, 0.74],
                        (above - 28.0) / 15.0,
                    )
                };
                // Climate tint via the shared function so the map matches the
                // world (snowy poles, dry equator strip).
                let base = {
                    let world_x = min_x + (x as f32 / (size - 1) as f32) * width;
                    let world_z = min_z + (y as f32 / (size - 1) as f32) * depth;
                    let climate = shared::worldgen::climate_at(
                        climate_seed,
                        world_x,
                        world_z,
                        h,
                        width * 0.5,
                    );
                    let frosted = lerp3(base, [0.62, 0.66, 0.70], climate.frost * 0.55);
                    let snowed = lerp3(
                        frosted,
                        [0.88, 0.91, 0.96],
                        climate.snow * (1.0 - ((dx * dx + dz * dz).sqrt() * 1.4).clamp(0.0, 0.8)),
                    );
                    // Desert south (mirrors terrain_splat.wgsl): savanna
                    // yellowing, then quadratically toward sand.
                    let scorched = lerp3(
                        snowed,
                        [snowed[0] * 1.14, snowed[1] * 1.05, snowed[2] * 0.72],
                        climate.dry,
                    );
                    lerp3(
                        scorched,
                        [0.82, 0.72, 0.50],
                        climate.dry * climate.dry * 0.55,
                    )
                };
                [base[0] * shade, base[1] * shade, base[2] * shade]
            };

            let rgb = if water_t > 0.0 && !underwater {
                let wl = water_level.unwrap_or(0.0);
                let depth_t = ((wl - h) / 8.0).clamp(0.0, 1.0);
                let water_rgb = lerp3([0.36, 0.66, 0.80], [0.05, 0.20, 0.38], depth_t);
                lerp3(rgb, water_rgb, water_t)
            } else {
                rgb
            };

            pixels.push((rgb[0].clamp(0.0, 1.0) * 255.0) as u8);
            pixels.push((rgb[1].clamp(0.0, 1.0) * 255.0) as u8);
            pixels.push((rgb[2].clamp(0.0, 1.0) * 255.0) as u8);
            pixels.push(255);
        }
    }

    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub(super) fn ensure_marker_assets(
    map_open: Res<MapOpen>,
    mut assets: ResMut<MapMarkerAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    if !map_open.0 || assets.player_arrow.is_some() {
        return;
    }
    let arrow = build_player_arrow_image(PLAYER_ARROW_TEX, EMBER);
    assets.player_arrow = Some(images.add(arrow));
}

pub(super) fn update_map_image_handle(
    map_texture: Res<MapTexture>,
    mut images: Query<&mut ImageNode, With<MapImage>>,
) {
    if !map_texture.is_changed() {
        return;
    }
    let Some(handle) = map_texture.handle.as_ref() else {
        return;
    };
    for mut image in images.iter_mut() {
        *image = ImageNode::new(handle.clone());
    }
}

pub(super) fn update_marker_image_handle(
    marker_assets: Res<MapMarkerAssets>,
    mut markers: Query<&mut ImageNode, With<MapPlayerMarker>>,
) {
    if !marker_assets.is_changed() {
        return;
    }
    let Some(handle) = marker_assets.player_arrow.as_ref() else {
        return;
    };
    for mut image in markers.iter_mut() {
        *image = ImageNode::new(handle.clone());
    }
}

pub(super) fn build_player_arrow_image(size: u32, color: Color) -> Image {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let rgba = color.to_srgba();
    let fill = [
        (rgba.red * 255.0) as u8,
        (rgba.green * 255.0) as u8,
        (rgba.blue * 255.0) as u8,
        255,
    ];

    let center = (size as f32 - 1.0) * 0.5;
    let max_half = size as f32 * 0.45;

    for y in 0..size {
        let t = y as f32 / (size as f32 - 1.0);
        let half = t * max_half;
        for x in 0..size {
            let dx = (x as f32 - center).abs();
            if dx <= half {
                let idx = ((y * size + x) * 4) as usize;
                pixels[idx] = fill[0];
                pixels[idx + 1] = fill[1];
                pixels[idx + 2] = fill[2];
                pixels[idx + 3] = fill[3];
            }
        }
    }

    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
