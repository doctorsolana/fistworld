//! Civic upgrade levels through the real settlement scene and door consumers.

use super::CaptureConfig;
use bevy::prelude::*;
use shared::components::{
    BuildingDoorDemand, CivicHallLevel, PlayerPosition, Settlement, SettlementId, SettlementTier,
};
use shared::terrain::WorldTerrain;

pub(super) fn stage_capture_civic_halls(mut commands: Commands) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HALLS") else {
        return;
    };
    assert!(matches!(
        mode.as_str(),
        "moot" | "village" | "town" | "lineup"
    ));
    commands.queue(move |world: &mut World| {
        let focus = world.resource::<CaptureConfig>().shots[0].focus;
        let ground = world
            .resource::<WorldTerrain>()
            .get_height(focus.x, focus.z);
        for (index, (name, level, tier)) in [
            ("moot", CivicHallLevel::Moot, SettlementTier::Hamlet),
            ("village", CivicHallLevel::Village, SettlementTier::Village),
            ("town", CivicHallLevel::Town, SettlementTier::Town),
        ]
        .into_iter()
        .enumerate()
        {
            if mode != "lineup" && mode != name {
                continue;
            }
            let offset = if mode == "lineup" {
                (index as f32 - 1.0) * 19.0
            } else {
                0.0
            };
            let at = Vec3::new(focus.x + offset, ground, focus.z);
            // Founding reserves and levels the largest Hall before the first
            // rung exists. Keep the exact shared centre/extent contract here.
            let definition = CivicHallLevel::largest_supported()
                .building_type()
                .definition();
            let centre = definition.world_footprint_center(at, 0.0);
            world.resource_mut::<WorldTerrain>().apply_flatten_rect(
                Vec3::new(centre.x, ground, centre.y),
                definition.terrain_flat_half_extents(),
                0.0,
                definition.terrain_blend_width(),
            );
            world.spawn((
                Settlement {
                    name: format!("Civic Art {name}"),
                    tier,
                    residents: 30,
                    treasury: 0,
                },
                SettlementId(970 + index as u64),
                level,
                PlayerPosition(at),
                BuildingDoorDemand {
                    open: std::env::var("FISTFORCE_CAPTURE_DOORS").as_deref() == Ok("open"),
                },
            ));
        }
    });
}
