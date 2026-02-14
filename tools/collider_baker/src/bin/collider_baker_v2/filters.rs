use std::collections::HashSet;

use bevy::prelude::*;

pub(crate) fn trunk_slice_y(mut vertices: Vec<Vec3>, percent: f32) -> Vec<Vec3> {
    let percent = percent.clamp(0.01, 1.0);
    let original = vertices.clone();
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for v in vertices.iter() {
        min_y = min_y.min(v.y);
        max_y = max_y.max(v.y);
    }

    let height = (max_y - min_y).max(1e-4);
    let threshold = min_y + height * percent;

    vertices.retain(|v| v.y <= threshold);
    if vertices.len() < 16 {
        original
    } else {
        vertices
    }
}

pub(crate) fn filter_xz_percentile(vertices: Vec<Vec3>, percentile: f32) -> Vec<Vec3> {
    if vertices.len() < 4 {
        return vertices;
    }
    let percentile = percentile.clamp(0.5, 1.0);

    let mut distances: Vec<(usize, f32)> = vertices
        .iter()
        .enumerate()
        .map(|(i, v)| (i, (v.x * v.x + v.z * v.z).sqrt()))
        .collect();

    distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let idx = ((distances.len() as f32 * percentile) as usize).min(distances.len() - 1);
    let threshold = distances[idx].1;

    let filtered: Vec<Vec3> = vertices
        .iter()
        .filter(|v| (v.x * v.x + v.z * v.z).sqrt() <= threshold)
        .copied()
        .collect();

    if filtered.len() < 16 {
        warn!(
            "XZ percentile filter removed too many vertices ({} -> {}), using larger threshold",
            vertices.len(),
            filtered.len()
        );
        return vertices;
    }

    filtered
}

pub(crate) fn dedup_quantized(vertices: Vec<Vec3>, grid: f32) -> Vec<Vec3> {
    let inv = 1.0 / grid.max(1e-6);
    let mut seen: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut out = Vec::with_capacity(vertices.len());

    for v in vertices {
        let key = (
            (v.x * inv).round() as i32,
            (v.y * inv).round() as i32,
            (v.z * inv).round() as i32,
        );
        if seen.insert(key) {
            out.push(v);
        }
    }

    out
}
