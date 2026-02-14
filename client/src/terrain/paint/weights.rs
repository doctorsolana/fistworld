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

pub(crate) fn build_weightmap_weights(
    generator: &TerrainGenerator,
    coord: ChunkCoord,
    ops: &[TerrainPaintOp],
    resolution: u32,
) -> Vec<[u8; 4]> {
    let mut weights = vec![[0u8; 4]; (resolution * resolution) as usize];
    let origin = coord.world_pos();
    let step = CHUNK_SIZE / resolution as f32;

    for zi in 0..resolution {
        for xi in 0..resolution {
            let world_x = origin.x + (xi as f32 + 0.5) * step;
            let world_z = origin.z + (zi as f32 + 0.5) * step;
            let base = generator.get_surface_weights(world_x, world_z);
            let idx = (zi * resolution + xi) as usize;
            weights[idx] = weights_to_bytes(base);
        }
    }

    if !ops.is_empty() {
        let chunk_min = Vec2::new(origin.x, origin.z);
        let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);
        for op in ops.iter() {
            if paint_op_intersects_chunk(op, chunk_min, chunk_max) {
                apply_paint_op_to_weights(op, chunk_min, &mut weights, resolution);
            }
        }
    }

    weights
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
        weights,
    }
}

pub(crate) fn update_weightmap_image(data: &WeightMapData, images: &mut Assets<Image>) {
    if let Some(image) = images.get_mut(&data.handle) {
        let required = (data.resolution * data.resolution * 4) as usize;
        let bytes = image.data.get_or_insert_with(Vec::new);
        if bytes.capacity() < required {
            bytes.reserve(required - bytes.capacity());
        }
        bytes.clear();
        for w in data.weights.iter() {
            bytes.extend_from_slice(w);
        }
    }
}

pub(super) fn apply_layer_weight(weights: &mut [u8; 4], layer: TerrainLayer, t: f32) {
    if t <= 0.0 {
        return;
    }
    let t = t.clamp(0.0, 1.0);
    let mut wf = bytes_to_weights(*weights);
    let idx = layer.index();
    for (i, value) in wf.iter_mut().enumerate() {
        if i == idx {
            *value = *value + t * (1.0 - *value);
        } else {
            *value *= 1.0 - t;
        }
    }
    *weights = weights_to_bytes(wf);
}

fn bytes_to_weights(weights: [u8; 4]) -> [f32; 4] {
    [
        weights[0] as f32 / 255.0,
        weights[1] as f32 / 255.0,
        weights[2] as f32 / 255.0,
        weights[3] as f32 / 255.0,
    ]
}

fn weights_to_bytes(weights: [f32; 4]) -> [u8; 4] {
    let mut norm = [
        weights[0].max(0.0),
        weights[1].max(0.0),
        weights[2].max(0.0),
        weights[3].max(0.0),
    ];
    let sum = norm[0] + norm[1] + norm[2] + norm[3];
    if sum <= f32::EPSILON {
        return [255, 0, 0, 0];
    }
    for v in &mut norm {
        *v /= sum;
    }
    let mut out = [
        (norm[0] * 255.0).round().clamp(0.0, 255.0) as u8,
        (norm[1] * 255.0).round().clamp(0.0, 255.0) as u8,
        (norm[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        (norm[3] * 255.0).round().clamp(0.0, 255.0) as u8,
    ];
    let total = out[0] as i32 + out[1] as i32 + out[2] as i32 + out[3] as i32;
    let diff = 255 - total;
    if diff != 0 {
        let mut max_idx = 0usize;
        let mut max_val = out[0];
        for (idx, val) in out.iter().enumerate().skip(1) {
            if *val > max_val {
                max_val = *val;
                max_idx = idx;
            }
        }
        let adjusted = (out[max_idx] as i32 + diff).clamp(0, 255) as u8;
        out[max_idx] = adjusted;
    }
    out
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
