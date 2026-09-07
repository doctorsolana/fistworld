use bevy::prelude::*;

pub const CONTACT_DISTANCE: f32 = 1.65;

#[derive(Clone, Copy, Debug)]
pub struct Footprint {
    pub centre: Vec2,
    pub facing: Vec2,
    pub half_width: f32,
    pub half_depth: f32,
}
impl Footprint {
    /// Closest point on the actual occupied rectangle, approached from where
    /// this soldier is now. No battalion-wide front/flank/rear assignment.
    pub fn approach(self, from: Vec2) -> Vec2 {
        let nearest = self.clamp(from, 0.0);
        nearest + (from - nearest).try_normalize().unwrap_or(-self.facing) * CONTACT_DISTANCE
    }
    pub fn clamp(self, point: Vec2, margin: f32) -> Vec2 {
        let right = Vec2::new(self.facing.y, -self.facing.x);
        let offset = point - self.centre;
        self.centre
            + right
                * offset
                    .dot(right)
                    .clamp(-self.half_width - margin, self.half_width + margin)
            + self.facing
                * offset
                    .dot(self.facing)
                    .clamp(-self.half_depth - margin, self.half_depth + margin)
    }
}
