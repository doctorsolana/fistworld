//! Explicit controlled defense fixture, separate from natural city growth.

use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::*;
use shared::economy::{Good, GoodsInventory, MarketSeller, MootMarket};
use shared::region::RegionCoord;
use shared::terrain::{ChunkCoord, WorldTerrain};

use super::{
    construction::WallWork,
    geometry::{fit_circuit, Plot, RoadApproach},
};

#[derive(Resource, Clone, Debug)]
pub struct DefenseLabFixture {
    pub settlement_id: SettlementId,
    pub gate_position: Vec3,
    pub prebuilt_sections: usize,
    pub work_sections: [Entity; 2],
    observed_completions: usize,
    arrival_position: Vec3,
    hall: Entity,
    arrival_started: bool,
}

/// Opt-in runtime companion to the actual town growth lab. No flag means no
/// queries or mutation after the one-time environment check.
pub fn setup_defense_lab(
    world: &mut World,
    mut enabled: Local<Option<bool>>,
    mut next_attempt: Local<f64>,
) {
    let enabled = *enabled.get_or_insert_with(|| {
        std::env::var("FISTWORLD_LAB_DEFENSE_FIXTURE").as_deref() == Ok("1")
    });
    if !enabled {
        return;
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    if now < *next_attempt {
        return;
    }
    *next_attempt = now + 1.0;
    if let Some(fixture) = world.get_resource::<DefenseLabFixture>().cloned() {
        // Begin the fixture's ordinary immigration journey only once a real
        // client is connected, so passage cannot finish while it boots.
        if !fixture.arrival_started
            && world
                .query_filtered::<Entity, (
                    With<lightyear::prelude::server::ClientOf>,
                    With<lightyear::prelude::Connected>,
                )>()
                .iter(world)
                .next()
                .is_some()
        {
            spawn_arriving_carpenter(world, fixture.arrival_position, fixture.hall);
            world.resource_mut::<DefenseLabFixture>().arrival_started = true;
            info!("CONTROLLED DEFENSE FIXTURE: connected client ready; arriving carpenter begins real gateway journey");
        }
        let completed = fixture
            .work_sections
            .iter()
            .filter(|entity| {
                world
                    .get::<FortificationSegment>(**entity)
                    .is_some_and(|section| section.complete)
            })
            .count();
        if completed != fixture.observed_completions {
            info!("CONTROLLED DEFENSE FIXTURE progress: paid sections completed={completed}/2 at gateway {:?}, settlement {:?}", fixture.gate_position, fixture.settlement_id);
            world
                .resource_mut::<DefenseLabFixture>()
                .observed_completions = completed;
        }
        return;
    }
    if let Some(fixture) = try_setup(world) {
        world.insert_resource(fixture);
    }
}

fn try_setup(world: &mut World) -> Option<DefenseLabFixture> {
    let (hall, id, position, charter) = world
        .query::<(
            Entity,
            &SettlementId,
            &Settlement,
            &PlayerPosition,
            &SettlementDevelopment,
        )>()
        .iter(world)
        .find(|(_, _, town, _, _)| town.residents >= 8)
        .map(|(e, id, _, p, c)| (e, *id, p.0, c.clone()))?;
    if world.get::<SettlementDefenses>(hall).is_some() {
        return None;
    }
    let seller = world
        .query_filtered::<&PersonId, With<CharacterKind>>()
        .iter(world)
        .copied()
        .min_by_key(|id| id.0)?;
    let mut occupied: Vec<_> = world
        .query::<(
            &shared::building::PlacedBuilding,
            &shared::building::BuildingPosition,
        )>()
        .iter(world)
        .map(|(b, p)| {
            let d = b.building_type.definition();
            Plot {
                center: d.world_footprint_center(p.0, b.rotation),
                half_extents: d.footprint * 0.5,
                rotation: b.rotation,
            }
        })
        .collect();
    occupied.extend(
        world
            .query::<&SettlementCivicSquare>()
            .iter(world)
            .map(|square| Plot {
                center: square.center.xz(),
                half_extents: square.half_extents,
                rotation: square.rotation,
            }),
    );
    for site in world
        .query::<&crate::world::village::UnderConstruction>()
        .iter(world)
    {
        let d = site.kind.placement_definition();
        occupied.push(Plot {
            center: d.world_footprint_center(site.position, site.rotation),
            half_extents: d.footprint * 0.5,
            rotation: site.rotation,
        });
        for p in site
            .kind
            .field_positions(site.position, site.rotation)
            .into_iter()
            .flatten()
        {
            occupied.push(Plot {
                center: p.xz(),
                half_extents: site.kind.field_half_extents().unwrap(),
                rotation: site.rotation,
            });
        }
        if let Some(p) = site.kind.pasture_position(site.position, site.rotation) {
            occupied.push(Plot {
                center: p.xz(),
                half_extents: site.kind.pasture_half_extents().unwrap(),
                rotation: site.rotation,
            });
        }
    }
    occupied.extend(
        world
            .query_filtered::<(&PlayerPosition, &PlayerRotation), With<FarmField>>()
            .iter(world)
            .map(|(p, r)| Plot {
                center: p.0.xz(),
                half_extents: SettlementBuildingKind::Farmstead
                    .field_half_extents()
                    .unwrap(),
                rotation: r.0,
            }),
    );
    occupied.extend(
        world
            .query_filtered::<(&PlayerPosition, &PlayerRotation), With<LivestockPasture>>()
            .iter(world)
            .map(|(p, r)| Plot {
                center: p.0.xz(),
                half_extents: SettlementBuildingKind::LivestockFarm
                    .pasture_half_extents()
                    .unwrap(),
                rotation: r.0,
            }),
    );
    let mut roads: Vec<_> = world
        .query::<&VillageRoad>()
        .iter(world)
        .map(|r| RoadApproach {
            points: r.points.clone(),
            width: r.reservation_width(),
        })
        .collect();
    roads.extend(
        world
            .query::<&crate::world::village_roads::PlannedRoadAccess>()
            .iter(world)
            .map(|r| RoadApproach {
                points: r.points.clone(),
                width: r.half_width * 2.,
            }),
    );
    let terrain = world.get_resource::<WorldTerrain>()?;
    assert_eq!(
        terrain.generator.active_map_id(),
        "village_lab",
        "controlled defense fixture requires village_lab land"
    );
    let library = world.get_resource::<crate::collision::library::DerivedColliderLibrary>()?;
    let mut cache = std::collections::HashMap::new();
    let mut plan = fit_circuit(
        id,
        0,
        position.xz(),
        60.,
        charter.inner_wall,
        charter.plan_seed,
        &[position.xz()],
        &occupied,
        &roads,
        |p| {
            let h = terrain.get_height(p.x, p.y);
            if terrain
                .water_surface_height(p.x, p.y)
                .is_some_and(|water| h < water + 0.7)
            {
                return None;
            }
            let chunk = ChunkCoord::from_world_pos(Vec3::new(p.x, h, p.y));
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let chunk = ChunkCoord::new(chunk.x + dx, chunk.z + dz);
                    let props = cache.entry(chunk).or_insert_with(|| {
                        shared::props::generate_chunk_blocking_props(&terrain.generator, chunk)
                    });
                    if props.iter().any(|prop| {
                        library.by_kind.get(&prop.kind).is_some_and(|shape| {
                            prop.position.distance(p) < shape.horizontal_radius * prop.scale + 0.6
                        })
                    }) {
                        return None;
                    }
                }
            }
            Some(h)
        },
    )?;
    let gate_index = plan
        .sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == FortificationKind::Gate)
        .min_by(|(_, a), (_, b)| a.midpoint().z.total_cmp(&b.midpoint().z))?
        .0;
    let work_gate = plan
        .sections
        .iter()
        .enumerate()
        .filter(|(index, s)| *index != gate_index && s.kind == FortificationKind::Gate)
        .min_by(|(_, a), (_, b)| {
            a.midpoint()
                .distance_squared(plan.sections[gate_index].midpoint())
                .total_cmp(
                    &b.midpoint()
                        .distance_squared(plan.sections[gate_index].midpoint()),
                )
        })?
        .0;
    let work_wall = plan
        .sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == FortificationKind::Wall)
        .min_by(|(_, a), (_, b)| {
            a.midpoint()
                .distance_squared(plan.sections[work_gate].midpoint())
                .total_cmp(
                    &b.midpoint()
                        .distance_squared(plan.sections[work_gate].midpoint()),
                )
        })?
        .0;
    let gate_position = plan.sections[gate_index].midpoint();
    let outward = (gate_position.xz() - position.xz()).normalize_or_zero();
    let requested = gate_position + Vec3::new(outward.x, 0., outward.y) * 12.;
    let arrival = crate::world::dev::safe_villager_spawn_position(
        requested,
        0xdefe_1234,
        terrain,
        world.get_resource::<shared::spatial::SpatialObstacleGrid>(),
        world.get_resource::<crate::collision::library::StaticColliders>(),
        Some(library),
    )?;
    let mut jobs = Vec::new();
    for (index, section) in plan.sections.iter_mut().enumerate() {
        section.complete = index != work_gate && index != work_wall;
        let midpoint = section.midpoint();
        let entity = world
            .spawn((
                section.clone(),
                PlayerPosition(midpoint),
                RegionCoord::from_world_pos(midpoint),
                GoodsInventory::new(512),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        if !section.complete {
            world.entity_mut(entity).insert(WallWork::default());
            jobs.push(entity);
        }
    }
    let mut town = world.get_mut::<Settlement>(hall)?;
    town.tier = SettlementTier::Village;
    town.treasury = 50_000;
    for (good, amount) in [(Good::Wood, 100), (Good::Stone, 100), (Good::Bread, 400)] {
        let added = world.get_mut::<GoodsInventory>(hall)?.add(good, amount);
        world.get_mut::<MootMarket>(hall)?.consign(
            MarketSeller::Person(seller),
            good,
            added,
            good.base_price(),
        );
    }
    world.entity_mut(hall).insert(SettlementDefenses {
        circuits: vec![plan.clone()],
    });
    let fixture = DefenseLabFixture {
        settlement_id: id,
        gate_position,
        prebuilt_sections: plan.sections.len() - 2,
        work_sections: [jobs[0], jobs[1]],
        observed_completions: 0,
        arrival_position: arrival,
        hall,
        arrival_started: false,
    };
    info!("CONTROLLED DEFENSE FIXTURE (not natural growth): {:?}; prebuilt={} real paid work sections=2; gate camera focus={:.2},{:.2}; arriving carpenter starts outside and must use a real gateway",id,fixture.prebuilt_sections,gate_position.x,gate_position.z);
    if let Ok(path) = std::env::var("FISTWORLD_LAB_DEFENSE_METADATA") {
        let json=format!("{{\"fixture\":\"controlled-funded-defense\",\"natural_growth\":false,\"settlement_id\":{},\"prebuilt_sections\":{},\"paid_work_sections\":2,\"gate_position\":[{},{},{}]}}",id.0,fixture.prebuilt_sections,gate_position.x,gate_position.y,gate_position.z);
        std::fs::write(path, json).expect("write controlled defense fixture metadata");
    }
    Some(fixture)
}

fn spawn_arriving_carpenter(world: &mut World, arrival: Vec3, hall: Entity) -> Entity {
    // This is a real immigrant using the ordinary constructor and navigator;
    // only their starting point and time belong to the controlled fixture.
    let terrain = world.resource::<WorldTerrain>();
    let position = world.get::<PlayerPosition>(hall).unwrap().0;
    let rotation = world
        .get::<PlayerRotation>(hall)
        .map_or(0.0, |rotation| rotation.0);
    let destination = SettlementBuildingKind::Hall.entrance_position(position, rotation);
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let arriving = {
        let mut commands = Commands::new(&mut queue, world);
        crate::player::hero::spawn_villager(&mut commands, terrain, 0xdefe_1234, arrival)
    };
    queue.apply(world);
    world.entity_mut(arriving).insert((
        CharacterName("Defense lab arriving carpenter".into()),
        crate::world::village::VillagerIntent::Travelling { settlement: hall },
        crate::player::hero::MoveTarget(destination),
    ));
    arriving
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defense_fixture_waits_for_an_identified_populated_hall() {
        let mut world = World::new();
        assert!(try_setup(&mut world).is_none());
        world.spawn((
            SettlementId(1),
            Settlement {
                name: "Waiting".into(),
                tier: SettlementTier::Hamlet,
                residents: 7,
                treasury: 0,
            },
            PlayerPosition(Vec3::ZERO),
            SettlementDevelopment::from_seed(23, 0),
        ));
        assert!(try_setup(&mut world).is_none());
        assert!(world
            .query::<&FortificationSegment>()
            .iter(&world)
            .next()
            .is_none());
    }

    #[test]
    #[ignore = "controlled defense fixture requires village_lab terrain"]
    fn defense_fixture_fits_dry_land_once_and_preserves_two_real_worksites() {
        assert_eq!(
            std::env::var("CITYSIM_MAP_ID").as_deref(),
            Ok("village_lab")
        );
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Startup, crate::collision::library::setup_baked_colliders);
        app.update();
        let (position, _, _) = crate::world::village_lab_scenario::choose_town_growth_site(
            app.world().resource::<WorldTerrain>(),
        );
        app.world_mut().spawn((
            SettlementId(1),
            Settlement {
                name: "Controlled defense test".into(),
                tier: SettlementTier::Hamlet,
                residents: 8,
                treasury: 0,
            },
            PlayerPosition(position),
            SettlementDevelopment::from_seed(23, 0),
            GoodsInventory::new_partitioned(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ));
        app.world_mut().spawn((
            PersonId(1),
            CharacterKind::Villager,
            shared::economy::Wallet::default(),
            PlayerPosition(position),
        ));
        let fixture = try_setup(app.world_mut()).expect("validated inland lab fits an enclosure");
        assert_eq!(fixture.settlement_id, SettlementId(1));
        let sections: Vec<_> = app
            .world_mut()
            .query::<&FortificationSegment>()
            .iter(app.world())
            .cloned()
            .collect();
        assert_eq!(sections.iter().filter(|s| !s.complete).count(), 2);
        assert_eq!(
            sections.iter().filter(|s| s.complete).count(),
            fixture.prebuilt_sections
        );
        for job in fixture.work_sections {
            assert!(app.world().get::<WallWork>(job).is_some());
        }
        let terrain = app.world().resource::<WorldTerrain>();
        for section in &sections {
            for point in [section.start, section.end] {
                assert!((point.y - terrain.get_height(point.x, point.z)).abs() < 0.001);
                assert!(terrain
                    .water_surface_height(point.x, point.z)
                    .is_none_or(|water| point.y > water + 0.7));
            }
        }
        assert!(try_setup(app.world_mut()).is_none());
        assert_eq!(
            app.world_mut()
                .query::<&FortificationSegment>()
                .iter(app.world())
                .count(),
            sections.len()
        );
        assert!(!fixture.arrival_started);
        spawn_arriving_carpenter(app.world_mut(), fixture.arrival_position, fixture.hall);
        assert_eq!(
            app.world_mut()
                .query::<&CharacterName>()
                .iter(app.world())
                .filter(|name| name.0 == "Defense lab arriving carpenter")
                .count(),
            1
        );
    }
}
