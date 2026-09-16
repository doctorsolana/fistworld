//! Explicitly staged active work, separate from the record/index probes.

use super::*;
use crate::player::hero::{TacticalCrowdGrid, rebuild_tactical_crowd_grid};
use crate::world::village_roads::{NavigationRouteFailed, TravelRoute};
use shared::terrain::ChunkCoord;

fn timings(mut durations: Vec<Duration>) -> Timing {
    durations.sort_unstable();
    Timing {
        average: durations.iter().copied().sum::<Duration>() / durations.len() as u32,
        p50: percentile(&durations, 50),
        p95: percentile(&durations, 95),
        p99: percentile(&durations, 99),
        max: *durations.last().expect("at least one sample"),
    }
}

pub(super) fn bench_farm_batches(towns: usize, npcs: usize, samples: usize) {
    // A fresh fixture prevents the daily policy/needs probes from changing the
    // population that this particular production hot-path measurement exercises.
    let mut app = configure_app(towns, npcs);
    let world = app.world_mut();
    let workers: Vec<_> = world
        .query::<(Entity, &FarmerRoutine)>()
        .iter(world)
        .map(|(worker, routine)| {
            let stand = world.get::<PlayerPosition>(routine.field).unwrap().0;
            let quality = world
                .get::<SettlementBuilding>(routine.farmstead)
                .unwrap()
                .quality;
            (
                worker,
                stand,
                super::super::farmer_seconds_per_wheat(quality),
            )
        })
        .collect();
    let initial_wallets: u64 = world
        .query::<&Wallet>()
        .iter(world)
        .map(|w| w.balance())
        .sum();
    let initial_entities = world.entities().len();
    let mut durations = Vec::with_capacity(samples);
    let expected_batch = u64::from(super::super::FARM_CARRY_BATCH_UNITS) * workers.len() as u64;
    let mut total_produced = 0_u64;
    for serial in 0..(WARMUP_RUNS + samples) {
        // Author one almost-finished field visit per worker outside the timer.
        // The real runner must add the last tick of labour, fill the finite
        // basket, post its exact output ledger, and issue the return journey.
        // Emptying these test baskets is not a market or a conservation claim.
        for &(worker, stand, seconds_per_wheat) in &workers {
            let mut entity = world.entity_mut(worker);
            entity.remove::<MoveTarget>();
            entity
                .get_mut::<GoodsInventory>()
                .unwrap()
                .remove(Good::Wheat, u32::MAX);
            entity.get_mut::<PlayerPosition>().unwrap().0 = stand;
            let mut routine = entity.get_mut::<FarmerRoutine>().unwrap();
            routine.work_stand = stand;
            routine.phase = FarmerPhase::Farming;
            routine.harvest_seconds =
                seconds_per_wheat * super::super::FARM_CARRY_BATCH_UNITS as f32 - 0.5 / 60.0;
        }
        advance_clock(world, 1.0 / 60.0);
        let started = Instant::now();
        world.run_schedule(PhysicalWorkBench);
        if serial >= WARMUP_RUNS {
            durations.push(started.elapsed());
        }
        let produced: u64 = workers
            .iter()
            .map(|(worker, ..)| {
                assert!(matches!(
                    world.get::<FarmerRoutine>(*worker).unwrap().phase,
                    FarmerPhase::ReturningToFarmstead
                ));
                assert!(
                    world.get::<MoveTarget>(*worker).is_some(),
                    "full basket needs a real return order"
                );
                u64::from(
                    world
                        .get::<GoodsInventory>(*worker)
                        .unwrap()
                        .amount(Good::Wheat),
                )
            })
            .sum();
        assert_eq!(
            produced, expected_batch,
            "staged work did not produce real carried Wheat"
        );
        total_produced += produced;
    }
    let recorded: u64 = world
        .query::<&BusinessAccount>()
        .iter(world)
        .map(|account| u64::from(account.current_day.produced_units))
        .sum();
    assert_eq!(
        recorded, total_produced,
        "physical output and business ledgers differ"
    );
    let final_wallets: u64 = world
        .query::<&Wallet>()
        .iter(world)
        .map(|w| w.balance())
        .sum();
    assert_eq!(
        final_wallets, initial_wallets,
        "harvest invented or spent personal money"
    );
    assert_eq!(world.entities().len(), initial_entities);
    print_timing("staged farm full baskets", &timings(durations));
    println!(
        "SCALE production workers={} units_per_sample={expected_batch} ledger_units_with_warmup={recorded} staging_outside_timer=true field_travel_and_sales=false",
        workers.len()
    );
}

fn movement_fixture(npcs: usize, moving: usize, samples: usize) -> (App, Vec<(Entity, Vec3)>) {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<SpatialObstacleGrid>()
        .init_resource::<VillageRoadGraph>()
        .init_resource::<TacticalCrowdGrid>()
        .insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 16,
            max_milliseconds_per_tick: 4.0,
        });
    let mut terrain = WorldTerrain::default();
    let bounds = terrain.generator.active_map_bounds();
    let center = (bounds.min_vec2() + bounds.max_vec2()) * 0.5;
    let columns = (npcs as f32).sqrt().ceil() as usize;
    let rows = npcs.div_ceil(columns);
    let route_length =
        40.0 + (samples + WARMUP_RUNS) as f32 / 60.0 * shared::player::HERO_MOVE_SPEED;
    let half = Vec2::new(
        columns as f32 * 1.5 + route_length * 0.5 + 12.0,
        rows as f32 * 1.5 + 12.0,
    );
    assert!(
        bounds.contains_xz(center.x - half.x, center.y - half.y)
            && bounds.contains_xz(center.x + half.x, center.y + half.y),
        "requested movement cohort does not fit the active map"
    );
    let starts: Vec<_> = (0..npcs)
        .map(|index| {
            center
                + Vec2::new(
                    (index % columns) as f32 * 3.0 - columns as f32 * 1.5 - route_length * 0.5,
                    (index / columns) as f32 * 3.0 - rows as f32 * 1.5,
                )
        })
        .collect();
    let height = starts
        .iter()
        .flat_map(|start| [*start, *start + Vec2::X * route_length])
        .map(|point| {
            terrain.get_height(point.x, point.y).max(
                terrain
                    .water_surface_height(point.x, point.y)
                    .unwrap_or(f32::NEG_INFINITY),
            )
        })
        .fold(100.0_f32, f32::max)
        + 3.0;
    let chunks = terrain.apply_flatten_rect(Vec3::new(center.x, height, center.y), half, 0.0, 0.0);
    let mut colliders = StaticColliders::default();
    // This is deliberately an authored clear forecourt, not a generated forest
    // or a loaded-prop performance test. Mark its empty chunks authoritative.
    colliders.loaded_chunks.extend(chunks);
    let mut movers = Vec::with_capacity(moving);
    for (index, start) in starts.into_iter().enumerate() {
        let position = Vec3::new(start.x, terrain.get_height(start.x, start.y), start.y);
        let mut person = app.world_mut().spawn((
            CharacterKind::Villager,
            PersonId(index as u64 + 1),
            PlayerPosition(position),
            PlayerRotation(0.0),
            RegionCoord::from_world_pos(position),
        ));
        if index < moving {
            let goal_x = start.x + route_length;
            assert!(terrain.get_water_height(start.x, start.y).is_none());
            assert!(terrain.get_water_height(goal_x, start.y).is_none());
            assert!(
                colliders
                    .loaded_chunks
                    .contains(&ChunkCoord::from_world_pos(position))
            );
            person.insert(MoveTarget(Vec3::new(
                goal_x,
                terrain.get_height(goal_x, start.y),
                start.y,
            )));
            movers.push((person.id(), position));
        }
    }
    app.insert_resource(terrain).insert_resource(colliders);
    app.world_mut()
        .spawn((WorldTime::new_default(), TimeWarp::clamped(1.0)));
    app.add_systems(
        RoutingBench,
        (queue_villager_travel_routes, plan_villager_travel_routes).chain(),
    );
    app.add_systems(
        MovementBench,
        (rebuild_tactical_crowd_grid, step_units).chain(),
    );
    (app, movers)
}

pub(super) fn bench_movement(npcs: usize, moving: usize, samples: usize) {
    let (mut app, movers) = movement_fixture(npcs, moving, samples);
    let world = app.world_mut();
    let mut durations = Vec::new();
    // Measure the actual cold backlog, including warm-up and deferred queue
    // installation. Movement is held until every order has a certified route.
    for _ in 0..(moving * 8 + 128) {
        advance_clock(world, 1.0 / 60.0);
        let started = Instant::now();
        world.run_schedule(RoutingBench);
        durations.push(started.elapsed());
        assert!(
            world
                .query::<&NavigationRouteFailed>()
                .iter(world)
                .next()
                .is_none(),
            "clear forecourt route failed"
        );
        if movers
            .iter()
            .all(|(entity, _)| world.get::<TravelRoute>(*entity).is_some())
        {
            break;
        }
    }
    let pending = world.query::<&NavigationRoutePending>().iter(world).count();
    assert_eq!(pending, 0, "bounded route queue did not drain");
    assert!(
        movers
            .iter()
            .all(|(entity, _)| world.get::<TravelRoute>(*entity).is_some()),
        "every requested mover must have a real certified route"
    );
    let ticks = durations.len();
    let total: Duration = durations.iter().sum();
    print_timing("cold route queue slices", &timings(durations));
    println!(
        "SCALE routing movers={moving} drain_ticks={ticks} simulated_wait_s={:.3} total_cpu_ms={:.3} request_cap=16 authored_clear_ground=true includes_prop_loading=false",
        ticks as f64 / 60.0,
        milliseconds(total)
    );
    let movement = bench_schedule(world, MovementBench, samples);
    print_timing("dispersed physical motion", &movement);
    let minimum_progress = movers
        .iter()
        .map(|(entity, start)| {
            assert!(
                world.get::<MoveTarget>(*entity).is_some(),
                "movement samples exhausted their journey"
            );
            assert!(world.get::<NavigationRouteFailed>(*entity).is_none());
            world.get::<PlayerPosition>(*entity).unwrap().0.x - start.x
        })
        .fold(f32::INFINITY, f32::min);
    assert!(
        minimum_progress > 0.1,
        "some route-certified actors never moved"
    );
    assert_eq!(world.query::<&PersonId>().iter(world).count(), npcs);
    println!(
        "SCALE movement residents={npcs} moving={moving} minimum_progress_m={minimum_progress:.3} spacing_m=3 includes_crowd_index=true dt_s={:.6}",
        1.0 / 60.0
    );
}

#[test]
fn active_scale_fixtures_produce_recorded_baskets_and_move_distinct_people() {
    bench_farm_batches(2, 20, 2);
    bench_movement(20, 12, 2);
}
