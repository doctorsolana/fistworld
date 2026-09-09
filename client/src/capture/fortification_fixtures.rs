//! Repeatable geometry review through the production defense renderer.
//! The isolated sample is explicitly not a simulation-growth result.

use bevy::prelude::*;
use shared::components::{
    CharacterActivity, CharacterKind, CharacterName, FortificationKind, FortificationMaterial,
    FortificationSegment, PlayerPosition, PlayerRotation, SettlementId,
};
use shared::terrain::WorldTerrain;

pub(super) fn stage(mut commands: Commands) {
    if std::env::var("FISTFORCE_CAPTURE_FORTIFICATIONS").as_deref() != Ok("review") {
        return;
    }
    commands.queue(|world: &mut World| {
        let manifest =
            shared::character::CharacterManifest::load().expect("gate scale-reference outfit");
        for (z, material) in [
            (108.0, FortificationMaterial::Palisade),
            (134.0, FortificationMaterial::Stone),
        ] {
            for (left, right, kind) in [
                (-118.0, -104.0, FortificationKind::Wall),
                (-104.0, -96.0, FortificationKind::Gate),
                (-96.0, -82.0, FortificationKind::Wall),
            ] {
                let terrain = world.resource::<WorldTerrain>();
                let start = Vec3::new(left, terrain.get_height(left, z), z);
                let end = Vec3::new(right, terrain.get_height(right, z), z);
                assert!(
                    terrain
                        .water_surface_height(left, z)
                        .is_none_or(|water| start.y > water),
                    "defense fixture must stay on land"
                );
                let section = FortificationSegment {
                    settlement_id: SettlementId(999),
                    circuit: 0,
                    start,
                    end,
                    kind,
                    material,
                    complete: true,
                };
                world.spawn((PlayerPosition(section.midpoint()), section));
            }
            let at = Vec3::new(
                -101.0,
                world.resource::<WorldTerrain>().get_height(-101.0, z - 2.0),
                z - 2.0,
            );
            world.spawn((
                CharacterKind::Villager,
                CharacterName("Gateway scale reference".into()),
                shared::components::HeroOutfit::from_manifest(&manifest),
                shared::components::CharacterMotion::default(),
                CharacterActivity::Idle,
                PlayerPosition(at),
                PlayerRotation(0.0),
            ));
        }
        // Reuse semantic readiness for all generated defense meshes.
        world.insert_resource(super::town_fixtures::TownCaptureScenes::default());
    });
}
