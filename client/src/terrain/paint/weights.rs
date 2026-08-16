//! weights systems.

use super::*;

pub(crate) fn build_weightmap_image(weights: &[[u8; 4]], resolution: u32) -> Image {
    let mut data = Vec::with_capacity((resolution * resolution * 4) as usize);
    for w in weights.iter() {
        data.extend_from_slice(w);
    }

    let mut image = Image::new(
        Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        WEIGHTMAP_FORMAT,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = weightmap_sampler();
    image
}

pub(crate) fn build_weightmap_from_weights(
    weights: Vec<[u8; 4]>,
    resolution: u32,
    images: &mut Assets<Image>,
) -> WeightMapData {
    let image = build_weightmap_image(&weights, resolution);
    let handle = images.add(image);
    WeightMapData {
        handle,
        resolution,
        base_weights: weights.clone(),
        weights,
    }
}

pub(crate) fn log_weightmap_stats(coord: ChunkCoord, data: &WeightMapData, context: &str) {
    let mut min = [1.0f32; 4];
    let mut max = [0.0f32; 4];
    for w in data.weights.iter() {
        for i in 0..4 {
            let wf = w[i] as f32 / 255.0;
            min[i] = min[i].min(wf);
            max[i] = max[i].max(wf);
        }
    }
    debug!(
        "Weightmap {:?} ({}) min [{:.2}, {:.2}, {:.2}, {:.2}] max [{:.2}, {:.2}, {:.2}, {:.2}]",
        coord, context, min[0], min[1], min[2], min[3], max[0], max[1], max[2], max[3]
    );
    if max[3] > 0.02 {
        info!(
            "Cobblestone present in {:?} ({}) max {:.2}",
            coord, context, max[3]
        );
    }
}
