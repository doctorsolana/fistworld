//! One physical civic crew per settlement. Procurement uses the existing Hall
//! market and municipal payroll; goods travel in the worker's real inventory.

use bevy::prelude::*;
use shared::components::*;
use shared::economy::{CivicAccount, Good, GoodsInventory, MarketSeller, MootMarket};

use crate::player::hero::MoveTarget;
use crate::world::settlement_development::CivicHallBuilderRoutine;
use crate::world::village::BusinessEventQueue;

#[derive(Component, Default, Clone)]
pub(super) struct WallWork {
    builder: Option<Entity>,
    carried: u32,
    seconds_worked: f32,
    retry_after: f64,
    material: Option<FortificationMaterial>,
    returning: bool,
}

fn world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

fn release_worker(world: &mut World, worker: Entity) {
    if let Ok(mut entity) = world.get_entity_mut(worker) {
        entity.remove::<CivicHallBuilderRoutine>();
        entity.remove::<MoveTarget>();
        entity.remove::<crate::world::village_roads::TravelRoute>();
        entity.remove::<crate::world::village_roads::NavigationRoutePending>();
        entity.remove::<crate::world::village_roads::NavigationRouteFailed>();
        entity.insert(CharacterActivity::Idle);
    }
}

fn set_destination(world: &mut World, worker: Entity, destination: Vec3) {
    if world
        .get::<MoveTarget>(worker)
        .is_some_and(|target| target.0.distance_squared(destination) < 0.01)
    {
        return;
    }
    world
        .entity_mut(worker)
        .insert((MoveTarget(destination), CharacterActivity::Idle));
}

fn distance(world: &World, worker: Entity, target: Vec3) -> f32 {
    world
        .get::<PlayerPosition>(worker)
        .map_or(f32::INFINITY, |p| p.0.xz().distance(target.xz()))
}

/// Transfer real consigned goods at the Hall into the named worker's carry
/// inventory. Receipts settle through the same business-event pass as houses.
fn purchase_batch(
    world: &mut World,
    hall: Entity,
    id: SettlementId,
    worker: Entity,
    good: Good,
    wanted: u32,
    day: u32,
) -> u32 {
    let free = world
        .get::<GoodsInventory>(worker)
        .map_or(0, |inventory| inventory.free_bulk() / good.bulk_per_unit());
    let request = wanted.min(free);
    if request == 0 {
        return 0;
    }
    let fills;
    let bought;
    {
        let mut query = world.query::<(
            &mut Settlement,
            &mut MootMarket,
            &mut GoodsInventory,
            Option<&MootAdministration>,
            Option<&SettlementPolicies>,
            Option<&mut CivicAccount>,
        )>();
        let Ok((mut settlement, mut market, mut stock, administration, policies, account)) =
            query.get_mut(world, hall)
        else {
            return 0;
        };
        let budget = crate::world::village::civic::civic_discretionary_budget(
            &settlement,
            administration,
            policies,
        );
        let purchase = market.purchase_recording_demand(
            good,
            request,
            budget,
            None,
            Some(MarketSeller::Treasury(id)),
        );
        bought = purchase.trade.units;
        if bought == 0 {
            return 0;
        }
        let removed = stock.remove(good, bought);
        assert_eq!(
            removed, bought,
            "consigned wall materials must be backed by real Hall stock"
        );
        settlement.treasury -= purchase.trade.pennies;
        if let Some(mut account) = account {
            account.record_material_expense(day.saturating_add(1), purchase.trade.pennies);
        }
        fills = purchase.fills;
    }
    world
        .resource_mut::<BusinessEventQueue>()
        .record_market_purchase(day, id, fills);
    let added = world
        .get_mut::<GoodsInventory>(worker)
        .unwrap()
        .add(good, bought);
    assert_eq!(added, bought);
    bought
}

fn free_worker(world: &mut World, id: SettlementId) -> Option<Entity> {
    let mut query = world.query::<(Entity, &CivicEmployment)>();
    query
        .iter(world)
        .filter(|(entity, job)| {
            job.settlement == id
                && matches!(job.role, CivicRole::CityWorker | CivicRole::MootSteward)
                && world
                    .get::<crate::world::village::strategic::StrategicPerson>(*entity)
                    .is_none()
                && world.get::<CivicHallBuilderRoutine>(*entity).is_none()
                && world.get::<MoveTarget>(*entity).is_none()
                && world
                    .get::<crate::world::village::HomeRoutine>(*entity)
                    .is_none()
                && world
                    .get::<crate::world::village_roads::RoadBuilderRoutine>(*entity)
                    .is_none()
                && world
                    .get::<crate::world::village::MarketCollectionRoutine>(*entity)
                    .is_none()
                && world
                    .get::<crate::world::village::HouseholdShoppingRoutine>(*entity)
                    .is_none()
                && world
                    .get::<crate::world::village::MootQueueTicket>(*entity)
                    .is_none()
        })
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.to_bits())
}

fn occupied_by_actor(world: &mut World, section: &FortificationSegment) -> bool {
    let mut completed = section.clone();
    completed.complete = true;
    let obstacles: Vec<_> = completed.ground_obstacles().collect();
    world
        .query_filtered::<(&PlayerPosition,Has<Horse>,Has<Catapult>), Or<(With<CharacterKind>,With<Horse>,With<Catapult>)>>()
        .iter(world)
        .any(|(position,horse,catapult)| {
            let clearance=if catapult {CATAPULT_CLEARANCE} else if horse {HORSE_CLEARANCE} else {shared::physics::CHARACTER_NAV_RADIUS};
            obstacles.iter().any(|obstacle| {
                let mut body=obstacle.clone();
                body.half_extents+=Vec2::splat((clearance-shared::physics::CHARACTER_NAV_RADIUS).max(0.));
                body.contains_point(position.0.xz())
            })
        })
}

/// The exclusive system performs a bounded half-world-second project pass;
/// per-frame work is just the shared clock check. Existing off-screen civic
/// construction semantics are retained: a real embodied crew must do the work.
pub fn run_fortification_projects(
    world: &mut World,
    mut previous: Local<Option<f64>>,
    mut clock_query: Local<Option<bevy::ecs::query::QueryState<&'static WorldTime>>>,
    mut section_query: Local<Option<bevy::ecs::query::QueryState<&'static FortificationSegment>>>,
) {
    let clock_query = clock_query.get_or_insert_with(|| world.query::<&WorldTime>());
    let Some(clock) = clock_query.iter(world).next().cloned() else {
        return;
    };
    let now = world_seconds(&clock);
    let Some(last) = *previous else {
        *previous = Some(now);
        return;
    };
    if now < last {
        *previous = Some(now);
        return;
    }
    if now - last < 0.5 {
        return;
    }
    let elapsed = (now - last).max(0.0) as f32;
    *previous = Some(now);
    if !world.contains_resource::<BusinessEventQueue>() {
        return;
    }
    let section_query = section_query.get_or_insert_with(|| world.query::<&FortificationSegment>());
    if section_query.iter(world).next().is_none() {
        return;
    }
    let halls: Vec<_> = world
        .query::<(
            Entity,
            &SettlementId,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        )>()
        .iter(world)
        .map(|(entity, id, town, position, rotation)| {
            (
                entity,
                *id,
                town.tier,
                position.0,
                rotation.map_or(0., |r| r.0),
            )
        })
        .collect();
    // Collect once, partition by stable settlement id, never scan all people
    // for every individual wall section.
    let mut projects: Vec<_> = world
        .query::<(Entity, &FortificationSegment, &WallWork)>()
        .iter(world)
        .map(|(entity, section, work)| (entity, section.clone(), work.clone()))
        .collect();
    projects.sort_by_key(|(entity, section, work)| {
        (
            section.settlement_id.0,
            work.builder.is_none(),
            section.circuit,
            section.kind != FortificationKind::Gate,
            entity.to_bits(),
        )
    });
    for (hall, id, tier, hall_position, hall_rotation) in halls {
        if tier < SettlementTier::Village {
            continue;
        }
        let project = projects.iter().find(|(_, section, work)| {
            section.settlement_id == id
                && work.retry_after <= now
                && (!section.complete
                    || (tier >= SettlementTier::Town
                        && section.circuit == 0
                        && section.material == FortificationMaterial::Palisade))
        });
        let Some((entity, section, work)) = project else {
            continue;
        };
        let entity = *entity;
        let mut work = work.clone();
        let desired =
            *work
                .material
                .get_or_insert(if tier >= SettlementTier::Town && section.circuit == 0 {
                    FortificationMaterial::Stone
                } else {
                    FortificationMaterial::Palisade
                });
        let good = desired.good();
        let required = section.material_required(desired);
        let staged = world
            .get::<GoodsInventory>(entity)
            .map_or(0, |inventory| inventory.amount(good));
        let hall_door =
            SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
        let middle = section.midpoint();
        let along = (section.end.xz() - section.start.xz()).normalize_or_zero();
        let mut inward = Vec2::new(-along.y, along.x);
        if inward.dot(hall_position.xz() - middle.xz()) < 0. {
            inward = -inward;
        }
        let stand_xz = middle.xz() + inward * 3.2;
        let stand = world
            .get_resource::<shared::terrain::WorldTerrain>()
            .map_or(Vec3::new(stand_xz.x, middle.y, stand_xz.y), |terrain| {
                Vec3::new(
                    stand_xz.x,
                    terrain.get_height(stand_xz.x, stand_xz.y),
                    stand_xz.y,
                )
            });
        work.builder = work.builder.filter(|worker| {
            world
                .get::<CivicHallBuilderRoutine>(*worker)
                .is_some_and(|routine| routine.project == entity)
                && world
                    .get::<CivicEmployment>(*worker)
                    .is_some_and(|job| job.settlement == id)
        });
        if work.builder.is_none() {
            work.carried = 0;
        }
        if !clock.is_day() && work.carried == 0 {
            if let Some(worker) = work.builder.take() {
                release_worker(world, worker);
            }
            world.entity_mut(entity).insert(work);
            continue;
        }
        if work.builder.is_none() {
            // City development and road repair keep their existing priority.
            if world
                .query_filtered::<&BuildingOf, With<CivicHallUpgradeWorksite>>()
                .iter(world)
                .any(|owner| owner.0 == id)
            {
                continue;
            }
            let Some(worker) = free_worker(world, id) else {
                continue;
            };
            if world.get::<GoodsInventory>(worker).is_none() {
                world
                    .entity_mut(worker)
                    .insert(GoodsInventory::new(shared::economy::capacity::VILLAGER));
            }
            world
                .entity_mut(worker)
                .insert(CivicHallBuilderRoutine { project: entity });
            work.builder = Some(worker);
            set_destination(
                world,
                worker,
                if staged >= required { stand } else { hall_door },
            );
            world.entity_mut(entity).insert(work);
            continue;
        }
        let worker = work.builder.unwrap();
        if world
            .get::<crate::world::village_roads::NavigationRouteFailed>(worker)
            .is_some()
        {
            if work.carried > 0 {
                work.returning = true;
            } else {
                release_worker(world, worker);
                work.builder = None;
                work.retry_after = now + 120.;
                world.entity_mut(entity).insert(work);
                continue;
            }
        }
        if work.returning {
            if distance(world, worker, hall_door) > 1.8 {
                set_destination(world, worker, hall_door);
            } else {
                // A failed work approach returns civic cargo to physical Hall
                // stock, consigned for its real owner. No goods or coins vanish.
                let free = world
                    .get::<GoodsInventory>(hall)
                    .map_or(0, |inventory| inventory.free_bulk() / good.bulk_per_unit());
                let returned = world
                    .get_mut::<GoodsInventory>(worker)
                    .map_or(0, |mut inventory| {
                        inventory.remove(good, work.carried.min(free))
                    });
                world
                    .get_mut::<GoodsInventory>(hall)
                    .unwrap()
                    .add(good, returned);
                if let Some(mut market) = world.get_mut::<MootMarket>(hall) {
                    let price = market.suggested_price(good);
                    market.consign(MarketSeller::Treasury(id), good, returned, price);
                }
                work.carried = work.carried.saturating_sub(returned);
                if work.carried == 0 {
                    release_worker(world, worker);
                    work.builder = None;
                    work.returning = false;
                    work.retry_after = now + 120.;
                }
            }
            world.entity_mut(entity).insert(work);
            continue;
        }
        if work.carried > 0 {
            if distance(world, worker, stand) > 1.6 {
                set_destination(world, worker, stand);
            } else {
                let delivered = world
                    .get_mut::<GoodsInventory>(worker)
                    .map_or(0, |mut inventory| inventory.remove(good, work.carried));
                world
                    .get_mut::<GoodsInventory>(entity)
                    .unwrap()
                    .add(good, delivered);
                work.carried = 0;
                world.entity_mut(worker).remove::<MoveTarget>();
            }
            world.entity_mut(entity).insert(work);
            continue;
        }
        if staged < required {
            if distance(world, worker, hall_door) > 1.8 {
                set_destination(world, worker, hall_door);
            } else {
                work.carried =
                    purchase_batch(world, hall, id, worker, good, required - staged, clock.day);
                if work.carried == 0 {
                    release_worker(world, worker);
                    work.builder = None;
                    work.retry_after = now + 60.;
                } else {
                    set_destination(world, worker, stand);
                }
            }
            world.entity_mut(entity).insert(work);
            continue;
        }
        if world
            .get::<crate::world::village_roads::NavigationRouteFailed>(worker)
            .is_some()
            && distance(world, worker, stand) > 1.6
        {
            release_worker(world, worker);
            work.builder = None;
            work.retry_after = now + 120.;
            world.entity_mut(entity).insert(work);
            continue;
        }
        if distance(world, worker, stand) > 1.6 {
            set_destination(world, worker, stand);
            world.entity_mut(entity).insert(work);
            continue;
        }
        world
            .entity_mut(worker)
            .remove::<MoveTarget>()
            .remove::<crate::world::village_roads::TravelRoute>();
        world
            .get_mut::<CharacterActivity>(worker)
            .map(|mut activity| activity.set_if_neq(CharacterActivity::Building));
        let toward = middle - stand;
        if let Some(mut rotation) = world.get_mut::<PlayerRotation>(worker) {
            rotation.set_if_neq(PlayerRotation(f32::atan2(-toward.x, -toward.z)));
        }
        work.seconds_worked += elapsed;
        // Work pauses while a pedestrian is crossing: publishing a collider
        // around an actor must never force an arbitrary pushout or teleport.
        let mut finished_shape = section.clone();
        finished_shape.material = desired;
        if work.seconds_worked >= section.length() * 5.0 + 12.0
            && !occupied_by_actor(world, &finished_shape)
        {
            let removed = world
                .get_mut::<GoodsInventory>(entity)
                .unwrap()
                .remove(good, required);
            assert_eq!(removed, required);
            let mut completed = section.clone();
            completed.material = desired;
            completed.complete = true;
            world.entity_mut(entity).insert(completed);
            release_worker(world, worker);
            work = WallWork::default();
            info!(
                "Settlement {:?}: completed {:?} defense section in circuit {}",
                id, desired, section.circuit
            );
        }
        world.entity_mut(entity).insert(work);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civic_procurement_moves_stock_and_money_into_real_worker_cargo() {
        let mut world = World::new();
        world.init_resource::<BusinessEventQueue>();
        let id = SettlementId(1);
        let good = Good::Wood;
        let mut market = MootMarket::default();
        market.consign(MarketSeller::Treasury(SettlementId(2)), good, 10, 3);
        let mut stock = GoodsInventory::new(1000);
        stock.add(good, 10);
        let hall = world
            .spawn((
                Settlement {
                    name: "Test".into(),
                    tier: SettlementTier::Town,
                    residents: 50,
                    treasury: 1000,
                },
                market,
                stock,
            ))
            .id();
        let worker = world.spawn(GoodsInventory::new(1000)).id();
        let bought = purchase_batch(&mut world, hall, id, worker, good, 5, 0);
        assert_eq!(bought, 5);
        assert_eq!(world.get::<GoodsInventory>(hall).unwrap().amount(good), 5);
        assert_eq!(world.get::<GoodsInventory>(worker).unwrap().amount(good), 5);
        assert!(world.get::<Settlement>(hall).unwrap().treasury < 1000);
    }
    #[test]
    fn a_character_inside_a_future_wall_delays_collision_publication() {
        let mut world = World::new();
        let wall = FortificationSegment {
            settlement_id: SettlementId(1),
            circuit: 0,
            start: Vec3::new(-4., 0., 0.),
            end: Vec3::new(4., 0., 0.),
            kind: FortificationKind::Wall,
            material: FortificationMaterial::Palisade,
            complete: false,
        };
        // Position query ownership is CharacterKind, independently of movement.
        let actor = world
            .spawn((CharacterKind::Villager, PlayerPosition(Vec3::ZERO)))
            .id();
        assert!(occupied_by_actor(&mut world, &wall));
        world.get_mut::<PlayerPosition>(actor).unwrap().0.z = 4.;
        assert!(!occupied_by_actor(&mut world, &wall));
    }
    #[test]
    fn a_gate_waits_for_occupied_jambs_but_not_people_under_its_clear_opening() {
        let mut world = World::new();
        let gate = FortificationSegment {
            settlement_id: SettlementId(1),
            circuit: 0,
            start: Vec3::new(-4.0, 0.0, -4.0),
            end: Vec3::new(4.0, 0.0, 4.0),
            kind: FortificationKind::Gate,
            material: FortificationMaterial::Stone,
            complete: false,
        };
        let actor = world
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(gate.gate_post_centers()[0]),
            ))
            .id();
        assert!(occupied_by_actor(&mut world, &gate));
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = gate.midpoint();
        assert!(!occupied_by_actor(&mut world, &gate));
        world.get_mut::<PlayerPosition>(actor).unwrap().0 = gate.gate_post_centers()[1];
        assert!(occupied_by_actor(&mut world, &gate));
    }

    /// Run explicitly with CITYSIM_MAP_ID=village_lab. This uses the same
    /// navigation, embodied mover and civic scheduling as the real server.
    #[test]
    #[ignore = "connected-scale civic construction lab; requires village_lab terrain"]
    fn defense_worker_physically_hauls_and_constructs_in_the_shared_village_schedule() {
        assert_eq!(
            std::env::var("CITYSIM_MAP_ID").as_deref(),
            Ok("village_lab")
        );
        let mut app = App::new();
        app.insert_resource(shared::terrain::WorldTerrain::default());
        crate::world::village_lab::configure_lab(&mut app);
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(25.)));
        let (position, _, _) = crate::world::village_lab_scenario::choose_town_growth_site(
            app.world().resource::<shared::terrain::WorldTerrain>(),
        );
        let hall = crate::world::village_lab::spawn_lab_village(
            app.world_mut(),
            "Defense Works",
            "DefenseWorker",
            CivicStrategy::MutualAid,
            position,
            8,
            SettlementTier::Village,
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.update();
        let id = *app.world().get::<SettlementId>(hall).unwrap();
        let seller_id = app
            .world_mut()
            .query::<&PersonId>()
            .iter(app.world())
            .copied()
            .next()
            .unwrap();
        {
            let mut town = app.world_mut().get_mut::<Settlement>(hall).unwrap();
            town.treasury = 50_000;
        }
        let wood = Good::Wood;
        app.world_mut()
            .get_mut::<GoodsInventory>(hall)
            .unwrap()
            .add(wood, 100);
        app.world_mut()
            .get_mut::<MootMarket>(hall)
            .unwrap()
            .consign(
                MarketSeller::Person(seller_id),
                wood,
                100,
                wood.base_price(),
            );
        let ground = app.world().resource::<shared::terrain::WorldTerrain>();
        let start = Vec3::new(
            position.x - 4.,
            ground.get_height(position.x - 4., position.z - 25.),
            position.z - 25.,
        );
        let end = Vec3::new(
            position.x + 4.,
            ground.get_height(position.x + 4., position.z - 25.),
            position.z - 25.,
        );
        let segment = FortificationSegment {
            settlement_id: id,
            circuit: 0,
            start,
            end,
            kind: FortificationKind::Wall,
            material: FortificationMaterial::Palisade,
            complete: false,
        };
        let section = app
            .world_mut()
            .spawn((segment, GoodsInventory::new(512), WallWork::default()))
            .id();
        let mut seen_carried = false;
        let mut seen_delivered = false;
        let mut seen_hammer = false;
        let mut promoted = false;
        for tick in 0..25_000 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
            app.update();
            let work = app.world().get::<WallWork>(section).unwrap();
            seen_carried |= work.carried > 0;
            seen_delivered |= app
                .world()
                .get::<GoodsInventory>(section)
                .unwrap()
                .amount(wood)
                > 0;
            seen_hammer |= work.builder.is_some_and(|worker| {
                app.world().get::<CharacterActivity>(worker) == Some(&CharacterActivity::Building)
            });
            if seen_carried && !promoted {
                // Promotion must not turn the wood already in the worker's
                // arms into stone or abandon the funded palisade project.
                app.world_mut().get_mut::<Settlement>(hall).unwrap().tier = SettlementTier::Town;
                promoted = true;
            }
            if app
                .world()
                .get::<FortificationSegment>(section)
                .unwrap()
                .complete
            {
                assert_eq!(
                    app.world()
                        .get::<FortificationSegment>(section)
                        .unwrap()
                        .material,
                    FortificationMaterial::Palisade
                );
                assert!(
                    seen_carried && seen_delivered && seen_hammer,
                    "pickup, physical delivery and hammering must all precede completion"
                );
                assert_eq!(
                    app.world()
                        .get::<GoodsInventory>(section)
                        .unwrap()
                        .amount(wood),
                    0
                );
                println!("defense construction completed after {tick} real shared-schedule ticks");
                return;
            }
        }
        panic!("defense did not complete: carried={seen_carried}, delivered={seen_delivered}, hammer={seen_hammer}; work={:?}",app.world().get::<WallWork>(section).map(|w|(w.builder,w.carried,w.seconds_worked,w.retry_after)));
    }
}
