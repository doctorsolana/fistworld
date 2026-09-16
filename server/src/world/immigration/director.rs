//! World-wide admission followed by a separate bounded destination decision.
use super::admission::{ChoosingSettlement, commit_choice, enter_world};
use super::*;

type Waiting<'w, 's> =
    Query<'w, 's, (Entity, &'static ChoosingSettlement, &'static PlayerPosition)>;

/// Share the existing one-slice budget fairly. A difficult sea route keeps its
/// frontier, but cannot prevent another real hull from trying an easy landing.
fn next_decision(
    director: &mut NaturalImmigrationDirector,
    waiting: &Waiting,
    people: &Query<&CharacterKind>,
    now: f64,
) {
    let live = |boat| {
        waiting
            .get(boat)
            .is_ok_and(|(_, arrival, _)| people.get(arrival.passenger).is_ok())
    };
    director.suspended_decisions.retain(|boat, _| live(*boat));
    if let Some(boat) = director.deciding.take() {
        let state = PendingDecision {
            landfall: director.pending_landfall.take(),
            water: director.pending_water.take(),
        };
        if live(boat) && (state.landfall.is_some() || state.water.is_some()) {
            director.suspended_decisions.insert(boat, state);
        }
    }
    let key = |boat: Entity, arrival: &ChoosingSettlement| (arrival.decision_seed, boat.to_bits());
    let eligible = || {
        waiting.iter().filter(|(_, arrival, _)| {
            now >= arrival.retry_at && people.get(arrival.passenger).is_ok()
        })
    };
    let next = eligible()
        .filter(|(boat, arrival, _)| {
            director
                .decision_cursor
                .is_none_or(|cursor| key(*boat, arrival) > cursor)
        })
        .min_by_key(|(boat, arrival, _)| key(*boat, arrival))
        .or_else(|| eligible().min_by_key(|(boat, arrival, _)| key(*boat, arrival)));
    if let Some((boat, arrival, _)) = next {
        director.deciding = Some(boat);
        director.decision_cursor = Some(key(boat, arrival));
        if let Some(state) = director.suspended_decisions.remove(&boat) {
            director.pending_landfall = state.landfall;
            director.pending_water = state.water;
        }
    }
}

fn defer_choice(
    commands: &mut Commands,
    director: &mut NaturalImmigrationDirector,
    waiting: &Waiting,
    now: f64,
    cycle: f64,
    failed: Option<Entity>,
) {
    director.pending_landfall = None;
    director.pending_water = None;
    if let Some(boat) = director.deciding.take() {
        if let Ok((_, arrival, _)) = waiting.get(boat) {
            let mut next = arrival.clone();
            if let Some(failed) = failed {
                if !next.rejected_towns.contains(&failed) {
                    if next.rejected_towns.len() >= 32 {
                        next.rejected_towns.remove(0);
                    }
                    next.rejected_towns.push(failed);
                }
                // A saturated rejection window must yield to other boats.
                // Keeping the window permits a 33rd town after the backoff;
                // evicting the oldest without waiting could cycle forever.
                next.retry_at = if next.rejected_towns.len() >= 32 {
                    now + cycle * f64::from(RETRY_DELAY_DAYS)
                } else {
                    now
                };
            } else {
                next.rejected_towns.clear();
                next.retry_at = now + cycle * f64::from(RETRY_DELAY_DAYS);
            }
            commands.entity(boat).insert(next);
        }
    }
}

/// At most one real entry or one retained decision slice per tick. New bodies
/// enter without a town target; selection only sees them on a later ECS pass.
pub fn plan_natural_immigration(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    clock: Query<&WorldTime>,
    settlements: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&SettlementEconomy>,
        Option<&SettlementId>,
    )>,
    active: Query<(), With<ImmigrantArrivalBoat>>,
    people: Query<&CharacterKind>,
    arrival_bodies: ArrivalBodies,
    waiting: Waiting,
    mut director: ResMut<NaturalImmigrationDirector>,
    mut villager_seed: ResMut<crate::world::dev::VillagerSeed>,
    mut water_navigation: ResMut<VesselNavigationQueue>,
    start_config: Option<Res<crate::world::start_config::WorldStartConfig>>,
) {
    if !director.configured {
        if let Some(config) = start_config.as_deref() {
            director.configured = true;
            let rate = std::env::var("FISTWORLD_IMMIGRANTS_PER_DAY")
                .ok()
                .and_then(|raw| raw.parse::<f32>().ok())
                .filter(|rate| rate.is_finite() && *rate >= 0.)
                .or(config.immigrants_per_day);
            if let Some(rate) = rate {
                director.steady_rate = true;
                if rate == 0. {
                    director.enabled = false;
                } else {
                    director.interval_days = interval_days_for_rate(rate);
                }
            }
            if std::env::var("FISTWORLD_WORLD_NPC_CAP")
                .ok()
                .and_then(|raw| raw.parse::<usize>().ok())
                .is_none()
            {
                director.world_npc_cap = config.world_npc_cap;
            }
        }
    }
    if !director.enabled && director.manual_arrivals == 0 && waiting.is_empty() {
        return;
    }
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = absolute_world_seconds(clock);
    let cycle = f64::from(clock.cycle_duration());
    let revision = (
        terrain.generator.active_map_content_hash(),
        terrain.modification_version(),
    );
    if director.terrain_revision.is_some_and(|old| old != revision) {
        if director
            .terrain_revision
            .is_some_and(|old| old.0 != revision.0)
        {
            director.coastal_approaches.clear();
            director.pending_landfall = None;
            director.pending_water = None;
            director.suspended_decisions.clear();
        }
        // Cached results carry no read footprint, so they invalidate globally.
        // Retained searches own chunk dependencies and preserve unrelated work.
        director.settlement_landfalls.clear();
    }
    director.terrain_revision = Some(revision);
    director
        .settlement_landfalls
        .retain(|entity, _| settlements.get(*entity).is_ok());
    for (boat, arrival, _) in waiting.iter() {
        if people.get(arrival.passenger).is_err() {
            commands.entity(boat).despawn();
            water_navigation.take(boat);
        }
    }
    if director.deciding.is_some_and(|boat| {
        !waiting
            .get(boat)
            .is_ok_and(|(_, arrival, _)| people.get(arrival.passenger).is_ok())
    }) {
        director.deciding = None;
        director.pending_landfall = None;
        director.pending_water = None;
    }

    // Entering the world does not depend on somebody else's path search.
    // A due entry uses this pass; retained decision work resumes next tick.
    if try_admit_arrival(
        &mut commands,
        &terrain,
        clock,
        &mut director,
        &mut villager_seed,
        &water_navigation,
        &people,
        &arrival_bodies,
        active.iter().count(),
    ) {
        return;
    }

    next_decision(&mut director, &waiting, &people, now);

    if let Some(mut pending) = director.pending_water.take() {
        let unchanged = settlements.get(pending.choice.entity).is_ok_and(
            |(_, town, position, rotation, _, _)| {
                town.tier != shared::components::SettlementTier::Ruins
                    && SettlementBuildingKind::Hall
                        .entrance_position(position.0, rotation.map_or(0., |r| r.0))
                        .distance_squared(pending.choice.entrance)
                        <= LANDFALL_ENTRANCE_EPSILON_SQ
            },
        );
        if !unchanged {
            defer_choice(
                &mut commands,
                &mut director,
                &waiting,
                now,
                cycle,
                Some(pending.choice.entity),
            );
            return;
        }
        match water_navigation
            .cache
            .advance(&mut pending.search, &terrain)
        {
            WaterPlanResult::Pending => director.pending_water = Some(pending),
            WaterPlanResult::Complete(None) => defer_choice(
                &mut commands,
                &mut director,
                &waiting,
                now,
                cycle,
                Some(pending.choice.entity),
            ),
            WaterPlanResult::Complete(Some(route)) => {
                let Some(boat) = director.deciding else {
                    return;
                };
                let Ok((_, arrival, position)) = waiting.get(boat) else {
                    return;
                };
                // This is the same real hull, not a new spawn that could claim
                // somebody else's berth or be offset after certification.
                if position.0.xz().distance_squared(pending.entry.start.xz()) > 0.0001 {
                    pending.entry.start = position.0;
                    pending.search = water_navigation.cache.begin(
                        &terrain,
                        position.0.xz(),
                        pending.voyage.mooring,
                    );
                    director.pending_water = Some(pending);
                    return;
                }
                let id = settlements
                    .get(pending.choice.entity)
                    .ok()
                    .and_then(|(_, _, _, _, _, id)| id.copied());
                commit_choice(
                    &mut commands,
                    &terrain,
                    boat,
                    arrival,
                    position.0,
                    &pending.choice,
                    id,
                    pending.voyage,
                    route,
                    water_navigation.cache.geometry.revision(&terrain),
                    now,
                );
                director.deciding = None;
            }
        }
        return;
    }
    if let Some(mut pending) = director.pending_landfall.take() {
        let unchanged = settlements.get(pending.choice.entity).is_ok_and(
            |(_, town, position, rotation, _, _)| {
                town.tier != shared::components::SettlementTier::Ruins
                    && SettlementBuildingKind::Hall
                        .entrance_position(position.0, rotation.map_or(0., |r| r.0))
                        .distance_squared(pending.choice.entrance)
                        <= LANDFALL_ENTRANCE_EPSILON_SQ
            },
        );
        if !unchanged {
            defer_choice(
                &mut commands,
                &mut director,
                &waiting,
                now,
                cycle,
                Some(pending.choice.entity),
            );
            return;
        }
        // Another hull may have completed this same town's landfall while we
        // yielded. Reuse its proof instead of finishing a duplicate search.
        let cached = director
            .settlement_landfalls
            .get(&pending.choice.entity)
            .copied()
            .filter(|cached| cached.matches(pending.choice.entrance));
        let result = match cached {
            Some(CachedSettlementLandfall::Reachable { voyage, .. }) => {
                PendingLandfallResult::Reachable(voyage)
            }
            Some(CachedSettlementLandfall::Unreachable { .. }) => {
                PendingLandfallResult::Unreachable
            }
            None => pending.advance(&terrain),
        };
        match result {
            PendingLandfallResult::Pending => {
                director.pending_landfall = Some(pending);
                return;
            }
            PendingLandfallResult::Unreachable => {
                director.settlement_landfalls.insert(
                    pending.choice.entity,
                    CachedSettlementLandfall::Unreachable {
                        entrance: pending.choice.entrance,
                    },
                );
                defer_choice(
                    &mut commands,
                    &mut director,
                    &waiting,
                    now,
                    cycle,
                    Some(pending.choice.entity),
                );
                return;
            }
            PendingLandfallResult::Reachable(voyage) => {
                director.settlement_landfalls.insert(
                    pending.choice.entity,
                    CachedSettlementLandfall::Reachable {
                        entrance: pending.choice.entrance,
                        voyage,
                    },
                );
                let Some(boat) = director.deciding else {
                    return;
                };
                let Ok((_, arrival, position)) = waiting.get(boat) else {
                    return;
                };
                // Continue the very choice whose landfall just succeeded.
                // Re-ranking here made dynamic town scores repeatedly steal
                // the next phase, even though this hull had a valid proof.
                director.pending_water = Some(begin_water_proof(
                    pending.choice,
                    voyage,
                    arrival,
                    position.0,
                    &terrain,
                    &mut water_navigation,
                ));
                return;
            }
        }
    }

    if let Some(boat) = director.deciding {
        let Ok((_, arrival, position)) = waiting.get(boat) else {
            return;
        };
        let choice = settlements
            .iter()
            .filter_map(|(entity, town, location, rotation, economy, _)| {
                if town.tier == shared::components::SettlementTier::Ruins
                    || arrival.rejected_towns.contains(&entity)
                {
                    return None;
                }
                let entrance = SettlementBuildingKind::Hall
                    .entrance_position(location.0, rotation.map_or(0., |r| r.0));
                if director
                    .settlement_landfalls
                    .get(&entity)
                    .copied()
                    .filter(|cached| cached.matches(entrance))
                    .is_some_and(|cached| {
                        matches!(cached, CachedSettlementLandfall::Unreachable { .. })
                    })
                {
                    return None;
                }
                let score = settlement_attractiveness(
                    town,
                    economy,
                    arrival.decision_seed,
                    entity,
                    position.0,
                    location.0,
                );
                if !score.is_finite() {
                    return None;
                }
                Some(SettlementChoice {
                    entity,
                    name: town.name.clone(),
                    position: location.0,
                    entrance,
                    score,
                })
            })
            .max_by(|a, b| a.score.total_cmp(&b.score));
        // Attractiveness ranks valid destinations; poverty is never a veto on
        // joining the best reachable town. Only failed geometry causes retry.
        let Some(choice) = choice else {
            defer_choice(&mut commands, &mut director, &waiting, now, cycle, None);
            return;
        };
        let cached = director
            .settlement_landfalls
            .get(&choice.entity)
            .copied()
            .filter(|cached| cached.matches(choice.entrance));
        if let Some(CachedSettlementLandfall::Reachable { voyage, .. }) = cached {
            director.pending_water = Some(begin_water_proof(
                choice,
                voyage,
                arrival,
                position.0,
                &terrain,
                &mut water_navigation,
            ));
        } else {
            director.pending_landfall = Some(PendingLandfallSearch::new(
                &choice,
                &director.coastal_approaches,
            ));
        }
        return;
    }
}

fn begin_water_proof(
    choice: SettlementChoice,
    voyage: CoastalVoyage,
    arrival: &ChoosingSettlement,
    position: Vec3,
    terrain: &WorldTerrain,
    navigation: &mut VesselNavigationQueue,
) -> PendingWaterVoyage {
    PendingWaterVoyage {
        choice,
        entry: CoastalVoyage {
            start: position,
            yaw: 0.,
            mooring: position.xz(),
            landing: arrival.facts.entry,
        },
        voyage,
        search: navigation
            .cache
            .begin(terrain, position.xz(), voyage.mooring),
    }
}

/// A bounded independent cadence; failures or a full fleet leave decision work
/// eligible on the same tick, and successful entries never allocate catch-up debt.
fn try_admit_arrival(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    clock: &WorldTime,
    director: &mut NaturalImmigrationDirector,
    villager_seed: &mut crate::world::dev::VillagerSeed,
    water_navigation: &VesselNavigationQueue,
    people: &Query<&CharacterKind>,
    arrival_bodies: &ArrivalBodies,
    active_count: usize,
) -> bool {
    let now = absolute_world_seconds(clock);
    let cycle = f64::from(clock.cycle_duration());
    // Only actual world admission counts the population. Eight bounded boats
    // can wait without a town; no-town or blocked years cannot allocate debt.
    if active_count >= MAX_ACTIVE_VOYAGES {
        return false;
    }
    let manual = director.manual_arrivals > 0;
    if !manual {
        if !director.enabled {
            return false;
        }
        let due = director
            .next_arrival_world_seconds
            .get_or_insert(now + cycle * f64::from(FIRST_ARRIVAL_DELAY_DAYS));
        if now < *due {
            return false;
        }
    }
    let population = people
        .iter()
        .filter(|kind| **kind == CharacterKind::Villager)
        .count();
    if population >= director.world_npc_cap {
        director.population_cap_announced = true;
        if manual {
            director.finish_manual_arrival();
        } else {
            director.next_arrival_world_seconds = Some(now + cycle * f64::from(RETRY_DELAY_DAYS));
        }
        return false;
    }
    director.population_cap_announced = false;
    if director.coastal_approaches.is_empty() {
        director.coastal_approaches = coastal_voyages(terrain, 0);
    }
    let decision_seed = director.sequence.wrapping_add(1) ^ (u64::from(clock.day) << 32);
    let entry = (!director.coastal_approaches.is_empty()).then(|| {
        director.coastal_approaches
            [mixed(decision_seed) as usize % director.coastal_approaches.len()]
    });
    let Some(entry) = entry.and_then(|entry| {
        ArrivalOccupancy::from_bodies(arrival_bodies).vacant_voyage_matching(
            terrain,
            entry,
            decision_seed,
            |point| {
                water_navigation.cache.geometry.point_clear(
                    terrain,
                    point,
                    crate::player::boat::clearance::WatercraftClearance::DINGHY,
                )
            },
        )
    }) else {
        director.defer_arrival(manual, now, cycle);
        return false;
    };
    // The coast is chosen before consulting any town data. There is no town
    // quota, promised destination, or pre-spawn attraction decision here.
    enter_world(
        commands,
        terrain,
        villager_seed,
        entry,
        now,
        decision_seed,
        manual,
    );
    director.sequence = director.sequence.wrapping_add(1);
    if manual {
        director.finish_manual_arrival();
    } else {
        let interval = if director.steady_rate {
            director.interval_days
        } else {
            let preference = (mixed(decision_seed ^ 0x53a9) & 0xffff) as f32 / u16::MAX as f32;
            director.interval_days
                * seasonal_interval_multiplier(clock.day)
                * (0.85 + preference * 0.30)
        };
        director.next_arrival_world_seconds = Some(now + cycle * f64::from(interval));
    }
    true
}
