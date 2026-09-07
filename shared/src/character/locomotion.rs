//! Metre/second and water-surface contract shared by movement and animation.

pub const WALK_CYCLE_SPEED: f32 = 1.6;
pub const RUN_CYCLE_SPEED: f32 = 3.52;
pub const RUN_ENTER_SPEED: f32 = 2.6;
pub const RUN_EXIT_SPEED: f32 = 2.2;
pub const SWIM_SPEED: f32 = 1.6;
pub const SWIM_DEPTH: f32 = 0.85;

/// Shallow water remains wading. Deep-water character origins follow the water
/// surface; swim clips place the trunk below that origin and the face above it.
pub fn swimming_surface(ground: f32, water: Option<f32>) -> Option<f32> {
    water.filter(|surface| {
        surface.is_finite() && ground.is_finite() && *surface - ground >= SWIM_DEPTH
    })
}

pub fn swimming_at(ground: f32, water: Option<f32>, origin_y: f32, aboard: bool) -> bool {
    !aboard
        && swimming_surface(ground, water).is_some_and(|surface| (origin_y - surface).abs() < 0.45)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_deep_water_at_the_surface_is_swimming() {
        assert_eq!(swimming_surface(0.0, Some(0.4)), None);
        assert!(swimming_at(-2.0, Some(0.0), 0.0, false));
        assert!(!swimming_at(-2.0, Some(0.0), 1.2, false));
        assert!(!swimming_at(-2.0, Some(0.0), 0.0, true));
        assert!(!swimming_at(0.0, None, 0.0, false));
    }
}
