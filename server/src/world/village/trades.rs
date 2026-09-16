//! Tactical farm, fishing and lumber production loops.
//!
//! Output exists only after embodied workers perform and deposit a physical
//! batch. Employment and workplace attachment are joined through durable IDs.

use super::*;

const FARM_WORK_REACH: f32 = 0.45;
// The pier's work point is a visible standing position, not the wider staging
// area. Movement reaches goals within 0.15m, so this permits ordinary arrival
// without starting the fishing clip several metres before the end of the deck.
const FISH_WORK_REACH: f32 = 0.6;

#[cfg(test)]
#[path = "producer_recovery_tests.rs"]
mod producer_recovery_tests;

#[cfg(test)]
#[path = "loaded_delivery_recovery_tests.rs"]
mod loaded_delivery_recovery_tests;

/// Test-journal evidence for hired farmers that have no productive routine.
/// This samples the real admission inputs, including loaded prop collision;
/// it never schedules work or changes the simulation.
#[cfg(test)]
pub(crate) fn farmer_admission_diagnostics(world: &mut World) -> serde_json::Value {
    let blocked: HashSet<_> = world
        .query_filtered::<Entity, super::worker_activity::ProductionStartBlocked>()
        .iter(world)
        .collect();
    let requests: HashSet<_> = world
        .query_filtered::<Entity, With<RoadRequest>>()
        .iter(world)
        .collect();
    let fields: Vec<_> = world
        .query::<(
            Entity,
            &FarmField,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::AttachedTo,
        )>()
        .iter(world)
        .map(|(e, f, p, r, a)| (e, f.clone(), p.0, r.0, a.0))
        .collect();
    let farms: Vec<_> = world
        .query::<(
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        )>()
        .iter(world)
        .filter(|(_, b, ..)| b.kind == SettlementBuildingKind::Farmstead)
        .map(|(e, b, p, r, id, home)| (e, b.kind, p.0, r.0, *id, home.0))
        .collect();
    let employees: Vec<_> = world
        .query::<(
            Entity,
            &CharacterName,
            &shared::components::PersonId,
            &shared::components::EmployedAt,
            &VillagerIntent,
        )>()
        .iter(world)
        .map(|(e, n, id, job, intent)| (e, n.0.clone(), *id, job.0, intent.clone()))
        .collect();
    let roads: Vec<_> = world
        .query::<(&VillageRoad, &shared::components::RoadOf)>()
        .iter(world)
        .map(|(r, home)| (r.clone(), home.0))
        .collect();
    let halls: HashMap<_, _> = world
        .query::<(
            Entity,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        )>()
        .iter(world)
        .map(|(e, id, p, r)| (*id, (e, p.0, r.map_or(0.0, |r| r.0))))
        .collect();
    let clock = world.query::<&WorldTime>().iter(world).next().cloned();
    let terrain = world.get_resource::<WorldTerrain>();
    let obstacles = world.get_resource::<SpatialObstacleGrid>();
    let colliders = world.get_resource::<StaticColliders>();
    let derived = world.get_resource::<DerivedColliderLibrary>();
    let mut records = Vec::new();
    for (farm, kind, at, yaw, id, home) in farms {
        let road_refs: Vec<_> = roads
            .iter()
            .filter_map(|(r, h)| (*h == home).then_some(r))
            .collect();
        let hall = halls.get(&home);
        let road_ready = !requests.contains(&farm)
            && hall.is_some_and(|(_, p, r)| {
                road_refs.is_empty()
                    || crate::world::village_roads::building_has_connected_road(
                        kind, at, yaw, *p, *r, &road_refs,
                    )
            });
        for (worker, name, person, job, intent) in &employees {
            if *job != id {
                continue;
            }
            let mut owners = Vec::new();
            macro_rules! owner {
                ($ty:ty) => {
                    if world.get::<$ty>(*worker).is_some() {
                        owners.push(stringify!($ty));
                    }
                };
            }
            owner!(FarmerRoutine);
            owner!(FishingRoutine);
            owner!(LumberjackRoutine);
            owner!(QuarryRoutine);
            owner!(ProcessingRoutine);
            owner!(HouseholdShoppingRoutine);
            owner!(HomeRoutine);
            owner!(MootQueueTicket);
            owner!(MootMealRoutine);
            owner!(MarketCollectionRoutine);
            owner!(InternalDeliveryRoutine);
            owner!(TradeRouteRoutine);
            owner!(TavernWorkerRoutine);
            owner!(TavernVisitRoutine);
            owner!(RoadBuilderRoutine);
            owner!(ConstructionMaterialRoutine);
            owner!(WorkplaceDoorTransit);
            owner!(PierTraversal);
            owner!(super::worker_activity::doors::WorkplaceInterior);
            owner!(super::worker_activity::EmploymentReleaseRequested);
            let stands: Vec<_> = fields.iter().filter(|(_,f,_,_,attached)|
                *attached == id || f.farmstead.xz().distance_squared(at.xz()) < 0.01)
                .map(|(entity,field,p,r,attached)| {
                    let stand = terrain.and_then(|terrain| crate::world::village_roads::reachable_farm_work_stand_in_shape(
                        terrain,at,yaw,*p,stable_name_hash(name),obstacles,colliders,derived,field.shape.as_ref()));
                    let without_props = terrain.and_then(|terrain| crate::world::village_roads::reachable_farm_work_stand_in_shape(
                        terrain,at,yaw,*p,stable_name_hash(name),obstacles,None,None,field.shape.as_ref()));
                    serde_json::json!({"entity":entity.to_bits().to_string(), "attached_to":attached,
                        "plot":field.plot_index,"productive_fraction":field.productive_fraction(),
                        "field_position":p,"field_rotation":r,"actual_work_stand":stand,
                        "work_stand_without_props":without_props})
                }).collect();
            let props: Vec<_> = colliders.into_iter().flat_map(|c|c.instances.iter())
                .filter(|(_,p)|p.position.xz().distance_squared(at.xz()) < 40_f32.powi(2))
                .map(|(id,p)|serde_json::json!({"id":id,"kind":format!("{:?}",p.kind),"position":p.position,
                    "scale":p.scale,"radius":derived.and_then(|d|d.by_kind.get(&p.kind)).map(|d|d.horizontal_radius*p.scale)}))
                .collect();
            records.push(serde_json::json!({"business":id,"person":person,
                "road_ready":road_ready,"road_request":requests.contains(&farm),
                "settled_in_hall":hall.is_some_and(|(h,_,_)| intent.is_settled() && intent.settlement()==Some(*h)),
                "off_duty_day":world.get::<WorkerOffDuty>(*worker).map(|d|d.day),
                "production_start_blocked":blocked.contains(worker),"owners":owners,"fields":stands,
                "nearby_live_props":props,
                "work_hours":clock.as_ref().is_some_and(|clock| super::worker_activity::schedule::ORDINARY.contains(clock)),
                "day":clock.as_ref().map(|clock|clock.day)}));
        }
    }
    serde_json::json!(records)
}

pub(super) fn workplace_road_is_ready(
    building: Entity,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    settlement_id: shared::components::SettlementId,
    hall_position: Vec3,
    hall_rotation: f32,
    roads: &Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: &Query<(), With<RoadRequest>>,
) -> bool {
    if road_requests.get(building).is_ok() {
        return false;
    }
    let settlement_roads: Vec<_> = roads
        .iter()
        .filter_map(|(road, road_of)| (road_of.0 == settlement_id).then_some(road))
        .collect();
    // Focused unit fixtures predating physical roads retain their lightweight
    // seam. A live settlement publishes a RoadRequest before the first road,
    // so this fallback never opens a real unconnected workplace.
    settlement_roads.is_empty()
        || crate::world::village_roads::building_has_connected_road(
            kind,
            position,
            rotation,
            hall_position,
            hall_rotation,
            &settlement_roads,
        )
}

#[path = "field_parcels.rs"]
mod field_parcels;

#[path = "farm_productivity.rs"]
pub(super) mod farm_productivity;

/// Fit one connected crop parcel and retain its two authoritative worker records.
/// Fitting happens once per missing/legacy parcel; ordinary ticks only inspect IDs.
pub fn ensure_farm_fields(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    farms: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
    )>,
    fields: Query<(
        Entity,
        &FarmField,
        &shared::components::AttachedTo,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    roads: Query<&VillageRoad>,
    pending: Query<&UnderConstruction>,
    accesses: Query<&crate::world::village_roads::PlannedRoadAccess>,
    yards: Query<(
        &shared::components::HouseholdYard,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    walls: Query<&shared::components::FortificationSegment>,
    bodies: Query<
        (
            &PlayerPosition,
            Has<shared::components::Horse>,
            Has<shared::components::Mounted>,
        ),
        (
            Or<(With<CharacterKind>, With<shared::components::Horse>)>,
            Without<shared::components::AboardBoat>,
            Without<crate::player::hero::OfflineHero>,
        ),
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    // Surveyed crop land can exist before its fence. Publish solid rails only
    // when no embodied actor intersects them; never close geometry through a
    // stationary hero, a mounted rider or a passing farm worker.
    let mut granted = 0;
    for (entity, field, _, position, rotation) in &fields {
        if field.layout_version != 1 {
            continue;
        }
        let mut fenced = field.clone();
        fenced.layout_version = 2;
        let proposed = fenced.ground_obstacles(position.0, rotation.0);
        let occupied = bodies.iter().any(|(p, horse, mounted)| {
            if p.0.xz().distance_squared(position.0.xz()) > 60_f32.powi(2) {
                return false;
            }
            let extra = if horse || mounted { 0.55 } else { 0.06 };
            proposed.iter().any(|obstacle| {
                shared::rotation::world_to_local_xz(p.0.xz() - obstacle.center, obstacle.rotation)
                    .abs()
                    .cmple(obstacle.half_extents + Vec2::splat(extra))
                    .all()
            })
        });
        if !occupied {
            commands.entity(entity).insert(fenced);
            granted += 1;
            if granted >= 4 {
                break;
            }
        }
    }
    let planted: HashMap<_, _> = fields
        .iter()
        .map(|(entity, field, attached, _, _)| ((attached.0, field.plot_index), (entity, field)))
        .collect();
    let mut ordered: Vec<_> = farms
        .iter()
        .filter(|(_, farm, _, _, id)| {
            farm.kind == SettlementBuildingKind::Farmstead
                && !(0..shared::components::FARM_FIELDS_PER_FARMSTEAD).all(|i| {
                    planted
                        .get(&(**id, i))
                        .is_some_and(|(_, f)| f.layout_version >= 1)
                })
        })
        .collect();
    if ordered.is_empty() {
        return;
    }
    let mut accepted: Vec<_> = fields
        .iter()
        .map(|(_, f, a, p, r)| (a.0, f.clone(), p.0, r.0))
        .collect();
    ordered.sort_by_key(|(_, _, _, _, id)| id.0);
    // A large-world migration never performs an unbounded set of terrain surveys
    // in one tick. Stable BuildingId order also decides simultaneous open land.
    for (farm_entity, farm, position, rotation, building_id) in ordered.into_iter().take(2) {
        let nearby_buildings: Vec<_> = farms
            .iter()
            .filter(|(entity, _, other, _, _)| {
                *entity != farm_entity && other.0.xz().distance(position.0.xz()) < 90.
            })
            .map(|(_, b, p, r, _)| (b.kind, p.0, r.0))
            .collect();
        let nearby_roads: Vec<_> = roads
            .iter()
            .filter(|r| r.contains_reserved_point(position.0.xz(), 80.))
            .collect();
        let sites: Vec<_> = pending
            .iter()
            .filter(|site| site.position.xz().distance(position.0.xz()) < 90.)
            .collect();
        let local_yards: Vec<_> = yards
            .iter()
            .filter(|(_, p, _)| p.0.xz().distance(position.0.xz()) < 90.)
            .collect();
        let wall_obstacles: Vec<_> = walls
            .iter()
            .flat_map(|wall| wall.ground_obstacles())
            .collect();
        let shapes = field_parcels::fit(
            &terrain,
            position.0,
            rotation.0,
            &nearby_buildings,
            &nearby_roads,
            derived.as_deref(),
            |point| {
                !accepted.iter().any(|(owner, field, p, r)| {
                    *owner != *building_id && field.contains_world_point(point, *p, *r, 2.0)
                }) && !local_yards
                    .iter()
                    .any(|(yard, p, r)| yard.contains_world_point(point, p.0, r.0, 1.0))
                    && !accesses
                        .iter()
                        .any(|access| access.intersects_circle(point, 2.0))
                    && !wall_obstacles.iter().any(|obstacle| {
                        let p = shared::rotation::world_to_local_xz(
                            point - obstacle.center,
                            obstacle.rotation,
                        );
                        p.abs()
                            .cmple(obstacle.half_extents + Vec2::splat(2.0))
                            .all()
                    })
                    && !sites.iter().any(|site| {
                        shared::building::clearance_zones_for_building(
                            site.position,
                            site.kind.placement_definition().building_type,
                            site.rotation,
                        )
                        .iter()
                        .any(|zone| zone.contains_point(point))
                            || site
                                .kind
                                .intended_field_positions(site.position, site.rotation)
                                .is_some_and(|centers| {
                                    centers.iter().any(|center| {
                                        shared::rotation::world_to_local_xz(
                                            point - center.xz(),
                                            site.rotation,
                                        )
                                        .abs()
                                        .cmple(
                                            site.kind.intended_field_half_extents().unwrap()
                                                + Vec2::splat(2.),
                                        )
                                        .all()
                                    })
                                })
                    })
            },
        );
        accepted.retain(|(owner, _, _, _)| *owner != *building_id);
        for (index, shape) in shapes.into_iter().enumerate() {
            let index = index as u8;
            let mut field_position = farm
                .kind
                .field_position_at(position.0, rotation.0, index)
                .unwrap();
            field_position.y = terrain.get_height(field_position.x, field_position.z);
            let field = if let Some((_, existing)) = planted.get(&(*building_id, index)) {
                let mut updated = (*existing).clone();
                // Expansion never removes a previously accepted work stand or
                // productive area just because a neighbouring site has changed.
                let old = updated.accepted_shape();
                let contains_old = shape.contains_shape(&old, 0.05);
                if !old.is_valid() || (shape.area() >= old.area() && contains_old) {
                    updated.shape = Some(shape);
                }
                if updated.shape.is_none() {
                    updated.shape = Some(old);
                }
                updated.layout_version = 1;
                updated
            } else {
                FarmField {
                    settlement: farm.settlement.clone(),
                    farmstead: position.0,
                    plot_index: index,
                    quality: farm.quality,
                    shape: Some(shape),
                    layout_version: 1,
                }
            };
            accepted.push((*building_id, field.clone(), field_position, rotation.0));
            if let Some((entity, _)) = planted.get(&(*building_id, index)) {
                commands.entity(*entity).insert(field);
            } else {
                commands.spawn((
                    field,
                    PlayerPosition(field_position),
                    PlayerRotation(rotation.0),
                    shared::components::AttachedTo(*building_id),
                    Replicate::to_clients(NetworkTarget::All),
                ));
            }
        }
    }
}

/// Establish the one separately replicated, walkable pasture behind every
/// completed Livestock Farm. Sheep are cheap client-side presentation; this
/// record is the authoritative land claim and worker destination.
pub fn ensure_livestock_pastures(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    farms: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
    )>,
    pastures: Query<&shared::components::AttachedTo, With<LivestockPasture>>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let existing: HashSet<_> = pastures.iter().map(|attached| attached.0).collect();
    for (farm, position, rotation, building_id) in farms.iter() {
        if farm.kind != SettlementBuildingKind::LivestockFarm || existing.contains(building_id) {
            continue;
        }
        let Some(mut pasture_position) = farm.kind.pasture_position(position.0, rotation.0) else {
            continue;
        };
        pasture_position.y = terrain.get_height(pasture_position.x, pasture_position.z);
        let pasture = commands
            .spawn((
                LivestockPasture {
                    settlement: farm.settlement.clone(),
                    livestock_farm: position.0,
                    quality: farm.quality,
                },
                PlayerPosition(pasture_position),
                PlayerRotation(rotation.0),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        commands
            .entity(pasture)
            .insert(shared::components::AttachedTo(*building_id));
        info!(
            "Village '{}': fenced a grazing pasture beside its Livestock Farm at {:.1},{:.1}",
            farm.settlement, pasture_position.x, pasture_position.z
        );
    }
}

#[cfg(test)]
mod livestock_pasture_tests {
    use super::*;

    #[test]
    fn each_completed_livestock_farm_gets_one_stable_pasture() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, ensure_livestock_pastures);
        let building_id = shared::components::BuildingId(8_001);
        let origin = Vec3::new(120.0, 0.0, -80.0);
        app.world_mut().spawn((
            building_id,
            SettlementBuilding {
                kind: SettlementBuildingKind::LivestockFarm,
                settlement: "Pasture Test".into(),
                owner: Some("Ada".into()),
                quality: 0.82,
                workers: vec![],
            },
            PlayerPosition(origin),
            PlayerRotation(0.0),
        ));

        app.update();
        app.update();

        let world = app.world_mut();
        let pastures: Vec<_> = world
            .query::<(
                &LivestockPasture,
                &shared::components::AttachedTo,
                &PlayerPosition,
            )>()
            .iter(world)
            .collect();
        assert_eq!(pastures.len(), 1, "the ensure pass must be idempotent");
        assert_eq!(pastures[0].1.0, building_id);
        assert_eq!(pastures[0].0.quality, 0.82);
        let expected = SettlementBuildingKind::LivestockFarm
            .pasture_position(origin, 0.0)
            .unwrap();
        assert_eq!(pastures[0].2.0.x, expected.x);
        assert_eq!(pastures[0].2.0.z, expected.z);
    }
}

/// Place the separate collider-free pier behind every completed Fisherman's
/// Hut. The asset origin is its landward end; the authored piles extend below
/// that origin, so water level is the stable placement plane on every coast.
pub fn ensure_fishing_piers(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    huts: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
    )>,
    piers: Query<&shared::components::AttachedTo, With<FishingPier>>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let Some(water) = terrain.water_level() else {
        return;
    };
    for (hut, position, rotation, building_id) in huts.iter() {
        if hut.kind != SettlementBuildingKind::FishermansHut
            || piers
                .iter()
                .any(|attached_to| attached_to.0 == *building_id)
        {
            continue;
        }
        let Some(mut pier_position) = hut.kind.pier_position(position.0, rotation.0) else {
            continue;
        };
        pier_position.y = water;
        let pier = commands
            .spawn((
                FishingPier {
                    settlement: hut.settlement.clone(),
                    fishermans_hut: position.0,
                    quality: hut.quality,
                },
                PlayerPosition(pier_position),
                PlayerRotation(rotation.0),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        commands
            .entity(pier)
            .insert(shared::components::AttachedTo(*building_id));
        info!(
            "Village '{}': set a fishing pier behind its Fisherman's Hut",
            hut.settlement
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn assign_farmer_routines(
    mut commands: Commands,
    off_duty_workers: Query<&WorkerOffDuty>,
    committed_workers: Query<(), super::worker_activity::ProductionStartBlocked>,
    world_time: Query<&WorldTime>,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    farms: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    fields: Query<(
        Entity,
        &FarmField,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::AttachedTo,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&FarmerHarvestProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<QuarryRoutine>,
            Without<ProcessingRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !super::worker_activity::schedule::ORDINARY.contains(clock) {
        return;
    }
    let mut eligible_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, _, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            eligible_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
    }
    if eligible_by_building.is_empty() {
        return;
    }
    let mut fields_by_farm: HashMap<
        shared::components::BuildingId,
        Vec<(Entity, u8, Vec3, f32, &FarmField)>,
    > = HashMap::new();
    for (field_entity, field, position, rotation, attached_to) in fields.iter() {
        if field.productive_fraction() <= 0.0 {
            continue;
        }
        fields_by_farm.entry(attached_to.0).or_default().push((
            field_entity,
            field.plot_index,
            position.0,
            rotation.0,
            field,
        ));
    }
    for farm_fields in fields_by_farm.values_mut() {
        farm_fields.sort_by_key(|(_, plot_index, _, _, _)| *plot_index);
    }
    let mut claimed = HashSet::new();
    for (farmstead, farm, position, rotation, building_id, building_of) in farms.iter() {
        if farm.kind != SettlementBuildingKind::Farmstead {
            continue;
        }
        let Some(employees) = eligible_by_building.get(building_id) else {
            continue;
        };
        let Some(farm_fields) = fields_by_farm.get(building_id) else {
            continue;
        };
        let Some((hall, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, _, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        if !workplace_road_is_ready(
            farmstead,
            farm.kind,
            position.0,
            rotation.0,
            building_of.0,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            super::workplace_access::defer_shift(
                &mut commands,
                employees,
                farmstead,
                clock.day,
                &off_duty_workers,
            );
            continue;
        }
        for (worker_index, employee) in employees
            .iter()
            .take(farm.kind.positions() as usize)
            .enumerate()
        {
            let (field, _, field_position, field_rotation, field_record) =
                farm_fields[worker_index % farm_fields.len()];
            let Ok((worker, name, intent, _, off_duty, progress, employment)) =
                villagers.get(*employee)
            else {
                continue;
            };
            if committed_workers.get(worker).is_ok()
                || claimed.contains(&worker)
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let progress = progress
                .filter(|progress| progress.farmstead == farmstead && progress.field == field);
            let harvest_seconds = progress.map_or(0.0, |progress| progress.seconds);
            let production_day = progress.map_or(u32::MAX, |progress| progress.production_day);
            let produced_today = progress.map_or(0, |progress| progress.produced_today);
            let worker_salt = stable_name_hash(&name.0);
            let work_stand = if let Some(terrain) = terrain.as_deref() {
                crate::world::village_roads::reachable_farm_work_stand_in_shape(
                    terrain,
                    position.0,
                    rotation.0,
                    field_position,
                    worker_salt,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                    field_record.shape.as_ref(),
                )
            } else {
                farm_work_stand(
                    field_position,
                    field_rotation,
                    worker_salt,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                )
                .filter(|point| {
                    field_record.contains_world_point(
                        point.xz(),
                        field_position,
                        field_rotation,
                        -0.3,
                    )
                })
            };
            let Some(work_stand) = work_stand else {
                debug!(
                    "Farmer {} cannot reach a certified standing point for Farmstead {:?}",
                    name.0, building_id
                );
                continue;
            };
            claimed.insert(worker);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<FarmerHarvestProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    FarmerRoutine {
                        farmstead,
                        field,
                        hall,
                        work_stand,
                        harvest_seconds,
                        failed_workplace_routes: 0,
                        production_day,
                        produced_today,
                        phase: FarmerPhase::GoingToFarmstead,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(farm.kind.entrance_position(position.0, rotation.0)),
                ));
        }
    }
}

/// Run the producer-owned part of the physical wheat loop: field to Farmstead.
/// Farmers never sell or haul to the Moot Hall. The market porter independently
/// evaluates the Farmstead's sale policy and collects only approved surplus.
#[allow(clippy::too_many_arguments)]
pub fn run_farmer_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    _economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    farms: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    mut fields: farm_productivity::FarmProductivity,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut operating_plans: Query<&mut BusinessStaffingForecast, Without<CharacterKind>>,
    (activity_busy, interiors): (
        Query<(), super::worker_activity::ProductionPausedBy>,
        Query<&WorkplaceInterior>,
    ),
    release_requested: Query<(), With<super::worker_activity::EmploymentReleaseRequested>>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut CharacterActivity,
            Option<&mut CharacterAttributes>,
            &mut FarmerRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    fields.refresh();
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = super::worker_activity::schedule::ORDINARY.contains(clock);

    for (
        worker,
        name,
        employment,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut activity,
        mut attributes,
        mut routine,
        move_target,
        route_failed,
    ) in workers.iter_mut()
    {
        let workday_active = workday_active && !release_requested.contains(worker);
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || activity_busy.get(worker).is_ok()
            || road_builder.is_some()
        {
            // Keep the accumulated harvest, but recover the physical commute
            // after another routine takes the worker away from the field.
            // Returning also deposits any company crop still in their basket.
            routine.phase = FarmerPhase::ReturningToFarmstead;
            continue;
        }
        if door_transit.is_some() {
            continue;
        }
        if !intent.is_settled() {
            routine.phase = FarmerPhase::ReturningToFarmstead;
            if *activity != CharacterActivity::Idle {
                activity.set_if_neq(CharacterActivity::Idle);
            }
            continue;
        }
        let Ok((farm, farm_position, farm_rotation, building_id, building_of)) =
            farms.get(routine.farmstead)
        else {
            commands.entity(worker).remove::<FarmerRoutine>();
            super::worker_activity::lifecycle::cancel_travel(&mut commands, worker, None);
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        };
        if farm.kind != SettlementBuildingKind::Farmstead || employment.0 != *building_id {
            commands.entity(worker).remove::<FarmerRoutine>();
            super::worker_activity::lifecycle::cancel_travel(
                &mut commands,
                worker,
                interiors.get(worker).ok().filter(|_| intent.is_settled()),
            );
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        let Some((_field, attached_to)) = fields.field(routine.field) else {
            continue;
        };
        let Ok(settlement_id) = halls.get(routine.hall) else {
            continue;
        };
        if attached_to.0 != *building_id || *settlement_id != building_of.0 {
            continue;
        }
        let farm_entrance = farm
            .kind
            .entrance_position(farm_position.0, farm_rotation.0);
        let farm_inside = farm
            .kind
            .interior_door_position(farm_position.0, farm_rotation.0);

        if route_failed.is_some()
            && inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
        {
            routine.phase = FarmerPhase::ReturningToFarmstead;
            super::worker_activity::lifecycle::retain_failed_delivery(
                &mut commands,
                worker,
                position.0,
                farm_entrance,
                DOOR_REACH,
                move_target,
                &mut activity,
            );
            continue;
        }

        if let Some(failed) = route_failed {
            let retry_target = match routine.phase {
                FarmerPhase::WalkingToField { stand } => Some(stand),
                FarmerPhase::GoingToFarmstead | FarmerPhase::ReturningToFarmstead => {
                    Some(farm_entrance)
                }
                FarmerPhase::Inside { .. } | FarmerPhase::Farming | FarmerPhase::EndingShift => {
                    None
                }
            };
            routine.failed_workplace_routes = routine.failed_workplace_routes.saturating_add(1);
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            if routine.failed_workplace_routes >= MAX_WORKPLACE_ROUTE_FAILURES || !workday_active {
                warn!(
                    "Farmer {} could not reach the workplace at {:.1},{:.1} after {} routes; ending the empty-handed shift",
                    name.0, failed.goal.x, failed.goal.z, routine.failed_workplace_routes,
                );
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    production_day,
                    &*routine,
                    &mut activity,
                );
                continue;
            }
            if let Some(target) = retry_target {
                // One failed commute must not turn an employed farmer into an
                // ambient idler for the rest of the day. Keep the exact work
                // phase (and any carried Wheat), then let the route planner
                // try a different certified corridor.
                commands.entity(worker).insert(MoveTarget(target));
                activity.set_if_neq(CharacterActivity::Idle);
            } else {
                warn!(
                    "Farmer {} discarded stale route failure for {:.1},{:.1} while {:?}",
                    name.0, failed.goal.x, failed.goal.z, routine.phase
                );
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                FarmerPhase::Inside { .. } => {
                    activity.set_if_neq(CharacterActivity::Idle);
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        farm_position.0,
                        farm_entrance,
                        farm_inside,
                        farm_entrance,
                    );
                    routine.phase = FarmerPhase::EndingShift;
                    continue;
                }
                FarmerPhase::WalkingToField { .. } | FarmerPhase::Farming => {
                    activity.set_if_neq(CharacterActivity::Idle);
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                FarmerPhase::GoingToFarmstead
                    if ground_distance(position.0, farm_entrance) <= DOOR_REACH =>
                {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                    {
                        routine.phase = FarmerPhase::ReturningToFarmstead;
                        continue;
                    }
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                FarmerPhase::EndingShift => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                    {
                        ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                        routine.phase = FarmerPhase::ReturningToFarmstead;
                        continue;
                    }
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                FarmerPhase::GoingToFarmstead | FarmerPhase::ReturningToFarmstead => {}
            }
        }

        match routine.phase {
            FarmerPhase::GoingToFarmstead => {
                if ground_distance(position.0, farm_entrance) <= DOOR_REACH {
                    routine.failed_workplace_routes = 0;
                    if !workday_active {
                        if inventories
                            .get_mut(worker)
                            .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                        {
                            routine.phase = FarmerPhase::ReturningToFarmstead;
                            continue;
                        }
                        super::worker_activity::lifecycle::finish(
                            &mut commands,
                            worker,
                            production_day,
                            &*routine,
                            &mut activity,
                        );
                        continue;
                    }
                    activity.set_if_neq(CharacterActivity::Idle);
                    begin_workplace_entry(
                        &mut commands,
                        worker,
                        farm_position.0,
                        farm_entrance,
                        farm_inside,
                    );
                    routine.phase = FarmerPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                }
            }
            FarmerPhase::Inside { seconds_left } => {
                activity.set_if_neq(CharacterActivity::Indoors);
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = FarmerPhase::Inside { seconds_left: left };
                    continue;
                }
                if inventories
                    .get(routine.farmstead)
                    .is_ok_and(|store| store.free_bulk() < Good::Wheat.bulk_per_unit())
                {
                    // Resume when a porter frees storage, without starting
                    // repeated empty field trips or discarding carried goods.
                    routine.phase = FarmerPhase::Inside { seconds_left: 0.0 };
                    continue;
                }
                let stand = routine.work_stand;
                activity.set_if_neq(CharacterActivity::Idle);
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    farm_position.0,
                    farm_entrance,
                    farm_inside,
                    stand,
                );
                routine.phase = FarmerPhase::WalkingToField { stand };
            }
            FarmerPhase::WalkingToField { stand } => {
                // Work candidates are inset 0.6m into accepted crop ground.
                // The general 2.5m interaction reach stopped farmers outside
                // small parcels before they ever reached their crop rows.
                if ground_distance(position.0, stand) <= FARM_WORK_REACH {
                    routine.failed_workplace_routes = 0;
                    commands.entity(worker).remove::<MoveTarget>();
                    activity.set_if_neq(CharacterActivity::Farming);
                    routine.phase = FarmerPhase::Farming;
                } else {
                    ensure_move_target(&mut commands, worker, move_target, stand);
                }
            }
            FarmerPhase::Farming => {
                if ground_distance(position.0, routine.work_stand) > FARM_WORK_REACH {
                    let stand = routine.work_stand;
                    activity.set_if_neq(CharacterActivity::Idle);
                    ensure_move_target(&mut commands, worker, move_target, stand);
                    routine.phase = FarmerPhase::WalkingToField { stand };
                    continue;
                }
                activity.set_if_neq(CharacterActivity::Farming);
                let seconds_per_wheat = farmer_seconds_per_wheat(farm.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    // The carried bundle is a completed field basket, not a
                    // frame-by-frame progress meter. Publishing the first of
                    // two Wheat immediately made the client replace the
                    // harvest animation with a stationary carry pose for the
                    // entire second production interval. Retain continuous
                    // labour internally, then materialise the full batch and
                    // leave the field on the same tick.
                    let carried = carrier.amount(Good::Wheat);
                    let needed = FARM_CARRY_BATCH_UNITS
                        .saturating_sub(carried)
                        .min(carrier.free_bulk() / Good::Wheat.bulk_per_unit());
                    if needed == 0 {
                        activity.set_if_neq(CharacterActivity::Idle);
                        commands.entity(worker).insert(MoveTarget(farm_entrance));
                        routine.phase = FarmerPhase::ReturningToFarmstead;
                        continue;
                    }
                    routine.harvest_seconds += dt * fields.fraction(*building_id);
                    let batch_seconds = seconds_per_wheat * needed as f32;
                    if needed > 0 && routine.harvest_seconds < batch_seconds {
                        continue;
                    }
                    let first_harvest_today = routine.produced_today == 0;
                    let produced = carrier.add(Good::Wheat, needed);
                    if produced > 0 {
                        // This basket ends the field visit. Any remainder of
                        // an accelerated tick happened after it was full.
                        routine.harvest_seconds = 0.0;
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    if let Ok(mut plan) = operating_plans.get_mut(routine.farmstead) {
                        plan.record(production_day, produced);
                    }
                    business_events.record_production(production_day, *building_id, produced);
                    // Wheat is agricultural output, not a ration. The mill
                    // records food production only when this becomes Flour.
                    // Attribute progression remains once per productive day
                    // even though production itself has no daily cap.
                    if produced > 0 && first_harvest_today {
                        if let Some(attributes) = attributes.as_deref_mut() {
                            attributes.train_physique(1);
                        }
                    }
                    let batch_ready = carrier.amount(Good::Wheat) >= FARM_CARRY_BATCH_UNITS;
                    let can_keep_harvesting =
                        !batch_ready && carrier.free_bulk() >= Good::Wheat.bulk_per_unit();
                    if can_keep_harvesting {
                        continue;
                    }
                }
                activity.set_if_neq(CharacterActivity::Idle);
                commands.entity(worker).insert(MoveTarget(farm_entrance));
                routine.phase = FarmerPhase::ReturningToFarmstead;
            }
            FarmerPhase::ReturningToFarmstead => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, farm_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    continue;
                }
                routine.failed_workplace_routes = 0;
                commands.entity(worker).remove::<MoveTarget>();
                let unloaded =
                    unload_worker_output(&mut inventories, worker, routine.farmstead, Good::Wheat);
                if !unloaded {
                    // A full store is backpressure, not a licence to take
                    // company stock home. Wait at the Farmstead until a porter
                    // frees space, preserving every unit in personal cargo.
                    activity.set_if_neq(CharacterActivity::Idle);
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                if !workday_active {
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                activity.set_if_neq(CharacterActivity::Idle);
                begin_workplace_entry(
                    &mut commands,
                    worker,
                    farm_position.0,
                    farm_entrance,
                    farm_inside,
                );
                routine.phase = FarmerPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            FarmerPhase::EndingShift => {
                activity.set_if_neq(CharacterActivity::Idle);
                if inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.amount(Good::Wheat) > 0)
                {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    production_day,
                    &*routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Attach the physical hut-to-pier loop to each named fisher.
pub fn assign_fishing_routines(
    mut commands: Commands,
    off_duty_workers: Query<&WorkerOffDuty>,
    committed_workers: Query<(), super::worker_activity::ProductionStartBlocked>,
    world_time: Query<&WorldTime>,
    huts: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    piers: Query<(Entity, &FishingPier, &shared::components::AttachedTo)>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&FishingWorkProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<QuarryRoutine>,
            Without<ProcessingRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !super::worker_activity::schedule::ORDINARY.contains(clock) {
        return;
    }
    let mut eligible_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, _, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            eligible_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
    }
    if eligible_by_building.is_empty() {
        return;
    }
    let mut claimed = HashSet::new();
    for (hut_entity, hut, hut_position, hut_rotation, building_id, building_of) in huts.iter() {
        if hut.kind != SettlementBuildingKind::FishermansHut {
            continue;
        }
        let Some(employees) = eligible_by_building.get(building_id) else {
            continue;
        };
        let Some((pier, _, _)) = piers
            .iter()
            .find(|(_, _, attached_to)| attached_to.0 == *building_id)
        else {
            continue;
        };
        let Some((hall, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, _, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        if !workplace_road_is_ready(
            hut_entity,
            hut.kind,
            hut_position.0,
            hut_rotation.0,
            building_of.0,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            super::workplace_access::defer_shift(
                &mut commands,
                employees,
                hut_entity,
                clock.day,
                &off_duty_workers,
            );
            continue;
        }
        for employee in employees.iter().take(hut.kind.positions() as usize) {
            let Ok((worker, worker_display_name, intent, _, off_duty, progress, employment)) =
                villagers.get(*employee)
            else {
                continue;
            };
            if claimed.contains(&worker)
                || committed_workers.get(worker).is_ok()
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let progress =
                progress.filter(|progress| progress.hut == hut_entity && progress.pier == pier);
            let catch_seconds = progress.map_or(0.0, |progress| progress.seconds);
            let production_day = progress.map_or(u32::MAX, |progress| progress.production_day);
            let produced_today = progress.map_or(0, |progress| progress.produced_today);
            claimed.insert(worker);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<FishingWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    FishingRoutine {
                        hut: hut_entity,
                        pier,
                        hall,
                        catch_seconds,
                        failed_workplace_routes: 0,
                        production_day,
                        produced_today,
                        phase: FishingPhase::GoingToHut,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(hut.kind.entrance_position(hut_position.0, hut_rotation.0)),
                ));
            info!(
                "Village '{}': {} began fishing from the new pier",
                hut.settlement, worker_display_name.0,
            );
        }
    }
}

fn fishing_land_point(terrain: &WorldTerrain, hut: Vec3, rotation: f32, local: Vec2) -> Vec3 {
    let offset = shared::rotation::local_to_world_xz(local, rotation);
    let x = hut.x + offset.x;
    let z = hut.z + offset.y;
    Vec3::new(x, terrain.get_height(x, z), z)
}

pub(super) fn fishing_deck_points(pier: Vec3, rotation: f32) -> (Vec3, Vec3) {
    const DECK_HEIGHT: f32 = 0.52;
    let deck_start = Vec3::new(pier.x, pier.y + DECK_HEIGHT, pier.z);
    let offset = shared::rotation::local_to_world_xz(Vec2::new(0.0, 6.25), rotation);
    let fish_spot = Vec3::new(pier.x + offset.x, pier.y + DECK_HEIGHT, pier.z + offset.y);
    (deck_start, fish_spot)
}

fn install_fishing_route(
    commands: &mut Commands,
    worker: Entity,
    goal: Vec3,
    waypoints: impl IntoIterator<Item = Vec3>,
    traversal: PierTraversal,
) {
    commands
        .entity(worker)
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            MoveTarget(goal),
            TravelRoute {
                goal,
                waypoints: waypoints
                    .into_iter()
                    .map(|position| RouteWaypoint {
                        position,
                        on_road: false,
                    })
                    .collect(),
                next: 0,
                geometry_version: 0,
            },
            traversal,
        ));
}

/// Run the visible direct-food loop:
/// hut -> safe side route -> pier -> fish until a carry batch is full -> hut.
/// The fisher deposits only into the private hut store; the Moot porter later
/// collects policy-approved surplus as a separate job.
#[allow(clippy::too_many_arguments)]
pub fn run_fishing_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    world_time: Query<&WorldTime>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    huts: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    piers: Query<
        (
            &FishingPier,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::AttachedTo,
        ),
        Without<CharacterKind>,
    >,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut operating_plans: Query<&mut BusinessStaffingForecast, Without<CharacterKind>>,
    (activity_busy, interiors): (
        Query<(), super::worker_activity::ProductionPausedBy>,
        Query<&WorkplaceInterior>,
    ),
    release_requested: Query<(), With<super::worker_activity::EmploymentReleaseRequested>>,
    on_pier: Query<(), With<PierTraversal>>,
    mut workers: Query<
        (
            Entity,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut FishingRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = super::worker_activity::schedule::ORDINARY.contains(clock);

    for (
        worker,
        employment,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_failed,
    ) in workers.iter_mut()
    {
        let workday_active = workday_active && !release_requested.contains(worker);
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        let interrupted = home_routine.is_some()
            || shopping.is_some()
            || activity_busy.get(worker).is_ok()
            || road_builder.is_some();
        let exiting_for_errand = interrupted && on_pier.contains(worker);
        if interrupted && !exiting_for_errand {
            // Another routine owns the trip and presentation. Remember work
            // progress, not the old physical phase: a meal can leave a fisher
            // at the Hall after removing its route to the pier.
            if !matches!(routine.phase, FishingPhase::GoingToHut) {
                routine.phase = FishingPhase::GoingToHut;
                commands.entity(worker).remove::<PierTraversal>();
            }
            continue;
        }
        if door_transit.is_some() {
            continue;
        }
        if !intent.is_settled() {
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        let Ok((hut, hut_position, hut_rotation, building_id, building_of)) = huts.get(routine.hut)
        else {
            commands.entity(worker).remove::<FishingRoutine>();
            super::worker_activity::lifecycle::cancel_travel(&mut commands, worker, None);
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        };
        if hut.kind != SettlementBuildingKind::FishermansHut || employment.0 != *building_id {
            commands.entity(worker).remove::<FishingRoutine>();
            super::worker_activity::lifecycle::cancel_travel(
                &mut commands,
                worker,
                interiors.get(worker).ok().filter(|_| intent.is_settled()),
            );
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        let Ok((pier, pier_position, pier_rotation, attached_to)) = piers.get(routine.pier) else {
            continue;
        };
        let Ok(settlement_id) = halls.get(routine.hall) else {
            continue;
        };
        if attached_to.0 != *building_id || *settlement_id != building_of.0 {
            continue;
        }

        let entrance = hut.kind.entrance_position(hut_position.0, hut_rotation.0);
        let inside = hut
            .kind
            .interior_door_position(hut_position.0, hut_rotation.0);
        let staging = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, -4.45),
        );
        let nets = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, -0.35),
        );
        let rear = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, 2.85),
        );
        let (deck_start, fish_spot) = fishing_deck_points(pier_position.0, pier_rotation.0);
        let traversal = PierTraversal {
            deck_start,
            deck_end: fish_spot,
        };

        // Paid personal service can reserve a ticket while the fisher is on
        // the pier, but must wait for this authored route to reach dry land.
        // Do not reset to a land-only hut route or stop behind the busy filter.
        if exiting_for_errand && !matches!(routine.phase, FishingPhase::ReturningFromPier { .. }) {
            activity.set_if_neq(CharacterActivity::Idle);
            install_fishing_route(
                &mut commands,
                worker,
                staging,
                [deck_start, rear, nets, staging],
                traversal,
            );
            routine.phase = FishingPhase::ReturningFromPier { staging };
            continue;
        }

        // Recover interrupted authored routes before doing work. Never grant
        // fish or play the fishing clip just because an old phase survived a
        // different routine's movement. Walk off the deck through its real
        // shoreline approach; ordinary land navigation cannot cross its water.
        let stranded_pier_phase = match routine.phase {
            FishingPhase::Fishing => ground_distance(position.0, fish_spot) > FISH_WORK_REACH,
            FishingPhase::WalkingToPier => {
                move_target.is_none() && ground_distance(position.0, fish_spot) > FISH_WORK_REACH
            }
            FishingPhase::ReturningFromPier { .. } => {
                move_target.is_none() && ground_distance(position.0, staging) > WORK_REACH
            }
            _ => false,
        };
        if stranded_pier_phase && route_failed.is_none() {
            activity.set_if_neq(CharacterActivity::Idle);
            if traversal
                .deck_height_at(Vec2::new(position.0.x, position.0.z))
                .is_some()
            {
                install_fishing_route(
                    &mut commands,
                    worker,
                    staging,
                    [deck_start, rear, nets, staging],
                    traversal,
                );
                routine.phase = FishingPhase::ReturningFromPier { staging };
            } else {
                commands
                    .entity(worker)
                    .remove::<PierTraversal>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .insert(MoveTarget(entrance));
                routine.phase = FishingPhase::GoingToHut;
            }
            continue;
        }

        if route_failed.is_some()
            && inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
        {
            if traversal
                .deck_height_at(Vec2::new(position.0.x, position.0.z))
                .is_some()
            {
                // A loaded fisher must leave the actual deck before asking the
                // land navigator to resume the failed workplace delivery.
                activity.set_if_neq(CharacterActivity::Idle);
                install_fishing_route(
                    &mut commands,
                    worker,
                    staging,
                    [deck_start, rear, nets, staging],
                    traversal,
                );
                routine.phase = FishingPhase::ReturningFromPier { staging };
            } else {
                routine.phase = FishingPhase::ReturningToHut;
                super::worker_activity::lifecycle::retain_failed_delivery(
                    &mut commands,
                    worker,
                    position.0,
                    entrance,
                    DOOR_REACH,
                    move_target,
                    &mut activity,
                );
            }
            continue;
        }

        if let Some(failed) = route_failed {
            routine.failed_workplace_routes = routine.failed_workplace_routes.saturating_add(1);
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            if routine.failed_workplace_routes >= MAX_WORKPLACE_ROUTE_FAILURES || !workday_active {
                warn!(
                    "Fisher could not reach the hut at {:.1},{:.1} after {} routes; ending the empty-handed shift",
                    failed.goal.x, failed.goal.z, routine.failed_workplace_routes,
                );
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    production_day,
                    &*routine,
                    &mut activity,
                );
            } else {
                commands.entity(worker).insert(MoveTarget(entrance));
                routine.phase = FishingPhase::ReturningToHut;
                activity.set_if_neq(CharacterActivity::Idle);
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                FishingPhase::Inside { .. } => {
                    activity.set_if_neq(CharacterActivity::Idle);
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        hut_position.0,
                        entrance,
                        inside,
                        entrance,
                    );
                    routine.phase = FishingPhase::EndingShift;
                    continue;
                }
                FishingPhase::Fishing | FishingPhase::WalkingToPier => {
                    activity.set_if_neq(CharacterActivity::Idle);
                    install_fishing_route(
                        &mut commands,
                        worker,
                        staging,
                        [deck_start, rear, nets, staging],
                        traversal,
                    );
                    routine.phase = FishingPhase::ReturningFromPier { staging };
                    continue;
                }
                FishingPhase::StagingForPier { .. } => {
                    commands
                        .entity(worker)
                        .remove::<PierTraversal>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>();
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                FishingPhase::GoingToHut if ground_distance(position.0, entrance) <= DOOR_REACH => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                    {
                        routine.phase = FishingPhase::ReturningToHut;
                        continue;
                    }
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                FishingPhase::EndingShift => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                    {
                        ensure_move_target(&mut commands, worker, move_target, entrance);
                        routine.phase = FishingPhase::ReturningToHut;
                        continue;
                    }
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                FishingPhase::GoingToHut
                | FishingPhase::ReturningFromPier { .. }
                | FishingPhase::ReturningToHut => {}
            }
        }

        match routine.phase {
            FishingPhase::GoingToHut => {
                if ground_distance(position.0, entrance) <= DOOR_REACH {
                    routine.failed_workplace_routes = 0;
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                    {
                        routine.phase = FishingPhase::ReturningToHut;
                        continue;
                    }
                    if !workday_active {
                        super::worker_activity::lifecycle::finish(
                            &mut commands,
                            worker,
                            production_day,
                            &*routine,
                            &mut activity,
                        );
                        continue;
                    }
                    activity.set_if_neq(CharacterActivity::Idle);
                    begin_workplace_entry(&mut commands, worker, hut_position.0, entrance, inside);
                    routine.phase = FishingPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                }
            }
            FishingPhase::Inside { seconds_left } => {
                activity.set_if_neq(CharacterActivity::Indoors);
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = FishingPhase::Inside { seconds_left: left };
                    continue;
                }
                activity.set_if_neq(CharacterActivity::Idle);
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    hut_position.0,
                    entrance,
                    inside,
                    staging,
                );
                routine.phase = FishingPhase::StagingForPier { staging };
            }
            FishingPhase::StagingForPier { staging } => {
                if ground_distance(position.0, staging) > WORK_REACH {
                    ensure_move_target(&mut commands, worker, move_target, staging);
                    continue;
                }
                routine.failed_workplace_routes = 0;
                install_fishing_route(
                    &mut commands,
                    worker,
                    fish_spot,
                    [nets, rear, deck_start, fish_spot],
                    traversal,
                );
                routine.phase = FishingPhase::WalkingToPier;
            }
            FishingPhase::WalkingToPier => {
                if ground_distance(position.0, fish_spot) <= FISH_WORK_REACH {
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .insert(traversal);
                    let outward = Vec2::new(fish_spot.x - deck_start.x, fish_spot.z - deck_start.z);
                    if outward.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-outward.x, -outward.y);
                    }
                    activity.set_if_neq(CharacterActivity::Fishing);
                    routine.phase = FishingPhase::Fishing;
                }
            }
            FishingPhase::Fishing => {
                // Deck ownership lasts while standing still as well as while
                // walking. Otherwise a personal errand can request a land
                // route straight across the water under the fishing spot.
                if !on_pier.contains(worker) {
                    commands.entity(worker).insert(traversal);
                }
                activity.set_if_neq(CharacterActivity::Fishing);
                let seconds_per_food = fisher_seconds_per_food(pier.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    let carried = carrier.amount(Good::Food);
                    let needed = FISH_CARRY_BATCH_UNITS
                        .saturating_sub(carried)
                        .min(carrier.free_bulk() / Good::Food.bulk_per_unit());
                    if needed == 0 {
                        activity.set_if_neq(CharacterActivity::Idle);
                        install_fishing_route(
                            &mut commands,
                            worker,
                            staging,
                            [deck_start, rear, nets, staging],
                            traversal,
                        );
                        routine.phase = FishingPhase::ReturningFromPier { staging };
                        continue;
                    }
                    routine.catch_seconds += dt;
                    let batch_seconds = seconds_per_food * needed as f32;
                    if needed > 0 && routine.catch_seconds < batch_seconds {
                        continue;
                    }
                    let produced = carrier.add(Good::Food, needed);
                    if produced > 0 {
                        // A full catch must be carried ashore before another
                        // work interval can earn fish.
                        routine.catch_seconds = 0.0;
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    if let Ok(mut plan) = operating_plans.get_mut(routine.hut) {
                        plan.record(production_day, produced);
                    }
                    business_events.record_production(production_day, *building_id, produced);
                    economy_runtime.record_food_production(routine.hall, produced);
                    if carrier.amount(Good::Food) < FISH_CARRY_BATCH_UNITS
                        && carrier.free_bulk() >= Good::Food.bulk_per_unit()
                    {
                        continue;
                    }
                }
                activity.set_if_neq(CharacterActivity::Idle);
                install_fishing_route(
                    &mut commands,
                    worker,
                    staging,
                    [deck_start, rear, nets, staging],
                    traversal,
                );
                routine.phase = FishingPhase::ReturningFromPier { staging };
            }
            FishingPhase::ReturningFromPier { staging } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, staging) > WORK_REACH {
                    continue;
                }
                commands
                    .entity(worker)
                    .remove::<PierTraversal>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .insert(MoveTarget(entrance));
                routine.phase = FishingPhase::ReturningToHut;
            }
            FishingPhase::ReturningToHut => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    continue;
                }
                routine.failed_workplace_routes = 0;
                commands.entity(worker).remove::<MoveTarget>();
                let unloaded =
                    unload_worker_output(&mut inventories, worker, routine.hut, Good::Food);
                if !unloaded {
                    activity.set_if_neq(CharacterActivity::Idle);
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                if !workday_active {
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                begin_workplace_entry(&mut commands, worker, hut_position.0, entrance, inside);
                routine.phase = FishingPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            FishingPhase::EndingShift => {
                if inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.amount(Good::Food) > 0)
                {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    production_day,
                    &*routine,
                    &mut activity,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "fishing_routine_tests.rs"]
mod fishing_routine_tests;

/// Attach a physical work routine to every staffed lumberjack hut.
///
/// `EmployedAt(BuildingId)` is authoritative. The readable name roster is only
/// a compatibility path for buildings loaded before durable relationships.
pub fn assign_lumberjack_routines(
    mut commands: Commands,
    off_duty_workers: Query<&WorkerOffDuty>,
    committed_workers: Query<(), super::worker_activity::ProductionStartBlocked>,
    world_time: Query<&WorldTime>,
    huts: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    road_requests: Query<(), With<RoadRequest>>,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&LumberjackWorkProgress>,
            Option<&shared::components::EmployedAt>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
            Without<QuarryRoutine>,
            Without<ProcessingRoutine>,
            Without<MootQueueTicket>,
            Without<MootMealRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !super::worker_activity::schedule::ORDINARY.contains(clock) {
        return;
    }
    let mut eligible_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, _, _, _, _, employment) in villagers.iter() {
        if let Some(employment) = employment {
            eligible_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
    }
    if eligible_by_building.is_empty() {
        return;
    }
    let mut claimed = HashSet::new();
    for (hut_entity, building, hut_position, hut_rotation, building_id, building_of) in huts.iter()
    {
        if building.kind != SettlementBuildingKind::LumberjackHut {
            continue;
        }
        let Some(employees) = eligible_by_building.get(building_id) else {
            continue;
        };
        let Some((hall, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(_, settlement_id, _, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        if !workplace_road_is_ready(
            hut_entity,
            building.kind,
            hut_position.0,
            hut_rotation.0,
            building_of.0,
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
            &roads,
            &road_requests,
        ) {
            super::workplace_access::defer_shift(
                &mut commands,
                employees,
                hut_entity,
                clock.day,
                &off_duty_workers,
            );
            continue;
        }
        for employee in employees.iter().take(building.kind.positions() as usize) {
            let Ok((worker, worker_display_name, intent, _, off_duty, progress, employment)) =
                villagers.get(*employee)
            else {
                continue;
            };
            if committed_workers.get(worker).is_ok()
                || claimed.contains(&worker)
                || !intent.is_settled()
                || intent.settlement() != Some(hall)
                || off_duty.is_some_and(|off_duty| off_duty.day == clock.day)
                || employment.is_none_or(|employment| employment.0 != *building_id)
            {
                continue;
            }
            let progress = progress.filter(|progress| progress.hut == hut_entity);
            let (cycle, chop_seconds) =
                progress.map_or((0, 0.0), |progress| (progress.cycle, progress.chop_seconds));
            let production_day = progress.map_or(u32::MAX, |progress| progress.production_day);
            let produced_today = progress.map_or(0, |progress| progress.produced_today);
            claimed.insert(worker);
            let entrance = building
                .kind
                .entrance_position(hut_position.0, hut_rotation.0);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<LumberjackWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    LumberjackRoutine {
                        hut: hut_entity,
                        hall,
                        cycle,
                        failed_tree_routes: 0,
                        failed_hut_routes: 0,
                        chop_seconds,
                        production_day,
                        produced_today,
                        phase: LumberjackPhase::GoingToHut,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(entrance),
                ));
            info!(
                "Village '{}': {} began working from the lumberjack hut",
                building.settlement, worker_display_name.0
            );
        }
    }
}

/// Run the observed woodcutting loop:
/// hut -> tree -> chop -> carry -> hut, with periodic hauling to the hall.
///
/// Every transfer uses bounded inventories. A full destination leaves the
/// remainder in the source, so congestion is visible as a worker who cannot
/// complete the next leg rather than as deleted resources.
#[allow(clippy::too_many_arguments)]
pub fn run_lumberjack_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    huts: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
        ),
        Without<CharacterKind>,
    >,
    halls: Query<&shared::components::SettlementId, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut operating_plans: Query<&mut BusinessStaffingForecast, Without<CharacterKind>>,
    mut tree_candidates: Local<TreeWorkCandidateCache>,
    (activity_busy, interiors): (
        Query<(), super::worker_activity::ProductionPausedBy>,
        Query<&WorkplaceInterior>,
    ),
    release_requested: Query<(), With<super::worker_activity::EmploymentReleaseRequested>>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &shared::components::EmployedAt,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut LumberjackRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let dt = simulation_time.world_seconds();
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = super::worker_activity::schedule::ORDINARY.contains(clock);

    for (
        worker,
        name,
        employment,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_pending,
        route_failed,
    ) in workers.iter_mut()
    {
        let workday_active = workday_active && !release_requested.contains(worker);
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || activity_busy.get(worker).is_ok()
            || road_builder.is_some()
        {
            // Shopping, meals or building work may finish far from the tree.
            // Preserve labour and cargo while recovering through the hut;
            // the interrupted physical work phase is no longer trustworthy.
            routine.phase = LumberjackPhase::ReturningToHut;
            continue;
        }
        if door_transit.is_some() {
            continue;
        }
        // A permit temporarily takes this person's builder time. Construction
        // owns their destination until it releases them back to residency;
        // their ordinary job then resumes with a physical return to the hut.
        if !intent.is_settled() {
            routine.phase = LumberjackPhase::ReturningToHut;
            if *activity != CharacterActivity::Idle {
                activity.set_if_neq(CharacterActivity::Idle);
            }
            continue;
        }
        let Ok((hut, hut_position, hut_rotation, building_id, building_of)) = huts.get(routine.hut)
        else {
            commands.entity(worker).remove::<LumberjackRoutine>();
            super::worker_activity::lifecycle::cancel_travel(&mut commands, worker, None);
            if *activity != CharacterActivity::Idle {
                activity.set_if_neq(CharacterActivity::Idle);
            }
            continue;
        };
        if hut.kind != SettlementBuildingKind::LumberjackHut || employment.0 != *building_id {
            commands.entity(worker).remove::<LumberjackRoutine>();
            super::worker_activity::lifecycle::cancel_travel(
                &mut commands,
                worker,
                interiors.get(worker).ok().filter(|_| intent.is_settled()),
            );
            if *activity != CharacterActivity::Idle {
                activity.set_if_neq(CharacterActivity::Idle);
            }
            continue;
        }
        let Ok(settlement_id) = halls.get(routine.hall) else {
            continue;
        };
        if *settlement_id != building_of.0 {
            continue;
        }

        let hut_entrance = hut.kind.entrance_position(hut_position.0, hut_rotation.0);
        let hut_inside = hut
            .kind
            .interior_door_position(hut_position.0, hut_rotation.0);

        if route_failed.is_some()
            && inventories
                .get_mut(worker)
                .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
        {
            routine.phase = LumberjackPhase::ReturningToHut;
            super::worker_activity::lifecycle::retain_failed_delivery(
                &mut commands,
                worker,
                position.0,
                hut_entrance,
                DOOR_REACH,
                move_target,
                &mut activity,
            );
            continue;
        }

        if let Some(failed) = route_failed.filter(|_| {
            matches!(
                routine.phase,
                LumberjackPhase::GoingToHut | LumberjackPhase::ReturningToHut
            )
        }) {
            routine.failed_hut_routes = routine.failed_hut_routes.saturating_add(1);
            if routine.failed_hut_routes >= MAX_WORKPLACE_ROUTE_FAILURES || !workday_active {
                warn!(
                    "Woodcutter {} could not reach the hut at {:.1},{:.1} after {} routes; ending the empty-handed shift",
                    name.0, failed.goal.x, failed.goal.z, routine.failed_hut_routes,
                );
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    production_day,
                    &*routine,
                    &mut activity,
                );
            } else {
                commands
                    .entity(worker)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .insert(MoveTarget(hut_entrance));
                activity.set_if_neq(CharacterActivity::Idle);
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                LumberjackPhase::Inside { .. } => {
                    activity.set_if_neq(CharacterActivity::Idle);
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        hut_position.0,
                        hut_entrance,
                        hut_inside,
                        hut_entrance,
                    );
                    routine.phase = LumberjackPhase::EndingShift;
                    continue;
                }
                LumberjackPhase::WalkingToTree { .. } | LumberjackPhase::Chopping { .. } => {
                    commands
                        .entity(worker)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    activity.set_if_neq(CharacterActivity::Idle);
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                LumberjackPhase::GoingToHut
                    if ground_distance(position.0, hut_entrance) <= DOOR_REACH =>
                {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                    {
                        routine.phase = LumberjackPhase::ReturningToHut;
                        continue;
                    }
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                LumberjackPhase::EndingShift => {
                    if inventories
                        .get_mut(worker)
                        .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                    {
                        ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                        routine.phase = LumberjackPhase::ReturningToHut;
                        continue;
                    }
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                LumberjackPhase::GoingToHut | LumberjackPhase::ReturningToHut => {}
            }
        }

        match routine.phase {
            LumberjackPhase::GoingToHut => {
                if ground_distance(position.0, hut_entrance) <= DOOR_REACH {
                    routine.failed_hut_routes = 0;
                    if !workday_active {
                        if inventories
                            .get_mut(worker)
                            .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                        {
                            routine.phase = LumberjackPhase::ReturningToHut;
                            continue;
                        }
                        super::worker_activity::lifecycle::finish(
                            &mut commands,
                            worker,
                            production_day,
                            &*routine,
                            &mut activity,
                        );
                        continue;
                    }
                    activity.set_if_neq(CharacterActivity::Idle);
                    begin_workplace_entry(
                        &mut commands,
                        worker,
                        hut_position.0,
                        hut_entrance,
                        hut_inside,
                    );
                    routine.phase = LumberjackPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                }
            }
            LumberjackPhase::Inside { seconds_left } => {
                if *activity != CharacterActivity::Indoors {
                    activity.set_if_neq(CharacterActivity::Indoors);
                }
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = LumberjackPhase::Inside { seconds_left: left };
                    continue;
                }
                if inventories
                    .get(routine.hut)
                    .is_ok_and(|store| store.free_bulk() < Good::Wood.bulk_per_unit())
                {
                    routine.phase = LumberjackPhase::Inside { seconds_left: 0.0 };
                    continue;
                }
                let salt = stable_name_hash(&name.0);
                let (tree, stand) = match find_tree_for_cycle_cached(
                    &mut tree_candidates,
                    &terrain,
                    derived.as_deref(),
                    obstacles.as_deref(),
                    hut_position.0,
                    routine.cycle,
                    salt,
                ) {
                    TreeCandidateLookup::Pending => {
                        // Prop generation is deliberately spread across ticks
                        // to avoid a first-search frame hitch. Remain ready to
                        // leave as soon as the bounded cache finishes.
                        routine.phase = LumberjackPhase::Inside { seconds_left: 0.0 };
                        continue;
                    }
                    TreeCandidateLookup::Unavailable => {
                        // Move through the deterministic pool when a tree has
                        // no collision-free interaction point. Retrying the
                        // same cycle here previously created an indoor loop.
                        routine.cycle = routine.cycle.wrapping_add(1);
                        routine.phase = LumberjackPhase::Inside {
                            seconds_left: INDOOR_REST_SECONDS,
                        };
                        continue;
                    }
                    TreeCandidateLookup::Found { tree, stand } => (tree, stand),
                };
                activity.set_if_neq(CharacterActivity::Idle);
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    hut_position.0,
                    hut_entrance,
                    hut_inside,
                    stand,
                );
                routine.phase = LumberjackPhase::WalkingToTree { tree, stand };
            }
            LumberjackPhase::WalkingToTree { tree, stand } => {
                if let Some(failed) = route_failed {
                    let (cycle, failed_routes, widened) =
                        advance_failed_tree_candidate(routine.cycle, routine.failed_tree_routes);
                    routine.cycle = cycle;
                    routine.failed_tree_routes = failed_routes;
                    if widened {
                        warn!(
                            "Woodcutter {} exhausted twelve tree approaches after {:.1},{:.1}; widening the search instead of stopping",
                            name.0, failed.goal.x, failed.goal.z
                        );
                    }
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>()
                        .insert(MoveTarget(hut_entrance));
                    routine.phase = LumberjackPhase::GoingToHut;
                    activity.set_if_neq(CharacterActivity::Idle);
                    continue;
                }
                if route_pending.is_some_and(NavigationRoutePending::exhausted) {
                    // A coastline or cliff can put the deterministic nearest
                    // tree on a different landmass. Abandon that target and
                    // advance the stable search salt instead of staring at an
                    // exhausted path request forever.
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>();
                    routine.cycle = routine.cycle.wrapping_add(1);
                    routine.phase = LumberjackPhase::GoingToHut;
                    commands.entity(worker).insert(MoveTarget(hut_entrance));
                    activity.set_if_neq(CharacterActivity::Idle);
                    continue;
                }
                if ground_distance(position.0, stand) <= TREE_WORK_REACH {
                    routine.failed_tree_routes = 0;
                    commands.entity(worker).remove::<MoveTarget>();
                    let to_tree = tree - position.0;
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.z);
                    }
                    activity.set_if_neq(CharacterActivity::Chopping);
                    routine.phase = LumberjackPhase::Chopping { tree, stand };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, stand);
                }
            }
            LumberjackPhase::Chopping { tree, stand } => {
                if ground_distance(position.0, stand) > TREE_WORK_REACH {
                    activity.set_if_neq(CharacterActivity::Idle);
                    ensure_move_target(&mut commands, worker, move_target, stand);
                    routine.phase = LumberjackPhase::WalkingToTree { tree, stand };
                    continue;
                }
                if inventories
                    .get(worker)
                    .is_ok_and(|carrier| carrier.free_bulk() < Good::Wood.bulk_per_unit())
                {
                    activity.set_if_neq(CharacterActivity::Idle);
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                if *activity != CharacterActivity::Chopping {
                    activity.set_if_neq(CharacterActivity::Chopping);
                }
                routine.chop_seconds += dt;
                let required_seconds = lumber_seconds_per_tree(hut.quality);
                if routine.chop_seconds < required_seconds {
                    continue;
                }
                let yield_units = lumber_tree_yield(hut.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    let produced = carrier.add(Good::Wood, yield_units);
                    if produced > 0 {
                        // One tree interaction ends at its physical load; the
                        // trip back cannot be banked as work on another tree.
                        routine.chop_seconds = 0.0;
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    if let Ok(mut plan) = operating_plans.get_mut(routine.hut) {
                        plan.record(production_day, produced);
                    }
                    business_events.record_production(production_day, *building_id, produced);
                }
                routine.cycle = routine.cycle.wrapping_add(1);
                activity.set_if_neq(CharacterActivity::Idle);
                commands.entity(worker).insert(MoveTarget(hut_entrance));
                routine.phase = LumberjackPhase::ReturningToHut;
            }
            LumberjackPhase::ReturningToHut => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, hut_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    continue;
                }
                routine.failed_hut_routes = 0;
                commands.entity(worker).remove::<MoveTarget>();
                let unloaded =
                    unload_worker_output(&mut inventories, worker, routine.hut, Good::Wood);
                if !unloaded {
                    activity.set_if_neq(CharacterActivity::Idle);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                if !workday_active {
                    super::worker_activity::lifecycle::finish(
                        &mut commands,
                        worker,
                        production_day,
                        &*routine,
                        &mut activity,
                    );
                    continue;
                }
                activity.set_if_neq(CharacterActivity::Idle);
                begin_workplace_entry(
                    &mut commands,
                    worker,
                    hut_position.0,
                    hut_entrance,
                    hut_inside,
                );
                routine.phase = LumberjackPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            LumberjackPhase::EndingShift => {
                activity.set_if_neq(CharacterActivity::Idle);
                if inventories
                    .get_mut(worker)
                    .is_ok_and(|inventory| inventory.amount(Good::Wood) > 0)
                {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                super::worker_activity::lifecycle::finish(
                    &mut commands,
                    worker,
                    production_day,
                    &*routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Keep the small replicated carried-load view in sync with private inventory.
pub fn sync_carried_load(
    projects: Option<Res<crate::world::house_upgrades::HouseUpgradeProjects>>,
    mut carriers: Query<
        (
            Entity,
            &GoodsInventory,
            &mut CarriedLoad,
            Option<&crate::world::house_upgrades::HouseUpgradeBuilderRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    for (entity, inventory, mut carried, upgrade) in carriers.iter_mut() {
        let next = upgrade
            .and_then(|_| projects.as_ref())
            .and_then(|projects| projects.carried_load_for_worker(entity))
            .unwrap_or_else(|| CarriedLoad::from_inventory(inventory));
        if *carried != next {
            *carried = next;
        }
    }
}

/// Publish the physical handcart only while a porter is performing a freight
/// trip. The empty outbound leg still needs the cart; outside a trip the
/// Moot's dual-role steward stays unburdened between trips. Bridge contracts
/// explicitly borrow a cart; a captain leaves it ashore throughout boarding,
/// sailing and disembarkation.
pub fn sync_porter_cart_state(
    mut commands: Commands,
    porters: Query<
        (
            Entity,
            &GoodsInventory,
            Has<MarketCollectionRoutine>,
            Has<InternalDeliveryRoutine>,
            Has<PorterCargoCapacity>,
            Has<crate::world::regional_roads::bridge::BridgeBuilder>,
            Has<crate::world::shipping::crew::ShipCrew>,
            Option<&shared::economy::PorterCartState>,
        ),
        (
            With<CharacterKind>,
            Or<(
                With<MarketCollectionRoutine>,
                With<InternalDeliveryRoutine>,
                With<PorterCargoCapacity>,
                With<crate::world::regional_roads::bridge::BridgeBuilder>,
                With<crate::world::shipping::crew::ShipCrew>,
                With<shared::economy::PorterCartState>,
            )>,
        ),
    >,
) {
    for (
        entity,
        inventory,
        collecting,
        delivering,
        porter_capacity,
        bridge_builder,
        ship_crew,
        current,
    ) in porters.iter()
    {
        // Keep an abnormal in-flight load visible even if its routine was
        // cancelled. `sync_porter_cargo_capacity` preserves the matching
        // allowance until the goods have safely left the character inventory.
        let active = !ship_crew
            && (collecting
                || delivering
                || bridge_builder
                || (porter_capacity && !inventory.is_empty()));
        if !active {
            if current.is_some() {
                commands
                    .entity(entity)
                    .remove::<shared::economy::PorterCartState>();
            }
            continue;
        }

        let desired = shared::economy::PorterCartState::for_used_bulk(inventory.used_bulk());
        if current.is_none_or(|current| *current != desired) {
            commands.entity(entity).insert(desired);
        }
    }
}

/// Pick a real deterministic tree prop and a collision-free chopping point.
///
/// Tree removal/regrowth needs stable resource-node ids and a sparse depletion
/// map. Until that layer exists, the worker uses the same authored tree the
/// client already draws but does not yet remove it after harvesting.
pub(super) fn advance_failed_tree_candidate(
    mut cycle: u32,
    mut failed_routes: u8,
) -> (u32, u8, bool) {
    failed_routes = failed_routes.saturating_add(1);
    cycle = cycle.wrapping_add(1);
    if failed_routes < 12 {
        return (cycle, failed_routes, false);
    }
    // `cycle` is the deterministic search salt. Jump farther after a whole
    // failed batch so the next search does not immediately revisit the same
    // local candidate set. Resetting the diagnostic counter keeps failure
    // bounded without turning twelve bad trees into a terminal AI state.
    cycle = cycle.wrapping_add(12);
    failed_routes = 0;
    (cycle, failed_routes, true)
}

fn timber_retry_delay(failures: u8) -> f64 {
    let exponent = u32::from(failures.saturating_sub(1)).min(8);
    (TIMBER_RETRY_BASE_SECONDS * 2_f64.powi(exponent as i32)).min(TIMBER_RETRY_MAX_SECONDS)
}

pub(super) fn postpone_construction_tree_search(
    routine: &mut ConstructionMaterialRoutine,
    now: f64,
) -> bool {
    let retry_failures = routine.failed_tree_routes.saturating_add(1);
    let (cycle, failed_routes, widened) =
        advance_failed_tree_candidate(routine.cycle, routine.failed_tree_routes);
    routine.cycle = cycle;
    routine.failed_tree_routes = failed_routes;
    if widened {
        routine.rejected_trees.clear();
    }
    routine.tree_retry_after = now + timber_retry_delay(retry_failures);
    routine.phase = ConstructionMaterialPhase::Seeking;
    widened
}

pub(super) fn postpone_construction_store_route(
    routine: &mut ConstructionMaterialRoutine,
    now: f64,
) {
    routine.failed_store_routes = routine.failed_store_routes.saturating_add(1);
    routine.store_retry_after = now + timber_retry_delay(routine.failed_store_routes);
    routine.phase = ConstructionMaterialPhase::Seeking;
}

pub(super) const TREE_APPROACH_ANGLES: [f32; 8] = [
    0.0,
    std::f32::consts::FRAC_PI_4,
    -std::f32::consts::FRAC_PI_4,
    std::f32::consts::FRAC_PI_2,
    -std::f32::consts::FRAC_PI_2,
    3.0 * std::f32::consts::FRAC_PI_4,
    -3.0 * std::f32::consts::FRAC_PI_4,
    std::f32::consts::PI,
];

/// A failed route must not recreate the same interaction goal on the next
/// visit. `cycle` advances once per failed goal; after the routine has tried
/// every candidate tree, the preferred side of each trunk rotates as well.
pub(super) fn tree_approach_start(cycle: u32, choice_count: usize) -> usize {
    (cycle as usize / choice_count.max(1)) % TREE_APPROACH_ANGLES.len()
}

#[derive(Default)]
struct TreeWorkCandidates {
    spawns: Vec<shared::props::PropSpawn>,
    trees: Vec<usize>,
    blockers: Vec<TreeWorkBlocker>,
    blocker_cells: HashMap<(i32, i32), Vec<usize>>,
    max_blocker_radius: f32,
    pending_chunks: Vec<ChunkCoord>,
    complete: bool,
}

#[derive(Clone, Copy)]
struct TreeWorkBlocker {
    spawn_index: usize,
    center: Vec2,
    radius: f32,
}

const TREE_BLOCKER_CELL_SIZE: f32 = 8.0;

fn tree_blocker_cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / TREE_BLOCKER_CELL_SIZE).floor() as i32,
        (point.y / TREE_BLOCKER_CELL_SIZE).floor() as i32,
    )
}

impl TreeWorkCandidates {
    fn pending(hut: Vec3) -> Self {
        Self {
            pending_chunks: ChunkCoord::from_world_pos(hut).chunks_in_radius(2),
            ..default()
        }
    }

    fn collect(
        terrain: &WorldTerrain,
        hut: Vec3,
        derived: Option<&DerivedColliderLibrary>,
    ) -> Self {
        let mut candidates = Self::pending(hut);
        candidates.advance(terrain, hut, derived, usize::MAX);
        candidates
    }

    fn advance(
        &mut self,
        terrain: &WorldTerrain,
        hut: Vec3,
        derived: Option<&DerivedColliderLibrary>,
        chunk_budget: usize,
    ) {
        if self.complete {
            return;
        }
        for _ in 0..chunk_budget {
            let Some(chunk) = self.pending_chunks.pop() else {
                break;
            };
            self.spawns
                .extend(shared::props::generate_chunk_prop_spawns(
                    &terrain.generator,
                    chunk,
                ));
        }
        if !self.pending_chunks.is_empty() {
            return;
        }

        self.trees = self
            .spawns
            .iter()
            .enumerate()
            .filter(|(_, spawn)| spawn.kind.is_some_and(|kind| kind.is_tree()))
            .filter(|(_, spawn)| {
                let distance =
                    Vec2::new(spawn.position.x - hut.x, spawn.position.z - hut.z).length();
                distance <= TREE_MAX_DISTANCE
            })
            .map(|(index, _)| index)
            .collect();
        self.trees.sort_by(|a, b| {
            let a = self.spawns[*a].position;
            let b = self.spawns[*b].position;
            a.distance_squared(hut)
                .total_cmp(&b.distance_squared(hut))
                .then_with(|| a.x.total_cmp(&b.x))
                .then_with(|| a.z.total_cmp(&b.z))
        });

        let mut blockers = Vec::new();
        let mut blocker_cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut max_blocker_radius = 0.0_f32;
        for (spawn_index, spawn) in self.spawns.iter().enumerate() {
            let Some(kind) = spawn.kind.filter(|kind| kind.blocks_village_road()) else {
                continue;
            };
            let radius = derived
                .and_then(|library| library.by_kind.get(&kind))
                .map_or(0.75, |shape| shape.horizontal_radius)
                * spawn.scale
                + VILLAGER_PROP_RADIUS;
            let blocker_index = blockers.len();
            let center = Vec2::new(spawn.position.x, spawn.position.z);
            blockers.push(TreeWorkBlocker {
                spawn_index,
                center,
                radius,
            });
            blocker_cells
                .entry(tree_blocker_cell(center))
                .or_default()
                .push(blocker_index);
            max_blocker_radius = max_blocker_radius.max(radius);
        }
        self.blockers = blockers;
        self.blocker_cells = blocker_cells;
        self.max_blocker_radius = max_blocker_radius;
        self.complete = true;
    }

    fn stand_overlaps_prop(&self, stand: Vec2, tree_index: usize) -> bool {
        let origin = tree_blocker_cell(stand);
        let cell_radius = (self.max_blocker_radius / TREE_BLOCKER_CELL_SIZE).ceil() as i32 + 1;
        for cell_x in (origin.0 - cell_radius)..=(origin.0 + cell_radius) {
            for cell_z in (origin.1 - cell_radius)..=(origin.1 + cell_radius) {
                let Some(indices) = self.blocker_cells.get(&(cell_x, cell_z)) else {
                    continue;
                };
                for index in indices {
                    let blocker = self.blockers[*index];
                    if blocker.spawn_index != tree_index
                        && blocker.center.distance_squared(stand) < blocker.radius * blocker.radius
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Generated prop candidates are immutable until the future depletion layer
/// exists. Keep the expensive 5x5-chunk enumeration per workplace origin
/// instead of regenerating every tree in those chunks for every retry tick.
#[derive(Default)]
pub(crate) struct TreeWorkCandidateCache {
    by_origin: HashMap<ChunkCoord, TreeWorkCandidates>,
}

impl TreeWorkCandidateCache {
    fn candidates<'a>(
        &'a mut self,
        terrain: &WorldTerrain,
        derived: Option<&DerivedColliderLibrary>,
        hut: Vec3,
    ) -> Option<&'a TreeWorkCandidates> {
        const CHUNKS_PER_SEARCH_TICK: usize = 1;
        const MAX_CACHED_ORIGINS: usize = 512;
        let origin = ChunkCoord::from_world_pos(hut);
        if self.by_origin.len() >= MAX_CACHED_ORIGINS && !self.by_origin.contains_key(&origin) {
            self.by_origin.clear();
        }
        let candidates = self
            .by_origin
            .entry(origin)
            .or_insert_with(|| TreeWorkCandidates::pending(hut));
        candidates.advance(terrain, hut, derived, CHUNKS_PER_SEARCH_TICK);
        candidates.complete.then_some(candidates)
    }
}

pub(super) enum TreeCandidateLookup {
    Pending,
    Unavailable,
    Found { tree: Vec3, stand: Vec3 },
}

fn tree_work_position(
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    candidates: &TreeWorkCandidates,
    tree_index: usize,
    choice_count: usize,
) -> Option<(Vec3, Vec3)> {
    let tree_spawn = &candidates.spawns[tree_index];
    let tree = tree_spawn.position;
    let toward_hut = Vec2::new(hut.x - tree.x, hut.z - tree.z).normalize_or(Vec2::Y);
    let tree_radius = tree_spawn
        .kind
        .and_then(|kind| derived.and_then(|library| library.by_kind.get(&kind)))
        .map_or(0.75, |shape| shape.horizontal_radius)
        * tree_spawn.scale;
    let stand_distance = (tree_radius + VILLAGER_PROP_RADIUS + 0.12).max(1.1);

    // The natural first choice faces back toward the workplace. If its route
    // fails, the next pass starts on a different side of the trunk instead of
    // returning the identical locally-clear but globally-unreachable goal.
    // The ordering remains deterministic for reproducible simulation runs.
    let approach_start = tree_approach_start(cycle, choice_count);
    for step in 0..TREE_APPROACH_ANGLES.len() {
        let angle = TREE_APPROACH_ANGLES[(approach_start + step) % TREE_APPROACH_ANGLES.len()];
        let (sin, cos) = angle.sin_cos();
        let direction = Vec2::new(
            toward_hut.x * cos - toward_hut.y * sin,
            toward_hut.x * sin + toward_hut.y * cos,
        );
        let stand_xz = Vec2::new(tree.x, tree.z) + direction * stand_distance;
        if obstacles.is_some_and(|grid| grid.point_blocked(stand_xz)) {
            continue;
        }
        if candidates.stand_overlaps_prop(stand_xz, tree_index) {
            continue;
        }
        let stand = Vec3::new(
            stand_xz.x,
            terrain.get_height(stand_xz.x, stand_xz.y),
            stand_xz.y,
        );
        return Some((tree, stand));
    }
    None
}

fn find_tree_in_candidates(
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    salt: u32,
    candidates: &TreeWorkCandidates,
) -> Option<(Vec3, Vec3)> {
    // Keep a comfortably large deterministic candidate pool. Sparse coastal
    // groves may only contain two usable trees, while a dense forest should
    // not trap all workers on the same nearest twelve trunks.
    let choice_count = candidates.trees.len().min(48);
    if choice_count == 0 {
        return None;
    }
    let tree_index = candidates.trees[(cycle.wrapping_add(salt) as usize) % choice_count];
    tree_work_position(
        terrain,
        derived,
        obstacles,
        hut,
        cycle,
        candidates,
        tree_index,
        choice_count,
    )
}

pub(super) fn find_tree_for_cycle_cached(
    cache: &mut TreeWorkCandidateCache,
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    salt: u32,
) -> TreeCandidateLookup {
    let Some(candidates) = cache.candidates(terrain, derived, hut) else {
        return TreeCandidateLookup::Pending;
    };
    match find_tree_in_candidates(terrain, derived, obstacles, hut, cycle, salt, candidates) {
        Some((tree, stand)) => TreeCandidateLookup::Found { tree, stand },
        None => TreeCandidateLookup::Unavailable,
    }
}

/// Find a deterministic, collision-safe top-up tree close to the carrier.
///
/// Scan a bounded local pool, excluding the trunk just used and approaches
/// rejected by navigation. The salt only breaks equal-distance ties; it must
/// never send an empty builder past nearby timber to a random distant tree.
pub(super) fn find_nearby_tree_for_cycle_cached(
    cache: &mut TreeWorkCandidateCache,
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    carrier: Vec3,
    excluded_tree: Option<Vec3>,
    rejected_trees: &[Vec3],
    colliders: Option<&StaticColliders>,
    cycle: u32,
    salt: u32,
) -> TreeCandidateLookup {
    let Some(candidates) = cache.candidates(terrain, derived, hut) else {
        return TreeCandidateLookup::Pending;
    };
    let choice_count = candidates.trees.len().min(48);
    if choice_count == 0 {
        return TreeCandidateLookup::Unavailable;
    }

    let start = (cycle.wrapping_add(salt) as usize) % choice_count;
    let mut nearest: Option<(f32, usize, Vec3, Vec3)> = None;
    for offset in 0..choice_count {
        let order = (start + offset) % choice_count;
        let tree_index = candidates.trees[order];
        let tree = candidates.spawns[tree_index].position;
        if colliders.and_then(|colliders| colliders.tree_is_present(tree)) == Some(false) {
            continue;
        }
        if excluded_tree.is_some_and(|excluded| tree.distance_squared(excluded) < 0.01)
            || rejected_trees
                .iter()
                .any(|rejected| tree.distance_squared(*rejected) < 0.01)
        {
            continue;
        }
        let Some((tree, stand)) = tree_work_position(
            terrain,
            derived,
            obstacles,
            hut,
            cycle,
            candidates,
            tree_index,
            choice_count,
        ) else {
            continue;
        };
        let distance_squared = Vec2::new(carrier.x - stand.x, carrier.z - stand.z).length_squared();
        let is_nearer = nearest.as_ref().is_none_or(|(best, best_order, ..)| {
            distance_squared.total_cmp(best).is_lt()
                || (distance_squared.total_cmp(best).is_eq() && offset < *best_order)
        });
        if is_nearer {
            nearest = Some((distance_squared, offset, tree, stand));
        }
    }

    match nearest {
        Some((_, _, tree, stand)) => TreeCandidateLookup::Found { tree, stand },
        None => TreeCandidateLookup::Unavailable,
    }
}

/// Permit-time proof that a lumber workplace has at least one usable tree on
/// its own walkable landmass. Resource density alone is insufficient on a
/// coast: a tree 30 metres away across a channel is not timber supply.
///
/// Candidate props are generated once, and only the nearest bounded set pays a
/// terrain-only route check. This runs when the plot decision changes, not per
/// villager or simulation tick.
pub(crate) fn lumber_plot_has_reachable_tree(terrain: &WorldTerrain, hut: Vec3) -> bool {
    const PERMIT_TREE_CANDIDATES: usize = 12;
    let candidates = TreeWorkCandidates::collect(terrain, hut, None);
    let attempts = candidates.trees.len().min(PERMIT_TREE_CANDIDATES);
    (0..attempts).any(|cycle| {
        find_tree_in_candidates(terrain, None, None, hut, cycle as u32, 0, &candidates).is_some_and(
            |(_, stand)| {
                crate::world::village_roads::embodied_land_route_exists(terrain, hut, stand)
            },
        )
    })
}

pub(super) fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

fn farm_work_stand(
    field: Vec3,
    rotation: f32,
    worker_salt: u32,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<Vec3> {
    let side = if worker_salt & 1 == 0 { -1.5 } else { 1.5 };
    let candidates = [
        Vec2::new(side, 0.0),
        Vec2::new(-side, 0.0),
        Vec2::new(0.0, 2.0),
        Vec2::new(0.0, -2.0),
        Vec2::new(side, 2.5),
        Vec2::new(-side, 2.5),
        Vec2::new(side, -2.5),
        Vec2::new(-side, -2.5),
    ];
    candidates.into_iter().find_map(|local| {
        let offset = shared::rotation::local_to_world_xz(local, rotation);
        let point = Vec2::new(field.x + offset.x, field.z + offset.y);
        if obstacles.is_some_and(|obstacles| obstacles.point_blocked(point)) {
            return None;
        }
        if colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::navigation_point_is_clear_of_props(
                point, colliders, derived,
            )
        }) {
            return None;
        }
        Some(Vec3::new(point.x, field.y, point.y))
    })
}

pub(super) fn build_clip_facing(to_work: Vec3) -> f32 {
    f32::atan2(-to_work.x, -to_work.z)
}

/// A collision-safe point beyond an authored threshold.
///
/// The door anchor sits only a few centimetres outside some inflated building
/// footprints. Stopping merely within `DOOR_REACH` of that anchor can therefore
/// leave an actor inside the blocker forever. Door-controlled movement may
/// cross the wall, so finish the crossing a short distance beyond the anchor
/// before ordinary navigation takes ownership again.
pub(super) fn exterior_door_clearance_position(building: Vec3, door: Vec3) -> Vec3 {
    let outward = Vec2::new(door.x - building.x, door.z - building.z).normalize_or_zero();
    Vec3::new(
        door.x + outward.x * (DOOR_REACH + 0.35),
        door.y,
        door.z + outward.y * (DOOR_REACH + 0.35),
    )
}

/// Move one trade's physical output from its worker into the owning workplace.
/// `false` means cargo remains (normally because finite workplace storage is
/// full), so callers must retain the work routine rather than clocking off.
fn unload_worker_output(
    inventories: &mut Query<&mut GoodsInventory>,
    worker: Entity,
    workplace: Entity,
    good: Good,
) -> bool {
    let Ok([mut carrier, mut store]) = inventories.get_many_mut([worker, workplace]) else {
        return false;
    };
    carrier.transfer_to(&mut store, good, u32::MAX);
    carrier.amount(good) == 0
}
