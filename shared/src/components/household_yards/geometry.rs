//! Small convex parcel operations; setup-time only, in ordinary X/Z coordinates.

use bevy::prelude::*;

pub(super) fn area(points: &[Vec2]) -> f32 {
    edges(points).map(|(a, b)| a.perp_dot(b)).sum::<f32>() * 0.5
}

pub(super) fn edges(points: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .take(points.len())
}

pub(super) fn bounds(points: &[Vec2]) -> (Vec2, Vec2) {
    points.iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    )
}

pub(super) fn centroid(points: &[Vec2]) -> Vec2 {
    let mut center = Vec2::ZERO;
    let mut weight = 0.0;
    for (a, b) in edges(points) {
        let cross = a.perp_dot(b);
        center += (a + b) * cross;
        weight += cross;
    }
    if weight.abs() < 0.0001 {
        points.iter().copied().sum::<Vec2>() / points.len().max(1) as f32
    } else {
        center / (3.0 * weight)
    }
}

pub(super) fn contains(points: &[Vec2], p: Vec2, margin: f32) -> bool {
    points.len() >= 3
        && edges(points).all(|(a, b)| {
            let e = b - a;
            e.perp_dot(p - a) >= -margin * e.length()
        })
}

pub(super) fn valid_convex(points: &[Vec2]) -> bool {
    (3..=16).contains(&points.len())
        && points.iter().all(|p| p.is_finite())
        && area(points) > 0.01
        && points.iter().enumerate().all(|(i, a)| {
            let b = points[(i + 1) % points.len()];
            let c = points[(i + 2) % points.len()];
            a.distance_squared(b) > 0.0025 && (b - *a).perp_dot(c - b) >= -0.0001
        })
}

/// Retain n·p <= distance, preserving a counter-clockwise convex boundary.
pub(super) fn clip(points: &[Vec2], n: Vec2, distance: f32) -> Vec<Vec2> {
    let mut next = Vec::with_capacity(points.len() + 1);
    for (a, b) in edges(points) {
        let da = n.dot(a) - distance;
        let db = n.dot(b) - distance;
        if da <= 0.00001 {
            next.push(a);
        }
        if (da < 0.0) != (db < 0.0) {
            next.push(a.lerp(b, da / (da - db)));
        }
    }
    // Repeated corner intersections create zero-length rails and unstable
    // normals. Remove only sub-centimetre duplicates, not useful clipped edges.
    next.dedup_by(|a, b| a.distance_squared(*b) < 0.0001);
    if next.len() > 1 && next[0].distance_squared(*next.last().unwrap()) < 0.0001 {
        next.pop();
    }
    next
}

pub(super) fn overlaps(a: &[Vec2], b: &[Vec2], margin: f32) -> bool {
    [a, b].into_iter().all(|polygon| {
        edges(polygon).all(|(p, q)| {
            let e = q - p;
            let n = Vec2::new(e.y, -e.x).normalize_or_zero();
            let max_a = a
                .iter()
                .map(|p| n.dot(*p))
                .fold(f32::NEG_INFINITY, f32::max);
            let min_a = a.iter().map(|p| n.dot(*p)).fold(f32::INFINITY, f32::min);
            let max_b = b
                .iter()
                .map(|p| n.dot(*p))
                .fold(f32::NEG_INFINITY, f32::max);
            let min_b = b.iter().map(|p| n.dot(*p)).fold(f32::INFINITY, f32::min);
            max_a >= min_b - margin && max_b >= min_a - margin
        })
    })
}
