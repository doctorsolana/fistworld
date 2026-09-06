use bevy::prelude::*;

pub const CONTACT_DISTANCE: f32 = 1.55;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Face {
    Front,
    Left,
    Right,
    Rear,
}
impl Face {
    pub const ALL: [Self; 4] = [Self::Front, Self::Left, Self::Right, Self::Rear];
}

#[derive(Clone, Copy, Debug)]
pub struct Footprint {
    pub centre: Vec2,
    pub facing: Vec2,
    pub half_width: f32,
    pub half_depth: f32,
}
impl Footprint {
    pub fn normal(self, face: Face) -> Vec2 {
        let right = Vec2::new(self.facing.y, -self.facing.x);
        match face {
            Face::Front => self.facing,
            Face::Rear => -self.facing,
            Face::Left => -right,
            Face::Right => right,
        }
    }
    pub fn half_length(self, face: Face) -> f32 {
        match face {
            Face::Front | Face::Rear => self.half_width,
            _ => self.half_depth,
        }
    }
    pub fn edge(self, face: Face) -> Vec2 {
        self.centre
            + self.normal(face)
                * match face {
                    Face::Front | Face::Rear => self.half_depth,
                    _ => self.half_width,
                }
    }
    pub fn contact(self, face: Face) -> Vec2 {
        self.edge(face) + self.normal(face) * CONTACT_DISTANCE
    }
    /// Approach a flank outside the enemy rectangle before turning toward it.
    /// The margin includes our half frontage, so the inside files cannot cut
    /// diagonally through the enemy while the formation changes heading.
    pub fn staging_point(self, face: Face, from: Vec2, half_frontage: f32) -> Option<Vec2> {
        let normal = self.normal(face);
        let tangent = Vec2::new(normal.y, -normal.x);
        let d = from - self.edge(face);
        let clearance = half_frontage + 2.0;
        if d.dot(normal) < clearance && d.dot(tangent).abs() > self.half_length(face) + 1.0 {
            Some(
                self.edge(face)
                    + normal * (clearance + 2.0)
                    + tangent
                        * d.dot(tangent).signum()
                        * (self.half_length(face) + clearance + 2.0),
            )
        } else {
            None
        }
    }
}
