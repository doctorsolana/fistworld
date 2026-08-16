//! THE terrain splat material.
//!
//! One `AsBindGroup` definition shared by every terrain renderer. Multiple Rust
//! binding layouts against one shader can drift without a compiler error.
//!
//! The rule that follows: **a binding is added here or not at all.**

use bevy::asset::Asset;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::paint::TerrainLayer;

// ---------------------------------------------------------------------------
// The layer table -- single source of truth
// ---------------------------------------------------------------------------

/// Everything that is true about one terrain layer, in one place.
///
/// The four-layer contract used to be spread across the KTX2 build order, the shader's
/// `uv_grass`/`uv_dirt`/`uv_sand`/`uv_cobble` names, the palette literals, the `layer_tiling`
/// vector and display colours, with nothing cross-checking any of them.
pub struct TerrainLayerDef {
    pub layer: TerrainLayer,
    /// What a human calls it in tools and diagnostics.
    pub display_name: &'static str,
    /// Source image packed into this index of `terrain_albedo_array.ktx2`.
    /// `tools/terrain_ktx_builder` reads the order from here.
    ///
    /// Two of these do not match their layer's name, and that is recorded rather than fixed.
    /// See the per-layer comments: with `stylize.x = 1.0` the albedo texture supplies only an
    /// 18% chroma grain, so "correcting" the images would change the look for no gain.
    pub albedo_source: &'static str,
    /// Source image packed into this index of `terrain_normal_array.ktx2`.
    pub normal_source: &'static str,
    /// World metres per texture repeat.
    pub tile_metres: f32,
    /// The flat colour this layer actually renders as. Because `stylize.x` is 1.0, this --
    /// not the albedo texture -- is what you see.
    pub color: [f32; 4],
}

/// Index in this array IS the array-texture layer index IS `TerrainLayer::index()`.
/// `layer_table_indices_match` pins that; do not reorder without rebuilding the KTX2 arrays.
pub const TERRAIN_LAYERS: [TerrainLayerDef; 4] = [
    TerrainLayerDef {
        layer: TerrainLayer::Grass,
        display_name: "Grass",
        albedo_source: "Grass_Texture_01.png",
        normal_source: "Ground_Normals_01.png",
        tile_metres: 8.0,
        color: [0.26, 0.45, 0.20, 1.0],
    },
    TerrainLayerDef {
        layer: TerrainLayer::Dirt,
        display_name: "Dirt",
        // NOT a dirt image -- this is the second GRASS texture, and it is deliberate that the
        // table says so out loud. The albedo array only contributes an 18% chroma grain
        // (`bands.z`), so what the ground reads as is `color` below, which is brown. Swapping
        // in a dirt image would shift the grain by under 3% and change a look that is signed
        // off. The lie was previously invisible: the builder said Grass_Texture_02 and the
        // shader said `uv_dirt`, in different files, with nothing connecting them.
        // Was `Ground_Normals_02.png`, which is the SAME IMAGE as `Dirt_Normals_01.png` --
        // identical decoded pixels, and the 1k derivatives were byte-identical files. The
        // array was carrying 5.3 MB of it twice. Naming the real file makes the duplication
        // impossible to reintroduce; the built array is bit-identical either way.
        albedo_source: "Grass_Texture_02.png",
        normal_source: "Dirt_Normals_01.png",
        tile_metres: 7.0,
        // Dark enough to read as compacted earth in full daylight while the
        // splat shoulder still has room to fade naturally into meadow grass.
        color: [0.32, 0.23, 0.15, 1.0],
    },
    TerrainLayerDef {
        layer: TerrainLayer::Sand,
        display_name: "Sand",
        // Also not what it says: the repo has no sand image, so dirt's stands in for the grain.
        // Same reasoning as Dirt above -- `color` is what you actually see.
        albedo_source: "Dirt_Texture_01.png",
        // There is no sand normal map in the repo; dirt's relief stands in. Deliberate,
        // not an oversight -- sand reads by its palette colour, and at 6 m tiling under an
        // RTS camera the relief difference is not visible.
        normal_source: "Dirt_Normals_01.png",
        tile_metres: 6.0,
        color: [0.78, 0.70, 0.50, 1.0],
    },
    TerrainLayerDef {
        layer: TerrainLayer::Cobblestone,
        display_name: "Cobblestone",
        albedo_source: "Cobblestone_Texture_01.png",
        normal_source: "Cobblestone_Normals_01.png",
        tile_metres: 5.0,
        color: [0.40, 0.39, 0.38, 1.0],
    },
];

/// Asset paths for the packed arrays. The builder writes these; both crates load them.
pub const TERRAIN_ALBEDO_ARRAY: &str = "textures/terrain/optimized_1k/terrain_albedo_array.ktx2";
pub const TERRAIN_NORMAL_ARRAY: &str = "textures/terrain/optimized_1k/terrain_normal_array.ktx2";

/// UV tiling per layer, derived from the table so it cannot drift from it.
pub fn layer_tiling() -> Vec4 {
    Vec4::new(
        TERRAIN_LAYERS[0].tile_metres,
        TERRAIN_LAYERS[1].tile_metres,
        TERRAIN_LAYERS[2].tile_metres,
        TERRAIN_LAYERS[3].tile_metres,
    )
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

/// Splatmap material definition (StandardMaterial + extension).
pub type TerrainSplatMaterial = ExtendedMaterial<StandardMaterial, TerrainSplatExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerrainSplatExtension {
    // Weight map (RGBA): grass, dirt, sand, cobblestone.
    #[texture(100)]
    #[sampler(101)]
    pub weight_map: Handle<Image>,

    // Array textures, indexed by `TERRAIN_LAYERS` position.
    #[texture(102, dimension = "2d_array")]
    #[sampler(103)]
    pub albedo_array: Handle<Image>,
    #[texture(104, dimension = "2d_array")]
    #[sampler(105)]
    pub normal_array: Handle<Image>,

    // UV tiling per layer.
    #[uniform(120)]
    pub layer_tiling: Vec4,

    // Debug mode selector.
    #[uniform(121)]
    pub debug_mode: u32,

    // 1.0 = full normal mapping, 0.0 = skip normal-map contribution.
    #[uniform(122)]
    pub normal_strength: f32,

    // x: water level, y: enabled, z: server clock offset, w: surface offset.
    #[uniform(123)]
    pub water_params: Vec4,

    // --- Stylised palette ---
    //
    // The photographic splat textures read as "realistic dirt" no matter how the frame is
    // graded, which fights the low-poly look. These flat per-layer colours replace them.
    // `stylize.x` blends between the two (0 = photo textures, 1 = flat colour) so the
    // change stays A/B-able instead of being a one-way rewrite.
    //
    // Packed into ONE binding on purpose: seven separate `#[uniform]` attributes each
    // allocate their own buffer, which overran the Metal vertex-stage buffer limit
    // ("pipeline needs too many buffers in the vertex stage: 1 vertex and 17 layout").
    #[uniform(124)]
    pub palette: TerrainPalette,
}

impl MaterialExtension for TerrainSplatExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_splat.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "shaders/terrain_splat.wgsl".into()
    }
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct TerrainPalette {
    pub grass: Vec4,
    pub dirt: Vec4,
    pub sand: Vec4,
    pub cobble: Vec4,
    pub rock: Vec4,
    pub stylize: Vec4,
    pub bands: Vec4,
    /// Cloud shadow field: x coverage, y inv world scale, zw wind offset.
    /// Lives in the palette (binding 124) because a new binding would overrun
    /// the Metal buffer limit documented on `TerrainSplatExtension::palette`.
    pub clouds_a: Vec4,
    /// Cloud shadow field: xy sun projection (sun_dir.xz / sun_dir.y),
    /// z shadow strength, w seed phase.
    pub clouds_b: Vec4,
    /// x: anchor time (client seconds), z: drift speed in client secs
    /// (world speed x warp); shaders extrapolate wind past the anchor.
    pub clouds_c: Vec4,
    /// x: map half extent (m), y: climate seed phase, zw: reserved. Static
    /// per map; pushed by sync_cloud_shadow_params alongside the cloud lanes.
    pub climate: Vec4,
    /// THE storm system (one per map): xy = cell center at the wind anchor
    /// (shaders extrapolate it with the cloud drift), z = storminess 0..1
    /// (0 whenever clouds are disabled -- the rain must vanish with its sky),
    /// w: reserved. Live per frame-ish; pushed by sync_cloud_shadow_params.
    pub storm: Vec4,
}

/// Flat palette for the stylised terrain.
///
/// Slightly desaturated, slightly blue-shifted in shadow-facing values so the world reads
/// storybook rather than photographic. The four layer colours come from [`TERRAIN_LAYERS`],
/// so tools and the rendered ground cannot disagree.
///
/// Authored values sit darker/richer than the intended on-screen result: the sun's exposure
/// and the aerial haze both wash them out. A first pass used mid-value colours and the whole
/// world came out pale and bland.
pub fn stylized_palette() -> TerrainPalette {
    TerrainPalette {
        grass: Vec4::from_array(TERRAIN_LAYERS[0].color),
        dirt: Vec4::from_array(TERRAIN_LAYERS[1].color),
        sand: Vec4::from_array(TERRAIN_LAYERS[2].color),
        cobble: Vec4::from_array(TERRAIN_LAYERS[3].color),
        // Not a splat layer: blended in by slope, so it has no weightmap channel.
        rock: Vec4::new(0.36, 0.35, 0.38, 1.0),
        // 1.0 stylised, 5 bands, gentle banding, strong slope rock.
        stylize: Vec4::new(1.0, 5.0, 0.10, 0.85),
        // Bands span 0..90m of height, with a little texture break-up so large flat areas
        // are not perfectly uniform (which reads as untextured rather than stylised).
        //
        // `bands.z` is the entire remaining contribution of the 5.3 MB albedo array: a
        // chroma-only grain multiply. Set it to 0.0 to preview the terrain with no albedo
        // texture at all -- that is exactly what deleting the array would look like.
        bands: Vec4::new(0.0, 90.0, 0.18, 0.0),
        // Cloud shadows start off (strength 0 = shade 1.0 exactly);
        // sync_cloud_shadow_params owns these fields at runtime.
        clouds_a: Vec4::ZERO,
        clouds_b: Vec4::ZERO,
        clouds_c: Vec4::ZERO,
        // Half extent must default SANE, not zero: a zero half extent makes
        // the shader read |z| metres as latitude -- every chunk flashes full
        // snow/desert for any frame that renders before the first
        // sync_cloud_shadow_params write lands.
        climate: Vec4::new(4096.0, 0.0, 0.0, 0.0),
        // Storm center parked far off-world, strength 0.
        storm: Vec4::new(1.0e8, 1.0e8, 0.0, 0.0),
    }
}

// ---------------------------------------------------------------------------
// Samplers
// ---------------------------------------------------------------------------

/// Sampler for the tiled terrain arrays.
///
/// `anisotropy_clamp` is the one filtering knob that changes what you see here. The RTS camera
/// sits ~40 degrees above the horizon, so ground textures are always viewed at a grazing angle --
/// the exact case trilinear filtering handles badly, blurring along the view direction. 8x costs
/// almost nothing on any GPU that can run this game and it only applies where minification is
/// anisotropic. It requires all three filters to be Linear, which they are.
pub fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    })
}

/// Sampler for the per-chunk weightmap.
///
/// Clamped, never repeated: this is one 64x64 image per chunk, and wrapping it would pull the
/// opposite edge's weights across the chunk seam.
pub fn weightmap_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table's position IS the array layer index. Everything downstream assumes it:
    /// the KTX2 build order, the shader's per-layer branches, and the weightmap channels.
    #[test]
    fn layer_table_indices_match() {
        for (i, def) in TERRAIN_LAYERS.iter().enumerate() {
            assert_eq!(
                def.layer.index(),
                i,
                "TERRAIN_LAYERS[{i}] holds {:?}, whose index() is {}",
                def.layer,
                def.layer.index()
            );
        }
    }

    /// The weightmap is RGBA: four channels, four layers, no room to grow without changing
    /// its format and every shader branch that reads it.
    #[test]
    fn layer_count_matches_weightmap_channels() {
        assert_eq!(TERRAIN_LAYERS.len(), 4);
    }

    #[test]
    fn tiling_vector_follows_the_table() {
        let t = layer_tiling();
        assert_eq!(t.x, TERRAIN_LAYERS[0].tile_metres);
        assert_eq!(t.y, TERRAIN_LAYERS[1].tile_metres);
        assert_eq!(t.z, TERRAIN_LAYERS[2].tile_metres);
        assert_eq!(t.w, TERRAIN_LAYERS[3].tile_metres);
    }

    /// The palette and layer table expose the same numbers.
    #[test]
    fn palette_colors_come_from_the_table() {
        let p = stylized_palette();
        assert_eq!(p.grass, Vec4::from_array(TERRAIN_LAYERS[0].color));
        assert_eq!(p.dirt, Vec4::from_array(TERRAIN_LAYERS[1].color));
        assert_eq!(p.sand, Vec4::from_array(TERRAIN_LAYERS[2].color));
        assert_eq!(p.cobble, Vec4::from_array(TERRAIN_LAYERS[3].color));
    }
}
