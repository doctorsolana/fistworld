//! The one geometric rule set for reserved land.
//!
//! NPC permit planning, the player's placement command and the client's
//! placement preview all decide "does this plot fit here" with the functions
//! in this file, so the three can never disagree about a wall gap. Every
//! reservation is an oriented rectangle plus a yard margin; the old
//! centre-to-centre discs survive only as the broad-phase reject inside
//! [`LandClaim::conflicts_with`].
//!
//! Two claims conflict when the shortest distance between their rectangles is
//! smaller than the sum of their margins (a rounded, not squared, inflation),
//! so a corner-to-corner diagonal never counts as closer than it really is.

use bevy::prelude::*;

use crate::components::{
    BuildingId, CivicHallLevel, FARM_FIELD_TERRACE_MARGIN, FarmField, RoadClass,
    SettlementBuildingKind, distance_squared_to_segment,
};

/// Yard kept around a cabin's reserved upgrade envelope on every side, so two
/// cabins stand at least 3.0 m apart wall to wall. The navigation grid pads
/// each shell by `CHARACTER_NAV_RADIUS` (0.28 m), which leaves a 2.44 m
/// walkable channel between two cabins: two people pass abreast in 1.68 m
/// (2 x 0.28 padding + 4 x 0.28 bodies) with room to spare.
pub const HOUSE_YARD_MARGIN: f32 = 1.5;
/// Working space kept on every side of a workplace shell for its woodpile,
/// stock piles and cart turning.
pub const WORKPLACE_YARD_MARGIN: f32 = 2.0;
/// Commons kept around the permanent civic Hall shell and the Marketplace.
pub const CIVIC_YARD_MARGIN: f32 = 3.0;
/// Verge kept outside a crop field or pasture fence so a neighbouring wall
/// never stands on the fence line. The remaining graded terrace may blend into
/// a neighbour's own landscaping.
pub const FIELD_VERGE_MARGIN: f32 = 1.0;
/// The forecourt reserved in front of the Hall door for the Moot service line
/// and commons: 16 m wide and 12 m deep.
pub const HALL_FORECOURT_HALF_EXTENTS: Vec2 = Vec2::new(8.0, 6.0);
/// Authored entrances sit just outside the wall. Every road survey begins with
/// this straight apron so A* cannot approach a door through the building's
/// side or rear, and no neighbour may build on it.
pub const DOOR_APRON_LENGTH: f32 = 2.25;
/// Half-width of the doorway apron: the Lane reservation plus a 0.2 m verge.
pub const DOOR_APRON_HALF_WIDTH: f32 = RoadClass::Lane.initial_reserved_width() * 0.5 + 0.2;
/// Verge a building keeps outside a road's protected corridor.
pub const ROAD_VERGE: f32 = 0.45;
/// Millimetre slack so a plot placed exactly at the required gap is legal.
const GAP_TOLERANCE: f32 = 1e-3;

/// What a reserved rectangle is, which decides how it conflicts and how a
/// refusal names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LandUse {
    /// A building shell, including a cabin's reserved upgrade envelope.
    Building,
    /// The straight approach in front of an authored door.
    Doorway,
    /// The Hall's reserved forecourt.
    Forecourt,
    /// A wheat field: accepted, legacy or intended.
    Field,
    /// A fenced grazing pasture.
    Pasture,
}

impl LandUse {
    /// Verge this land keeps outside a road or reserved lane corridor.
    pub const fn road_verge(self) -> f32 {
        match self {
            Self::Building => ROAD_VERGE,
            Self::Field => FARM_FIELD_TERRACE_MARGIN,
            Self::Pasture => 1.0,
            Self::Doorway | Self::Forecourt => 0.0,
        }
    }
}

/// One reserved oriented rectangle and the yard it keeps on every side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LandClaim {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub rotation: f32,
    pub margin: f32,
    pub land_use: LandUse,
}

impl LandClaim {
    pub const fn new(
        center: Vec2,
        half_extents: Vec2,
        rotation: f32,
        margin: f32,
        land_use: LandUse,
    ) -> Self {
        Self {
            center,
            half_extents,
            rotation,
            margin,
            land_use,
        }
    }

    /// Radius of the disc that certainly contains this claim and its yard.
    pub fn broad_radius(&self) -> f32 {
        self.half_extents.length() + self.margin
    }

    /// Edge gap the two rectangles must keep; `None` when the pair may meet.
    ///
    /// Door aprons are open ground rather than yards: two aprons may share a
    /// lane, but any other land is a hard blocker inside an apron.
    pub fn required_gap(&self, other: &Self) -> Option<f32> {
        match (self.land_use, other.land_use) {
            (LandUse::Doorway, LandUse::Doorway) => None,
            (LandUse::Doorway, _) | (_, LandUse::Doorway) => Some(0.0),
            _ => Some(self.margin + other.margin),
        }
    }

    pub fn corners(&self) -> [Vec2; 4] {
        let half = self.half_extents;
        [
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(half.x, half.y),
            Vec2::new(-half.x, half.y),
        ]
        .map(|corner| self.center + crate::rotation::local_to_world_xz(corner, self.rotation))
    }

    fn axes(&self) -> [Vec2; 2] {
        [
            crate::rotation::local_to_world_xz(Vec2::X, self.rotation),
            crate::rotation::local_to_world_xz(Vec2::Y, self.rotation),
        ]
    }

    /// Depth by which the two raw rectangles overlap (the shortest move that
    /// separates them), or zero when they are apart or merely touching.
    pub fn penetration(&self, other: &Self) -> f32 {
        let axes_a = self.axes();
        let axes_b = other.axes();
        let offset = other.center - self.center;
        let mut depth = f32::INFINITY;
        for axis in axes_a.into_iter().chain(axes_b) {
            let reach_a = self.half_extents.x * axes_a[0].dot(axis).abs()
                + self.half_extents.y * axes_a[1].dot(axis).abs();
            let reach_b = other.half_extents.x * axes_b[0].dot(axis).abs()
                + other.half_extents.y * axes_b[1].dot(axis).abs();
            let overlap = reach_a + reach_b - offset.dot(axis).abs();
            if overlap <= 0.0 {
                return 0.0;
            }
            depth = depth.min(overlap);
        }
        depth
    }

    /// Shortest distance between the two raw rectangles; zero when they
    /// overlap or touch.
    pub fn gap(&self, other: &Self) -> f32 {
        if self.penetration(other) > 0.0 {
            return 0.0;
        }
        let ours = self.corners();
        let theirs = other.corners();
        let mut best = f32::INFINITY;
        for i in 0..4 {
            for j in 0..4 {
                let next = (j + 1) % 4;
                best = best
                    .min(distance_squared_to_segment(
                        ours[i],
                        theirs[j],
                        theirs[next],
                    ))
                    .min(distance_squared_to_segment(theirs[i], ours[j], ours[next]));
            }
        }
        best.sqrt()
    }

    /// Whether the two claims may not coexist: a cheap disc reject first,
    /// then the exact edge gap against the required yard. Overlapping
    /// rectangles always conflict; the millimetre slack only decides an
    /// exact-gap placement, so a hard blocker with no yard still refuses
    /// anything inside it while allowing a rectangle that merely touches.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        let Some(required) = self.required_gap(other) else {
            return false;
        };
        let reach = self.broad_radius() + other.broad_radius();
        if self.center.distance_squared(other.center) >= reach * reach {
            return false;
        }
        self.penetration(other) > 0.0 || self.gap(other) < required - GAP_TOLERANCE
    }

    /// How far this claim must move to clear `other`, when it conflicts.
    pub fn shortfall(&self, other: &Self) -> Option<f32> {
        if !self.conflicts_with(other) {
            return None;
        }
        let required = self.required_gap(other).unwrap_or(0.0);
        Some((required - self.gap(other) + self.penetration(other)).max(0.0))
    }
}

/// A plot's own claims without a heap allocation: shell, fields, pasture and,
/// for an existing building, its doorway apron.
#[derive(Clone, Copy, Debug)]
pub struct PlotClaims {
    entries: [LandClaim; 5],
    len: usize,
}

impl PlotClaims {
    const EMPTY: LandClaim = LandClaim::new(Vec2::ZERO, Vec2::ZERO, 0.0, 0.0, LandUse::Building);

    fn new() -> Self {
        Self {
            entries: [Self::EMPTY; 5],
            len: 0,
        }
    }

    fn push(&mut self, claim: LandClaim) {
        self.entries[self.len] = claim;
        self.len += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &LandClaim> {
        self.entries[..self.len].iter()
    }

    pub fn as_slice(&self) -> &[LandClaim] {
        &self.entries[..self.len]
    }

    /// Whether any part of this plot conflicts with `other`.
    pub fn conflicts_with(&self, other: &LandClaim) -> bool {
        self.iter().any(|claim| claim.conflicts_with(other))
    }
}

/// Yard margin kept on every side of a kind's reserved shell.
pub fn yard_margin(kind: SettlementBuildingKind) -> f32 {
    match kind {
        SettlementBuildingKind::House => HOUSE_YARD_MARGIN,
        SettlementBuildingKind::Hall | SettlementBuildingKind::Market => CIVIC_YARD_MARGIN,
        SettlementBuildingKind::Farmstead
        | SettlementBuildingKind::LumberjackHut
        | SettlementBuildingKind::FishermansHut
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church
        | SettlementBuildingKind::Windmill
        | SettlementBuildingKind::Bakery
        | SettlementBuildingKind::StorageHall
        | SettlementBuildingKind::StoneQuarry
        | SettlementBuildingKind::LivestockFarm => WORKPLACE_YARD_MARGIN,
    }
}

/// The door and the far end of its straight apron, in ground metres.
///
/// A Tavern's door opens into its walkable courtyard, so its apron runs
/// straight until it is clear of the reserved patio before a full-width lane
/// may turn toward a street.
pub fn doorway_approach(kind: SettlementBuildingKind, plot: Vec3, rotation: f32) -> (Vec2, Vec2) {
    let door = kind.entrance_position(plot, rotation).xz();
    let center = plot.xz();
    let outward = (door - center).normalize_or_zero();
    let length = if kind == SettlementBuildingKind::Tavern {
        let reserved = kind.placement_definition();
        let front_reach = reserved.footprint.y * 0.5 - reserved.footprint_center.y;
        (front_reach + RoadClass::Lane.initial_reserved_width() * 0.5 + 0.55
            - door.distance(center))
        .max(DOOR_APRON_LENGTH)
    } else {
        DOOR_APRON_LENGTH
    };
    (door, door + outward * length)
}

/// The reserved shell of a plot: cabins reserve the union of both upgrade
/// lines so a later storey never needs the neighbours to move.
pub fn footprint_claim(kind: SettlementBuildingKind, plot: Vec3, rotation: f32) -> LandClaim {
    let definition = kind.placement_definition();
    LandClaim::new(
        definition.world_footprint_center(plot, rotation),
        definition.footprint * 0.5,
        rotation,
        yard_margin(kind),
        LandUse::Building,
    )
}

/// The doorway apron of an existing plot, which no neighbour may build on.
pub fn doorway_claim(kind: SettlementBuildingKind, plot: Vec3, rotation: f32) -> LandClaim {
    let (door, approach) = doorway_approach(kind, plot, rotation);
    let along = approach - door;
    LandClaim::new(
        (door + approach) * 0.5,
        Vec2::new(DOOR_APRON_HALF_WIDTH, along.length() * 0.5),
        along.x.atan2(along.y),
        0.0,
        LandUse::Doorway,
    )
}

fn field_claim(center: Vec3, half_extents: Vec2, rotation: f32) -> LandClaim {
    LandClaim::new(
        center.xz(),
        half_extents,
        rotation,
        FIELD_VERGE_MARGIN,
        LandUse::Field,
    )
}

/// The agricultural envelope a NEW Farmstead reserves for its two fields.
pub fn intended_field_claims(
    kind: SettlementBuildingKind,
    plot: Vec3,
    rotation: f32,
) -> Option<[LandClaim; 2]> {
    let fields = kind.intended_field_positions(plot, rotation)?;
    let half = kind.intended_field_half_extents()?;
    Some(fields.map(|field| field_claim(field, half, rotation)))
}

/// The fixed fallback field rectangles of a completed Farmstead. Its accepted
/// [`FarmField`] shapes, when present, reserve the actual crop bands as well.
pub fn legacy_field_claims(
    kind: SettlementBuildingKind,
    plot: Vec3,
    rotation: f32,
) -> Option<[LandClaim; 2]> {
    let fields = kind.field_positions(plot, rotation)?;
    let half = kind.field_half_extents()?;
    Some(fields.map(|field| field_claim(field, half, rotation)))
}

/// The accepted crop bands of one surveyed field.
pub fn accepted_field_claims(field: &FarmField, position: Vec3, rotation: f32) -> Vec<LandClaim> {
    field
        .reservation_rects(position, rotation, 0.0)
        .into_iter()
        .map(|(center, half, yaw)| field_claim(center, half, yaw))
        .collect()
}

/// The fenced pasture behind a Livestock Farm.
pub fn pasture_claim(kind: SettlementBuildingKind, plot: Vec3, rotation: f32) -> Option<LandClaim> {
    let pasture = kind.pasture_position(plot, rotation)?;
    let half = kind.pasture_half_extents()?;
    Some(LandClaim::new(
        pasture.xz(),
        half,
        rotation,
        FIELD_VERGE_MARGIN,
        LandUse::Pasture,
    ))
}

/// Everything a proposed or pending plot of `kind` would occupy, in the order
/// a refusal names them: the shell, then its fields, then its pasture.
pub fn proposed_plot_claims(kind: SettlementBuildingKind, plot: Vec3, rotation: f32) -> PlotClaims {
    let mut claims = PlotClaims::new();
    claims.push(footprint_claim(kind, plot, rotation));
    if let Some(fields) = intended_field_claims(kind, plot, rotation) {
        claims.push(fields[0]);
        claims.push(fields[1]);
    }
    if let Some(pasture) = pasture_claim(kind, plot, rotation) {
        claims.push(pasture);
    }
    claims
}

/// Everything an existing plot keeps reserved: its shell and doorway apron,
/// then its fields (the intended envelope while pending, the fixed fallback
/// rectangles once complete) and its pasture.
pub fn building_claims(
    kind: SettlementBuildingKind,
    plot: Vec3,
    rotation: f32,
    pending: bool,
) -> PlotClaims {
    let mut claims = PlotClaims::new();
    claims.push(footprint_claim(kind, plot, rotation));
    claims.push(doorway_claim(kind, plot, rotation));
    let fields = if pending {
        intended_field_claims(kind, plot, rotation)
    } else {
        legacy_field_claims(kind, plot, rotation)
    };
    if let Some(fields) = fields {
        claims.push(fields[0]);
        claims.push(fields[1]);
    }
    if let Some(pasture) = pasture_claim(kind, plot, rotation) {
        claims.push(pasture);
    }
    claims
}

/// The founding Hall's land: the complete future Town Hall shell with its
/// civic commons, and the forecourt in front of the permanent doorway.
/// Halls always face world -Z (rotation zero), as every Hall door lookup in
/// the planner assumes.
pub fn hall_claims(hall: Vec3) -> [LandClaim; 2] {
    let shell = LandClaim::new(
        CivicHallLevel::reserved_world_center(hall, 0.0),
        CivicHallLevel::reserved_half_extents(),
        0.0,
        CIVIC_YARD_MARGIN,
        LandUse::Building,
    );
    let door = SettlementBuildingKind::Hall
        .entrance_position(hall, 0.0)
        .xz();
    let forecourt = LandClaim::new(
        door + crate::rotation::local_to_world_xz(
            Vec2::new(0.0, -HALL_FORECOURT_HALF_EXTENTS.y),
            0.0,
        ),
        HALL_FORECOURT_HALF_EXTENTS,
        0.0,
        0.0,
        LandUse::Forecourt,
    );
    [shell, forecourt]
}

/// Whether a reserved road or lane corridor (`points` with `half_width`)
/// crosses a rotated rectangle inflated by `padding`. The whole polyline
/// counts, including any unbuilt suffix: approved infrastructure owns its
/// right-of-way before the dirt is laid.
pub fn polyline_intersects_rotated_rect(
    points: &[Vec2],
    half_width: f32,
    center: Vec2,
    half_extents: Vec2,
    rotation: f32,
    padding: f32,
) -> bool {
    let inflated = half_extents + Vec2::splat(half_width + padding.max(0.0));
    points.windows(2).any(|pair| {
        let start = crate::rotation::world_to_local_xz(pair[0] - center, rotation);
        let end = crate::rotation::world_to_local_xz(pair[1] - center, rotation);
        segment_intersects_axis_aligned_rect(start, end, inflated)
    })
}

/// Whether a corridor polyline passes within `radius` of a point: the cheap
/// broad phase in front of [`polyline_intersects_rotated_rect`].
pub fn polyline_within_radius(points: &[Vec2], half_width: f32, center: Vec2, radius: f32) -> bool {
    let reach = radius + half_width;
    points
        .windows(2)
        .any(|pair| distance_squared_to_segment(center, pair[0], pair[1]) <= reach * reach)
}

/// Whether a corridor crosses a land claim's rectangle plus its road verge,
/// disc-rejected first.
pub fn corridor_blocks_claim(points: &[Vec2], half_width: f32, claim: &LandClaim) -> bool {
    let verge = claim.land_use.road_verge();
    polyline_within_radius(
        points,
        half_width,
        claim.center,
        claim.half_extents.length() + verge,
    ) && polyline_intersects_rotated_rect(
        points,
        half_width,
        claim.center,
        claim.half_extents,
        claim.rotation,
        verge,
    )
}

/// The occupied claim needing the largest move to clear, with ties broken on
/// centre x then y, so the blocker a refusal names never depends on snapshot
/// order. Returns `(part index, occupied index, shortfall)`.
///
/// The server's refusal and the client's preview both pick their offender
/// here, so the two name the same neighbour for the same plot.
pub fn worst_conflict<'a>(
    parts: &[LandClaim],
    occupied: impl IntoIterator<Item = &'a LandClaim>,
) -> Option<(usize, usize, f32)> {
    let mut worst: Option<(usize, usize, Vec2, f32)> = None;
    for (occupied_index, land) in occupied.into_iter().enumerate() {
        for (part_index, part) in parts.iter().enumerate() {
            let Some(shortfall) = part.shortfall(land) else {
                continue;
            };
            let replace = worst.is_none_or(|(_, _, current, best)| {
                shortfall
                    .total_cmp(&best)
                    .then_with(|| current.x.total_cmp(&land.center.x))
                    .then_with(|| current.y.total_cmp(&land.center.y))
                    .is_gt()
            });
            if replace {
                worst = Some((part_index, occupied_index, land.center, shortfall));
            }
        }
    }
    worst.map(|(part, land, _, shortfall)| (part, land, shortfall))
}

/// Player-facing name of whoever reserved a piece of land: `HOUSE #7`,
/// `FARMSTEAD (under construction)`, `the Moot Hall` or `reserved land`.
/// Pending worksites have no durable [`BuildingId`] yet. The server's refusal
/// and the client's preview build the same string.
pub fn land_owner_label(
    kind: Option<SettlementBuildingKind>,
    id: Option<BuildingId>,
    pending: bool,
) -> String {
    match (kind, id, pending) {
        (Some(SettlementBuildingKind::Hall), ..) => "the Moot Hall".to_string(),
        (Some(kind), Some(id), _) => format!("{} #{}", kind.label(), id.0),
        (Some(kind), None, true) => format!("{} (under construction)", kind.label()),
        (Some(kind), None, false) => kind.label().to_string(),
        (None, ..) => "reserved land".to_string(),
    }
}

/// How a refusal names an occupied claim: the owner for a shell, or the
/// owner's doorway, wheat field or pasture.
pub fn describe_claim(claim: &LandClaim, owner: &str) -> String {
    match claim.land_use {
        LandUse::Building => owner.to_string(),
        LandUse::Doorway => format!("{owner}'s doorway"),
        LandUse::Forecourt => "the Moot Hall forecourt".to_string(),
        LandUse::Field => format!("{owner}'s wheat field"),
        LandUse::Pasture => format!("{owner}'s pasture"),
    }
}

/// The part of a proposed plot a refusal names; the shell itself is unnamed.
fn part_name(part: &LandClaim) -> &'static str {
    match part.land_use {
        LandUse::Field => "Wheat field",
        LandUse::Pasture => "Pasture",
        LandUse::Building | LandUse::Doorway | LandUse::Forecourt => "",
    }
}

/// "Too close to HOUSE #7: 2.0 m of 3.0 m.", "Overlaps HOUSE #7's doorway."
/// or "Wheat field too close to HOUSE #4: 0.7 m of 2.5 m." for a proposed
/// `part` against an occupied claim owned by `owner`.
pub fn land_conflict_sentence(part: &LandClaim, other: &LandClaim, owner: &str) -> String {
    let gap = part.gap(other);
    let required = part.required_gap(other).unwrap_or(0.0);
    let what = describe_claim(other, owner);
    let subject = part_name(part);
    if gap <= 0.0 || required <= 0.0 {
        if subject.is_empty() {
            format!("Overlaps {what}.")
        } else {
            format!("{subject} overlaps {what}.")
        }
    } else if subject.is_empty() {
        format!("Too close to {what}: {gap:.1} m of {required:.1} m.")
    } else {
        format!("{subject} too close to {what}: {gap:.1} m of {required:.1} m.")
    }
}

/// "Overlaps the access lane reserved for FARMSTEAD (under construction)."
pub fn lane_conflict_sentence(part: &LandClaim, owner: &str) -> String {
    let subject = part_name(part);
    if subject.is_empty() {
        format!("Overlaps the access lane reserved for {owner}.")
    } else {
        format!("{subject} overlaps the access lane reserved for {owner}.")
    }
}

/// A protected road corridor, named by the part of the plot that crosses it.
pub fn road_conflict_sentence(part: &LandClaim) -> &'static str {
    match part.land_use {
        LandUse::Field => "A road reservation crosses one of the wheat fields.",
        LandUse::Pasture => "A road reservation crosses the livestock pasture.",
        LandUse::Building | LandUse::Doorway | LandUse::Forecourt => {
            "The building footprint overlaps a road reservation."
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::SettlementBuildingKind as Kind;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn house(x: f32, z: f32, rotation: f32) -> LandClaim {
        footprint_claim(Kind::House, Vec3::new(x, 0.0, z), rotation)
    }

    #[test]
    fn the_gap_is_the_true_edge_distance_in_every_orientation() {
        let a = LandClaim::new(Vec2::ZERO, Vec2::new(2.0, 1.0), 0.0, 0.0, LandUse::Building);
        let beside = LandClaim::new(
            Vec2::new(5.0, 0.0),
            Vec2::new(1.0, 1.0),
            0.0,
            0.0,
            LandUse::Building,
        );
        assert!((a.gap(&beside) - 2.0).abs() < 1e-4);
        assert_eq!(a.penetration(&beside), 0.0);

        // Corner to corner on the diagonal: the squared inflation would call
        // this 3 m; the true distance is 3 sqrt 2.
        let diagonal = LandClaim::new(
            Vec2::new(6.0, 5.0),
            Vec2::new(1.0, 1.0),
            0.0,
            0.0,
            LandUse::Building,
        );
        assert!((a.gap(&diagonal) - 18.0_f32.sqrt()).abs() < 1e-4);

        // A rotated square whose corner points at the first rectangle.
        let rotated = LandClaim::new(
            Vec2::new(2.0 + 2.0_f32.sqrt() + 1.0, 0.0),
            Vec2::splat(1.0),
            FRAC_PI_4,
            0.0,
            LandUse::Building,
        );
        assert!((a.gap(&rotated) - 1.0).abs() < 1e-4, "{}", a.gap(&rotated));
        assert!((rotated.gap(&a) - 1.0).abs() < 1e-4);

        let overlapping = LandClaim::new(
            Vec2::new(2.5, 0.0),
            Vec2::new(1.0, 1.0),
            0.0,
            0.0,
            LandUse::Building,
        );
        assert_eq!(a.gap(&overlapping), 0.0);
        assert!((a.penetration(&overlapping) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn margins_add_and_a_millimetre_of_slack_keeps_exact_placements_legal() {
        let width = Kind::House.placement_definition().footprint.x;
        let first = house(0.0, 0.0, 0.0);
        let exact = house(width + 2.0 * HOUSE_YARD_MARGIN, 0.0, 0.0);
        assert!(!first.conflicts_with(&exact));
        assert!(!exact.conflicts_with(&first));
        assert_eq!(first.shortfall(&exact), None);

        let close = house(width + 2.0 * HOUSE_YARD_MARGIN - 0.5, 0.0, 0.0);
        assert!(first.conflicts_with(&close));
        let shortfall = first.shortfall(&close).unwrap();
        assert!((shortfall - 0.5).abs() < 1e-3, "{shortfall}");

        // Workplaces keep two metres each; a cabin beside a workshop needs 3.5 m.
        let mill = footprint_claim(Kind::Windmill, Vec3::ZERO, 0.0);
        let mill_width = Kind::Windmill.placement_definition().footprint.x;
        let cabin = |gap: f32| house(mill_width * 0.5 + gap + width * 0.5, 0.0, 0.0);
        assert!(mill.conflicts_with(&cabin(3.4)));
        assert!(!mill.conflicts_with(&cabin(3.6)));
    }

    #[test]
    fn a_broad_phase_disc_never_hides_a_real_conflict() {
        // Random-looking but deterministic pairs: whenever the discs are
        // apart the exact test must agree, so the disc is only ever a reject.
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            seed = crate::worldgen::splitmix64(seed);
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        for _ in 0..2_000 {
            let a = LandClaim::new(
                Vec2::new(next() * 40.0 - 20.0, next() * 40.0 - 20.0),
                Vec2::new(1.0 + next() * 6.0, 1.0 + next() * 6.0),
                next() * PI,
                next() * 3.0,
                LandUse::Building,
            );
            let b = LandClaim::new(
                Vec2::new(next() * 40.0 - 20.0, next() * 40.0 - 20.0),
                Vec2::new(1.0 + next() * 6.0, 1.0 + next() * 6.0),
                next() * PI,
                next() * 3.0,
                LandUse::Field,
            );
            let reach = a.broad_radius() + b.broad_radius();
            if a.center.distance(b.center) >= reach {
                assert!(a.gap(&b) >= a.margin + b.margin - 1e-3);
            }
            assert_eq!(a.conflicts_with(&b), b.conflicts_with(&a));
            assert!((a.gap(&b) - b.gap(&a)).abs() < 1e-3);
        }
    }

    #[test]
    fn door_aprons_are_hard_blockers_but_may_share_a_lane() {
        let plot = Vec3::new(10.0, 0.0, 10.0);
        let apron = doorway_claim(Kind::House, plot, 0.0);
        let (door, approach) = doorway_approach(Kind::House, plot, 0.0);
        assert!((apron.half_extents.y * 2.0 - DOOR_APRON_LENGTH).abs() < 1e-4);
        assert!((apron.half_extents.x - DOOR_APRON_HALF_WIDTH).abs() < 1e-4);
        assert!(apron.center.distance((door + approach) * 0.5) < 1e-4);
        // The apron lies in front of the door, outside the cabin's own shell.
        let shell = footprint_claim(Kind::House, plot, 0.0);
        assert_eq!(
            shell.penetration(&apron),
            0.0,
            "the apron must start outside the wall"
        );
        assert!(apron.gap(&shell) < 0.5);
        for rotation in [0.0, FRAC_PI_2, PI, 1.234] {
            let turned = doorway_claim(Kind::House, plot, rotation);
            let (door, approach) = doorway_approach(Kind::House, plot, rotation);
            let corners = turned.corners();
            let along = (approach - door).normalize();
            // Two corners sit at the door end, two at the approach end.
            let near = corners
                .iter()
                .filter(|corner| (**corner - door).dot(along).abs() < 1e-3)
                .count();
            assert_eq!(near, 2, "rotation {rotation}: {corners:?}");
        }

        // A fence on the doorstep is refused with no yard at all...
        let fence = LandClaim::new(apron.center, Vec2::splat(0.5), 0.0, 1.0, LandUse::Field);
        assert!(apron.conflicts_with(&fence));
        assert_eq!(apron.shortfall(&fence), fence.shortfall(&apron));
        // ...while a neighbour's apron may overlap this one.
        let other_apron =
            LandClaim::new(apron.center, apron.half_extents, PI, 0.0, LandUse::Doorway);
        assert!(!apron.conflicts_with(&other_apron));
        assert_eq!(apron.required_gap(&other_apron), None);
    }

    #[test]
    fn plot_claims_list_the_shell_first_then_fields_then_pasture() {
        let farm = proposed_plot_claims(Kind::Farmstead, Vec3::ZERO, 0.3);
        let uses: Vec<_> = farm.iter().map(|claim| claim.land_use).collect();
        assert_eq!(uses, [LandUse::Building, LandUse::Field, LandUse::Field]);
        assert!(
            farm.iter()
                .skip(1)
                .all(|field| (field.margin - FIELD_VERGE_MARGIN).abs() < 1e-6)
        );

        let barn = building_claims(Kind::LivestockFarm, Vec3::ZERO, 0.3, false);
        let uses: Vec<_> = barn.iter().map(|claim| claim.land_use).collect();
        assert_eq!(
            uses,
            [LandUse::Building, LandUse::Doorway, LandUse::Pasture]
        );

        let pending_farm = building_claims(Kind::Farmstead, Vec3::ZERO, 0.3, true);
        assert_eq!(pending_farm.as_slice().len(), 4);
        let completed_farm = building_claims(Kind::Farmstead, Vec3::ZERO, 0.3, false);
        // The fixed fallback fields sit inside the intended envelope.
        for legacy in completed_farm
            .iter()
            .filter(|c| c.land_use == LandUse::Field)
        {
            assert!(
                pending_farm
                    .iter()
                    .filter(|c| c.land_use == LandUse::Field)
                    .any(|intended| intended.penetration(legacy) > 0.0)
            );
        }
        assert_eq!(
            proposed_plot_claims(Kind::House, Vec3::ZERO, 0.0)
                .as_slice()
                .len(),
            1
        );
    }

    #[test]
    fn the_hall_reserves_its_future_shell_and_a_forecourt_in_front_of_the_door() {
        let hall = Vec3::new(100.0, 0.0, 50.0);
        let [shell, forecourt] = hall_claims(hall);
        assert_eq!(shell.margin, CIVIC_YARD_MARGIN);
        assert_eq!(forecourt.margin, 0.0);
        assert_eq!(forecourt.half_extents, HALL_FORECOURT_HALF_EXTENTS);
        let door = Kind::Hall.entrance_position(hall, 0.0).xz();
        // Halls face -Z: the forecourt extends from the door away from the shell.
        assert!((forecourt.center.y - (door.y - HALL_FORECOURT_HALF_EXTENTS.y)).abs() < 1e-4);
        assert!((forecourt.center.x - door.x).abs() < 1e-4);
        assert_eq!(shell.penetration(&forecourt), 0.0);
        assert!(
            shell.gap(&forecourt) < 1.0,
            "forecourt must begin at the door"
        );

        // A cabin beside the Hall keeps 4.5 m from the future shell...
        let side = CivicHallLevel::reserved_half_extents().x
            + CIVIC_YARD_MARGIN
            + HOUSE_YARD_MARGIN
            + Kind::House.placement_definition().footprint.x * 0.5;
        let beside = house(shell.center.x + side + 0.05, shell.center.y, 0.0);
        assert!(!beside.conflicts_with(&shell) && !beside.conflicts_with(&forecourt));
        let too_close = house(shell.center.x + side - 0.5, shell.center.y, 0.0);
        assert!(too_close.conflicts_with(&shell));
        // ...and stays 1.5 m outside the forecourt.
        let depth = Kind::House.placement_definition().footprint.y * 0.5;
        let front = forecourt.center.y - HALL_FORECOURT_HALF_EXTENTS.y - HOUSE_YARD_MARGIN;
        let facing = house(door.x, front - depth - 0.095 - 0.05, PI);
        assert!(
            !facing.conflicts_with(&forecourt),
            "{}",
            facing.gap(&forecourt)
        );
        let intruding = house(door.x, front - depth + 0.5, PI);
        assert!(intruding.conflicts_with(&forecourt));
    }

    #[test]
    fn corridors_are_tested_against_the_rectangle_not_a_disc() {
        // A lane along X; a cabin side-on to it with its long wall parallel.
        let points = [Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0)];
        let half_width = RoadClass::Lane.initial_reserved_width() * 0.5;
        let width = Kind::House.placement_definition().footprint.x;
        let threshold = width * 0.5 + half_width + ROAD_VERGE;
        let clear = footprint_claim(
            Kind::House,
            Vec3::new(0.0, 0.0, threshold + 0.05),
            FRAC_PI_2,
        );
        assert!(!corridor_blocks_claim(&points, half_width, &clear));
        let corner_inside =
            footprint_claim(Kind::House, Vec3::new(0.0, 0.0, threshold - 0.3), FRAC_PI_2);
        assert!(corridor_blocks_claim(&points, half_width, &corner_inside));
        // The old disc rule would have refused the clear cabin: its corner
        // radius is larger than its half-width.
        let corner_radius = Kind::House.placement_definition().root_footprint_radius();
        assert!(corner_radius + ROAD_VERGE + half_width > threshold + 0.05);

        // Fields keep their whole graded terrace clear of a corridor.
        let field = LandClaim::new(
            Vec2::new(0.0, 10.0 + half_width + FARM_FIELD_TERRACE_MARGIN + 0.1),
            Vec2::new(10.0, 10.0),
            0.0,
            FIELD_VERGE_MARGIN,
            LandUse::Field,
        );
        assert!(!corridor_blocks_claim(&points, half_width, &field));
        let field = LandClaim {
            center: field.center - Vec2::Y * 0.2,
            ..field
        };
        assert!(corridor_blocks_claim(&points, half_width, &field));
    }

    #[test]
    fn refusal_wording_names_the_owner_and_measures_the_gap() {
        assert_eq!(
            land_owner_label(Some(Kind::House), Some(BuildingId(7)), false),
            "HOUSE #7"
        );
        assert_eq!(
            land_owner_label(Some(Kind::Farmstead), None, true),
            "FARMSTEAD (under construction)"
        );
        assert_eq!(
            land_owner_label(Some(Kind::Hall), None, false),
            "the Moot Hall"
        );
        assert_eq!(land_owner_label(None, None, false), "reserved land");

        let width = Kind::House.placement_definition().footprint.x;
        let neighbour = house(0.0, 0.0, 0.0);
        let close = house(width + 2.0 * HOUSE_YARD_MARGIN - 1.0, 0.0, 0.0);
        assert_eq!(
            land_conflict_sentence(&close, &neighbour, "HOUSE #7"),
            "Too close to HOUSE #7: 2.0 m of 3.0 m."
        );
        let apron = doorway_claim(Kind::House, Vec3::ZERO, 0.0);
        let fence = LandClaim::new(
            apron.center,
            Vec2::splat(0.5),
            0.0,
            FIELD_VERGE_MARGIN,
            LandUse::Field,
        );
        assert_eq!(
            land_conflict_sentence(&fence, &apron, "HOUSE #7"),
            "Wheat field overlaps HOUSE #7's doorway."
        );
        assert_eq!(
            lane_conflict_sentence(&close, "FARMSTEAD (under construction)"),
            "Overlaps the access lane reserved for FARMSTEAD (under construction)."
        );
        assert_eq!(
            road_conflict_sentence(&fence),
            "A road reservation crosses one of the wheat fields."
        );
    }

    #[test]
    fn the_worst_conflict_is_independent_of_snapshot_order() {
        let a = house(9.0, 0.0, 0.0);
        let b = house(-10.5, 0.0, 0.0);
        let plot = [house(0.0, 0.0, 0.0)];
        let (_, forward, shortfall_forward) = worst_conflict(&plot, [&a, &b]).unwrap();
        let (_, backward, shortfall_backward) = worst_conflict(&plot, [&b, &a]).unwrap();
        assert_eq!(shortfall_forward, shortfall_backward);
        assert_eq!(forward, 0);
        assert_eq!(backward, 1);
        assert!(worst_conflict(&plot, [&house(40.0, 0.0, 0.0)]).is_none());
    }
}
