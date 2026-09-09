//! Explicit presentation fixture for the public square, not a grown town.
use bevy::prelude::*;
use shared::components::*;
use shared::terrain::WorldTerrain;

pub(super) fn stage(mut commands: Commands) {
    if std::env::var("FISTFORCE_CAPTURE_CIVIC_SQUARE").as_deref() != Ok("review") {
        return;
    }
    commands.queue(|world: &mut World| {
        let square = SettlementCivicSquare {
            center: Vec3::new(-100.0, 0.0, 96.0),
            half_extents: Vec2::splat(14.0),
            rotation: 0.0,
            market_position: Vec3::new(-100.0, 0.0, 90.0),
            market_rotation: std::f32::consts::PI,
        };
        let ground = |world: &World, point: Vec3| {
            let terrain = world.resource::<WorldTerrain>();
            let y = terrain.get_height(point.x, point.z);
            assert!(terrain
                .water_surface_height(point.x, point.z)
                .is_none_or(|water| y > water));
            Vec3::new(point.x, y, point.z)
        };
        let mut roots = Vec::new();
        let hall = ground(world, Vec3::new(-100.0, 0.0, 120.0));
        roots.push(
            world
                .spawn((
                    Settlement {
                        name: "Square presentation fixture".into(),
                        tier: SettlementTier::Town,
                        residents: 100,
                        treasury: 0,
                    },
                    SettlementId(995),
                    PlayerPosition(hall),
                    PlayerRotation(0.0),
                    CivicHallLevel::Town,
                    BuildingDoorDemand::default(),
                    square.clone(),
                ))
                .id(),
        );
        let market = ground(world, square.market_position);
        let mut sites = vec![(
            SettlementBuildingKind::Market,
            market,
            square.market_rotation,
        )];
        for side in [-1.0, 1.0] {
            for z in [84.0, 101.0] {
                sites.push((
                    SettlementBuildingKind::House,
                    ground(world, Vec3::new(-100.0 + side * 24.0, 0.0, z)),
                    side * std::f32::consts::FRAC_PI_2,
                ));
            }
        }
        // Match ordinary building earthworks so foundations sit on real ground.
        for (kind, point, rotation) in &sites {
            let def = kind.art().definition();
            world.resource_mut::<WorldTerrain>().apply_flatten_rect(
                *point,
                def.terrain_flat_half_extents(),
                *rotation,
                def.terrain_blend_width(),
            );
        }
        let def = CivicHallLevel::Town.building_type().definition();
        world.resource_mut::<WorldTerrain>().apply_flatten_rect(
            hall,
            def.terrain_flat_half_extents(),
            0.0,
            def.terrain_blend_width(),
        );
        for (kind, point, rotation) in sites {
            let mut entity = world.spawn((
                SettlementBuilding {
                    kind,
                    settlement: "Square presentation fixture".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                BuildingOf(SettlementId(995)),
                PlayerPosition(point),
                PlayerRotation(rotation),
                BuildingDoorDemand::default(),
            ));
            if kind == SettlementBuildingKind::Market {
                entity.insert(MarketLevel::Paved);
            } else {
                entity.insert(HouseAppearance::for_new_house(SettlementTier::Town, point));
            }
            roots.push(entity.id());
        }
        world.insert_resource(super::town_fixtures::TownCaptureScenes(roots));
    });
}
