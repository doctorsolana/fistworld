use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Conservative trunk-radius padding used when a built road persists a tree
/// clearance across client prop streaming and server collider reloads.
pub const ROAD_CLEARED_TREE_PADDING: f32 = 1.35;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoadSurface {
    #[default]
    Dirt,
    Stone,
}

impl RoadSurface {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dirt => "DIRT",
            Self::Stone => "STONE",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoadClass {
    #[default]
    Lane,
    Main,
}

impl RoadClass {
    /// Protected right-of-way assigned when a new road is surveyed.
    ///
    /// The built dirt ribbon may begin much narrower. This corridor keeps later
    /// paving, shoulders and modest widening from requiring buildings or crop
    /// fields to move. These are intentionally not enormous urban boulevards:
    /// villages should retain narrow local lanes and only their principal
    /// approaches receive the broader reservation.
    pub const fn initial_reserved_width(self) -> f32 {
        match self {
            Self::Lane => 4.0,
            Self::Main => 6.0,
        }
    }
}

/// One naturally surveyed path joining a building door to its village network.
///
/// The whole polyline is one replicated component. `built_through` advances a
/// point at a time while the builder works, so the client rebuilds one small
/// ribbon mesh rather than maintaining an entity for every two metres of road.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VillageRoad {
    pub settlement: String,
    pub builder: String,
    /// Ground-plane world points. Height is always sampled from authoritative
    /// terrain, keeping the payload compact and the road seated after edits.
    pub points: Vec<Vec2>,
    /// Number of leading points whose connecting segments have been built.
    pub built_through: u16,
    /// Width of the surface that is physically usable and rendered today.
    pub width: f32,
    /// Protected corridor for future widening, shoulders and paving.
    ///
    /// A zero value identifies a road loaded from an older save/packet. It
    /// falls back to that road's current width: old settlements cannot claim
    /// that they reserved land which may already contain a building.
    #[serde(default)]
    pub reserved_width: f32,
    #[serde(default)]
    pub surface: RoadSurface,
    #[serde(default)]
    pub class: RoadClass,
    /// Stone removed from the settlement store for this road. A stone road is
    /// never cosmetic: this must reach [`Self::stone_required`] first.
    #[serde(default)]
    pub stone_committed: u32,
}

impl VillageRoad {
    pub fn built_points(&self) -> &[Vec2] {
        &self.points[..usize::from(self.built_through).min(self.points.len())]
    }

    pub fn is_complete(&self) -> bool {
        usize::from(self.built_through) >= self.points.len()
    }

    pub fn total_length(&self) -> f32 {
        self.points
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .sum()
    }

    /// Effective protected width, including backward compatibility for roads
    /// created before explicit right-of-way reservations existed.
    pub fn reservation_width(&self) -> f32 {
        if self.reserved_width > 0.0 {
            self.reserved_width.max(self.width)
        } else {
            self.width
        }
    }

    /// Widen the physical surface without ever claiming new land.
    pub fn widen_within_reservation(&mut self, requested_width: f32) {
        let permitted = requested_width.min(self.reservation_width());
        self.width = self.width.max(permitted);
    }

    pub fn stone_required(&self) -> u32 {
        // One stone unit paves roughly four metres of a main road. Wider roads
        // naturally cost more without needing per-tile entities. A new main
        // pays for the mature 4m paved surface even while its visible dirt is
        // still narrow; a legacy road cannot be charged for width outside the
        // ground it safely owns.
        let paved_width = match self.class {
            RoadClass::Main => self.width.max(4.0_f32.min(self.reservation_width())),
            RoadClass::Lane => self.width,
        };
        ((self.total_length() * (paved_width / 4.0)) / 4.0)
            .ceil()
            .max(1.0) as u32
    }

    /// Whether a ground point lies on the completed portion of this path.
    pub fn contains_built_point(&self, point: Vec2, padding: f32) -> bool {
        let radius = self.width * 0.5 + padding.max(0.0);
        let radius_sq = radius * radius;
        self.built_points()
            .windows(2)
            .any(|pair| distance_squared_to_segment(point, pair[0], pair[1]) <= radius_sq)
    }

    /// Whether a point overlaps the protected corridor of any planned segment.
    /// Unlike [`Self::contains_built_point`], this deliberately includes the
    /// unfinished suffix: approved infrastructure owns its right-of-way before
    /// the final shovel of dirt is placed.
    pub fn contains_reserved_point(&self, point: Vec2, padding: f32) -> bool {
        let radius = self.reservation_width() * 0.5 + padding.max(0.0);
        let radius_sq = radius * radius;
        self.points
            .windows(2)
            .any(|pair| distance_squared_to_segment(point, pair[0], pair[1]) <= radius_sq)
    }

    /// Whether any planned segment's full ribbon overlaps a rotated rectangle.
    ///
    /// Site selection uses the complete polyline rather than only its built
    /// prefix: a half-finished road is already committed infrastructure. The
    /// rectangle is conservatively inflated by the RESERVED road half-width
    /// and caller padding, which prevents a later wider ribbon clipping a crop
    /// even when its initial dirt centreline technically misses the field.
    pub fn intersects_rotated_rect(
        &self,
        center: Vec2,
        half_extents: Vec2,
        rotation: f32,
        padding: f32,
    ) -> bool {
        let inflated =
            half_extents + Vec2::splat(self.reservation_width() * 0.5 + padding.max(0.0));
        self.points.windows(2).any(|pair| {
            let start = crate::rotation::world_to_local_xz(pair[0] - center, rotation);
            let end = crate::rotation::world_to_local_xz(pair[1] - center, rotation);
            segment_intersects_axis_aligned_rect(start, end, inflated)
        })
    }
}

fn segment_intersects_axis_aligned_rect(start: Vec2, end: Vec2, half: Vec2) -> bool {
    let delta = end - start;
    let mut enter = 0.0_f32;
    let mut exit = 1.0_f32;

    for (origin, direction, extent) in [(start.x, delta.x, half.x), (start.y, delta.y, half.y)] {
        if direction.abs() <= 1e-6 {
            if origin.abs() > extent {
                return false;
            }
            continue;
        }
        let mut near = (-extent - origin) / direction;
        let mut far = (extent - origin) / direction;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        enter = enter.max(near);
        exit = exit.min(far);
        if enter > exit {
            return false;
        }
    }
    true
}

pub fn distance_squared_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_squared();
    if length_sq <= 1e-6 {
        return point.distance_squared(start);
    }
    let t = ((point - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    point.distance_squared(start + segment * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_built_prefix_counts_as_road() {
        let road = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::ZERO, Vec2::X * 5.0, Vec2::X * 10.0],
            built_through: 2,
            width: 2.0,
            reserved_width: 4.0,
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        assert!(road.contains_built_point(Vec2::new(3.0, 0.8), 0.0));
        assert!(!road.contains_built_point(Vec2::new(8.0, 0.0), 0.0));
        assert!(road.contains_reserved_point(Vec2::new(8.0, 1.8), 0.0));
    }

    #[test]
    fn planned_road_ribbon_detects_a_rotated_field_before_it_is_built() {
        let road = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::new(-8.0, 0.0), Vec2::new(8.0, 0.0)],
            built_through: 1,
            width: 2.0,
            reserved_width: 4.0,
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };

        assert!(road.intersects_rotated_rect(
            Vec2::ZERO,
            Vec2::new(4.0, 5.5),
            35.0_f32.to_radians(),
            0.25,
        ));
        assert!(!road.intersects_rotated_rect(
            Vec2::new(0.0, 12.0),
            Vec2::new(4.0, 5.5),
            35.0_f32.to_radians(),
            0.25,
        ));
    }

    #[test]
    fn widening_is_bounded_by_the_original_right_of_way() {
        let mut new_main = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::ZERO, Vec2::X * 10.0],
            built_through: 2,
            width: 2.6,
            reserved_width: RoadClass::Main.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Main,
            stone_committed: 0,
        };
        new_main.widen_within_reservation(4.0);
        assert_eq!(new_main.width, 4.0);

        let mut legacy = VillageRoad {
            reserved_width: 0.0,
            width: 2.6,
            ..new_main
        };
        legacy.widen_within_reservation(4.0);
        assert_eq!(
            legacy.width, 2.6,
            "an old save owns no invisible extra land"
        );
    }

    #[test]
    fn future_width_is_protected_before_the_dirt_surface_grows() {
        let road = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::ZERO, Vec2::X * 10.0],
            built_through: 2,
            width: 2.6,
            reserved_width: RoadClass::Main.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Main,
            stone_committed: 0,
        };
        let future_shoulder = Vec2::new(5.0, 2.7);

        assert!(!road.contains_built_point(future_shoulder, 0.0));
        assert!(road.contains_reserved_point(future_shoulder, 0.0));
    }

    #[test]
    fn stone_cost_uses_the_mature_width_but_respects_legacy_limits() {
        let new_main = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::ZERO, Vec2::X * 16.0],
            built_through: 2,
            width: 2.6,
            reserved_width: RoadClass::Main.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Main,
            stone_committed: 0,
        };
        assert_eq!(new_main.stone_required(), 4);

        let legacy_main = VillageRoad {
            reserved_width: 0.0,
            ..new_main
        };
        assert_eq!(legacy_main.stone_required(), 3);
    }
}
