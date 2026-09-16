//! Initial conditions and read-only evidence for the connected worker lab.
//! After staging, only ordinary hiring, navigation and production mutate workers.

use super::*;
use shared::components::{BuildingId, BuildingOf, CharacterKind, EmployedAt, PersonId};
use shared::economy::{
    BusinessManagementPolicy, BusinessSalePolicy, BusinessStaffingPolicy, BusinessWagePolicy,
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

const NAME: &str = "Worker Lifecycle Lab";

#[derive(Component)]
pub(crate) struct FixturePerson;

#[derive(Component)]
pub(crate) struct FixtureWorkplace {
    work_point: Vec3,
    entrance: Vec3,
}

#[derive(Resource)]
pub(crate) struct WorkerTrace {
    output: BufWriter<File>,
    started: Instant,
    sampled: Instant,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn stage(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    villager_seed: &mut crate::world::dev::VillagerSeed,
    ids: &mut crate::world::identity::WorldIdAllocator,
    mut colliders: Option<&mut crate::collision::library::StaticColliders>,
    derived: Option<&crate::collision::library::DerivedColliderLibrary>,
) {
    let (hall, _, fish_position, fish_rotation, _, _) = choose_secure_site(terrain);
    let settlement_id = ids.settlement();
    let mut pantry = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
    pantry.add(Good::Bread, 32);
    let mut market = MootMarket::founding();
    market.consign(
        MarketSeller::Treasury(settlement_id),
        Good::Bread,
        32,
        Good::Bread.base_price(),
    );
    let hall_entity = commands
        .spawn((
            settlement_id,
            Settlement {
                name: NAME.into(),
                tier: SettlementTier::Hamlet,
                residents: 8,
                treasury: shared::economy::STARTING_TREASURY_MONEY,
            },
            pantry,
            market,
            shared::components::SettlementPolicies::default(),
            PlayerPosition(hall),
            PlayerRotation(0.0),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    let mut people = Vec::new();
    for index in 0..8 {
        villager_seed.0 = villager_seed.0.wrapping_add(1);
        let door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
        let requested = door
            + Vec3::new(
                (index % 4) as f32 * 1.5 - 2.25,
                0.0,
                -2.0 - (index / 4) as f32 * 1.5,
            );
        let position = crate::world::dev::safe_villager_spawn_position(
            requested,
            villager_seed.0,
            terrain,
            None,
            colliders.as_deref(),
            derived,
        )
        .expect("worker lab needs a safe initial resident position");
        let person = ids.person();
        let entity =
            crate::player::hero::spawn_villager(commands, terrain, villager_seed.0, position);
        commands.entity(entity).insert((
            person,
            FixturePerson,
            Residence(NAME.into()),
            shared::components::ResidentOf(settlement_id),
            village::VillagerIntent::Resident {
                settlement: hall_entity,
            },
        ));
        people.push(person);
    }

    let mut occupied = vec![(hall, SettlementBuildingKind::Hall.clearance())];
    let mut roads = Vec::<VillageRoad>::new();
    let mut blockers = Vec::new();
    let mut manifest_sites = Vec::new();
    for (index, kind) in [
        SettlementBuildingKind::FishermansHut,
        SettlementBuildingKind::LivestockFarm,
    ]
    .into_iter()
    .enumerate()
    {
        let road_refs: Vec<_> = roads.iter().collect();
        let candidates: Vec<_> = if kind == SettlementBuildingKind::FishermansHut {
            // The coarse shoreline search proves dry ground and fishing water,
            // but not the full doorway/road/permanent-prop contract. Certify
            // nearby shore poses instead of treating its first answer as final.
            std::iter::once((fish_position, fish_rotation))
                .chain(
                    [2.0_f32, 4.0, 6.0, 8.0, 12.0, 18.0]
                        .into_iter()
                        .flat_map(|radius| {
                            (0..16).flat_map(move |bearing| {
                                let angle = bearing as f32 * std::f32::consts::TAU / 16.0;
                                let x = fish_position.x + radius * angle.cos();
                                let z = fish_position.z + radius * angle.sin();
                                let position = Vec3::new(x, terrain.get_height(x, z), z);
                                [0.0_f32, -15.0, 15.0, -30.0, 30.0]
                                    .into_iter()
                                    .map(move |turn| (position, fish_rotation + turn.to_radians()))
                            })
                        }),
                )
                .collect()
        } else {
            [40.0_f32, 55.0, 72.0, 92.0, 120.0, 155.0, 190.0]
                .into_iter()
                .flat_map(|radius| {
                    (0..32).map(move |step| {
                        let angle = step as f32 * std::f32::consts::TAU / 32.0;
                        let x = hall.x + radius * angle.cos();
                        let z = hall.z + radius * angle.sin();
                        let toward_hall = Vec2::new(hall.x - x, hall.z - z).normalize_or_zero();
                        (
                            Vec3::new(x, terrain.get_height(x, z), z),
                            (-toward_hall.x).atan2(-toward_hall.y),
                        )
                    })
                })
                .collect()
        };
        let mut refusals = std::collections::BTreeMap::<String, usize>::new();
        let mut attempts = 0;
        let approval = candidates
            .into_iter()
            .find_map(|(position, rotation)| {
                attempts += 1;
                let approval = village::validate_manual_plot(
                    terrain,
                    hall,
                    kind,
                    position,
                    rotation,
                    &occupied,
                    &road_refs,
                    &[],
                    &blockers,
                    colliders.as_deref(),
                    derived,
                    None,
                    &[],
                ).and_then(|plot| {
                    if plot.road_access.len() < 2 {
                        return Err("Validated plot has no physical road connector".into());
                    }
                    if colliders.as_deref().zip(derived).is_some_and(|(props, shapes)| {
                        !crate::world::village_roads::road_access_is_clear_of_permanent_props(
                            &plot.road_access, RoadClass::Lane.initial_reserved_width(), props, shapes,
                        )
                    }) {
                        return Err("A permanent object blocks the completed road reservation".into());
                    }
                    Ok(plot)
                });
                match approval {
                    Ok(plot) => Some(plot),
                    Err(reason) => {
                        if attempts <= 3 {
                            info!(?kind, ?position, rotation, %reason, "Worker fixture candidate refused");
                        }
                        *refusals.entry(reason).or_default() += 1;
                        None
                    }
                }
            })
            .unwrap_or_else(|| panic!("Worker fixture has no valid {kind:?} site near Hall {hall:?} after {attempts} candidates: {refusals:?}"));
        info!(?kind, position = ?approval.position, rotation = approval.rotation,
              attempts, ?refusals, "Worker fixture certified workplace and complete road");
        if let (Some(props), Some(shapes)) = (colliders.as_deref_mut(), derived) {
            crate::world::village_roads::clear_completed_road_trees(
                &approval.road_access,
                2.5,
                props,
                shapes,
            );
        }
        let building_id = ids.building();
        let company = ids.company();
        commands.spawn(village::new_company_bundle(
            company,
            format!("Worker Lab {}", kind.label()),
            0,
            people[index],
            50 * PENNIES_PER_COIN,
            50 * PENNIES_PER_COIN,
        ));
        let mut work_point = if kind == SettlementBuildingKind::FishermansHut {
            let mut pier = kind
                .pier_position(approval.position, approval.rotation)
                .expect("fishing pier anchor");
            pier.y = terrain.water_level().expect("worker lab coastline");
            let offset =
                shared::rotation::local_to_world_xz(Vec2::new(0.0, 6.25), approval.rotation);
            Vec3::new(pier.x + offset.x, pier.y + 0.52, pier.z + offset.y)
        } else {
            kind.pasture_position(approval.position, approval.rotation)
                .expect("livestock pasture")
        };
        if kind == SettlementBuildingKind::LivestockFarm {
            work_point.y = terrain.get_height(work_point.x, work_point.z);
        }
        let entrance = kind.entrance_position(approval.position, approval.rotation);
        let building = commands
            .spawn((
                building_id,
                BuildingOf(settlement_id),
                shared::components::OwnedBy(people[index]),
                shared::components::OperatedBy(company),
                SettlementBuilding {
                    kind,
                    settlement: NAME.into(),
                    owner: None,
                    quality: approval.quality,
                    workers: vec![],
                },
                GoodsInventory::new(kind.storage_bulk_capacity()),
                BusinessAccount::default(),
                BusinessManagementPolicy {
                    autopilot: false,
                    automatic_withdrawals: false,
                    ..default()
                },
                BusinessStaffingPolicy::new(1),
                BusinessWagePolicy {
                    daily_wage: 150,
                    automatic: false,
                    ..default()
                },
                BusinessSalePolicy {
                    collection_enabled: false,
                    ..default()
                },
                PlayerPosition(approval.position),
                PlayerRotation(approval.rotation),
                shared::building::PlacedBuilding {
                    building_type: kind.art(),
                    rotation: approval.rotation,
                },
                shared::building::BuildingPosition(approval.position),
            ))
            .id();
        commands.entity(building).insert((
            FixtureWorkplace {
                work_point,
                entrance,
            },
            Replicate::to_clients(NetworkTarget::All),
        ));
        let road = VillageRoad {
            settlement: NAME.into(),
            builder: "Initial worker fixture".into(),
            built_through: approval.road_access.len() as u16,
            points: approval.road_access,
            width: 2.5,
            reserved_width: RoadClass::Lane.initial_reserved_width(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };
        commands.spawn((
            road.clone(),
            shared::components::RoadOf(settlement_id),
            crate::world::village_roads::RoadConnectorFor { building },
            Replicate::to_clients(NetworkTarget::All),
        ));
        roads.push(road);
        blockers.extend(village::road_access_blockers_for_plot(
            kind,
            approval.position,
            approval.rotation,
        ));
        occupied.push((approval.position, kind.clearance()));
        manifest_sites.push(serde_json::json!({"id":building_id.0,"kind":format!("{kind:?}"),"position":approval.position.to_array(),"work_point":work_point.to_array(),"entrance":entrance.to_array(),"quality":approval.quality,"initial_stock":0}));
    }
    let directory = std::env::var_os("FISTWORLD_WORKER_TRACE_DIR")
        .expect("worker-lifecycle requires FISTWORLD_WORKER_TRACE_DIR");
    std::fs::create_dir_all(&directory).expect("worker trace directory");
    std::fs::write(std::path::Path::new(&directory).join("fixture.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "scenario":"worker-lifecycle","hall":hall.to_array(),"people":people.iter().map(|p|p.0).collect::<Vec<_>>(),"sites":manifest_sites,
        "initial_employment":0,"initial_bread":32,"initial_company_cash":10000,"initial_person_cash":8000,"initial_treasury":shared::economy::STARTING_TREASURY_MONEY,
        "scope":"Initial valid workplaces, roads, finite food/cash and eight residents. No production grants or worker orders after staging. Manual one-position site policies retain output for deposit evidence."
    })).unwrap()).expect("worker fixture manifest");
    commands.insert_resource(WorkerTrace {
        output: BufWriter::new(
            File::create(std::path::Path::new(&directory).join("workers.jsonl"))
                .expect("worker trace file"),
        ),
        started: Instant::now(),
        sampled: Instant::now() - Duration::from_secs(1),
    });
    info!(
        "Worker lifecycle fixture ready: two empty workplaces, eight unemployed residents, ordinary hiring and physical production"
    );
}

#[allow(clippy::type_complexity)]
pub(crate) fn sample(
    trace: Option<ResMut<WorkerTrace>>,
    clock: Query<(&WorldTime, &TimeWarp)>,
    sites: Query<(
        &BuildingId,
        &SettlementBuilding,
        &GoodsInventory,
        &FixtureWorkplace,
    )>,
    workers: Query<
        (
            &PersonId,
            &PlayerPosition,
            Option<&EmployedAt>,
            Option<&CharacterActivity>,
            Option<&shared::components::CharacterObjective>,
            &GoodsInventory,
            Option<&crate::player::hero::MoveTarget>,
            Option<&village::FishingRoutine>,
            Option<&village::QuarryRoutine>,
        ),
        (With<FixturePerson>, With<CharacterKind>),
    >,
) {
    let Some(mut trace) = trace else {
        return;
    };
    let (day, world_seconds, warp) = clock
        .iter()
        .next()
        .map_or((0, 0.0, 1.0), |(c, w)| (c.day, c.seconds_in_cycle, w.0));
    // This system is chained after production/deposit routines and before
    // Navigation's step_units. At accelerated time the worker can leave the
    // store between wall-clock samples, so retain every simulated tick and
    // witness its position on the actual inventory-transfer tick. This is
    // opt-in lab evidence only; normal play never inserts WorkerTrace.
    if warp <= 1.0 && trace.sampled.elapsed() < Duration::from_millis(250) {
        return;
    }
    trace.sampled = Instant::now();
    let sites: Vec<_> = sites.iter().map(|(id, building, stock, geometry)| serde_json::json!({
        "id":id.0,"kind":format!("{:?}",building.kind),"work_point":geometry.work_point.to_array(),"entrance":geometry.entrance.to_array(),
        "fish":stock.amount(Good::Food),"meat":stock.amount(Good::Meat),"wool":stock.amount(Good::Wool)
    })).collect();
    let actors: Vec<_> = workers.iter().map(|(id, at, job, activity, objective, cargo, target, fishing, herding)| serde_json::json!({
        "id":id.0,"position":at.0.to_array(),"workplace":job.map(|job|job.0.0),"activity":activity,"objective":objective,"target":target.map(|t|t.0.to_array()),
        "fish":cargo.amount(Good::Food),"meat":cargo.amount(Good::Meat),"wool":cargo.amount(Good::Wool),"simulation":"canonical",
        "routine":fishing.map(|r|format!("{r:?}")).or_else(||herding.map(|r|format!("{r:?}")))
    })).collect();
    let row = serde_json::json!({"elapsed":trace.started.elapsed().as_secs_f64(),"day":day,"world_seconds":world_seconds,"warp":warp,"sample_stage":"after_work_before_movement","sites":sites,"actors":actors});
    writeln!(trace.output, "{row}").expect("worker trace write");
    trace.output.flush().expect("worker trace flush");
}
