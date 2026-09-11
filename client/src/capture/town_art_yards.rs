//! Explicit presentation-only migration of a fixed town's outdoor spaces.
//! Live clients only consume accepted server yards; they never grant land.

use bevy::prelude::*;
use std::collections::HashSet;

use shared::components::{HouseholdYardLand, SettlementBuildingKind};
use shared::settlement_snapshot::TownSnapshot;
use shared::terrain::{ChunkCoord, WorldTerrain};

pub(super) fn fit_yards(snapshot: &mut TownSnapshot, terrain: &WorldTerrain) -> Result<(), String> {
    let asset_root = std::env::var_os("BEVY_ASSET_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
    let colliders =
        shared::colliders::load_baked_collider_db_from_file(asset_root.join("colliders.bin"))?;
    let mut land = HouseholdYardLand::default();
    for building in &snapshot.buildings {
        if building.kind == SettlementBuildingKind::House && building.construction.is_none() {
            land.reserve_house(
                building.house.unwrap_or_default(),
                building.position,
                building.rotation,
            );
        } else if building.kind == SettlementBuildingKind::Farmstead
            && snapshot
                .fields
                .iter()
                .any(|field| field.component.farmstead == building.position)
        {
            land.reserve_building_without_inferred_fields(
                building.kind,
                building.position,
                building.rotation,
            );
        } else {
            land.reserve_building(building.kind, building.position, building.rotation);
        }
    }
    for town in &snapshot.settlements {
        land.reserve_building(SettlementBuildingKind::Hall, town.position, town.rotation);
        if let Some(square) = &town.civic_square {
            land.reserve_rect(square.center.xz(), square.half_extents, square.rotation);
        }
    }
    for road in &snapshot.roads {
        land.reserve_road(&road.road);
    }
    for field in &snapshot.fields {
        land.reserve_field(&field.component, field.position, field.rotation);
    }
    for defense in &snapshot.fortifications {
        land.reserve_segment(
            defense.start.xz(),
            defense.end.xz(),
            shared::components::DEFENSE_CORRIDOR_HALF_WIDTH,
        );
    }
    let mut houses: Vec<_> = snapshot
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| b.kind == SettlementBuildingKind::House && b.construction.is_none())
        .map(|(i, b)| (i, b.position))
        .collect();
    houses.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.z.total_cmp(&b.1.z)));
    let mut surveyed = HashSet::new();
    for (_, at) in &houses {
        let chunk = ChunkCoord::from_world_pos(*at);
        for x in chunk.x - 1..=chunk.x + 1 {
            for z in chunk.z - 1..=chunk.z + 1 {
                let chunk = ChunkCoord::new(x, z);
                if !surveyed.insert(chunk) {
                    continue;
                }
                for prop in shared::props::generate_chunk_blocking_props(&terrain.generator, chunk)
                {
                    use shared::colliders::BakedCollider;
                    let radius = colliders
                        .entries
                        .get(prop.kind.id())
                        .map_or(1.5, |collider| {
                            let radius = |points: &[[f32; 3]]| {
                                points
                                    .iter()
                                    .map(|p| p[0] * p[0] + p[2] * p[2])
                                    .fold(0.0, f32::max)
                                    .sqrt()
                            };
                            match collider {
                                BakedCollider::ConvexHull { points } => radius(points),
                                BakedCollider::CompoundConvex { hulls } => {
                                    hulls.iter().map(|hull| radius(hull)).fold(0.0, f32::max)
                                }
                            }
                        });
                    // Same baked radius as connected authoring, with a
                    // conservative square bound for the offline reservation.
                    land.reserve_rect(prop.position, Vec2::splat(radius * prop.scale + 0.3), 0.0);
                }
            }
        }
    }
    let mut accepted = 0;
    for (index, at) in houses {
        let building = &mut snapshot.buildings[index];
        let seed = shared::components::household_yard_seed(at);
        let yard = land.fit_yard(
            building.house.unwrap_or_default(),
            at,
            building.rotation,
            seed,
            |_, _| true,
            |point| {
                let h = terrain.get_height(point.x, point.y);
                (!terrain
                    .water_surface_height(point.x, point.y)
                    .is_some_and(|water| h < water + 0.35))
                .then_some(h)
            },
        );
        if let Some(yard) = &yard {
            land.reserve_yard(yard, at, building.rotation);
            accepted += 1;
        }
        building.yard = yard;
    }
    info!("capture: fitted {accepted} household yards to the authored town layout");
    Ok(())
}
