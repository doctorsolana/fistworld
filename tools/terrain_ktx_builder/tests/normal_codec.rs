//! Which block format should the normal array use?
//!
//! BC7 and BC5 are both 8 bits per texel, so this is not a size question -- the
//! two arrays come out byte-for-byte the same length. It is only a question of
//! which one bends the surface normals less, and that is measurable rather than
//! arguable, so it is measured here instead of being decided by reputation.
//!
//! The received wisdom is that BC5 wins: it stores two channels as independent
//! BC4 blocks (8 interpolated endpoints, 3-bit indices each) and the shader
//! rebuilds Z as `sqrt(1 - x^2 - y^2)`, whereas BC7 spends part of its block
//! budget on a blue channel that a reconstructed normal throws away. Received
//! wisdom is exactly the sort of thing that turns out to be false on the
//! specific images a specific project ships, which is why this asserts on the
//! real source art and not on a synthetic gradient.
//!
//! Run it:
//! ```text
//! cargo test -p terrain_ktx_builder --features ktx2-native -- --nocapture
//! ```
#![cfg(feature = "ktx2-native")]

use std::path::PathBuf;

use image::imageops::FilterType;
use intel_tex_2::{bc7, RgSurface, RgbaSurface};
use shared::terrain::TERRAIN_LAYERS;

/// Decoded, unit-length tangent-space normal for one texel.
fn decode(x: u8, y: u8, z: u8) -> [f32; 3] {
    let v = [
        x as f32 / 127.5 - 1.0,
        y as f32 / 127.5 - 1.0,
        z as f32 / 127.5 - 1.0,
    ];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Rebuild Z the way a BC5 shader path has to: the map is unit length by
/// definition, so the third component is implied by the other two.
fn reconstruct(x: u8, y: u8) -> [f32; 3] {
    let nx = x as f32 / 127.5 - 1.0;
    let ny = y as f32 / 127.5 - 1.0;
    let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    [nx / len, ny / len, nz / len]
}

/// Angle between two unit vectors, in degrees. The honest unit for a normal
/// map: a per-channel byte delta says nothing about how far the light moved.
fn angle_degrees(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    dot.acos().to_degrees()
}

struct Error {
    mean: f32,
    p99: f32,
    max: f32,
}

/// Compare a decoded array of texels against the source normals.
fn measure(source: &[[f32; 3]], decoded: &[[f32; 3]]) -> Error {
    let mut angles: Vec<f32> = source
        .iter()
        .zip(decoded)
        .map(|(a, b)| angle_degrees(*a, *b))
        .collect();
    let mean = angles.iter().sum::<f32>() / angles.len() as f32;
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Error {
        mean,
        p99: angles[angles.len() * 99 / 100],
        max: *angles.last().unwrap(),
    }
}

#[test]
fn bc5_is_measured_against_bc7_for_normal_maps() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("asset_creation/terrain_source");

    // Every distinct normal source the layer table names -- deduplicated,
    // because sand borrows dirt's map and encoding it twice would weight the
    // average toward one image.
    let mut sources: Vec<&str> = TERRAIN_LAYERS.iter().map(|l| l.normal_source).collect();
    sources.sort_unstable();
    sources.dedup();

    let mut bc7_worse_everywhere = true;

    for name in sources {
        let img = image::open(root.join(name))
            .unwrap_or_else(|e| panic!("cannot open {name}: {e}"))
            .resize_exact(1024, 1024, FilterType::Lanczos3)
            .to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);

        let truth: Vec<[f32; 3]> = img
            .pixels()
            .map(|p| decode(p.0[0], p.0[1], p.0[2]))
            .collect();

        // --- BC7, exactly as the builder encodes it today ---
        let bc7_blocks = bc7::compress_blocks(
            &bc7::opaque_basic_settings(),
            &RgbaSurface {
                data: img.as_raw(),
                width: w as u32,
                height: h as u32,
                stride: w as u32 * 4,
            },
        );
        let mut bc7_out = vec![0u32; w * h];
        texture2ddecoder::decode_bc7(&bc7_blocks, w, h, &mut bc7_out).unwrap();
        let bc7_normals: Vec<[f32; 3]> = bc7_out
            .iter()
            .map(|p| {
                let [b, g, r, _] = p.to_le_bytes();
                decode(r, g, b)
            })
            .collect();

        // --- BC5: X and Y only, Z implied ---
        let rg: Vec<u8> = img.pixels().flat_map(|p| [p.0[0], p.0[1]]).collect();
        let bc5_blocks = intel_tex_2::bc5::compress_blocks(&RgSurface {
            data: &rg,
            width: w as u32,
            height: h as u32,
            stride: w as u32 * 2,
        });
        let mut bc5_out = vec![0u32; w * h];
        texture2ddecoder::decode_bc5(&bc5_blocks, w, h, &mut bc5_out).unwrap();
        let bc5_normals: Vec<[f32; 3]> = bc5_out
            .iter()
            .map(|p| {
                let [_, g, r, _] = p.to_le_bytes();
                reconstruct(r, g)
            })
            .collect();

        // The BC5 path also loses whatever the source said about Z, so compare
        // it against the same reconstruction applied to the *uncompressed*
        // image. That separates "BC5 compresses worse" from "this normal map
        // was not unit length to begin with", which are different problems with
        // different fixes.
        let rebuilt_truth: Vec<[f32; 3]> = img
            .pixels()
            .map(|p| reconstruct(p.0[0], p.0[1]))
            .collect();

        let bc7_err = measure(&truth, &bc7_normals);
        let bc5_err = measure(&truth, &bc5_normals);
        let bc5_codec_only = measure(&rebuilt_truth, &bc5_normals);
        let z_cost = measure(&truth, &rebuilt_truth);

        println!("\n{name}  ({w}x{h}, {} KB either way)", bc5_blocks.len() / 1024);
        println!(
            "  BC7            mean {:.3}deg  p99 {:.3}deg  max {:.3}deg",
            bc7_err.mean, bc7_err.p99, bc7_err.max
        );
        println!(
            "  BC5            mean {:.3}deg  p99 {:.3}deg  max {:.3}deg",
            bc5_err.mean, bc5_err.p99, bc5_err.max
        );
        println!(
            "    of which codec  {:.3}deg   Z-reconstruction alone {:.3}deg",
            bc5_codec_only.mean, z_cost.mean
        );
        assert_eq!(
            bc5_blocks.len(),
            bc7_blocks.len(),
            "BC5 and BC7 must be the same size, or this is not a like-for-like comparison"
        );

        if bc5_err.mean >= bc7_err.mean {
            bc7_worse_everywhere = false;
        }
    }

    println!(
        "\nBC5 beats BC7 on every layer: {bc7_worse_everywhere}\n\
         (informational -- this test measures, it does not mandate)"
    );
}
