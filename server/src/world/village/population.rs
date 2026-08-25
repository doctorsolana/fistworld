//! Settlement discovery, migration, arrival and authoritative resident counts.

use super::*;

/// A served immigrant is not a resident until they have physically cleared
/// the Hall counter.
///
/// The ready queue ticket deliberately remains attached while this component
/// exists. That keeps this person at the head of the FIFO line, so the next
/// applicant cannot walk into the same service point while route planning is
/// busy. Keeping the intent as `Travelling` also prevents household, work,
/// tavern and ambient schedulers from stealing the departure destination.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ImmigrationDeparture {
    settlement: Entity,
    service_serial: u64,
    destination: Vec3,
    failed_routes: u8,
}

const IMMIGRATION_DEPARTURE_REACH: f32 = 0.60;
const MAX_IMMIGRATION_DEPARTURE_ROUTE_FAILURES: u8 = 12;

/// Whether an embodied migrant has reached the permanently clear forecourt in
/// front of the civic Hall.
///
/// Every Hall level shares one authored door and front face while growing
/// backward. Use the largest supported shell plus the villager body clearance
/// as the boundary, so the founding Moot cannot accept someone from inside the
/// space reserved for its eventual Town Hall. The broad horizontal radius
/// still lets a crowded arrival join from either side of the visible line.
fn reached_visible_moot_forecourt(position: Vec3, hall: Vec3, rotation_y: f32) -> bool {
    if ground_distance(position, hall) > ARRIVAL_RADIUS {
        return false;
    }

    let local = shared::rotation::world_to_local_xz(
        Vec2::new(position.x - hall.x, position.z - hall.z),
        rotation_y,
    );
    let reserved = shared::components::CivicHallLevel::largest_supported()
        .building_type()
        .definition();
    let reserved_front = reserved.footprint_center.y
        - reserved.footprint.y * 0.5
        - crate::world::navgrid::VILLAGER_NAV_RADIUS;
    local.y <= reserved_front
}

/// Give every villager an intent, so the rest of the module can assume one.
///
/// Polls rather than reacting to `Added`, for the reason this codebase has now
/// been bitten by repeatedly: components arrive in separate batches and a
/// one-shot on `Added` misses whoever was assembled late.
pub fn tag_villager_intent(
    mut commands: Commands,
    villagers: Query<
        (
            Entity,
            &CharacterKind,
            Option<&VillagerIntent>,
            Option<&Occupation>,
            Option<&GoodsInventory>,
            Option<&CharacterActivity>,
            Option<&CarriedLoad>,
            Option<&Nutrition>,
            Option<&WorkStatus>,
            (
                Option<&shared::components::CommandedBy>,
                Option<&crate::player::combat::WarParty>,
            ),
        ),
        (With<CharacterName>, With<PlayerPosition>),
    >,
) {
    for (
        entity,
        kind,
        intent,
        occupation,
        inventory,
        activity,
        carried,
        nutrition,
        work_status,
        (commanded, war_party),
    ) in villagers.iter()
    {
        // Heroes are players' bodies and join nothing on their own.
        if *kind != CharacterKind::Villager {
            continue;
        }
        // Combatants are DISCHARGED from village life: conscription (and a
        // raider's war banner) strips or forgoes the intent on purpose, and
        // re-seeding it here would hand the body back to the village brain
        // one tick later. The physical backfills below (inventory,
        // nutrition, activity) still apply - a soldier eats like anyone.
        let conscripted = commanded.is_some() || war_party.is_some();
        let mut entity_commands = commands.entity(entity);
        if intent.is_none() && !conscripted {
            entity_commands.insert(VillagerIntent::Idle);
        }
        // These are backfilled as well as attached by the normal spawn path so
        // old saves and focused tests cannot create bottomless or visually
        // ambiguous villagers.
        if occupation.is_none() {
            entity_commands.insert(Occupation::default());
        }
        if inventory.is_none() {
            entity_commands.insert(GoodsInventory::new(shared::economy::capacity::VILLAGER));
        }
        if activity.is_none() {
            entity_commands.insert(CharacterActivity::Idle);
        }
        if carried.is_none() {
            entity_commands.insert(CarriedLoad::default());
        }
        if nutrition.is_none() {
            entity_commands.insert(Nutrition::default());
        }
        if work_status.is_none() && !conscripted {
            entity_commands.insert(if occupation.is_some_and(|value| value.0.is_some()) {
                WorkStatus::Employed
            } else {
                WorkStatus::LookingForWork
            });
        }
    }
}

/// Uncommitted villagers pick somewhere to live and start walking.
pub fn seek_settlement(
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut clock: ResMut<VillageClock>,
    mut commands: Commands,
    settlements: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    // Conscripts are excluded outright: even one who somehow regains an Idle
    // intent must never be marched off to immigrate mid-battle.
    mut villagers: Query<
        (
            Entity,
            &PlayerPosition,
            &mut VillagerIntent,
            Option<&MigrationCooldown>,
        ),
        Without<shared::components::CommandedBy>,
    >,
    road_graph: Option<Res<VillageRoadGraph>>,
) {
    // Migration admission is a CPU-facing decision queue. It deliberately
    // follows real time rather than world warp: 25x should make an admitted
    // person walk faster, not create 250 long A* requests on one tick.
    if simulation_time.factor() <= 0.0 {
        return;
    }
    clock.seek += simulation_time.real_seconds();
    if clock.seek < SEEK_INTERVAL {
        return;
    }
    clock.seek = (clock.seek - SEEK_INTERVAL).min(SEEK_INTERVAL);

    let now = simulation_time.elapsed_real_seconds_f64();
    let mut admitted = 0usize;

    for (entity, position, mut intent, cooldown) in villagers.iter_mut() {
        if admitted >= MAX_MIGRATION_ADMISSIONS_PER_PASS {
            break;
        }
        if !matches!(*intent, VillagerIntent::Idle) {
            continue;
        }
        // Nearest non-ruined settlement. Ruins have no hall to walk to and
        // nobody to join.
        let nearest = settlements
            .iter()
            .filter(|(_, settlement, _, _)| {
                settlement.tier != shared::components::SettlementTier::Ruins
            })
            .filter(|(entity, _, hall, rotation)| {
                cooldown.is_none_or(|cooldown| {
                    let entrance = SettlementBuildingKind::Hall
                        .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
                    let opportunity = road_graph.as_deref().map_or(0, |graph| {
                        graph.cohort_route_opportunity_version(Vec2::new(entrance.x, entrance.z))
                    });
                    !cooldown.blocks(*entity, now, opportunity)
                })
            })
            .min_by(|a, b| {
                a.2 .0
                    .distance_squared(position.0)
                    .total_cmp(&b.2 .0.distance_squared(position.0))
            });
        // No settlement anywhere: stay idle and look again next tick. A villager
        // with nowhere to go is a real state, not an error.
        let Some((settlement_entity, _, hall, rotation)) = nearest else {
            continue;
        };
        if cooldown.is_some_and(|cooldown| cooldown.settlement != settlement_entity) {
            commands.entity(entity).remove::<MigrationCooldown>();
        }
        *intent = VillagerIntent::Travelling {
            settlement: settlement_entity,
        };
        admitted += 1;
        // The hall centre is inside its solid navigation footprint. Before
        // authored building obstacles existed, walking there happened to work;
        // now it correctly produces no route and can strand every founder in
        // `Travelling`. The authored entrance is both reachable and still
        // inside ARRIVAL_RADIUS of the settlement origin.
        let entrance = SettlementBuildingKind::Hall
            .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
        commands.entity(entity).insert(MoveTarget(entrance));
    }
}

/// Villagers who reached their hall become residents of that settlement.
pub fn arrive_at_settlement(
    mut commands: Commands,
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    halls: Query<(&PlayerPosition, &Settlement, Option<&PlayerRotation>), With<Settlement>>,
    mut villagers: Query<(
        Entity,
        &PlayerPosition,
        &mut VillagerIntent,
        Option<&MoveTarget>,
        Option<&NavigationRouteFailed>,
        Option<&MigrationCooldown>,
        Option<&MootQueueTicket>,
        Has<strategic::StrategicPerson>,
        Has<ImmigrationDeparture>,
    )>,
    road_graph: Option<Res<VillageRoadGraph>>,
) {
    let now = simulation_time.elapsed_real_seconds_f64();
    // Departure destinations are small physical reservations, not abstract
    // labels. Include every embodied person already occupying the apron so a
    // blocked candidate search cannot make two different service serials
    // converge on the same first valid point.
    let mut occupied_departure_places = villagers
        .iter()
        .map(|(_, position, ..)| Vec2::new(position.0.x, position.0.z))
        .collect::<Vec<_>>();
    for (
        entity,
        position,
        mut intent,
        move_target,
        route_failed,
        cooldown,
        queue_ticket,
        strategic,
        departing,
    ) in villagers.iter_mut()
    {
        let VillagerIntent::Travelling { settlement } = *intent else {
            continue;
        };
        // The settlement went away mid-journey: go back to looking.
        let Ok((hall, place, rotation)) = halls.get(settlement) else {
            *intent = VillagerIntent::Idle;
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<MootQueueTicket>()
                .remove::<ImmigrationDeparture>()
                .remove::<Residence>();
            continue;
        };

        // The served applicant remains a tactical migrant and retains their
        // ready queue ticket until `advance_immigration_departures` observes
        // them at the clear exit. Do not enqueue them again or let this pass
        // replace that protected destination.
        if departing {
            continue;
        }

        // Once the tactical migrant reaches the hall, the visible Moot line
        // owns their movement. Some queue slots are deliberately farther than
        // ARRIVAL_RADIUS from the hall, so ordinary migration repair must not
        // pull a waiting applicant out of the line.
        if let Some(ticket) = queue_ticket {
            if ticket.hall == settlement && ticket.kind == MootServiceKind::Immigration {
                if !ticket.is_ready() {
                    continue;
                }
                if strategic {
                    finish_immigration(&mut commands, entity, &mut intent, settlement, place);
                } else {
                    let destination = begin_immigration_departure(
                        &mut commands,
                        entity,
                        settlement,
                        ticket.serial(),
                        position.0,
                        hall.0,
                        rotation.map_or(0.0, |rotation| rotation.0),
                        terrain.as_deref(),
                        obstacles.as_deref(),
                        colliders.as_deref(),
                        derived.as_deref(),
                        &occupied_departure_places,
                    );
                    occupied_departure_places.push(Vec2::new(destination.x, destination.z));
                }
                continue;
            }
            // A travelling villager cannot legitimately use another hall
            // service yet. Let that authoritative service finish instead of
            // fighting its queue destination.
            continue;
        }
        let hall_rotation = rotation.map_or(0.0, |rotation| rotation.0);
        let entrance = SettlementBuildingKind::Hall.entrance_position(hall.0, hall_rotation);
        // Strategic migrants have no embodied forecourt. Focused tests without
        // the queue resource likewise preserve the old cheap radial handoff.
        // A visible migrant must actually reach the permanently clear area in
        // FRONT of the Hall; entering the radius from behind must not cancel
        // the certified route that is taking them around the building.
        let reached_arrival = if strategic || queue_clock.is_none() {
            ground_distance(position.0, hall.0) <= ARRIVAL_RADIUS
        } else {
            reached_visible_moot_forecourt(position.0, hall.0, hall_rotation)
        };
        if !reached_arrival {
            if let Some(failed) = route_failed {
                debug!(
                    "Villager could not migrate to '{}' through {:.1},{:.1}; reconsidering after cooldown",
                    place.name, failed.goal.x, failed.goal.z
                );
                *intent = VillagerIntent::Idle;
                let cohort_opportunity_version = road_graph.as_deref().map_or(0, |graph| {
                    graph.cohort_route_opportunity_version(Vec2::new(entrance.x, entrance.z))
                });
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .insert(MigrationCooldown::after_failure(
                        cooldown.copied(),
                        settlement,
                        now,
                        cohort_opportunity_version,
                    ));
                continue;
            }
            // Repair old saves/live entities that still point at the solid hall
            // centre, and recover if any other system removed the journey. A
            // changed target also wakes the bounded route planner after its
            // previous blocked-route retry limit.
            ensure_move_target(&mut commands, entity, move_target, entrance);
            continue;
        }

        // Strategic migrants have no embodied forecourt to display. Tactical
        // migrants register through the same FIFO line used by permits and
        // food, becoming residents only when the counter serves them.
        if strategic || queue_clock.is_none() {
            finish_immigration(&mut commands, entity, &mut intent, settlement, place);
        } else if let Some(clock) = queue_clock.as_deref_mut() {
            moot_services::enqueue_moot_service(
                &mut commands,
                clock,
                entity,
                settlement,
                MootServiceKind::Immigration,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn begin_immigration_departure(
    commands: &mut Commands,
    entity: Entity,
    settlement: Entity,
    service_serial: u64,
    position: Vec3,
    hall: Vec3,
    yaw: f32,
    terrain: Option<&WorldTerrain>,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    occupied: &[Vec2],
) -> Vec3 {
    let destination = choose_immigration_departure_target(
        service_serial,
        position,
        hall,
        yaw,
        terrain,
        obstacles,
        colliders,
        derived,
        occupied,
    );
    // MootQueueTransit lets the common, certified straight exit bypass a
    // town-scale A* backlog. If live geometry rejects that segment,
    // `advance_immigration_departures` removes the transit marker and the
    // ordinary route planner takes over on the next navigation pass.
    commands
        .entity(entity)
        .insert((
            ImmigrationDeparture {
                settlement,
                service_serial,
                destination,
                failed_routes: 0,
            },
            MoveTarget(destination),
            MootQueueTransit,
        ))
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<MigrationCooldown>();
    destination
}

/// Complete protected counter departures and recover the rare route whose
/// live geometry changed after its initial corridor was certified.
#[allow(clippy::type_complexity)]
pub fn advance_immigration_departures(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    halls: Query<(&PlayerPosition, &Settlement, Option<&PlayerRotation>), With<Settlement>>,
    mut villagers: Query<
        (
            Entity,
            &mut PlayerPosition,
            &mut VillagerIntent,
            &mut ImmigrationDeparture,
            Option<&MoveTarget>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
        ),
        Without<Settlement>,
    >,
    residents: Query<
        &PlayerPosition,
        (
            With<VillagerIntent>,
            Without<ImmigrationDeparture>,
            Without<Settlement>,
        ),
    >,
) {
    if villagers.is_empty() {
        return;
    }
    // Retries must dodge every embodied body near the counter, not only the
    // other departers this query can see - otherwise a retry can pick a spot
    // an applicant or loitering resident already occupies.
    let mut occupied_departure_places = villagers
        .iter()
        .map(|(_, position, ..)| Vec2::new(position.0.x, position.0.z))
        .collect::<Vec<_>>();
    occupied_departure_places.extend(
        residents
            .iter()
            .map(|position| Vec2::new(position.0.x, position.0.z)),
    );
    for (entity, mut position, mut intent, mut departure, move_target, pending, failed) in
        villagers.iter_mut()
    {
        let Ok((hall, place, rotation)) = halls.get(departure.settlement) else {
            *intent = VillagerIntent::Idle;
            commands
                .entity(entity)
                .remove::<ImmigrationDeparture>()
                .remove::<MootQueueTicket>()
                .remove::<MootQueueTransit>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };

        if ground_distance(position.0, departure.destination) <= IMMIGRATION_DEPARTURE_REACH {
            // Generic movement intentionally stops within its small arrival
            // tolerance. Snap this final sub-body-width remainder so adjacent
            // reserved departure places cannot visually collapse toward one
            // another after many admissions.
            position.0 = departure.destination;
            finish_immigration(
                &mut commands,
                entity,
                &mut intent,
                departure.settlement,
                place,
            );
            continue;
        }

        if failed.is_some() {
            departure.failed_routes = departure.failed_routes.saturating_add(1);
            let retry_serial = departure
                .service_serial
                .wrapping_add(u64::from(departure.failed_routes).wrapping_mul(0x9E37_79B9));
            departure.destination = choose_immigration_departure_target(
                retry_serial,
                position.0,
                hall.0,
                rotation.map_or(0.0, |rotation| rotation.0),
                terrain.as_deref(),
                obstacles.as_deref(),
                colliders.as_deref(),
                derived.as_deref(),
                &occupied_departure_places,
            );
            if departure.failed_routes >= MAX_IMMIGRATION_DEPARTURE_ROUTE_FAILURES {
                // A departer who cannot route out must never keep the FIFO
                // head: that would freeze this hall's whole immigration line
                // behind one boxed-in person - the exact symptom this phase
                // exists to prevent. Admit them where they stand; ordinary
                // resident scheduling and arrival dispersal own them from the
                // next pass.
                warn!(
                    "Immigrant could not clear the '{}' counter after {} routes; admitting in place and releasing the line",
                    place.name, departure.failed_routes
                );
                finish_immigration(
                    &mut commands,
                    entity,
                    &mut intent,
                    departure.settlement,
                    place,
                );
                continue;
            }
            commands
                .entity(entity)
                .insert((MoveTarget(departure.destination), MootQueueTransit))
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        }

        // Direct motion installs Pending when geometry changed underneath the
        // certified exit. Dropping only the bypass marker lets normal A* own
        // the same authoritative destination; the ready ticket still blocks
        // the next applicant from entering the counter.
        if pending.is_some() {
            commands.entity(entity).remove::<MootQueueTransit>();
            continue;
        }
        ensure_move_target(&mut commands, entity, move_target, departure.destination);
    }
}

fn finish_immigration(
    commands: &mut Commands,
    entity: Entity,
    intent: &mut VillagerIntent,
    settlement: Entity,
    place: &Settlement,
) {
    *intent = VillagerIntent::Resident { settlement };
    // Residence is the replicated half of the same fact, so a client can
    // name who lives where without knowing anything about intents.
    let mut entity_commands = commands.entity(entity);
    entity_commands
        .remove::<MootQueueTicket>()
        .remove::<ImmigrationDeparture>()
        .remove::<MootQueueTransit>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<MigrationCooldown>()
        .insert(Residence(place.name.clone()));
}

fn immigration_counter_exit_candidate(service_serial: u64, hall: Vec3, yaw: f32) -> Vec3 {
    // This is only a protected step away from the occupied counter—not a
    // waiting area and not a post-immigration formation. A pair of irrational
    // sequences distributes candidates irregularly through the front
    // semicircle. As soon as one is reached, ordinary resident scheduling owns
    // the person during the same simulation chain.
    const EXIT_CANDIDATES: u64 = 256;
    const GOLDEN_FRACTION: f32 = 0.618_034;
    const SILVER_FRACTION: f32 = 0.414_214;
    let slot = service_serial.saturating_sub(1) % EXIT_CANDIDATES;
    let angle_t = ((slot as f32 + 0.5) * GOLDEN_FRACTION).fract();
    let radius_t = ((slot as f32 + 0.5) * SILVER_FRACTION).fract();
    let angle = -1.18 + angle_t * 2.36;
    // The service point sits roughly 5.85 metres in front of the Hall. Keep
    // the protected leg to an actual step away from the doorway (about 2-5m
    // from the counter), then let ordinary resident routines take over. A
    // much larger apron made a served crowd look like a second ceremonial
    // formation and unnecessarily throttled a long immigration line.
    let radius = 7.75 + radius_t * 3.25;
    let local = Vec2::new(angle.sin() * radius, -angle.cos() * radius);
    let offset = shared::rotation::local_to_world_xz(local, yaw);
    Vec3::new(hall.x + offset.x, hall.y, hall.z + offset.y)
}

#[allow(clippy::too_many_arguments)]
fn choose_immigration_departure_target(
    service_serial: u64,
    start: Vec3,
    hall: Vec3,
    yaw: f32,
    terrain: Option<&WorldTerrain>,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    occupied: &[Vec2],
) -> Vec3 {
    const SLOTS: u64 = 256;
    for offset in 0..SLOTS {
        let serial = service_serial.wrapping_add(offset).max(1);
        let mut target = immigration_counter_exit_candidate(serial, hall, yaw);
        if let Some(terrain) = terrain {
            target.y = terrain.get_height(target.x, target.z);
            if !immigration_departure_terrain_clear(terrain, start, target) {
                continue;
            }
        }
        let start_xz = Vec2::new(start.x, start.z);
        let target_xz = Vec2::new(target.x, target.z);
        if occupied
            .iter()
            .any(|position| position.distance_squared(target_xz) <= 1.45 * 1.45)
        {
            continue;
        }
        if crate::player::hero::navigation_segment_clear(
            start_xz, target_xz, obstacles, colliders, derived,
        ) {
            return target;
        }
    }

    // A Hall can exist in a focused test without collision or terrain
    // resources. In a malformed live plot this fallback remains an ordinary
    // routed destination; most importantly, the ready ticket continues to
    // hold the line instead of admitting a crowd into the service point.
    let mut target = immigration_counter_exit_candidate(service_serial, hall, yaw);
    if let Some(terrain) = terrain {
        target.y = terrain.get_height(target.x, target.z);
    }
    target
}

fn immigration_departure_terrain_clear(terrain: &WorldTerrain, start: Vec3, end: Vec3) -> bool {
    crate::player::hero::terrain_segment_walkable(
        terrain,
        Vec2::new(start.x, start.z),
        Vec2::new(end.x, end.z),
    )
}

/// Recount every settlement's residents from the villagers who live there.
///
/// DERIVED every tick rather than incremented on arrival, because a counter
/// that is nudged by events drifts the first time an event is missed -- and a
/// resident count that disagrees with the people standing there is exactly the
/// kind of lie the encyclopedia must never tell.
pub fn recount_residents(
    mut settlements: Query<(Entity, &mut Settlement)>,
    villagers: Query<&VillagerIntent>,
) {
    let mut counts: HashMap<Entity, u32> = HashMap::new();
    for intent in villagers.iter() {
        if !intent.counts_as_resident() {
            continue;
        }
        let settlement = intent
            .settlement()
            .expect("resident intents always name their settlement");
        *counts.entry(settlement).or_default() += 1;
    }

    for (entity, mut settlement) in settlements.iter_mut() {
        let count = counts.get(&entity).copied().unwrap_or(0);
        // Change detection drives replication; an idle village must not
        // re-send its count every tick.
        if settlement.residents != count {
            info!(
                "Village '{}': {} resident(s), was {}",
                settlement.name, count, settlement.residents
            );
            settlement.residents = count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immigration_counter_exits_are_near_irregular_places_in_front_of_the_hall() {
        let hall = Vec3::new(40.0, 2.0, -30.0);
        let yaw = 0.73;
        let mut targets = Vec::new();
        for serial in 1..=256 {
            let target = immigration_counter_exit_candidate(serial, hall, yaw);
            assert_eq!(target.y, hall.y);
            assert!((7.7..=11.1).contains(&ground_distance(target, hall)));
            let local = shared::rotation::world_to_local_xz(
                Vec2::new(target.x - hall.x, target.z - hall.z),
                yaw,
            );
            assert!(local.y < -3.0, "exit escaped behind the Hall: {local:?}");
            assert!(targets.iter().all(|previous| *previous != target));
            targets.push(target);
        }
    }

    /// A departer who cannot route out must never keep the hall's FIFO head:
    /// at the failure cap they are admitted where they stand so the
    /// immigration line keeps moving. Under the pre-fix behavior (reset the
    /// counter and hold the head) this test fails with the departure still
    /// attached.
    #[test]
    fn boxed_in_departer_is_admitted_in_place_instead_of_freezing_the_line() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        let hall = world
            .spawn((
                Settlement {
                    name: "Blocked Hollow".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let exit = Vec3::new(9.0, 0.0, 0.0);
        let departer = world
            .spawn((
                PlayerPosition(Vec3::new(0.0, 0.0, -2.0)),
                VillagerIntent::Travelling { settlement: hall },
                ImmigrationDeparture {
                    settlement: hall,
                    service_serial: 7,
                    destination: exit,
                    failed_routes: MAX_IMMIGRATION_DEPARTURE_ROUTE_FAILURES - 1,
                },
                crate::world::village_roads::NavigationRouteFailed { goal: exit },
            ))
            .id();

        world
            .run_system_once(advance_immigration_departures)
            .unwrap();

        assert!(
            world.get::<ImmigrationDeparture>(departer).is_none(),
            "the failure cap must release the departure phase, not reset it"
        );
        assert!(matches!(
            world.get::<VillagerIntent>(departer),
            Some(VillagerIntent::Resident { settlement }) if *settlement == hall
        ));
        assert!(world.get::<Residence>(departer).is_some());
    }
}
