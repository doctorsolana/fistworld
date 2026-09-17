//! Occupied land: the one snapshot NPC planning and player permits both search.
//!
//! Every reservation is a shared [`LandClaim`], so the geometry cannot drift
//! between the two placement paths or the client preview. This module only
//! adds who owns each claim, so a refusal can say whose yard, field or lane
//! is in the way, and turns the offending pair into the structured
//! [`PlacementBlocker`] carried beside the refusal text.

use bevy::prelude::*;
use shared::components::{
    BuildingId, FarmField, LandClaim, LandUse, ReservedAccessLane, SettlementBuilding,
    SettlementBuildingKind, SettlementId, accepted_field_claims, building_claims, hall_claims,
    land_conflict_sentence, land_owner_label, lane_conflict_sentence, road_conflict_sentence,
};
use shared::protocol::{PlacementBlocker, PlacementBlockerKind};

use crate::world::village::UnderConstruction;
use crate::world::village_roads::PlannedRoadAccess;

/// Who reserved a piece of land, for the refusal message and the client's
/// highlight. Pending worksites have no durable [`BuildingId`] yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LandOwner {
    pub(crate) entity: Option<Entity>,
    pub(crate) id: Option<BuildingId>,
    pub(crate) kind: Option<SettlementBuildingKind>,
    pub(crate) pending: bool,
}

impl LandOwner {
    /// A reservation nobody in particular owns (fixtures and civic ground).
    pub(crate) const ANONYMOUS: Self = Self {
        entity: None,
        id: None,
        kind: None,
        pending: false,
    };

    pub(crate) const HALL: Self = Self {
        entity: None,
        id: None,
        kind: Some(SettlementBuildingKind::Hall),
        pending: false,
    };

    pub(crate) fn completed(
        entity: Option<Entity>,
        id: Option<BuildingId>,
        kind: SettlementBuildingKind,
    ) -> Self {
        Self {
            entity,
            id,
            kind: Some(kind),
            pending: false,
        }
    }

    pub(crate) fn pending(entity: Option<Entity>, kind: SettlementBuildingKind) -> Self {
        Self {
            entity,
            id: None,
            kind: Some(kind),
            pending: true,
        }
    }

    /// The owner of a lane reservation, read from whatever the reserving
    /// entity is today: a pending worksite, a completed but still roadless
    /// building, or infrastructure with no building at all.
    pub(crate) fn of_reserving_entity(
        entity: Entity,
        site: Option<&UnderConstruction>,
        building: Option<&SettlementBuilding>,
        id: Option<&BuildingId>,
    ) -> Self {
        if let Some(site) = site {
            Self::pending(Some(entity), site.kind)
        } else if let Some(building) = building {
            Self::completed(Some(entity), id.copied(), building.kind)
        } else {
            Self {
                entity: Some(entity),
                ..Self::ANONYMOUS
            }
        }
    }

    /// Player-facing name: `HOUSE #7`, `FARMSTEAD (under construction)`,
    /// `the Moot Hall` or `reserved land`, worded by the shared rule the
    /// client preview also uses.
    pub(crate) fn label(&self) -> String {
        land_owner_label(self.kind, self.id, self.pending)
    }
}

/// One reserved rectangle and its owner.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OccupiedLand {
    pub(crate) claim: LandClaim,
    pub(crate) owner: LandOwner,
}

impl OccupiedLand {
    fn tagged(claims: impl IntoIterator<Item = LandClaim>, owner: LandOwner) -> Vec<Self> {
        claims
            .into_iter()
            .map(|claim| Self { claim, owner })
            .collect()
    }

    /// The founding Hall's future shell, commons and forecourt.
    pub(crate) fn hall(hall: Vec3) -> Vec<Self> {
        Self::tagged(hall_claims(hall), LandOwner::HALL)
    }

    /// A completed building: its shell, doorway apron, fixed fallback fields
    /// and pasture. Accepted crop shapes are added separately by
    /// [`Self::field`].
    pub(crate) fn building(
        kind: SettlementBuildingKind,
        position: Vec3,
        rotation: f32,
        owner: LandOwner,
    ) -> Vec<Self> {
        Self::tagged(
            building_claims(kind, position, rotation, false)
                .iter()
                .copied(),
            owner,
        )
    }

    /// A pending or same-tick accepted plot: its shell, doorway apron, the
    /// whole intended field envelope and its pasture.
    pub(crate) fn plot(
        kind: SettlementBuildingKind,
        position: Vec3,
        rotation: f32,
        owner: LandOwner,
    ) -> Vec<Self> {
        Self::tagged(
            building_claims(kind, position, rotation, true)
                .iter()
                .copied(),
            owner,
        )
    }

    /// The accepted crop bands of one surveyed field.
    pub(crate) fn field(
        field: &FarmField,
        position: Vec3,
        rotation: f32,
        owner: LandOwner,
    ) -> Vec<Self> {
        Self::tagged(accepted_field_claims(field, position, rotation), owner)
    }

    /// An anonymous solid reservation for fixtures and tests.
    pub(crate) fn block(center: Vec3, half_extents: Vec2, rotation: f32) -> Self {
        Self {
            claim: LandClaim::new(center.xz(), half_extents, rotation, 0.0, LandUse::Building),
            owner: LandOwner::ANONYMOUS,
        }
    }

    pub(crate) fn blocks(&self, claim: &LandClaim) -> bool {
        self.claim.conflicts_with(claim)
    }
}

/// Coarse plot count used to widen search bands and sign failed searches:
/// buildings, fields and pastures, exactly the entries the former disc list
/// carried, so search budgets did not change with the geometry.
pub(crate) fn occupied_plot_count(occupied: &[OccupiedLand]) -> usize {
    occupied
        .iter()
        .filter(|land| {
            matches!(
                land.claim.land_use,
                LandUse::Building | LandUse::Field | LandUse::Pasture
            )
        })
        .count()
}

/// The offending pair needing the largest move, so the blocker a refusal
/// names never depends on snapshot order. The rule itself is
/// [`shared::components::worst_conflict`], shared with the client preview.
pub(crate) fn worst_land_conflict<'a>(
    parts: &'a [LandClaim],
    occupied: &'a [OccupiedLand],
) -> Option<(&'a LandClaim, &'a OccupiedLand, f32)> {
    shared::components::worst_conflict(parts, occupied.iter().map(|land| &land.claim))
        .map(|(part, land, shortfall)| (&parts[part], &occupied[land], shortfall))
}

/// A reserved access corridor and who reserved it.
#[derive(Clone, Debug)]
pub(crate) struct LaneReservation {
    pub(crate) lane: ReservedAccessLane,
    pub(crate) settlement_id: SettlementId,
    pub(crate) owner: LandOwner,
}

impl LaneReservation {
    pub(crate) fn new(access: &PlannedRoadAccess, owner: LandOwner) -> Self {
        Self {
            lane: access.lane(),
            settlement_id: access.settlement_id,
            owner,
        }
    }

    pub(crate) fn blocks(&self, claim: &LandClaim) -> bool {
        self.lane.blocks_claim(claim)
    }
}

/// A completed building as the snapshot sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedBuilding {
    pub(crate) entity: Option<Entity>,
    pub(crate) id: Option<BuildingId>,
    pub(crate) kind: SettlementBuildingKind,
    pub(crate) position: Vec3,
    pub(crate) rotation: f32,
}

/// A pending worksite, or a plot accepted earlier in the same tick.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingPlot {
    pub(crate) entity: Option<Entity>,
    pub(crate) kind: SettlementBuildingKind,
    pub(crate) position: Vec3,
    pub(crate) rotation: f32,
}

/// Every reservation a new plot must respect: completed buildings with their
/// aprons, fallback fields and pastures; the Hall; pending plots with their
/// intended field envelopes; and every accepted crop band. NPC planning and
/// the player command both build their list here, so the two cannot drift.
pub(crate) fn occupied_land_snapshot<'a>(
    hall: Vec3,
    buildings: &[PlacedBuilding],
    pending: &[PendingPlot],
    fields: impl IntoIterator<Item = (&'a FarmField, Vec3, f32)>,
) -> Vec<OccupiedLand> {
    let mut occupied = Vec::with_capacity(buildings.len() * 3 + pending.len() * 3 + 2);
    for building in buildings {
        occupied.extend(OccupiedLand::building(
            building.kind,
            building.position,
            building.rotation,
            LandOwner::completed(building.entity, building.id, building.kind),
        ));
    }
    occupied.extend(OccupiedLand::hall(hall));
    for plot in pending {
        occupied.extend(OccupiedLand::plot(
            plot.kind,
            plot.position,
            plot.rotation,
            LandOwner::pending(plot.entity, plot.kind),
        ));
    }
    for (field, position, rotation) in fields {
        let owner = buildings
            .iter()
            .find(|building| {
                building.kind == SettlementBuildingKind::Farmstead
                    && building
                        .position
                        .xz()
                        .distance_squared(field.farmstead.xz())
                        < 0.25
            })
            .map_or(
                LandOwner::completed(None, None, SettlementBuildingKind::Farmstead),
                |building| LandOwner::completed(building.entity, building.id, building.kind),
            );
        occupied.extend(OccupiedLand::field(field, position, rotation, owner));
    }
    occupied
}

/// Why a plot was refused: the sentence shown to the player and, when a
/// reservation refused it, the machine-readable blocker.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacementRefusal {
    pub(crate) message: String,
    pub(crate) blocker: Option<PlacementBlocker>,
}

impl From<&str> for PlacementRefusal {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
            blocker: None,
        }
    }
}

impl From<String> for PlacementRefusal {
    fn from(message: String) -> Self {
        Self {
            message,
            blocker: None,
        }
    }
}

impl std::fmt::Display for PlacementRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn centimetres(metres: f32) -> u32 {
    (metres.max(0.0) * 100.0).round() as u32
}

impl PlacementRefusal {
    /// "Too close to HOUSE #7: 1.9 m of 3.0 m." or "Overlaps HOUSE #7's doorway."
    /// The sentence comes from the shared rule set, so the client's preview
    /// guess and this authoritative refusal read identically.
    pub(crate) fn land(part: &LandClaim, land: &OccupiedLand) -> Self {
        let shortfall = part.shortfall(&land.claim).unwrap_or(0.0);
        let label = land.owner.label();
        Self {
            message: land_conflict_sentence(part, &land.claim, &label),
            blocker: Some(PlacementBlocker {
                kind: land.claim.land_use.into(),
                building: land.owner.id,
                label,
                shortfall_cm: centimetres(shortfall),
            }),
        }
    }

    /// "Overlaps the access lane reserved for FARMSTEAD (under construction)."
    pub(crate) fn lane(part: &LandClaim, lane: &LaneReservation) -> Self {
        let owner = lane.owner.label();
        Self {
            message: lane_conflict_sentence(part, &owner),
            blocker: Some(PlacementBlocker {
                kind: PlacementBlockerKind::AccessLane,
                building: lane.owner.id,
                label: owner,
                shortfall_cm: 0,
            }),
        }
    }

    /// A protected road corridor, named by the part of the plot that crosses it.
    pub(crate) fn road(part: &LandClaim) -> Self {
        Self {
            message: road_conflict_sentence(part).to_string(),
            blocker: Some(PlacementBlocker {
                kind: PlacementBlockerKind::Road,
                building: None,
                label: "a road reservation".to_string(),
                shortfall_cm: 0,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{HOUSE_YARD_MARGIN, footprint_claim};

    #[test]
    fn owners_are_named_by_kind_and_durable_id() {
        let kind = SettlementBuildingKind::House;
        assert_eq!(
            LandOwner::completed(None, Some(BuildingId(7)), kind).label(),
            "HOUSE #7"
        );
        assert_eq!(
            LandOwner::pending(None, SettlementBuildingKind::Farmstead).label(),
            "FARMSTEAD (under construction)"
        );
        assert_eq!(LandOwner::HALL.label(), "the Moot Hall");
        assert_eq!(LandOwner::ANONYMOUS.label(), "reserved land");
    }

    #[test]
    fn refusals_name_the_blocker_and_measure_the_shortfall() {
        let kind = SettlementBuildingKind::House;
        let width = kind.placement_definition().footprint.x;
        let neighbour = OccupiedLand::building(
            kind,
            Vec3::ZERO,
            0.0,
            LandOwner::completed(None, Some(BuildingId(7)), kind),
        );
        let close = footprint_claim(
            kind,
            Vec3::new(width + 2.0 * HOUSE_YARD_MARGIN - 1.04, 0.0, 0.0),
            0.0,
        );
        let parts = [close];
        let (part, land, shortfall) = worst_land_conflict(&parts, &neighbour).unwrap();
        assert!((shortfall - 1.04).abs() < 1e-3, "{shortfall}");
        let refusal = PlacementRefusal::land(part, land);
        assert_eq!(refusal.message, "Too close to HOUSE #7: 2.0 m of 3.0 m.");
        let blocker = refusal.blocker.unwrap();
        assert_eq!(blocker.kind, PlacementBlockerKind::Building);
        assert_eq!(blocker.building, Some(BuildingId(7)));
        assert_eq!(blocker.label, "HOUSE #7");
        assert_eq!(blocker.shortfall_cm, 104);

        // A field fence on the cabin's doorstep names the doorway.
        let apron = neighbour
            .iter()
            .find(|land| land.claim.land_use == LandUse::Doorway)
            .unwrap();
        let fence = LandClaim::new(
            apron.claim.center,
            Vec2::splat(0.5),
            0.0,
            shared::components::FIELD_VERGE_MARGIN,
            LandUse::Field,
        );
        let refusal = PlacementRefusal::land(&fence, apron);
        assert_eq!(refusal.message, "Wheat field overlaps HOUSE #7's doorway.");
        assert_eq!(refusal.blocker.unwrap().kind, PlacementBlockerKind::Doorway);
    }

    #[test]
    fn the_worst_conflict_is_independent_of_snapshot_order() {
        let kind = SettlementBuildingKind::House;
        let a = OccupiedLand::building(kind, Vec3::new(9.0, 0.0, 0.0), 0.0, LandOwner::ANONYMOUS);
        let b = OccupiedLand::building(kind, Vec3::new(-10.5, 0.0, 0.0), 0.0, LandOwner::ANONYMOUS);
        let plot = [footprint_claim(kind, Vec3::ZERO, 0.0)];
        let forward: Vec<_> = a.iter().chain(b.iter()).copied().collect();
        let backward: Vec<_> = b.iter().chain(a.iter()).copied().collect();
        let (_, worst_forward, shortfall_forward) = worst_land_conflict(&plot, &forward).unwrap();
        let (_, worst_backward, shortfall_backward) =
            worst_land_conflict(&plot, &backward).unwrap();
        assert_eq!(shortfall_forward, shortfall_backward);
        assert_eq!(worst_forward.claim, worst_backward.claim);
        assert_eq!(worst_forward.claim.center.x, 9.0);
    }

    #[test]
    fn the_snapshot_counts_only_plots_fields_and_pastures() {
        let hall = Vec3::new(1700.0, 0.0, 0.0);
        let buildings = [PlacedBuilding {
            entity: None,
            id: Some(BuildingId(2)),
            kind: SettlementBuildingKind::LivestockFarm,
            position: hall + Vec3::new(40.0, 0.0, 0.0),
            rotation: 0.0,
        }];
        let pending = [PendingPlot {
            entity: None,
            kind: SettlementBuildingKind::Farmstead,
            position: hall + Vec3::new(-60.0, 0.0, 0.0),
            rotation: 0.0,
        }];
        let occupied = occupied_land_snapshot(hall, &buildings, &pending, []);
        // barn + hall shell + farm shell + 2 intended fields + pasture
        assert_eq!(occupied_plot_count(&occupied), 6);
        assert!(
            occupied
                .iter()
                .any(|land| land.claim.land_use == LandUse::Pasture
                    && land.owner.label() == "LIVESTOCK FARM #2")
        );
        assert!(
            occupied
                .iter()
                .any(|land| land.claim.land_use == LandUse::Forecourt)
        );
        assert_eq!(
            occupied
                .iter()
                .filter(|land| land.claim.land_use == LandUse::Doorway)
                .count(),
            2
        );
    }
}
