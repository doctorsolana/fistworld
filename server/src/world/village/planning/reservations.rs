//! One defense reservation boundary for private and autonomous construction.

use bevy::prelude::*;
use shared::components::{FortificationKind, SettlementBuildingKind as Kind, SettlementDefenses};

use super::road_access::RoadAccessBlocker;

/// A charter may overlap another settlement's land. Copy only nearby immutable
/// circuits into this permit decision's compact geometry snapshot, regardless
/// of who owns them; this runs only when a plot is actually being considered.
pub(crate) fn nearby_defense_reservations<'a>(
    defenses: impl Iterator<Item = &'a SettlementDefenses>,
    center: Vec2,
    reach: f32,
) -> SettlementDefenses {
    SettlementDefenses {
        circuits: defenses
            .flat_map(|defenses| &defenses.circuits)
            .filter(|circuit| {
                circuit.sections.iter().any(|section| {
                    section.midpoint().xz().distance(center)
                        <= reach
                            + section.length() * 0.5
                            + shared::components::DEFENSE_CORRIDOR_HALF_WIDTH
                })
            })
            .cloned()
            .collect(),
    }
}

pub(crate) fn plot_intersects_defenses(
    defenses: &SettlementDefenses,
    kind: Kind,
    position: Vec3,
    rotation: f32,
) -> bool {
    let definition = kind.placement_definition();
    if defenses.intersects_footprint(
        definition.world_footprint_center(position, rotation),
        definition.footprint * 0.5 + Vec2::splat(0.45),
        rotation,
    ) {
        return true;
    }
    if let (Some(fields), Some(half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        if fields.into_iter().any(|field| {
            defenses.intersects_footprint(
                field.xz(),
                half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                rotation,
            )
        }) {
            return true;
        }
    }
    if let (Some(pasture), Some(half)) = (
        kind.pasture_position(position, rotation),
        kind.pasture_half_extents(),
    ) {
        if defenses.intersects_footprint(pasture.xz(), half + Vec2::splat(1.0), rotation) {
            return true;
        }
    }
    false
}

/// Future connectors respect reserved walls from the outset. Gate spans stay
/// traversable, so completing defenses cannot sever a recently approved road.
pub(crate) fn defense_access_blockers(
    defenses: &SettlementDefenses,
) -> impl Iterator<Item = RoadAccessBlocker> + '_ {
    defenses
        .circuits
        .iter()
        .flat_map(|circuit| &circuit.sections)
        .filter(|section| section.kind == FortificationKind::Wall)
        .map(|section| RoadAccessBlocker {
            center: section.midpoint().xz(),
            half: Vec2::new(
                section.length() * 0.5,
                shared::components::DEFENSE_CORRIDOR_HALF_WIDTH,
            ) + Vec2::splat(
                shared::components::RoadClass::Lane.initial_reserved_width() * 0.5 + 0.45,
            ),
            rotation: section.rotation(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{
        DefenseCircuit, FortificationMaterial, FortificationSegment, SettlementId,
    };

    fn defenses_at(point: Vec2) -> SettlementDefenses {
        SettlementDefenses {
            circuits: vec![DefenseCircuit {
                id: 0,
                center: Vec2::ZERO,
                boundary: vec![],
                sections: vec![FortificationSegment {
                    settlement_id: SettlementId(1),
                    circuit: 0,
                    start: Vec3::new(point.x - 3.0, 0.0, point.y),
                    end: Vec3::new(point.x + 3.0, 0.0, point.y),
                    kind: FortificationKind::Wall,
                    material: FortificationMaterial::Palisade,
                    complete: false,
                }],
            }],
        }
    }

    #[test]
    fn fields_and_pastures_reserve_defenses_even_when_the_cabin_is_clear() {
        for kind in [Kind::Farmstead, Kind::LivestockFarm] {
            let rotation = 0.7;
            let position = Vec3::ZERO;
            let remote = kind
                .field_positions(position, rotation)
                .map(|fields| fields[0])
                .or_else(|| kind.pasture_position(position, rotation))
                .unwrap();
            let defenses = defenses_at(remote.xz());
            assert!(!defenses.intersects_footprint(
                kind.placement_definition()
                    .world_footprint_center(position, rotation),
                kind.placement_definition().footprint * 0.5,
                rotation
            ));
            assert!(plot_intersects_defenses(
                &defenses, kind, position, rotation
            ));
        }
    }

    #[test]
    fn roads_respect_unbuilt_walls_but_keep_gateways_open() {
        let mut defenses = defenses_at(Vec2::ZERO);
        let blockers: Vec<_> = defense_access_blockers(&defenses).collect();
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].blocks_segment(Vec2::new(0.0, -10.0), Vec2::new(0.0, 10.0)));
        defenses.circuits[0].sections[0].kind = FortificationKind::Gate;
        assert_eq!(defense_access_blockers(&defenses).count(), 0);
        assert!(
            plot_intersects_defenses(&defenses, Kind::House, Vec3::ZERO, 0.0),
            "the gate is open to traffic, but no house may occupy its passage"
        );
    }

    #[test]
    fn neighboring_settlement_defenses_are_reserved_without_copying_distant_cities() {
        let near = defenses_at(Vec2::new(100.0, 0.0));
        let far = defenses_at(Vec2::new(1200.0, 0.0));
        let snapshot = nearby_defense_reservations([&near, &far].into_iter(), Vec2::ZERO, 360.0);
        assert_eq!(snapshot.circuits.len(), 1);
        assert!(plot_intersects_defenses(
            &snapshot,
            Kind::House,
            Vec3::new(100.0, 0.0, 0.0),
            0.0
        ));
        assert!(!plot_intersects_defenses(
            &snapshot,
            Kind::House,
            Vec3::new(1200.0, 0.0, 0.0),
            0.0
        ));
    }
}
