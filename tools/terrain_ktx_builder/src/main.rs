use anyhow::{Context, Result};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
#[cfg(feature = "ktx2-native")]
use ktx2_rw::{Ktx2Texture, VkFormat};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct TextureSpec {
    file_name: &'static str,
}

const ALBEDO_SPECS: [TextureSpec; 4] = [
    TextureSpec {
        file_name: "Grass_Texture_01.png",
    },
    TextureSpec {
        file_name: "Grass_Texture_02.png",
    },
    TextureSpec {
        file_name: "Dirt_Texture_01.png",
    },
    TextureSpec {
        file_name: "Cobblestone_Texture_01.png",
    },
];

const NORMAL_SPECS: [TextureSpec; 4] = [
    TextureSpec {
        file_name: "Ground_Normals_01.png",
    },
    TextureSpec {
        file_name: "Ground_Normals_02.png",
    },
    TextureSpec {
        file_name: "Dirt_Normals_01.png",
    },
    TextureSpec {
        file_name: "Cobblestone_Normals_01.png",
    },
];

fn main() -> Result<()> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("Failed to resolve workspace root from CARGO_MANIFEST_DIR")?
        .to_path_buf();

    let textures_root = workspace_root.join("client/assets/textures");
    let terrain_root = textures_root.join("terrain");
    let original_dir = terrain_root.join("original_2k");
    let optimized_dir = terrain_root.join("optimized_1k");

    fs::create_dir_all(&original_dir).context("Failed to create original_2k directory")?;
    fs::create_dir_all(&optimized_dir).context("Failed to create optimized_1k directory")?;

    println!("Workspace root: {}", workspace_root.display());
    println!("Backing up textures to: {}", original_dir.display());
    println!("Writing 1k textures to: {}", optimized_dir.display());

    for spec in ALBEDO_SPECS.iter().chain(NORMAL_SPECS.iter()) {
        let source = textures_root.join(spec.file_name);
        let backup = original_dir.join(spec.file_name);
        let optimized = optimized_dir.join(spec.file_name);

        if !source.exists() {
            anyhow::bail!("Missing source texture: {}", source.display());
        }

        fs::copy(&source, &backup).with_context(|| {
            format!(
                "Failed to copy source texture to backup: {} -> {}",
                source.display(),
                backup.display()
            )
        })?;

        let image = image::open(&source)
            .with_context(|| format!("Failed to decode image {}", source.display()))?;
        let resized = resize_to_1024(image);
        resized
            .save(&optimized)
            .with_context(|| format!("Failed to save resized image {}", optimized.display()))?;
    }

    #[cfg(feature = "ktx2-native")]
    {
        let albedo_output = optimized_dir.join("terrain_albedo_array.ktx2");
        let normal_output = optimized_dir.join("terrain_normal_array.ktx2");

        build_ktx2_array(
            &optimized_dir,
            &ALBEDO_SPECS,
            VkFormat::R8G8B8A8Srgb,
            &albedo_output,
        )?;
        build_ktx2_array(
            &optimized_dir,
            &NORMAL_SPECS,
            VkFormat::R8G8B8A8Unorm,
            &normal_output,
        )?;

        println!("Generated KTX2 arrays:");
        println!("  {}", albedo_output.display());
        println!("  {}", normal_output.display());
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

#[cfg(feature = "ktx2-native")]
fn build_ktx2_array(
    optimized_dir: &Path,
    specs: &[TextureSpec; 4],
    format: VkFormat,
    output_path: &Path,
) -> Result<()> {
    let mut images = Vec::with_capacity(specs.len());
    for spec in specs {
        let path = optimized_dir.join(spec.file_name);
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

    let max_dim = width.max(height);
    let mip_levels = u32::BITS - max_dim.leading_zeros();

    let mut texture =
        Ktx2Texture::create(width, height, 1, specs.len() as u32, 1, mip_levels, format)
            .context("Failed to create KTX2 texture")?;

    for (layer, image) in images.iter().enumerate() {
        let mut mip_image = image.clone();
        for level in 0..mip_levels {
            texture
                .set_image_data(level, layer as u32, 0, mip_image.as_raw())
                .with_context(|| {
                    format!("Failed to write image data level {} layer {}", level, layer)
                })?;

            if level + 1 < mip_levels {
                let next_w = (mip_image.width() / 2).max(1);
                let next_h = (mip_image.height() / 2).max(1);
                mip_image =
                    image::imageops::resize(&mip_image, next_w, next_h, FilterType::Triangle);
            }
        }
    }

    texture
        .set_metadata("GeneratedBy", b"terrain_ktx_builder")
        .context("Failed to set KTX metadata")?;
    texture
        .set_metadata("MipLevels", mip_levels.to_string().as_bytes())
        .context("Failed to set KTX metadata")?;
    texture
        .write_to_file(output_path)
        .with_context(|| format!("Failed to write KTX2 file {}", output_path.display()))?;

    Ok(())
}
