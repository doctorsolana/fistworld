//! Packs the terrain splat images into two BC7-compressed KTX2 texture arrays.
//!
//! Run:
//! ```text
//! cargo run -p terrain_ktx_builder --features ktx2-native
//! ```
//!
//! **Layer order is not defined here.** It comes from `shared::terrain::TERRAIN_LAYERS`, which
//! is also what the renderer reads for tiling and palette colours. That indirection is the
//! entire point: the four-layer contract used to live in seven uncoordinated places, and it had
//! already drifted -- the array's slot 1 held a grass image while every other file called slot 1
//! "dirt", and slots 1 and 2 of the normal array held the same picture twice under two names.
//!
//! ## Why BC7
//!
//! The arrays used to ship as uncompressed `R8G8B8A8`: 21.3 MB each on disk and the same again
//! in VRAM, 42.6 MB total, for four 1024x1024 images. BC7 is 1 byte per texel against 4, so the
//! pair lands near 10.6 MB with no visible difference -- and the albedo array only supplies an
//! 18% chroma grain, so it had quality budget to spare. BC7 is also the portable choice: every
//! desktop GPU has it, on Windows and on both Mac architectures. ASTC would have been Apple-only.
//!
//! The shader samples `.rgb` from albedo and `.xyz` from normals, so alpha is dead weight in
//! both; the opaque encoder settings spend that budget on the channels that are read.
//!
//! ## Why the normals are BC7 too, and not BC5
//!
//! BC5 is the textbook answer for normal maps: two independent BC4 channels, Z rebuilt in the
//! shader as `sqrt(1 - x^2 - y^2)`, none of the block budget wasted on a blue channel nobody
//! reads. It is also the same 8 bits per texel, so it would have cost nothing.
//!
//! It was tried and it is **worse here**. `tests/normal_codec.rs` encodes the real source art
//! both ways and measures the angular error of the decoded normals:
//!
//! | source | BC7 mean | BC5 mean | BC5 p99 |
//! |---|---|---|---|
//! | `Ground_Normals_01` | 0.031 deg | 0.023 deg | 0.464 deg |
//! | `Dirt_Normals_01` | 0.030 deg | 0.028 deg | 0.466 deg |
//! | `Cobblestone_Normals_01` | **0.191 deg** | **0.776 deg** | **11.755 deg** |
//!
//! Cobblestone decides it. Its mortar gaps hold normals near the horizon, where `x^2 + y^2`
//! approaches 1 and `sqrt(1 - x^2 - y^2)` turns a small XY error into a large Z error -- worst
//! texel 50.6 deg against BC7's 25.5 deg. The test also shows the reconstruction costs 0.375 deg
//! on that image *before any compression*, which means the map is not unit length to begin with
//! and its stored Z carries real information that BC5 cannot keep.
//!
//! So: same size, four times the error on the one layer that has relief worth having. BC7 stays.
//! Run the test before revisiting -- if the cobblestone art is ever replaced with a map that is
//! properly normalised, the answer could flip.

use anyhow::{Context, Result};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use std::fs;
use std::path::{Path, PathBuf};

use shared::terrain::TERRAIN_LAYERS;

/// Which images to pack, in layer order, pulled from the one table that defines it.
fn albedo_sources() -> Vec<&'static str> {
    TERRAIN_LAYERS.iter().map(|l| l.albedo_source).collect()
}

fn normal_sources() -> Vec<&'static str> {
    TERRAIN_LAYERS.iter().map(|l| l.normal_source).collect()
}

fn main() -> Result<()> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("Failed to resolve workspace root from CARGO_MANIFEST_DIR")?
        .to_path_buf();

    // Inputs live outside client/assets. They are 2k source art that only this tool reads, and
    // shipping them meant 4.6 MB of the game bundle was build input the client never opened.
    let textures_root = workspace_root.join("asset_creation/terrain_source");
    // Resized intermediates are build artifacts, so they go where build artifacts go.
    let optimized_dir = workspace_root.join("target/terrain_1k");
    // Only the two packed arrays are assets.
    let output_dir = workspace_root.join("client/assets/textures/terrain/optimized_1k");

    fs::create_dir_all(&optimized_dir).context("Failed to create intermediate directory")?;
    fs::create_dir_all(&output_dir).context("Failed to create output directory")?;

    println!("Workspace root: {}", workspace_root.display());
    println!("Sources:       {}", textures_root.display());
    println!("Intermediates: {}", optimized_dir.display());
    println!("Output:        {}", output_dir.display());

    // Resize every distinct source once. `sources` may name the same file twice (sand borrows
    // dirt's normal map), so deduplicate rather than doing the work again.
    let mut seen: Vec<&str> = Vec::new();
    for name in albedo_sources().into_iter().chain(normal_sources()) {
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);

        let source = textures_root.join(name);
        if !source.exists() {
            anyhow::bail!("Missing source texture: {}", source.display());
        }
        let image = image::open(&source)
            .with_context(|| format!("Failed to decode image {}", source.display()))?;
        resize_to_1024(image)
            .save(optimized_dir.join(name))
            .with_context(|| format!("Failed to save resized image {name}"))?;
    }

    #[cfg(feature = "ktx2-native")]
    {
        use ktx2::Format;

        let albedo_output = output_dir.join("terrain_albedo_array.ktx2");
        let normal_output = output_dir.join("terrain_normal_array.ktx2");

        // sRGB for colour, UNORM for normals: a normal map is a direction, not a colour, and
        // running it through the sRGB curve would bend the lighting.
        build_ktx2_array(
            &optimized_dir,
            &albedo_sources(),
            Format::BC7_SRGB_BLOCK,
            &albedo_output,
        )?;
        build_ktx2_array(
            &optimized_dir,
            &normal_sources(),
            Format::BC7_UNORM_BLOCK,
            &normal_output,
        )?;

        println!("Generated KTX2 arrays:");
        for path in [&albedo_output, &normal_output] {
            let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            println!(
                "  {} ({:.1} MB)",
                path.display(),
                bytes as f64 / 1_048_576.0
            );
        }
    }

    #[cfg(not(feature = "ktx2-native"))]
    {
        println!(
            "Skipped KTX2 array generation (enable with `cargo run -p terrain_ktx_builder --features ktx2-native`)"
        );
    }

    Ok(())
}

fn resize_to_1024(image: DynamicImage) -> DynamicImage {
    let (w, h) = image.dimensions();
    if w == 1024 && h == 1024 {
        return image;
    }
    image.resize_exact(1024, 1024, FilterType::Lanczos3)
}

/// Smallest mip we generate. BC7 addresses 4x4 blocks, so going below this would mean padding
/// partial blocks for levels that describe an area the size of a football pitch at 5-8 m tiling.
/// A partial chain is legal KTX2 and the levels below 4x4 are never selected in practice.
#[cfg(feature = "ktx2-native")]
const MIN_MIP_DIM: u32 = 4;

/// Every level's data must begin at a multiple of this. The KTX2 spec requires alignment to the
/// lcm of the texel block size and 4; a BC7 block is 16 bytes, so lcm(16, 4) = 16.
#[cfg(feature = "ktx2-native")]
const LEVEL_ALIGNMENT: usize = 16;

#[cfg(feature = "ktx2-native")]
fn build_ktx2_array(
    optimized_dir: &Path,
    sources: &[&str],
    format: ktx2::Format,
    output_path: &Path,
) -> Result<()> {
    use intel_tex_2::{bc7, RgbaSurface};
    use ktx2::{dfd, Header, Index, LevelIndex};

    let mut images = Vec::with_capacity(sources.len());
    for name in sources {
        let path = optimized_dir.join(name);
        let img = image::open(&path)
            .with_context(|| format!("Failed to decode optimized image {}", path.display()))?
            .to_rgba8();
        images.push(img);
    }

    let width = images[0].width();
    let height = images[0].height();
    for (idx, img) in images.iter().enumerate() {
        if img.width() != width || img.height() != height {
            anyhow::bail!(
                "Layer size mismatch at layer {}: expected {}x{}, got {}x{}",
                idx,
                width,
                height,
                img.width(),
                img.height()
            );
        }
    }

    // Levels from full size down to MIN_MIP_DIM inclusive.
    let mut level_dims = Vec::new();
    let (mut w, mut h) = (width, height);
    loop {
        level_dims.push((w, h));
        if w <= MIN_MIP_DIM || h <= MIN_MIP_DIM {
            break;
        }
        w = (w / 2).max(MIN_MIP_DIM);
        h = (h / 2).max(MIN_MIP_DIM);
    }
    let level_count = level_dims.len();

    // Alpha is never sampled from either array, so let the encoder spend its whole budget on
    // the channels the shader reads. `basic` rather than `slow`: on flat photographic ground
    // under an 18% grain multiply, the quality difference does not survive to the screen, and
    // `slow` turns a seconds-long build into a minutes-long one.
    let settings = bc7::opaque_basic_settings();

    // Encode: for each level, for each layer. Mip chains are built per layer by successive
    // halving from that layer's full-size image.
    let mut per_level: Vec<Vec<Vec<u8>>> = vec![Vec::with_capacity(images.len()); level_count];
    for image in &images {
        let mut mip = image.clone();
        for (level, &(lw, lh)) in level_dims.iter().enumerate() {
            if mip.width() != lw || mip.height() != lh {
                mip = image::imageops::resize(&mip, lw, lh, FilterType::Triangle);
            }
            let surface = RgbaSurface {
                data: mip.as_raw(),
                width: lw,
                height: lh,
                stride: lw * 4,
            };
            per_level[level].push(bc7::compress_blocks(&settings, &surface));

            let (next_w, next_h) = ((lw / 2).max(MIN_MIP_DIM), (lh / 2).max(MIN_MIP_DIM));
            if next_w != lw || next_h != lh {
                mip = image::imageops::resize(&mip, next_w, next_h, FilterType::Triangle);
            }
        }
    }

    // --- Assemble the container ---
    //
    // Layout: identifier+header (80) | level index | DFD | KVD | level data.
    // Level data is written smallest mip first, which is the order the spec recommends so a
    // streaming reader gets a usable low-res image before the rest arrives.
    // The crate derives a spec-correct descriptor (and the matching `type_size`) from the
    // format, so the fiddly part of KTX2 is not hand-written here.
    let (basic, type_size) = dfd::Basic::from_format(format)
        .map_err(|e| anyhow::anyhow!("failed to build DFD for {format:?}: {e}"))?;
    let dfd_bytes = dfd::Block::Basic(basic).to_vec();
    // The DFD section is prefixed by its own total size, itself included.
    let mut dfd_section = Vec::with_capacity(4 + dfd_bytes.len());
    dfd_section.extend_from_slice(&((4 + dfd_bytes.len()) as u32).to_le_bytes());
    dfd_section.extend_from_slice(&dfd_bytes);

    let kvd_section = key_value_section(&[("KTXwriter", "terrain_ktx_builder")]);

    let header_len = Header::LENGTH;
    let level_index_len = level_count * LevelIndex::LENGTH;
    let dfd_offset = header_len + level_index_len;
    let kvd_offset = dfd_offset + dfd_section.len();
    let mut cursor = kvd_offset + kvd_section.len();

    // Offsets, computed smallest-level-first but stored per level number.
    let mut level_indices = vec![
        LevelIndex {
            byte_offset: 0,
            byte_length: 0,
            uncompressed_byte_length: 0,
        };
        level_count
    ];
    let mut level_blobs: Vec<(usize, Vec<u8>)> = Vec::with_capacity(level_count);
    for level in (0..level_count).rev() {
        cursor = cursor.next_multiple_of(LEVEL_ALIGNMENT);
        let blob: Vec<u8> = per_level[level].concat();
        level_indices[level] = LevelIndex {
            byte_offset: cursor as u64,
            byte_length: blob.len() as u64,
            uncompressed_byte_length: blob.len() as u64,
        };
        cursor += blob.len();
        level_blobs.push((level, blob));
    }

    let header = Header {
        format: Some(format),
        // 1 for block-compressed formats; taken from the DFD builder rather than assumed.
        type_size,
        pixel_width: width,
        pixel_height: height,
        pixel_depth: 0,
        layer_count: images.len() as u32,
        face_count: 1,
        level_count: level_count as u32,
        supercompression_scheme: None,
        index: Index {
            dfd_byte_offset: dfd_offset as u32,
            dfd_byte_length: dfd_section.len() as u32,
            kvd_byte_offset: kvd_offset as u32,
            kvd_byte_length: kvd_section.len() as u32,
            sgd_byte_offset: 0,
            sgd_byte_length: 0,
        },
    };

    let mut out = Vec::with_capacity(cursor);
    out.extend_from_slice(&header.as_bytes());
    for index in &level_indices {
        out.extend_from_slice(&index.as_bytes());
    }
    out.extend_from_slice(&dfd_section);
    out.extend_from_slice(&kvd_section);
    for (level, blob) in level_blobs {
        let target = level_indices[level].byte_offset as usize;
        out.resize(target, 0); // mip padding
        out.extend_from_slice(&blob);
    }

    // Parse our own output before shipping it. Hand-assembled binary formats fail silently and
    // at load time, which is the worst place to find out.
    let reader = ktx2::Reader::new(&out)
        .map_err(|e| anyhow::anyhow!("wrote a KTX2 file that will not parse: {e:?}"))?;
    let parsed = reader.header();
    anyhow::ensure!(parsed.format == Some(format), "format did not round-trip");
    anyhow::ensure!(
        parsed.level_count as usize == level_count && reader.levels().len() == level_count,
        "level count did not round-trip"
    );
    anyhow::ensure!(
        parsed.layer_count as usize == images.len(),
        "layer count did not round-trip"
    );
    for (level, stored) in reader.levels().enumerate() {
        let expect: usize = per_level[level].iter().map(|b| b.len()).sum();
        anyhow::ensure!(
            stored.data.len() == expect,
            "level {level} is {} bytes, expected {expect}",
            stored.data.len()
        );
    }

    fs::write(output_path, &out)
        .with_context(|| format!("Failed to write KTX2 file {}", output_path.display()))?;

    Ok(())
}

/// KTX2 key/value data: u32 length, key, NUL, value, NUL, padded to 4 bytes.
#[cfg(feature = "ktx2-native")]
fn key_value_section(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in entries {
        let mut kv = Vec::new();
        kv.extend_from_slice(key.as_bytes());
        kv.push(0);
        kv.extend_from_slice(value.as_bytes());
        kv.push(0);
        out.extend_from_slice(&(kv.len() as u32).to_le_bytes());
        out.extend_from_slice(&kv);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}
