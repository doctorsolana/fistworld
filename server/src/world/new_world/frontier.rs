//! Bare-Hall founding. Certify real dry standing room before committing any
//! people; housing and private workplaces must subsequently be built normally.

use super::{layout::Layout, sites::Site};
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::{navgrid::NAVIGATION_SAMPLE_STEP, village_roads};
use bevy::prelude::*;
use shared::{components::SettlementBuildingKind, terrain::WorldTerrain};

pub(super) fn plan(
    terrain: &WorldTerrain,
    site: &Site,
    colliders: &StaticColliders,
    library: &DerivedColliderLibrary,
    founders: usize,
) -> Option<Layout> {
    let (door, approach) =
        village_roads::doorway_approach(SettlementBuildingKind::Hall, site.hall, 0.0);
    let outward = (approach - door).normalize_or_zero();
    let side = Vec2::new(-outward.y, outward.x);
    if !clear_corridor(terrain, door, approach, colliders, library) {
        return None;
    }
    let mut stands = Vec::with_capacity(founders);
    // Bounded frontage survey; no random scatter into the Hall shell and no
    // standing pair closer than two metres. Each accepted stand has its own
    // clear route back to the shared entrance, including trees and rocks.
    for row in 0..32 {
        for slot in 0..17 {
            let column = if slot == 0 {
                0
            } else if slot % 2 == 1 {
                (slot + 1) / 2
            } else {
                -(slot / 2)
            };
            let at = approach + outward * (2.0 + row as f32 * 2.0) + side * (column as f32 * 2.0);
            if clear_corridor(terrain, approach, at, colliders, library) {
                stands.push(Vec3::new(at.x, terrain.get_height(at.x, at.y), at.y));
                if stands.len() == founders {
                    return Some(Layout {
                        plots: Vec::new(),
                        population: founders,
                        // No paving, homes or private firms are prebuilt. The
                        // normal development planner owns the eventual centre.
                        square: None,
                        frontier_stands: stands,
                    });
                }
            }
        }
    }
    None
}

fn clear_corridor(
    terrain: &WorldTerrain,
    start: Vec2,
    end: Vec2,
    colliders: &StaticColliders,
    library: &DerivedColliderLibrary,
) -> bool {
    let bounds = terrain.generator.active_map_bounds();
    if !bounds.contains_xz(start.x, start.y)
        || !bounds.contains_xz(end.x, end.y)
        || !village_roads::road_segment_is_dry(terrain, start, end)
    {
        return false;
    }
    let samples = (start.distance(end) / NAVIGATION_SAMPLE_STEP)
        .ceil()
        .max(1.0) as usize;
    let mut last_height = terrain.get_height(start.x, start.y);
    (0..=samples).all(|index| {
        let at = start.lerp(end, index as f32 / samples as f32);
        let height = terrain.get_height(at.x, at.y);
        let passable = (height - last_height).abs() <= 0.47
            && village_roads::navigation_point_is_clear_of_props(at, colliders, library);
        last_height = height;
        passable
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::new_world::Community;
    use crate::world::start_config::WorldStartConfig;
    use bevy::ecs::system::RunSystemOnce;
    use shared::{
        components::*,
        economy::*,
        map::{HeightmapData, MapBounds},
        terrain::TerrainGenerator,
    };

    fn terrain(height: impl Fn(f32, f32) -> f32) -> WorldTerrain {
        let mut terrain = WorldTerrain::default();
        let mut map = terrain.generator.loaded_map().clone();
        let bounds = MapBounds {
            min: [-96.0; 2],
            max: [96.0; 2],
        };
        let mut heights = Vec::new();
        for z in 0..193 {
            for x in 0..193 {
                heights.push(height(x as f32 - 96.0, z as f32 - 96.0));
            }
        }
        map.definition.bounds = bounds;
        map.definition.generated = None;
        map.heightmap = HeightmapData::new(bounds, 193, 193, heights, Some(0.0));
        map.rivers = default();
        map.river_segments_by_chunk.clear();
        map.terrain_deltas_by_chunk.clear();
        // Do not change process-global map bounds in parallel unit tests.
        terrain.generator = TerrainGenerator::from_loaded_map(map);
        terrain
    }

    fn site() -> Site {
        Site {
            hall: Vec3::new(0.0, 2.0, 0.0),
            resources: shared::worldgen::ResourceProfile {
                farmland: 0.6,
                wood: 0.6,
                stone: 0.4,
                iron: 0.2,
            },
            potential_population: 48,
            rank: 0.6,
            salt: 17,
        }
    }

    #[test]
    fn frontier_stands_are_separated_grounded_and_cannot_cross_a_wet_apron() {
        let library = DerivedColliderLibrary {
            by_kind: Default::default(),
        };
        let colliders = StaticColliders::default();
        let dry = terrain(|_, _| 2.0);
        let plan = plan(&dry, &site(), &colliders, &library, 6).unwrap();
        assert!(plan.plots.is_empty() && plan.square.is_none());
        assert_eq!(plan.frontier_stands.len(), 6);
        let door = SettlementBuildingKind::Hall
            .entrance_position(site().hall, 0.0)
            .xz();
        for (index, point) in plan.frontier_stands.iter().enumerate() {
            assert_eq!(point.y, 2.0);
            assert!(point.z < door.y);
            assert!(plan.frontier_stands[..index]
                .iter()
                .all(|other| point.xz().distance(other.xz()) >= 1.99));
        }
        let flooded = terrain(|_, z| if z < -6.0 { -2.0 } else { 2.0 });
        assert!(super::plan(&flooded, &site(), &colliders, &library, 6).is_none());
    }

    #[test]
    fn a_tree_on_the_hall_apron_rejects_founders_instead_of_spawning_them_trapped() {
        use crate::collision::library::{DerivedCollider, StaticColliderInstance};
        use shared::props::PropKind;
        let terrain = terrain(|_, _| 2.0);
        let mut colliders = StaticColliders::default();
        let mut library = DerivedColliderLibrary {
            by_kind: Default::default(),
        };
        let (door, approach) =
            village_roads::doorway_approach(SettlementBuildingKind::Hall, site().hall, 0.0);
        let point = door.lerp(approach, 0.8);
        let cell = (
            (point.x / 16.0).floor() as i32,
            (point.y / 16.0).floor() as i32,
        );
        let kind = PropKind::OakA;
        library.by_kind.insert(
            kind,
            DerivedCollider {
                horizontal_radius: 1.0,
            },
        );
        colliders.instances.insert(
            1,
            StaticColliderInstance {
                kind,
                position: Vec3::new(point.x, 2.0, point.y),
                scale: 1.0,
                rotation: Quat::IDENTITY,
                cell,
            },
        );
        colliders.cells.insert(cell, vec![1]);
        assert!(plan(&terrain, &site(), &colliders, &library, 6).is_none());
    }

    #[test]
    fn frontier_spawn_creates_real_unhoused_founders_and_only_finite_hall_assets() {
        let mut world = World::new();
        let terrain = terrain(|_, _| 2.0);
        let library = DerivedColliderLibrary {
            by_kind: Default::default(),
        };
        let config = WorldStartConfig::from_ron(include_str!(
            "../../../../config/worlds/small-frontier.ron"
        ))
        .unwrap();
        let layout = plan(
            &terrain,
            &site(),
            &StaticColliders::default(),
            &library,
            config.founders_per_settlement,
        )
        .unwrap();
        let community = Community {
            site: site(),
            layout,
            name: "Newstead".into(),
            arrival: None,
            radius: 20.0,
            land_network: 1,
        };
        world.insert_resource(terrain);
        world.init_resource::<crate::world::identity::WorldIdAllocator>();
        world.init_resource::<crate::world::village::PublishedTerrainDeltas>();
        world.run_system_once(move |mut commands: Commands, mut terrain: ResMut<WorldTerrain>, mut ids: ResMut<crate::world::identity::WorldIdAllocator>, mut deltas: ResMut<crate::world::village::PublishedTerrainDeltas>| {
            super::super::spawn::community(&mut commands, &mut terrain, &mut ids, &mut deltas, &community, &config);
        }).unwrap();
        let (settlement_id, settlement, stock) = world
            .query::<(&SettlementId, &Settlement, &GoodsInventory)>()
            .single(&world)
            .unwrap();
        assert_eq!(settlement.tier, SettlementTier::Hamlet);
        assert_eq!(settlement.residents, 6);
        assert_eq!(settlement.treasury, 23 * PENNIES_PER_COIN);
        assert_eq!(stock.amount(Good::Bread), 20);
        assert_eq!(stock.amount(Good::Wheat), 6);
        assert_eq!(stock.amount(Good::Wood), 12);
        let id = *settlement_id;
        let people: Vec<_> = world
            .query::<(
                &PersonId,
                &ResidentOf,
                &Residence,
                &Wallet,
                Option<&LivesAt>,
                Option<&crate::world::village::HomeAssignment>,
            )>()
            .iter(&world)
            .collect();
        assert_eq!(people.len(), 6);
        let distinct: std::collections::HashSet<_> =
            people.iter().map(|person| person.0 .0).collect();
        assert_eq!(distinct.len(), 6);
        for (_, resident, residence, wallet, home, assignment) in people {
            assert_eq!(resident.0, id);
            assert_eq!(residence.0, "Newstead");
            assert_eq!(wallet.balance(), Wallet::founding_villager().balance());
            assert!(home.is_none() && assignment.is_none());
        }
        assert_eq!(world.query::<&SettlementBuilding>().iter(&world).count(), 0);
        assert_eq!(world.query::<&Company>().iter(&world).count(), 0);
        assert_eq!(world.query::<&VillageRoad>().iter(&world).count(), 0);
    }
}
