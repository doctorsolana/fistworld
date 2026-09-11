//! Height on the triangles rendered by a terrain cell, including shore refinement.
//! Cosmetic ground contact uses this; simulation keeps its smooth height sampler.

use bevy::prelude::*;

/// Corners are `[h00, h10, h01, h11]`; `uv` is in the cell's unit square.
/// Shore vertices interpolate the original four heights before triangulation.
pub(crate) fn rendered_cell_height(heights: [f32; 4], uv: Vec2, subdivisions: usize) -> f32 {
    if subdivisions == 1 {
        return triangle_height(heights, uv);
    }
    let step = 1.0 / subdivisions as f32;
    let cell = (uv / step)
        .floor()
        .min(Vec2::splat(subdivisions as f32 - 1.0));
    let lo = cell * step;
    let bilinear = |p: Vec2| {
        heights[0]
            .lerp(heights[1], p.x)
            .lerp(heights[2].lerp(heights[3], p.x), p.y)
    };
    triangle_height(
        [
            bilinear(lo),
            bilinear(lo + Vec2::X * step),
            bilinear(lo + Vec2::Y * step),
            bilinear(lo + Vec2::splat(step)),
        ],
        (uv - lo) / step,
    )
}

fn triangle_height([h00, h10, h01, h11]: [f32; 4], uv: Vec2) -> f32 {
    if uv.x + uv.y <= 1.0 {
        h00 + (h10 - h00) * uv.x + (h01 - h00) * uv.y
    } else {
        h11 + (h01 - h11) * (1.0 - uv.x) + (h10 - h11) * (1.0 - uv.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_height_uses_the_actual_cell_diagonal_and_refined_corners() {
        let saddle = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(rendered_cell_height(saddle, Vec2::splat(0.25), 1), 0.0);
        assert_eq!(rendered_cell_height(saddle, Vec2::splat(0.25), 4), 0.0625);
        assert_eq!(rendered_cell_height(saddle, Vec2::splat(0.75), 1), 0.5);
        for subdivisions in [1, 4] {
            for (uv, expected) in [
                (Vec2::ZERO, 0.0),
                (Vec2::X, 0.0),
                (Vec2::Y, 0.0),
                (Vec2::ONE, 1.0),
            ] {
                assert_eq!(rendered_cell_height(saddle, uv, subdivisions), expected);
            }
            let plane = [10.0, 10.2, 9.9, 10.1];
            let uv = Vec2::new(0.37, 0.61);
            assert!((rendered_cell_height(plane, uv, subdivisions) - 10.013).abs() < 0.00001);
        }
    }
}
