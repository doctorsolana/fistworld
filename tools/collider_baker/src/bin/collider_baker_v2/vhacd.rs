use bevy::prelude::*;

use parry3d::transformation::vhacd::VHACDParameters;

pub(crate) fn vhacd_params_for_mesh(triangle_count: usize) -> VHACDParameters {
    let (
        resolution,
        concavity,
        max_convex_hulls,
        plane_downsampling,
        convex_hull_downsampling,
        label,
    ) = if triangle_count < 2_000 {
        (96, 0.015, 12, 4, 4, "low")
    } else if triangle_count < 8_000 {
        (160, 0.006, 24, 2, 2, "medium")
    } else {
        (192, 0.004, 32, 2, 2, "high")
    };

    info!(
        "VHACD params ({label}): triangles={triangle_count}, resolution={resolution}, concavity={concavity}, max_hulls={max_convex_hulls}",
    );

    VHACDParameters {
        resolution,
        concavity,
        max_convex_hulls,
        plane_downsampling,
        convex_hull_downsampling,
        convex_hull_approximation: true,
        ..Default::default()
    }
}
