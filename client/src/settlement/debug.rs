//! Debug views of settlement hall envelopes and preferred planning rings.

use bevy::prelude::*;
use shared::components::{
    CivicHallLevel, PlayerPosition, PlayerRotation, Settlement, SettlementBuildingKind,
};
use shared::debug::DebugGizmoMode;

/// F4 view of the actual distance bands used by autonomous plot searches.
/// These are planning preferences, not a political border: green is housing,
/// amber is ordinary work, and blue is the furthest coastal search.
pub(super) fn debug_draw_settlement_planning_rings(
    mut gizmos: Gizmos,
    debug_mode: Res<DebugGizmoMode>,
    settlements: Query<(
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&CivicHallLevel>,
    )>,
) {
    if !debug_mode.0 {
        return;
    }

    let horizontal = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let house = SettlementBuildingKind::House.preferred_ring();
    let work = SettlementBuildingKind::Farmstead.preferred_ring();
    let fishing = SettlementBuildingKind::FishermansHut.preferred_ring();
    for (settlement, position, rotation, level) in settlements.iter() {
        let rotation = rotation.map_or(0.0, |rotation| rotation.0);
        let level = level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let current = level.building_type().definition();
        let current_center = current.world_footprint_center(position.0, rotation);
        let reserved_center = CivicHallLevel::reserved_world_center(position.0, rotation);
        let horizontal = Quat::from_rotation_y(rotation) * horizontal;
        gizmos.rect(
            Isometry3d::new(
                Vec3::new(current_center.x, position.0.y + 0.42, current_center.y),
                horizontal,
            ),
            current.footprint,
            Color::srgba(1.0, 0.78, 0.18, 0.95),
        );
        gizmos.rect(
            Isometry3d::new(
                Vec3::new(reserved_center.x, position.0.y + 0.40, reserved_center.y),
                horizontal,
            ),
            CivicHallLevel::reserved_half_extents() * 2.0,
            Color::srgba(0.92, 0.24, 1.0, 0.92),
        );
        let centre = position.0 + Vec3::Y * 0.35;
        for radius in [house.0, house.1] {
            gizmos
                .circle(
                    Isometry3d::new(centre, horizontal),
                    radius,
                    Color::srgba(0.2, 1.0, 0.35, 0.75),
                )
                .resolution(64);
        }
        for radius in [work.0, work.1] {
            gizmos
                .circle(
                    Isometry3d::new(centre, horizontal),
                    radius,
                    Color::srgba(1.0, 0.65, 0.15, 0.72),
                )
                .resolution(96);
        }
        gizmos
            .circle(
                Isometry3d::new(centre, horizontal),
                fishing.1,
                Color::srgba(0.15, 0.65, 1.0, 0.7),
            )
            .resolution(96);
    }
}
