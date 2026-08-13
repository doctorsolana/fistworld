//! Settlement discovery, migration, arrival and authoritative resident counts.

use super::*;

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
        ),
        (With<CharacterName>, With<PlayerPosition>),
    >,
) {
    for (entity, kind, intent, occupation, inventory, activity, carried, nutrition, work_status) in
        villagers.iter()
    {
        // Heroes are players' bodies and join nothing on their own.
        if *kind != CharacterKind::Villager {
            continue;
        }
        let mut entity_commands = commands.entity(entity);
        if intent.is_none() {
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
        if work_status.is_none() {
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
    mut villagers: Query<(
        Entity,
        &PlayerPosition,
        &mut VillagerIntent,
        Option<&MigrationCooldown>,
    )>,
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
    halls: Query<(&PlayerPosition, &Settlement, Option<&PlayerRotation>)>,
    mut villagers: Query<(
        Entity,
        &PlayerPosition,
        &mut VillagerIntent,
        Option<&MoveTarget>,
        Option<&NavigationRouteFailed>,
        Option<&MigrationCooldown>,
        Option<&MootQueueTicket>,
        Has<strategic::StrategicPerson>,
    )>,
    road_graph: Option<Res<VillageRoadGraph>>,
) {
    let now = simulation_time.elapsed_real_seconds_f64();
    for (
        entity,
        position,
        mut intent,
        move_target,
        route_failed,
        cooldown,
        queue_ticket,
        strategic,
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
                .remove::<Residence>();
            continue;
        };

        // Once the tactical migrant reaches the hall, the visible Moot line
        // owns their movement. Some queue slots are deliberately farther than
        // ARRIVAL_RADIUS from the hall, so ordinary migration repair must not
        // pull a waiting applicant out of the line.
        if let Some(ticket) = queue_ticket {
            if ticket.hall == settlement && ticket.kind == MootServiceKind::Immigration {
                if !ticket.is_ready() {
                    continue;
                }
                finish_immigration(&mut commands, entity, &mut intent, settlement, place);
                continue;
            }
            // A travelling villager cannot legitimately use another hall
            // service yet. Let that authoritative service finish instead of
            // fighting its queue destination.
            continue;
        }
        // Terrain height and the Hall's raised foundation are irrelevant to
        // reaching its forecourt. Comparing full 3-D distance could reject the
        // lone migrant standing beside everyone else on a steep plot.
        if ground_distance(position.0, hall.0) > ARRIVAL_RADIUS {
            if let Some(failed) = route_failed {
                debug!(
                    "Villager could not migrate to '{}' through {:.1},{:.1}; reconsidering after cooldown",
                    place.name, failed.goal.x, failed.goal.z
                );
                *intent = VillagerIntent::Idle;
                let entrance = SettlementBuildingKind::Hall
                    .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
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
            let entrance = SettlementBuildingKind::Hall
                .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
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
    commands
        .entity(entity)
        .remove::<MootQueueTicket>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<MigrationCooldown>()
        .insert(Residence(place.name.clone()));
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
