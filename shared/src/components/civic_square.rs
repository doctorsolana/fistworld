//! Durable civic open ground reserved near the Hall before housing fills it.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{oriented_rects_overlap, SettlementBuildingKind as Kind};

/// World-space center with Bevy yaw. `half_extents` describes the entire civic
/// apron; the authored12m Marketplace occupies only its far half and faces the
/// Hall. This reservation never moves when the Hall changes physical level.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementCivicSquare {
    pub center: Vec3,
    pub half_extents: Vec2,
    pub rotation: f32,
    pub market_position: Vec3,
    pub market_rotation: f32,
}

impl SettlementCivicSquare {
    pub fn intersects_rect(&self, center: Vec2, half: Vec2, rotation: f32) -> bool {
        oriented_rects_overlap(
            self.center.xz(),
            self.half_extents,
            self.rotation,
            center,
            half,
            rotation,
        )
    }

    pub fn contains_market(&self, position: Vec3, rotation: f32) -> bool {
        if self.market_position.xz().distance(position.xz()) > 0.5
            || (rotation - self.market_rotation).cos() < 0.999
        {
            return false;
        }
        let definition = Kind::Market.placement_definition();
        let local = crate::rotation::world_to_local_xz(
            definition.world_footprint_center(position, rotation) - self.center.xz(),
            self.rotation,
        );
        let relative_rotation = rotation - self.rotation;
        let half = definition.footprint * 0.5;
        let extent = Vec2::new(
            relative_rotation.cos().abs() * half.x + relative_rotation.sin().abs() * half.y,
            relative_rotation.sin().abs() * half.x + relative_rotation.cos().abs() * half.y,
        );
        (local.abs() + extent).cmple(self.half_extents).all()
    }

    /// Full property checks include agricultural adjuncts, not just the cabin.
    /// Hall upgrades reuse their separately reserved permanent footprint.
    pub fn blocks_plot(&self, kind: Kind, position: Vec3, rotation: f32) -> bool {
        if kind == Kind::Hall || (kind == Kind::Market && self.contains_market(position, rotation))
        {
            return false;
        }
        // Preserve the same conservative permit clearance an already-built
        // Market would own. Otherwise a later farm's bounding field circle can
        // invalidate this exact civic anchor without touching the apron itself.
        let market_clearance = Kind::Market.clearance();
        if position.xz().distance(self.market_position.xz()) < kind.clearance() + market_clearance {
            return true;
        }
        let definition = kind.placement_definition();
        if self.intersects_rect(
            definition.world_footprint_center(position, rotation),
            definition.footprint * 0.5 + Vec2::splat(0.5),
            rotation,
        ) {
            return true;
        }
        if let (Some(fields), Some(half)) = (
            kind.field_positions(position, rotation),
            kind.field_half_extents(),
        ) {
            if fields.into_iter().any(|field| {
                field.xz().distance(self.market_position.xz())
                    < half.length() + super::FARM_FIELD_TERRACE_MARGIN + market_clearance
                    || self.intersects_rect(
                        field.xz(),
                        half + Vec2::splat(super::FARM_FIELD_TERRACE_MARGIN),
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
            if pasture.xz().distance(self.market_position.xz())
                < half.length() + 2.0 + market_clearance
                || self.intersects_rect(pasture.xz(), half + Vec2::splat(1.0), rotation)
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> SettlementCivicSquare {
        SettlementCivicSquare {
            center: Vec3::new(0.0, 2.0, -24.0),
            half_extents: Vec2::splat(14.0),
            rotation: 0.0,
            market_position: Vec3::new(0.0, 2.0, -30.0),
            market_rotation: std::f32::consts::PI,
        }
    }
    #[test]
    fn only_the_intended_market_can_occupy_the_reserved_square() {
        let square = square();
        assert!(!square.blocks_plot(Kind::Market, square.market_position, square.market_rotation));
        assert!(square.blocks_plot(Kind::House, square.market_position, square.market_rotation));
        assert!(square.blocks_plot(
            Kind::Market,
            square.market_position + Vec3::X * 4.0,
            square.market_rotation
        ));
        assert!(!square.blocks_plot(Kind::Hall, Vec3::ZERO, 0.0));
    }
    #[test]
    fn rotated_fields_cannot_consume_a_square_outside_their_cabin() {
        let mut square = square();
        let position = Vec3::ZERO;
        let rotation = 0.7;
        square.center = Kind::Farmstead.field_positions(position, rotation).unwrap()[0];
        square.half_extents = Vec2::splat(3.0);
        assert!(square.blocks_plot(Kind::Farmstead, position, rotation));
        let definition = Kind::Farmstead.placement_definition();
        assert!(!square.intersects_rect(
            definition.world_footprint_center(position, rotation),
            definition.footprint * 0.5,
            rotation
        ));
        let bytes = serde_json::to_vec(&square).unwrap();
        assert_eq!(
            serde_json::from_slice::<SettlementCivicSquare>(&bytes).unwrap(),
            square
        );
    }
}
