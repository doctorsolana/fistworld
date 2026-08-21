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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_bounds_map_to_the_full_panel() {
        let bounds = MapBounds {
            min: [-100.0, -50.0],
            max: [300.0, 150.0],
        };

        assert_eq!(world_to_map(-100.0, -50.0, bounds), Some((0.0, 0.0)));
        assert_eq!(world_to_map(100.0, 50.0, bounds), Some((256.0, 256.0)));
        assert_eq!(
            world_to_map(300.0, 150.0, bounds),
            Some((MAP_PANEL_SIZE, MAP_PANEL_SIZE))
        );
    }
}
