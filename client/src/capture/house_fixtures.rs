//! Isolated occupied homes for the real building, window and door consumers.

use super::CaptureConfig;
use bevy::prelude::*;
use shared::components::{
    BuildingDoorDemand, BuildingId, CharacterActivity, CharacterKind, CharacterName,
    HouseAppearance, HouseLevel, HouseLine, Household, PlayerPosition, PlayerRotation, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementTier,
};
use shared::terrain::WorldTerrain;

const VARIANTS: [(&str, HouseLine, HouseLevel); 4] = [
    ("cabin-l1", HouseLine::Cabin, HouseLevel::Ground),
    ("long-l1", HouseLine::LongCabin, HouseLevel::Ground),
    ("cabin-l2", HouseLine::Cabin, HouseLevel::UpperStorey),
    ("long-l2", HouseLine::LongCabin, HouseLevel::UpperStorey),
];

pub(super) fn stage_capture_houses(mut commands: Commands) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HOUSES") else {
        return;
    };
    assert!(
        mode == "lineup" || VARIANTS.iter().any(|entry| entry.0 == mode),
        "unknown FISTFORCE_CAPTURE_HOUSES mode: {mode}"
    );
    // Run after startup commands have installed the same terrain used by gameplay.
    commands.queue(move |world: &mut World| {
        let focus = world.resource::<CaptureConfig>().shots[0].focus;
        world.spawn((
            Settlement {
                name: "House Model Lab".into(),
                tier: SettlementTier::Hamlet,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(focus + Vec3::new(0.0, 0.0, 140.0)),
        ));
        for (index, (name, line, level)) in VARIANTS.into_iter().enumerate() {
            if mode != "lineup" && mode != name {
                continue;
            }
            let appearance = HouseAppearance { line, level };
            let offset = if mode == "lineup" {
                Vec3::new(
                    if index % 2 == 0 { -6.0 } else { 6.0 },
                    0.0,
                    if index < 2 { -6.0 } else { 6.0 },
                )
            } else {
                Vec3::ZERO
            };
            let mut at = focus + offset;
            let mut terrain = world.resource_mut::<WorldTerrain>();
            at.y = terrain.get_height(at.x, at.z);
            let definition = appearance.building_type().definition();
            terrain.apply_flatten_rect(
                at,
                definition.terrain_flat_half_extents(),
                0.0,
                definition.terrain_blend_width(),
            );
            let resident = format!("Resident of {name}");
            world.spawn((
                CharacterName(resident.clone()),
                CharacterKind::Villager,
                CharacterActivity::Indoors,
            ));
            world.spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "House Model Lab".into(),
                    owner: Some(resident.clone()),
                    quality: 0.8,
                    workers: Vec::new(),
                },
                Household {
                    residents: vec![resident],
                    ..default()
                },
                appearance,
                BuildingId(900 + index as u64),
                PlayerPosition(at),
                PlayerRotation(0.0),
                BuildingDoorDemand {
                    open: std::env::var("FISTFORCE_CAPTURE_DOORS").as_deref() == Ok("open"),
                },
            ));
        }
    });
}
