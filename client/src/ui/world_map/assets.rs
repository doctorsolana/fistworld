//! assets systems.

use super::*;

pub(super) fn ensure_map_texture(
    map_open: Res<MapOpen>,
    map_config: Res<MapUiConfig>,
    mut map_texture: ResMut<MapTexture>,
    mut images: ResMut<Assets<Image>>,
) {
    if !map_open.0 || map_texture.handle.is_some() {
        return;
    }

    if let Some(image) = load_authored_minimap_image() {
        map_texture.handle = Some(images.add(image));
        return;
    }

    warn!("Map UI: minimap image missing, using generated minimap fallback");
    let bounds = map_config.bounds.unwrap_or_else(load_active_map_bounds);
    let image = build_map_image(MAP_TEX_SIZE, bounds);
    map_texture.handle = Some(images.add(image));
}

pub(super) fn ensure_marker_assets(
    map_open: Res<MapOpen>,
    mut assets: ResMut<MapMarkerAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    if !map_open.0 || assets.player_arrow.is_some() {
        return;
    }
    let arrow = build_player_arrow_image(PLAYER_ARROW_TEX, ACCENT_COLOR);
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

pub(super) fn load_authored_minimap_image() -> Option<Image> {
    let loaded = shared::map::load_default_map().ok()?;
    let rel = loaded.definition.terrain.minimap.as_deref()?;
    let path =
        shared::map::resolve_map_relative_file(&loaded.map_dir, &loaded.definition.map_id, rel)?;

    let bytes = std::fs::read(path).ok()?;
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let rgba = reader.decode().ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();

    Some(Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

pub(super) fn build_map_image(size: u32, bounds: MapBounds) -> Image {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let terrain = TerrainGenerator::new(WORLD_SEED);
    let min_x = bounds.min[0];
    let min_z = bounds.min[1];
    let width = bounds.width();
    let depth = bounds.depth();

    for y in 0..size {
        for x in 0..size {
            let nx = x as f32 / (size - 1) as f32;
            let nz = y as f32 / (size - 1) as f32;

            let world_x = min_x + nx * width;
            let world_z = min_z + nz * depth;
            let biome = terrain.get_biome(world_x, world_z);
            let water_height = terrain.get_water_height(world_x, world_z);
            let rgba = if water_height.is_some() {
                water_color(biome)
            } else {
                biome_color(biome)
            };
            pixels.push(rgba[0]);
            pixels.push(rgba[1]);
            pixels.push(rgba[2]);
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

pub(super) fn biome_color(biome: Biome) -> [u8; 3] {
    let rgba = biome.color().to_srgba();
    [
        (rgba.red * 255.0) as u8,
        (rgba.green * 255.0) as u8,
        (rgba.blue * 255.0) as u8,
    ]
}

pub(super) fn water_color(biome: Biome) -> [u8; 3] {
    let color = match biome {
        Biome::Ocean => Color::srgb(0.06, 0.22, 0.38),
        _ => Color::srgb(0.10, 0.50, 0.72),
    };
    let rgba = color.to_srgba();
    [
        (rgba.red * 255.0) as u8,
        (rgba.green * 255.0) as u8,
        (rgba.blue * 255.0) as u8,
    ]
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

pub(super) fn load_active_map_bounds() -> MapBounds {
    shared::map::load_default_map()
        .map(|loaded| loaded.definition.bounds)
        .unwrap_or_else(|_| TerrainGenerator::new(WORLD_SEED).active_map_bounds())
}
