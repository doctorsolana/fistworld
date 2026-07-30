//! Regions — the single spatial primitive the world is organised around.
//!
//! One division serves four jobs at once (see `docs/ARCHITECTURE.md`):
//!
//! - **Political**: who owns this land, the unit of conquest.
//! - **Interest management**: what the server replicates to a given client.
//! - **Simulation LOD**: whether this area runs the tactical sim or only the strategic one.
//! - **Persistence**: what gets saved and loaded as a unit.
//!
//! Keeping those aligned is deliberate. When they diverge you end up maintaining several
//! spatial partitions that disagree, and every feature has to reconcile them.
//!
//! This sits *above* `spatial::SPATIAL_CELL_SIZE` (8m), which is a fine-grained structure
//! for close-range queries. Regions are coarse and political; that grid is not.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Edge length of a region in metres.
///
/// Sized so a region is a meaningful political unit — roughly "a settlement and its
/// surrounding land" — rather than a streaming detail. The shipped 8192m map is
/// 17x17 regions.
pub const REGION_SIZE: f32 = 512.0;

/// Hard ceiling on a client's reported view radius, in metres.
///
/// The radius is CLIENT-SUPPLIED (it rides `PlayerInput` so zooming out widens
/// interest), and it feeds `rings = radius / REGION_SIZE` straight into
/// [`RegionCoord::in_radius`], which allocates a `(2*rings+1)^2` Vec. Unclamped
/// that is a one-message remote OOM: radius 1e6 asks for ~15M coords (~122MB)
/// per client per tick, and radius 1e9 overflows the `i32` span multiply.
///
/// 16384m is above anything a legitimate client can ask for — max zoom is 12000m
/// and the client scales it by 1.35 — while still covering the whole map from any
/// point on it, so clamping here costs a legitimate player nothing.
pub const MAX_VIEW_RADIUS: f32 = 16384.0;

/// Sanitise a client-reported view radius into a ring count.
///
/// Non-finite and non-positive values fall back to a single region, so a client
/// that has not sent input yet still receives its immediate surroundings.
pub fn view_radius_to_rings(reported: Option<f32>) -> i32 {
    let radius = reported
        .filter(|r| r.is_finite() && *r > 0.0)
        .unwrap_or(REGION_SIZE)
        .min(MAX_VIEW_RADIUS);
    (radius / REGION_SIZE).ceil() as i32
}

/// Integer coordinate of a region on the world grid.
#[derive(
    Component, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord,
)]
pub struct RegionCoord {
    pub x: i32,
    pub z: i32,
}

impl RegionCoord {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Region containing a world position. Y is ignored: regions are columns.
    #[inline]
    pub fn from_world_pos(pos: Vec3) -> Self {
        Self {
            x: (pos.x / REGION_SIZE).floor() as i32,
            z: (pos.z / REGION_SIZE).floor() as i32,
        }
    }

    /// Horizontal centre of the region (Y is zero — callers sample terrain if they need height).
    #[inline]
    pub fn center(self) -> Vec3 {
        Vec3::new(
            (self.x as f32 + 0.5) * REGION_SIZE,
            0.0,
            (self.z as f32 + 0.5) * REGION_SIZE,
        )
    }

    /// Inclusive-exclusive world bounds as `(min_xz, max_xz)`.
    #[inline]
    pub fn bounds(self) -> (Vec2, Vec2) {
        let min = Vec2::new(self.x as f32 * REGION_SIZE, self.z as f32 * REGION_SIZE);
        (min, min + Vec2::splat(REGION_SIZE))
    }

    /// Chebyshev (square-ring) distance in regions.
    #[inline]
    pub fn ring_distance(self, other: Self) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }

    /// All regions within `radius` rings, including `self`.
    pub fn in_radius(self, radius: i32) -> Vec<RegionCoord> {
        let radius = radius.max(0);
        let span = radius * 2 + 1;
        let mut out = Vec::with_capacity((span * span) as usize);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                out.push(RegionCoord::new(self.x + dx, self.z + dz));
            }
        }
        out
    }

    /// Shortest horizontal distance from a world position to this region's bounds.
    /// Zero when the position is inside.
    pub fn distance_to_world(self, pos: Vec3) -> f32 {
        let (min, max) = self.bounds();
        let clamped = Vec2::new(pos.x.clamp(min.x, max.x), pos.z.clamp(min.y, max.y));
        Vec2::new(pos.x - clamped.x, pos.z - clamped.y).length()
    }
}

/// How much simulation a region is currently receiving.
///
/// Every region always runs the strategic tick. `Tactical` additionally instantiates
/// individual units, which is expensive and only justified where someone is watching or
/// something is contested.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SimLevel {
    /// Cheap aggregate simulation. Runs everywhere, forever.
    #[default]
    Strategic,
    /// Full 60Hz unit simulation.
    Tactical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_positions_map_to_regions_including_negatives() {
        assert_eq!(RegionCoord::from_world_pos(Vec3::ZERO), RegionCoord::new(0, 0));
        assert_eq!(
            RegionCoord::from_world_pos(Vec3::new(REGION_SIZE * 1.5, 0.0, REGION_SIZE * 2.5)),
            RegionCoord::new(1, 2)
        );
        // floor(), not truncation — otherwise -0.5 and +0.5 collapse onto the same region.
        assert_eq!(
            RegionCoord::from_world_pos(Vec3::new(-1.0, 0.0, -1.0)),
            RegionCoord::new(-1, -1)
        );
    }

    #[test]
    fn center_round_trips_into_its_own_region() {
        for coord in [
            RegionCoord::new(0, 0),
            RegionCoord::new(3, -7),
            RegionCoord::new(-12, 5),
        ] {
            assert_eq!(RegionCoord::from_world_pos(coord.center()), coord);
        }
    }

    #[test]
    fn radius_covers_the_expected_square() {
        assert_eq!(RegionCoord::new(0, 0).in_radius(0).len(), 1);
        assert_eq!(RegionCoord::new(0, 0).in_radius(1).len(), 9);
        assert_eq!(RegionCoord::new(4, 4).in_radius(2).len(), 25);
    }

    #[test]
    fn distance_is_zero_inside_and_grows_outside() {
        let region = RegionCoord::new(0, 0);
        assert_eq!(region.distance_to_world(region.center()), 0.0);

        let outside = Vec3::new(REGION_SIZE + 100.0, 0.0, REGION_SIZE * 0.5);
        assert!((region.distance_to_world(outside) - 100.0).abs() < 0.001);
    }

    #[test]
    fn ring_distance_is_chebyshev() {
        let a = RegionCoord::new(0, 0);
        assert_eq!(a.ring_distance(RegionCoord::new(3, 1)), 3);
        assert_eq!(a.ring_distance(RegionCoord::new(-2, -5)), 5);
    }
}

#[cfg(test)]
mod view_radius_tests {
    use super::*;

    /// The clamp is a DoS guard, not a tuning knob: a hostile radius must not be
    /// able to make `in_radius` allocate without bound, and must not overflow the
    /// `span * span` multiply inside it.
    #[test]
    fn hostile_view_radius_is_clamped_to_a_survivable_ring_count() {
        let max_rings = view_radius_to_rings(Some(MAX_VIEW_RADIUS));
        for hostile in [1.0e6_f32, 1.0e9, 1.0e30, f32::MAX] {
            let rings = view_radius_to_rings(Some(hostile));
            assert_eq!(rings, max_rings, "radius {hostile} escaped the clamp");
        }
        // The clamped worst case must still be a sane allocation.
        let span = (max_rings * 2 + 1) as i64;
        assert!(
            span * span < 10_000,
            "clamped worst case still allocates {} coords",
            span * span
        );
    }

    /// Garbage must degrade to "your immediate surroundings", never to zero
    /// regions (an invisible world) or a panic.
    #[test]
    fn degenerate_view_radius_falls_back_to_one_region() {
        for bad in [None, Some(f32::NAN), Some(f32::INFINITY), Some(-1.0), Some(0.0)] {
            assert_eq!(view_radius_to_rings(bad), 1, "bad radius {bad:?}");
        }
    }

    /// A legitimate max-zoom client must be unaffected by the clamp, or the fix
    /// would silently shrink what real players can see.
    #[test]
    fn legitimate_max_zoom_is_not_clamped() {
        // client/src/camera_rts.rs: zoom_max 12000, reported radius = zoom * 1.35
        let legit = 12_000.0 * 1.35;
        assert!(legit <= MAX_VIEW_RADIUS, "clamp is below legitimate zoom");
        assert_eq!(
            view_radius_to_rings(Some(legit)),
            (legit / REGION_SIZE).ceil() as i32
        );
    }
}
