//! Artistic listening perspective shared by world-effect producers. Distance
//! attenuation remains native; these curves supply zoom, edge fade and timbre.

pub(super) const LISTENER_EAR_GAP: f32 = 0.25;

#[derive(Clone, Copy)]
pub(super) struct WorldSoundProfile {
    pub reference_distance: f32,
    pub edge_start: f32,
    pub hearing_radius: f32,
    pub close_zoom: f32,
    pub zoom_exponent: f32,
    pub zoom_fade_start: f32,
    pub zoom_fade_end: f32,
    pub near_cutoff_hz: f32,
    pub far_cutoff_hz: f32,
    pub filter_half_distance: f32,
    pub filter_camera_height: f32,
}

pub(super) const CART: WorldSoundProfile = WorldSoundProfile {
    reference_distance: 6.0,
    edge_start: 38.0,
    hearing_radius: 54.0,
    close_zoom: 12.0,
    zoom_exponent: 0.65,
    zoom_fade_start: 100.0,
    zoom_fade_end: 240.0,
    // Even minimum zoom is an overhead view, never an ear beside the wheel.
    // Keep the closest sound softened; distance/zoom only remove more detail.
    near_cutoff_hz: 2400.0,
    far_cutoff_hz: 700.0,
    filter_half_distance: 14.0,
    filter_camera_height: 0.25,
};

impl WorldSoundProfile {
    pub fn zoom_gain(self, zoom: f32) -> f32 {
        if !zoom.is_finite() {
            return 0.0;
        }
        (self.close_zoom / zoom.max(self.close_zoom)).powf(self.zoom_exponent)
            * smooth_cutoff(zoom, self.zoom_fade_start, self.zoom_fade_end)
    }

    pub fn edge_gain(self, distance: f32) -> f32 {
        smooth_cutoff(distance, self.edge_start, self.hearing_radius)
    }

    /// Ranking estimate only. Never multiply this into the sink gain as well.
    pub fn native_gain_estimate(self, distance: f32) -> f32 {
        (self.reference_distance / distance.max(self.reference_distance)).powi(2)
    }

    pub fn cutoff_hz(self, distance: f32, zoom: f32) -> f32 {
        let distance = distance
            .max(0.0)
            .hypot(zoom.max(0.0) * self.filter_camera_height);
        let detail = 1.0 / (1.0 + (distance / self.filter_half_distance).powf(1.6));
        self.far_cutoff_hz + (self.near_cutoff_hz - self.far_cutoff_hz) * detail
    }
}

fn smooth_cutoff(value: f32, start: f32, end: f32) -> f32 {
    let t = ((value - start) / (end - start)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn street_and_town_views_lose_gain_and_brightness_without_discontinuities() {
        assert_eq!(CART.zoom_gain(12.0), 1.0);
        assert!((0.6..0.7).contains(&CART.zoom_gain(24.0)));
        assert!(CART.zoom_gain(100.0) < 0.26);
        assert_eq!(CART.zoom_gain(240.0), 0.0);
        let mut previous = 1.0;
        for n in 120..=2500 {
            let zoom = n as f32 * 0.1;
            let gain = CART.zoom_gain(zoom);
            assert!(gain <= previous && previous - gain < 0.01);
            previous = gain;
        }
        for (near, far) in [(0.0, 12.0), (12.0, 35.0)] {
            assert!(CART.cutoff_hz(near, 24.0) > CART.cutoff_hz(far, 24.0));
        }
        assert!(CART.cutoff_hz(0.0, 100.0) < CART.cutoff_hz(0.0, 24.0));
        assert!(CART.cutoff_hz(35.0, 100.0) < 2000.0);
        assert_eq!(CART.edge_gain(CART.hearing_radius), 0.0);
        assert_eq!(CART.edge_gain(5.0), CART.edge_gain(25.0));
        assert_eq!(CART.native_gain_estimate(12.0), 0.25);
        assert!(LISTENER_EAR_GAP / CART.reference_distance <= 1.0 / 3.0);
    }
}
