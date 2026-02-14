//! projection systems.

use super::*;

pub(super) fn world_to_map(x: f32, z: f32, bounds: MapBounds) -> Option<(f32, f32)> {
    let width = bounds.width();
    let depth = bounds.depth();
    if width <= 0.0 || depth <= 0.0 {
        return None;
    }
    let nx = ((x - bounds.min[0]) / width).clamp(0.0, 1.0);
    let nz = ((z - bounds.min[1]) / depth).clamp(0.0, 1.0);
    let map_x = nx * MAP_PANEL_SIZE;
    let map_y = nz * MAP_PANEL_SIZE;
    Some((map_x, map_y))
}
