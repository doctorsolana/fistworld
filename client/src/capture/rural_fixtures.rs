//! Authored rural workplaces and their real walkable field/pasture consumers.

use super::CaptureConfig;
use bevy::prelude::*;
use shared::components::{
    BuildingDoorDemand, FarmField, LivestockPasture, PlayerPosition, PlayerRotation,
    SettlementBuilding, SettlementBuildingKind, FARM_FIELD_TERRACE_MARGIN,
};
use shared::terrain::WorldTerrain;

pub(super) fn stage_capture_rural(mut commands: Commands) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_RURAL") else {
        return;
    };
    assert!(matches!(
        mode.as_str(),
        "farmstead" | "livestock" | "quarry" | "church" | "lineup"
    ));
    commands.queue(move |world: &mut World| {
        let focus = world.resource::<CaptureConfig>().shots[0].focus;
        for (index, (name, kind)) in [
            ("farmstead", SettlementBuildingKind::Farmstead),
            ("livestock", SettlementBuildingKind::LivestockFarm),
            ("quarry", SettlementBuildingKind::StoneQuarry),
            ("church", SettlementBuildingKind::Church),
        ]
        .into_iter()
        .enumerate()
        {
            if mode != "lineup" && mode != name {
                continue;
            }
            let offset = if mode == "lineup" {
                (index as f32 - 1.5) * 29.0
            } else {
                0.0
            };
            // Match real placement: each site stands at its own terrain height.
            // A shared elevation digs the small farm plot into a distant hillside.
            let ground = world
                .resource::<WorldTerrain>()
                .get_height(focus.x + offset, focus.z);
            let at = Vec3::new(focus.x + offset, ground, focus.z);
            let def = kind.art().definition();
            let centre = def.world_footprint_center(at, 0.0);
            world.resource_mut::<WorldTerrain>().apply_flatten_rect(
                Vec3::new(centre.x, ground, centre.y),
                def.terrain_flat_half_extents(),
                0.0,
                def.terrain_blend_width(),
            );
            world.spawn((
                SettlementBuilding {
                    kind,
                    settlement: "Rural Art".into(),
                    owner: None,
                    quality: 0.9,
                    workers: vec!["Art inspection worker".into()],
                },
                PlayerPosition(at),
                PlayerRotation(0.0),
                BuildingDoorDemand { open: false },
            ));
            if let Some(fields) = kind.field_positions(at, 0.0) {
                for (index, position) in fields.into_iter().enumerate() {
                    world.resource_mut::<WorldTerrain>().apply_flatten_rect(
                        position,
                        kind.field_half_extents().unwrap() + Vec2::splat(FARM_FIELD_TERRACE_MARGIN),
                        0.0,
                        1.5,
                    );
                    world.spawn((
                        FarmField {
                            settlement: "Rural Art".into(),
                            farmstead: at,
                            plot_index: index as u8,
                            quality: 0.9,
                        },
                        PlayerPosition(position),
                        PlayerRotation(0.0),
                    ));
                }
            }
            if let Some(position) = kind.pasture_position(at, 0.0) {
                world.resource_mut::<WorldTerrain>().apply_flatten_rect(
                    position,
                    kind.pasture_half_extents().unwrap() + Vec2::splat(1.0),
                    0.0,
                    1.5,
                );
                world.spawn((
                    LivestockPasture {
                        settlement: "Rural Art".into(),
                        livestock_farm: at,
                        quality: 0.9,
                    },
                    PlayerPosition(position),
                    PlayerRotation(0.0),
                ));
            }
        }
    });
}
