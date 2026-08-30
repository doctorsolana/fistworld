//! Material supply, builder work and terrain publication for approved sites.

use super::*;

const MAX_CERTIFIED_DELIVERY_FAILURES: u8 = 12;

/// Runtime acknowledgement that a completed market has been checked against
/// the current 12 m square terrain contract. The marker is deliberately not
/// persisted: after a server restart the inexpensive height check runs once,
/// repairing saves whose market was levelled with the former 9 x 7 blockout.
#[derive(Component)]
pub(crate) struct MarketGroundLeveled;

fn material_stock_may_supply(
    owns_priority: bool,
    available_wood: u32,
    settlement_outstanding: u32,
) -> bool {
    owns_priority || available_wood >= settlement_outstanding
}

/// A builder who has not actually secured Wood has no reason to occupy the
/// public counter. Wait at that builder's own worksite while market stock,
/// priority or the staggered tree proof changes. One site has one builder, so
/// this is naturally distributed and requires no crowd reservation system.
/// Both supply channels are dead: no purchasable market Wood and twelve
/// exhausted tree candidates. Release this person to ordinary resident life
/// (sleep, meals, employment, leisure all resume through the normal systems)
/// instead of pinning them at the plot indefinitely. The site keeps its
/// delivered materials and parks under [`ConstructionSupplyCooldown`];
/// `recover_orphaned_construction` re-drafts a builder when it expires.
pub(super) fn give_up_starved_supply(
    commands: &mut Commands,
    builder: Entity,
    name: &CharacterName,
    site_entity: Entity,
    site: &mut UnderConstruction,
    previous: Option<&ConstructionSupplyCooldown>,
    now: f64,
) {
    warn!(
        "Village supplier {} has no wood source for the {}; returning to normal life while the site rests",
        name.0,
        site.kind.label(),
    );
    site.builder = None;
    commands
        .entity(site_entity)
        .insert(ConstructionSupplyCooldown::after_give_up(previous, now));
    commands
        .entity(builder)
        .insert(VillagerIntent::Resident {
            settlement: site.settlement,
        })
        .remove::<ConstructionMaterialRoutine>()
        .remove::<MootQueueTicket>()
        .remove::<MootQueueTransit>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>();
}

fn wait_at_own_worksite(
    commands: &mut Commands,
    builder: Entity,
    position: Vec3,
    move_target: Option<&MoveTarget>,
    stand: Vec3,
) {
    if ground_distance(position, stand) > WORK_REACH {
        ensure_move_target(commands, builder, move_target, stand);
    } else if move_target.is_some() {
        // Queue the removals only on the transition into waiting, never every
        // tick (the moot queue documents the same discipline): many waiting
        // builders must not each queue four no-op commands per tick.
        commands
            .entity(builder)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>();
    }
}

fn delivery_access_points(
    terrain: &WorldTerrain,
    access: Option<&PlannedRoadAccess>,
) -> Option<(Vec3, Vec3, Vec<RouteWaypoint>)> {
    let access = access?;
    let destination = *access.points.get(1)?;
    let entry = *access.points.last()?;
    let to_world = |point: Vec2| Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y);
    let destination = to_world(destination);
    let entry = to_world(entry);
    let mut waypoints: Vec<_> = access
        .points
        .iter()
        .skip(1)
        .rev()
        .copied()
        .map(|point| RouteWaypoint {
            position: to_world(point),
            on_road: false,
        })
        .collect();
    waypoints.dedup_by(|a, b| a.position.distance_squared(b.position) <= 0.01);
    Some((entry, destination, waypoints))
}

fn begin_material_delivery(
    commands: &mut Commands,
    builder: Entity,
    position: Vec3,
    fallback: Vec3,
    terrain: &WorldTerrain,
    access: Option<&PlannedRoadAccess>,
) -> ConstructionMaterialPhase {
    let Some((entry, destination, waypoints)) = delivery_access_points(terrain, access) else {
        commands.entity(builder).insert(MoveTarget(fallback));
        return ConstructionMaterialPhase::Delivering {
            destination: fallback,
        };
    };
    if ground_distance(position, entry) > WORK_REACH {
        commands.entity(builder).insert(MoveTarget(entry));
        ConstructionMaterialPhase::ApproachingDeliveryAccess { entry }
    } else {
        commands
            .entity(builder)
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .insert((
                MoveTarget(destination),
                TravelRoute {
                    goal: destination,
                    waypoints,
                    next: 0,
                    geometry_version: 0,
                },
            ));
        ConstructionMaterialPhase::Delivering { destination }
    }
}

fn begin_material_egress(
    commands: &mut Commands,
    builder: Entity,
    position: Vec3,
    terrain: &WorldTerrain,
    access: Option<&PlannedRoadAccess>,
) -> ConstructionMaterialPhase {
    let Some((exit, mut waypoints)) = delivery_egress_points(terrain, access) else {
        commands
            .entity(builder)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>();
        return ConstructionMaterialPhase::Seeking;
    };
    if ground_distance(position, exit) <= WORK_REACH {
        commands
            .entity(builder)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>();
        return ConstructionMaterialPhase::Seeking;
    }
    // Access points are stored door -> apron -> ... -> public network. A
    // delivery finishes at the apron (index 1), so follow the remainder in
    // its authored direction instead of asking bounded A* to rediscover a
    // 200-metre pre-road corridor from scratch.
    if waypoints.is_empty() {
        waypoints.push(RouteWaypoint {
            position: exit,
            on_road: false,
        });
    }
    commands
        .entity(builder)
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            MoveTarget(exit),
            TravelRoute {
                goal: exit,
                waypoints,
                next: 0,
                geometry_version: 0,
            },
        ));
    ConstructionMaterialPhase::LeavingDeliveryAccess { exit }
}

fn delivery_egress_points(
    terrain: &WorldTerrain,
    access: Option<&PlannedRoadAccess>,
) -> Option<(Vec3, Vec<RouteWaypoint>)> {
    let access = access?;
    let exit_point = access.points.last().copied()?;
    let to_world = |point: Vec2| Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y);
    let exit = to_world(exit_point);
    let waypoints = access
        .points
        .iter()
        .skip(2)
        .copied()
        .map(|point| RouteWaypoint {
            position: to_world(point),
            on_road: false,
        })
        .collect();
    Some((exit, waypoints))
}

/// Supply approved worksites with physical wood before construction begins.
///
/// Market Wood in the settlement hall is preferred and must be purchased by
/// the owner. If it is absent or unaffordable -- including the founding
/// deadlock before the first hut exists -- the builder walks to a real tree,
/// chops, carries what fits, and deposits it at the site.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_construction_material_logistics(
    simulation_time: crate::world::simulation_time::SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    world_time: Query<&WorldTime>,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut commands: Commands,
    mut settlements: Query<
        (
            Entity,
            &mut Settlement,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            Option<&MootAdministration>,
            Option<&SettlementPolicies>,
            Option<&mut shared::economy::CivicAccount>,
        ),
        Without<CharacterKind>,
    >,
    mut sites: Query<
        (
            Entity,
            &mut UnderConstruction,
            Option<&mut shared::components::ConstructionSite>,
            &PlayerPosition,
            Option<&PlannedRoadAccess>,
            Option<&ConstructionSupplyCooldown>,
            bevy::ecs::query::Has<crate::player::permits::PlayerConstructionProject>,
            bevy::ecs::query::Has<shared::economy::BusinessForSale>,
        ),
        Without<CharacterKind>,
    >,
    mut inventories: Query<&mut GoodsInventory>,
    mut markets: Query<&mut MootMarket>,
    marketplaces: Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            &PlayerRotation,
        ),
        Without<CharacterKind>,
    >,
    mut tree_candidates: Local<TreeWorkCandidateCache>,
    mut builders: Query<
        (
            Entity,
            &shared::components::PersonId,
            &CharacterName,
            &PlayerPosition,
            Option<&VillagerIntent>,
            Option<&PlayerConstructionAssignment>,
            Option<&HomeRoutine>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut ConstructionMaterialRoutine,
            Option<&MoveTarget>,
            Option<&TravelRoute>,
            Option<&NavigationRouteFailed>,
            Option<&MootQueueTicket>,
            Option<&mut Wallet>,
        ),
        (
            With<CharacterKind>,
            Without<crate::player::hero::OfflineHero>,
        ),
    >,
    // NPC-owned private sites can be carried by a different resident while
    // their material bill still belongs to the owner. Player heroes carry and
    // pay directly, so their wallet is already in `builders`; this disjoint
    // lookup covers only the NPC-owner/carrier case.
    mut non_builder_owner_wallets: Query<
        (&shared::components::PersonId, &mut Wallet),
        Without<ConstructionMaterialRoutine>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let daylight = clock.is_day();
    let day = clock.day;
    let dt = simulation_time.world_seconds();
    // Timber/store retries are world behaviour, so their cooldown must advance
    // with the master time warp. Using wall-clock time here made a 60-second
    // sparse-timber backoff last ten world minutes at 10x (and an hour at
    // 100x), which looked exactly like abandoned construction. The one-proof-
    // per-tick budget above still bounds the actual pathfinding cost.
    let now = f64::from(clock.day) * f64::from(clock.cycle_duration())
        + f64::from(clock.seconds_in_cycle);
    // A terrain proof is bounded but not free. Stagger simultaneous builders
    // across ticks instead of letting a migration wave perform the same
    // impossible-landmass search four times in one server frame.
    let mut tree_proof_used = false;
    // Scarce founding stock must finish useful buildings instead of being
    // smeared across every simultaneous permit. Prefer the site closest to
    // completion (then stable entity order) and advance to the next only after
    // it is fully supplied. Builders already carrying Wood may always finish
    // their delivery.
    let mut material_priority: HashMap<Entity, (u32, u64, Entity)> = HashMap::new();
    let mut material_outstanding: HashMap<Entity, u32> = HashMap::new();
    for (site_entity, site, _, _, _, _, _, _) in sites.iter_mut() {
        if site.stage != BuildStage::Supplying {
            continue;
        }
        let delivered = inventories
            .get(site_entity)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);
        let remaining = site
            .kind
            .construction_wood_required()
            .saturating_sub(delivered);
        if remaining == 0 {
            continue;
        }
        material_outstanding
            .entry(site.settlement)
            .and_modify(|outstanding| *outstanding = outstanding.saturating_add(remaining))
            .or_insert(remaining);
        let candidate = (remaining, site_entity.to_bits(), site_entity);
        let priority = material_priority
            .entry(site.settlement)
            .or_insert(candidate);
        if (candidate.0, candidate.1) < (priority.0, priority.1) {
            *priority = candidate;
        }
    }

    // Stock is not removed until a builder physically reaches the counter,
    // but it must be claimed before the walk begins. Otherwise every worksite
    // sees the same abundant snapshot and a crowd arrives for units already
    // promised to the people ahead of it.
    let mut reserved_store_wood: HashMap<Entity, u32> = HashMap::new();
    for builder in builders.iter_mut() {
        if let ConstructionMaterialPhase::CollectingFromStore {
            source,
            reserved_units,
            ..
        } = builder.9.phase
        {
            *reserved_store_wood.entry(source).or_default() += reserved_units;
        }
    }

    for (
        builder,
        person_id,
        name,
        position,
        intent,
        player_assignment,
        home_routine,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        travel_route,
        mut route_failed,
        queue_ticket,
        mut wallet,
    ) in builders.iter_mut()
    {
        if home_routine.is_some() {
            continue;
        }
        let villager_assigned =
            matches!(intent, Some(VillagerIntent::Building { site, .. }) if *site == routine.site);
        let player_assigned = player_assignment.is_some_and(|assignment| {
            assignment.site == routine.site
                && assignment.settlement
                    == sites
                        .get(routine.site)
                        .map(|(_, site, ..)| site.settlement)
                        .unwrap_or(Entity::PLACEHOLDER)
        });
        if !villager_assigned && !player_assigned {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<PlayerConstructionAssignment>()
                .remove::<MootQueueTicket>()
                .remove::<MootQueueTransit>()
                .remove::<MoveTarget>();
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        // Ordinary builders stop for the night, but an explicit player order
        // is not an employment shift. A commanded hero must keep gathering
        // and delivering materials overnight instead of remaining frozen in
        // the last visible activity until dawn.
        //
        // And nobody downs tools MID-TASK: a half-felled tree gets felled and
        // wood already on a shoulder still reaches its site — otherwise dusk
        // left haulers standing frozen beside the worksite, log in hand, until
        // morning. Only the un-invested phases (seeking, queueing at a
        // counter, walking to a tree) stand down at dusk, and they stand down
        // cleanly: stopped where they are, idle, ready for evening routines.
        let finishing_up = matches!(
            routine.phase,
            ConstructionMaterialPhase::Chopping { .. }
                | ConstructionMaterialPhase::UnloadingAtHall { .. }
                | ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
                | ConstructionMaterialPhase::Delivering { .. }
                | ConstructionMaterialPhase::LeavingDeliveryAccess { .. }
        );
        if !daylight && !player_assigned && !finishing_up {
            // The moot queue advances all night while this system - the only
            // consumer of Ready freight tickets - sleeps. Dissolve the line
            // at nightfall instead of leaving a Ready head frozen at the
            // counter with everyone queued behind it until dawn; the ticket
            // and its stock reservation are re-earned in the morning.
            if queue_ticket.is_some() {
                commands
                    .entity(builder)
                    .remove::<MootQueueTicket>()
                    .remove::<MootQueueTransit>();
            }
            if matches!(
                routine.phase,
                ConstructionMaterialPhase::CollectingFromStore { .. }
                    | ConstructionMaterialPhase::WalkingToTree { .. }
            ) {
                // Restart from a clean search at dawn: these phases re-issue
                // their own movement from Seeking, while a phase abandoned
                // with its MoveTarget stripped could stand at sunrise waiting
                // for a walk order that never comes back.
                routine.phase = ConstructionMaterialPhase::Seeking;
            }
            if move_target.is_some() || travel_route.is_some() {
                commands
                    .entity(builder)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>();
            }
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        }
        let Ok((
            _,
            mut site,
            mut site_view,
            _site_position,
            planned_access,
            supply_cooldown,
            site_is_player_project,
            site_listed_for_sale,
        )) = sites.get_mut(routine.site)
        else {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<PlayerConstructionAssignment>()
                .remove::<MootQueueTicket>()
                .remove::<MootQueueTransit>()
                .remove::<MoveTarget>();
            activity.set_if_neq(CharacterActivity::Idle);
            continue;
        };
        if site.stage != BuildStage::Supplying {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<MootQueueTicket>()
                .remove::<MootQueueTransit>();
            continue;
        }

        // A Hall pickup uses the shared visible FIFO line. Its movement and
        // route recovery belong to the queue until the counter says Ready;
        // interpreting those markers as a failed material route would eject
        // the builder from the line every frame.
        if queue_ticket.is_some()
            && matches!(
                routine.phase,
                ConstructionMaterialPhase::CollectingFromStore { .. }
            )
        {
            route_failed = None;
        }

        if let Some(failed) = route_failed {
            if matches!(
                routine.phase,
                ConstructionMaterialPhase::WalkingToTree { .. }
            ) {
                debug!(
                    "Village supplier {} could not reach tree stand {:.1},{:.1}; trying another tree",
                    name.0, failed.goal.x, failed.goal.z
                );
                let widened = postpone_construction_tree_search(&mut routine, now);
                if widened {
                    warn!(
                        "Village supplier {} exhausted twelve tree approaches; pausing the unavailable timber search",
                        name.0
                    );
                }
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
            } else if matches!(
                routine.phase,
                ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
                    | ConstructionMaterialPhase::Delivering { .. }
                    | ConstructionMaterialPhase::LeavingDeliveryAccess { .. }
            ) {
                // Material delivery predates the physical connector, but its
                // permit-time access reservation begins at a certified door
                // apron. Recover to that exact frontage rather than spiralling
                // ever farther around the plot: the old growing-radius retry
                // eventually selected water, map edges and crop plots and
                // could cycle hundreds of failed routes despite a valid road
                // claim sitting on the same worksite.
                routine.failed_delivery_routes = routine.failed_delivery_routes.wrapping_add(1);
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
                let leaving_access = matches!(
                    routine.phase,
                    ConstructionMaterialPhase::LeavingDeliveryAccess { .. }
                );
                if routine.failed_delivery_routes >= MAX_CERTIFIED_DELIVERY_FAILURES
                    && !leaving_access
                {
                    // The access corridor was accepted by the road-width
                    // terrain/building proof. Repeated tactical failures here
                    // mean the bounded fine-grid planner cannot rediscover it
                    // before construction creates the actual road. Preserve
                    // the physical cargo and transaction, but complete this
                    // last-mile handoff abstractly instead of deadlocking the
                    // settlement forever.
                    warn!(
                        "Village supplier {} could not traverse the certified {} access after {} routes; completing the carried-material handoff",
                        name.0,
                        site.kind.label(),
                        routine.failed_delivery_routes,
                    );
                    routine.phase = ConstructionMaterialPhase::Delivering {
                        destination: position.0,
                    };
                } else if leaving_access {
                    if routine.failed_delivery_routes >= MAX_CERTIFIED_DELIVERY_FAILURES {
                        // The cargo transaction is already complete. A tree
                        // or later shell can obstruct a long permit-time
                        // corridor before its physical road is finished; do
                        // not spend hundreds of long A* retries merely to
                        // return to the public-network endpoint. Keep the
                        // builder at their embodied position and let the next
                        // material decision choose a fresh reachable target.
                        warn!(
                            "Village supplier {} could not leave the certified {} access after {} routes; resuming material work from the current position",
                            name.0,
                            site.kind.label(),
                            routine.failed_delivery_routes,
                        );
                        routine.failed_delivery_routes = 0;
                        routine.phase = ConstructionMaterialPhase::Seeking;
                    } else {
                        debug!(
                            "Village supplier {} could not leave the delivery corridor at {:.1},{:.1}; retrying its certified egress",
                            name.0, failed.goal.x, failed.goal.z,
                        );
                        routine.phase = begin_material_egress(
                            &mut commands,
                            builder,
                            position.0,
                            &terrain,
                            planned_access,
                        );
                    }
                } else {
                    debug!(
                        "Village supplier {} could not reach delivery anchor {:.1},{:.1}; retrying through the certified access corridor",
                        name.0,
                        failed.goal.x,
                        failed.goal.z,
                    );
                    routine.phase = begin_material_delivery(
                        &mut commands,
                        builder,
                        position.0,
                        site.stand,
                        &terrain,
                        planned_access,
                    );
                }
            } else if matches!(
                routine.phase,
                ConstructionMaterialPhase::CollectingFromStore { .. }
                    | ConstructionMaterialPhase::UnloadingAtHall { .. }
            ) {
                // The old implementation handled failed tree and site routes
                // but left a failed trip to the Moot store attached forever.
                // Back off from that entrance and gather timber directly so a
                // single inaccessible market approach cannot freeze a site.
                debug!(
                    "Village supplier {} could not reach the Moot store at {:.1},{:.1}; falling back to gathered timber",
                    name.0, failed.goal.x, failed.goal.z
                );
                postpone_construction_store_route(&mut routine, now);
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
            } else {
                // Seeking and Chopping do not own navigation. Any failure seen
                // there belongs to an older task and must never become a
                // permanent guard that suppresses construction every tick.
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
            }
            continue;
        }

        let required = site.kind.construction_wood_required();
        let delivered = inventories
            .get(routine.site)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);
        if delivered >= required {
            activity.set_if_neq(CharacterActivity::Idle);
            // Release the hall freight-lane ticket too: a builder whose site
            // was topped up externally while they queued would otherwise hold
            // a Ready ticket forever and jam every later pickup at this hall.
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<MootQueueTicket>()
                .remove::<MootQueueTransit>()
                .insert(MoveTarget(site.stand));
            continue;
        }

        let Some((
            hall_entity,
            mut settlement,
            settlement_id,
            hall_position,
            hall_rotation,
            administration,
            policies,
            mut civic_account,
        )) = settlements
            .iter_mut()
            .find(|(entity, ..)| *entity == site.settlement)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map(|rotation| rotation.0).unwrap_or(0.0),
        );
        let public_store_entrance = nearest_public_market_entrance(
            position.0,
            hall_entrance,
            marketplaces
                .iter()
                .filter(|(building, building_of, ..)| {
                    building.kind == SettlementBuildingKind::Market
                        && building_of.0 == *settlement_id
                })
                .map(|(building, _, at, rotation)| {
                    building.kind.entrance_position(at.0, rotation.0)
                }),
        );
        let carried_wood = inventories
            .get(builder)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);

        // Once Wood changes hands, retain the ready freight ticket until the
        // delivery route is actually installed (or the builder has physically
        // reached a very close target). This keeps one body at the counter,
        // gives pathfinding a stable start instead of a crowd-shoved pile, and
        // only then lets the next freight applicant advance.
        let holding_freight_counter = queue_ticket.is_some_and(|ticket| {
            ticket.kind == MootServiceKind::ConstructionMaterial
                && carried_wood > 0
                && !matches!(
                    routine.phase,
                    ConstructionMaterialPhase::CollectingFromStore { .. }
                )
        });
        if holding_freight_counter {
            let phase_target = match routine.phase {
                ConstructionMaterialPhase::ApproachingDeliveryAccess { entry } => Some(entry),
                ConstructionMaterialPhase::Delivering { destination } => Some(destination),
                ConstructionMaterialPhase::LeavingDeliveryAccess { exit } => Some(exit),
                _ => None,
            };
            let route_ready = travel_route.is_some()
                || route_failed.is_some()
                || phase_target
                    .is_some_and(|target| ground_distance(position.0, target) <= WORK_REACH);
            if route_ready {
                commands
                    .entity(builder)
                    .remove::<MootQueueTicket>()
                    .remove::<MootQueueTransit>();
            } else {
                continue;
            }
        }
        let carries_other_goods = inventories.get(builder).is_ok_and(|inventory| {
            Good::ALL
                .iter()
                .any(|good| *good != Good::Wood && inventory.amount(*good) > 0)
        });
        let owns_material_priority = material_priority
            .get(&site.settlement)
            .is_none_or(|(_, _, priority)| *priority == routine.site);

        match routine.phase {
            ConstructionMaterialPhase::Seeking => {
                activity.set_if_neq(CharacterActivity::Idle);
                if carried_wood > 0 {
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    routine.phase = begin_material_delivery(
                        &mut commands,
                        builder,
                        position.0,
                        site.stand,
                        &terrain,
                        planned_access,
                    );
                    continue;
                }
                if carries_other_goods {
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, public_store_entrance);
                    routine.phase = ConstructionMaterialPhase::UnloadingAtHall {
                        hall: hall_entity,
                        entrance: public_store_entrance,
                    };
                    continue;
                }
                let public_budget = crate::world::village::civic::civic_discretionary_budget(
                    &settlement,
                    administration,
                    policies,
                );
                let affordable = if site.owner_id.is_none() {
                    markets.get(hall_entity).is_ok_and(|market| {
                        market
                            .preview_purchase(
                                Good::Wood,
                                1,
                                public_budget,
                                None,
                                Some(shared::economy::MarketSeller::Treasury(*settlement_id)),
                            )
                            .units
                            > 0
                    })
                } else if site.owner_id == Some(*person_id) {
                    match (markets.get(hall_entity), wallet.as_deref()) {
                        (Ok(market), Some(wallet)) => {
                            wallet.can_afford(market.pool(Good::Wood).ask)
                        }
                        // Compatibility for focused tests assembled without the
                        // finance initialiser. A live village always has both.
                        _ => true,
                    }
                } else {
                    site.owner_id.is_some_and(|owner_id| {
                        non_builder_owner_wallets
                            .iter()
                            .find(|(candidate, _)| **candidate == owner_id)
                            .is_some_and(|(_, owner_wallet)| {
                                markets.get(hall_entity).is_ok_and(|market| {
                                    owner_wallet.can_afford(market.pool(Good::Wood).ask)
                                })
                            })
                    })
                };
                let source = inventories
                    .get(hall_entity)
                    .is_ok_and(|inventory| {
                        now >= routine.store_retry_after
                            && inventory.amount(Good::Wood)
                                > reserved_store_wood.get(&hall_entity).copied().unwrap_or(0)
                            && affordable
                    })
                    .then_some((hall_entity, public_store_entrance));
                // A give-up must know the market was REALLY dry this tick -
                // nothing purchasable at any price for this owner - not merely
                // inside the post-failure store-route backoff window.
                let market_durably_dry = source.is_none() && now >= routine.store_retry_after;
                if let Some((source, entrance)) = source {
                    let available_wood = inventories
                        .get(hall_entity)
                        .map(|inventory| inventory.amount(Good::Wood))
                        .unwrap_or(0);
                    let outstanding = material_outstanding
                        .get(&site.settlement)
                        .copied()
                        .unwrap_or(required.saturating_sub(delivered));
                    // Serialising genuinely scarce founding stock prevents it
                    // being smeared across dozens of half-supplied sites. Once
                    // the market physically holds enough Wood to cover every
                    // outstanding site, however, a poor or temporarily
                    // unreachable priority owner must not block all other
                    // affordable builders behind it.
                    if !material_stock_may_supply(
                        owns_material_priority,
                        available_wood,
                        outstanding,
                    ) {
                        commands.entity(builder).remove::<ambient::AmbientRoutine>();
                        wait_at_own_worksite(
                            &mut commands,
                            builder,
                            position.0,
                            move_target,
                            site.stand,
                        );
                        continue;
                    }
                    let available_unreserved = available_wood
                        .saturating_sub(reserved_store_wood.get(&source).copied().unwrap_or(0));
                    let carry_room = inventories
                        .get(builder)
                        .map(|inventory| inventory.free_bulk() / Good::Wood.bulk_per_unit())
                        .unwrap_or(0);
                    let reserved_units = required
                        .saturating_sub(delivered)
                        .min(carry_room)
                        .min(available_unreserved);
                    if reserved_units == 0 {
                        wait_at_own_worksite(
                            &mut commands,
                            builder,
                            position.0,
                            move_target,
                            site.stand,
                        );
                        continue;
                    }
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    *reserved_store_wood.entry(source).or_default() += reserved_units;
                    routine.phase = ConstructionMaterialPhase::CollectingFromStore {
                        source,
                        entrance,
                        reserved_units,
                    };
                    if let Some(clock) = queue_clock.as_deref_mut() {
                        // The stock remains physically in the Hall until this
                        // applicant reaches the counter. The shared line gives
                        // every builder a distinct position and fast 0.5s
                        // handover instead of letting a dozen bodies occupy
                        // the exact authored door marker.
                        enqueue_moot_service(
                            &mut commands,
                            clock,
                            builder,
                            hall_entity,
                            MootServiceKind::ConstructionMaterial,
                        );
                    } else {
                        // Focused tests assembled without the queue resource
                        // retain the direct physical pickup seam.
                        ensure_move_target(&mut commands, builder, move_target, entrance);
                    }
                    continue;
                }

                if now < routine.tree_retry_after || tree_proof_used {
                    wait_at_own_worksite(
                        &mut commands,
                        builder,
                        position.0,
                        move_target,
                        site.stand,
                    );
                    continue;
                }
                commands.entity(builder).remove::<ambient::AmbientRoutine>();
                tree_proof_used = true;

                let salt = stable_name_hash(&name.0) ^ routine.site.to_bits() as u32;
                let (tree, stand) = match find_tree_for_cycle_cached(
                    &mut tree_candidates,
                    &terrain,
                    derived.as_deref(),
                    obstacles.as_deref(),
                    // A remote building plot is not the tree-search centre.
                    // Founding builders share the settlement's reachable
                    // woodland around its hall, then carry timber out to the
                    // site; otherwise an edge cabin can back off forever even
                    // while the same village visibly has a healthy forest.
                    hall_position.0,
                    routine.cycle,
                    salt,
                ) {
                    TreeCandidateLookup::Pending => {
                        // The immutable 5x5-chunk prop search is filled a few
                        // chunks per tick. Do not count cache preparation as a
                        // failed tree or impose a gameplay backoff.
                        wait_at_own_worksite(
                            &mut commands,
                            builder,
                            position.0,
                            move_target,
                            site.stand,
                        );
                        continue;
                    }
                    TreeCandidateLookup::Unavailable => {
                        // No locally valid stand for this deterministic
                        // candidate. Back off rather than retrying a sparse
                        // grove at the server tick rate.
                        let widened = postpone_construction_tree_search(&mut routine, now);
                        if widened
                            && market_durably_dry
                            && !player_assigned
                            && !site_is_player_project
                            && !site_listed_for_sale
                        {
                            give_up_starved_supply(
                                &mut commands,
                                builder,
                                name,
                                routine.site,
                                &mut site,
                                supply_cooldown,
                                now,
                            );
                            continue;
                        }
                        wait_at_own_worksite(
                            &mut commands,
                            builder,
                            position.0,
                            move_target,
                            site.stand,
                        );
                        continue;
                    }
                    TreeCandidateLookup::Found { tree, stand } => (tree, stand),
                };
                // Reject an obviously water-separated candidate before it
                // enters the ordinary navigation queue. This must stay a
                // cheap pre-check: running the permit-time 2,000-node landmass
                // A* for every timber retry produced repeated 100-185 ms
                // construction ticks under a 600-person load. The tactical
                // planner immediately below remains the authoritative,
                // obstacle-aware route proof.
                if !crate::world::village_roads::road_segment_is_coarsely_dry(
                    &terrain,
                    Vec2::new(hall_entrance.x, hall_entrance.z),
                    Vec2::new(stand.x, stand.z),
                ) {
                    let widened = postpone_construction_tree_search(&mut routine, now);
                    if widened
                        && market_durably_dry
                        && !player_assigned
                        && !site_is_player_project
                        && !site_listed_for_sale
                    {
                        give_up_starved_supply(
                            &mut commands,
                            builder,
                            name,
                            routine.site,
                            &mut site,
                            supply_cooldown,
                            now,
                        );
                        continue;
                    }
                    if widened {
                        warn!(
                            "Village supplier {} found no reachable timber after twelve candidates; waiting for market stock or terrain change",
                            name.0
                        );
                    }
                    wait_at_own_worksite(
                        &mut commands,
                        builder,
                        position.0,
                        move_target,
                        site.stand,
                    );
                    continue;
                }
                routine.tree_retry_after = 0.0;
                ensure_move_target(&mut commands, builder, move_target, stand);
                routine.phase = ConstructionMaterialPhase::WalkingToTree { tree, stand };
            }
            ConstructionMaterialPhase::UnloadingAtHall { hall, entrance } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                if let Ok([mut carrier, mut hall_store]) = inventories.get_many_mut([builder, hall])
                {
                    for good in Good::ALL {
                        if good != Good::Wood {
                            if let Ok(mut market) = markets.get_mut(hall) {
                                sell_carried_to_moot(
                                    &mut commands,
                                    hall,
                                    *person_id,
                                    wallet.as_deref_mut(),
                                    None,
                                    good,
                                    &mut carrier,
                                    &mut hall_store,
                                    &mut market,
                                );
                                // A buying pool with no cash must not trap a
                                // temporary builder forever. Unsold goods are
                                // consigned to common storage without creating
                                // coin; they remain physical and marketable.
                                carrier.transfer_to(&mut hall_store, good, u32::MAX);
                            } else {
                                carrier.transfer_to(&mut hall_store, good, u32::MAX);
                            }
                        }
                    }
                }
                routine.phase = ConstructionMaterialPhase::Seeking;
            }
            ConstructionMaterialPhase::CollectingFromStore {
                source,
                entrance,
                reserved_units,
            } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if let Some(ticket) = queue_ticket {
                    if ticket.kind != MootServiceKind::ConstructionMaterial || !ticket.is_ready() {
                        continue;
                    }
                } else if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    continue;
                }
                commands
                    .entity(builder)
                    .remove::<MootQueueTransit>()
                    .remove::<MoveTarget>();
                let remaining = required.saturating_sub(delivered);
                let moved = if let Ok([mut source_store, mut carrier]) =
                    inventories.get_many_mut([source, builder])
                {
                    let carry_room = carrier.free_bulk() / Good::Wood.bulk_per_unit();
                    let requested = remaining.min(carry_room).min(reserved_units);
                    if site.owner_id.is_none() {
                        let budget = crate::world::village::civic::civic_discretionary_budget(
                            &settlement,
                            administration,
                            policies,
                        );
                        match markets.get_mut(source) {
                            Ok(mut market) => {
                                let purchase = market.purchase(
                                    Good::Wood,
                                    requested,
                                    budget,
                                    None,
                                    Some(shared::economy::MarketSeller::Treasury(*settlement_id)),
                                );
                                if purchase.trade.units == 0
                                    || settlement.treasury < purchase.trade.pennies
                                {
                                    0
                                } else {
                                    let moved = source_store.transfer_to(
                                        &mut carrier,
                                        Good::Wood,
                                        purchase.trade.units,
                                    );
                                    debug_assert_eq!(moved, purchase.trade.units);
                                    settlement.treasury -= purchase.trade.pennies;
                                    if let Some(account) = civic_account.as_deref_mut() {
                                        account.record_material_expense(
                                            day.saturating_add(1),
                                            purchase.trade.pennies,
                                        );
                                    }
                                    business_events.record_market_purchase(
                                        day,
                                        *settlement_id,
                                        purchase.fills,
                                    );
                                    moved
                                }
                            }
                            Err(_) => 0,
                        }
                    } else if site.owner_id == Some(*person_id) {
                        match (markets.get_mut(source), wallet.as_deref_mut()) {
                            (Ok(mut market), Some(wallet)) => buy_from_moot(
                                day,
                                site.settlement_id,
                                Good::Wood,
                                requested,
                                wallet,
                                &mut source_store,
                                &mut carrier,
                                &mut market,
                                &mut business_events,
                            ),
                            _ => source_store.transfer_to(&mut carrier, Good::Wood, requested),
                        }
                    } else if let Some(owner_id) = site.owner_id {
                        match (
                            markets.get_mut(source),
                            non_builder_owner_wallets
                                .iter_mut()
                                .find(|(candidate, _)| **candidate == owner_id),
                        ) {
                            (Ok(mut market), Some((_, mut owner_wallet))) => buy_from_moot(
                                day,
                                site.settlement_id,
                                Good::Wood,
                                requested,
                                &mut owner_wallet,
                                &mut source_store,
                                &mut carrier,
                                &mut market,
                                &mut business_events,
                            ),
                            // If the owning character genuinely vanished, do
                            // not transfer private hall stock for free. The
                            // worker falls back to gathering physical timber.
                            _ => 0,
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };
                if moved > 0 {
                    routine.failed_store_routes = 0;
                    routine.store_retry_after = 0.0;
                    routine.phase = begin_material_delivery(
                        &mut commands,
                        builder,
                        position.0,
                        site.stand,
                        &terrain,
                        planned_access,
                    );
                } else {
                    // Nothing changed hands (listings sold out under the
                    // reservation, or funds fell short at the counter).
                    // Back off before seeking again - an instant re-queue
                    // can cycle a wood-less builder through the freight line
                    // forever without a single delivery.
                    commands.entity(builder).remove::<MootQueueTicket>();
                    routine.phase = ConstructionMaterialPhase::Seeking;
                    postpone_construction_store_route(&mut routine, now);
                }
            }
            ConstructionMaterialPhase::WalkingToTree { tree, stand } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, stand) <= WORK_REACH {
                    routine.failed_tree_routes = 0;
                    routine.tree_retry_after = 0.0;
                    commands.entity(builder).remove::<MoveTarget>();
                    let to_tree = tree - position.0;
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.z);
                    }
                    activity.set_if_neq(CharacterActivity::Chopping);
                    routine.phase = ConstructionMaterialPhase::Chopping {
                        tree,
                        seconds_left: CHOP_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, builder, move_target, stand);
                }
            }
            ConstructionMaterialPhase::Chopping { tree, seconds_left } => {
                activity.set_if_neq(CharacterActivity::Chopping);
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = ConstructionMaterialPhase::Chopping {
                        tree,
                        seconds_left: left,
                    };
                    continue;
                }
                let remaining = required.saturating_sub(delivered);
                if let Ok(mut carrier) = inventories.get_mut(builder) {
                    // This is a deadlock escape, not a substitute timber
                    // industry. An ordinary builder recovers two usable bundles
                    // per tree; a professional woodcutter recovers three, works
                    // faster in good forest, and can sell through the market.
                    carrier.add(Good::Wood, remaining.min(SELF_SUPPLY_TREE_YIELD));
                }
                routine.cycle = routine.cycle.wrapping_add(1);
                activity.set_if_neq(CharacterActivity::Idle);
                routine.phase = begin_material_delivery(
                    &mut commands,
                    builder,
                    position.0,
                    site.stand,
                    &terrain,
                    planned_access,
                );
            }
            ConstructionMaterialPhase::ApproachingDeliveryAccess { entry, .. } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, entry) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, entry);
                    continue;
                }
                routine.phase = begin_material_delivery(
                    &mut commands,
                    builder,
                    position.0,
                    site.stand,
                    &terrain,
                    planned_access,
                );
            }
            ConstructionMaterialPhase::Delivering { destination } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, destination) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, destination);
                    continue;
                }
                routine.failed_delivery_routes = 0;
                commands.entity(builder).remove::<MoveTarget>();
                let moved = if let Ok([mut carrier, mut site_store]) =
                    inventories.get_many_mut([builder, routine.site])
                {
                    carrier.transfer_to(&mut site_store, Good::Wood, required - delivered)
                } else {
                    0
                };
                let now_delivered = delivered.saturating_add(moved);
                info!(
                    "Village '{}': {} delivered wood to the {} ({now_delivered}/{required})",
                    settlement.name,
                    name.0,
                    site.kind.label(),
                );
                if now_delivered >= required {
                    // This exact point was just reached through the live route
                    // planner. Reuse that proof for the final hammering walk;
                    // the authored front stand may have become obstructed while
                    // the builder was gathering several timber loads.
                    site.stand = position.0;
                    if let Some(site_view) = site_view.as_deref_mut() {
                        site_view.stand = position.0;
                    }
                    commands
                        .entity(builder)
                        .remove::<ConstructionMaterialRoutine>()
                        .insert(MoveTarget(position.0));
                } else {
                    routine.phase = begin_material_egress(
                        &mut commands,
                        builder,
                        position.0,
                        &terrain,
                        planned_access,
                    );
                }
            }
            ConstructionMaterialPhase::LeavingDeliveryAccess { exit } => {
                activity.set_if_neq(CharacterActivity::Idle);
                if ground_distance(position.0, exit) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, exit);
                    continue;
                }
                routine.failed_delivery_routes = 0;
                commands
                    .entity(builder)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>();
                routine.phase = ConstructionMaterialPhase::Seeking;
            }
        }
    }
}

/// Drive every permitted building from grant to standing.
///
/// Three things happen in order, and the order is the point: the builder walks
/// out, the plot is cleared and levelled, and only then does the frame go up.
/// A building that simply materialised on a timer told you nothing about who
/// built it or what it cost.
#[allow(clippy::too_many_arguments)]
pub fn advance_construction(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    mut terrain: Option<ResMut<WorldTerrain>>,
    mut deltas: ResMut<PublishedTerrainDeltas>,
    settlements: Query<(&Settlement, &shared::components::SettlementId)>,
    positions: Query<&PlayerPosition>,
    move_targets: Query<&MoveTarget>,
    route_failures: Query<&NavigationRouteFailed>,
    home_routines: Query<(), With<HomeRoutine>>,
    mut intents: Query<&mut VillagerIntent>,
    mut activities: Query<&mut CharacterActivity>,
    mut pending: Query<(
        Entity,
        &mut UnderConstruction,
        &GoodsInventory,
        Option<&PlannedRoadAccess>,
        Option<&InheritedBusinessCapital>,
        Option<&BusinessProjectAccounting>,
        Option<&crate::player::permits::PlayerConstructionProject>,
        Option<&shared::components::HouseAppearance>,
    )>,
    mut sites: Query<&mut shared::components::ConstructionSite>,
    mut facings: Query<&mut PlayerRotation>,
    player_assignments: Query<&PlayerConstructionAssignment>,
) {
    let world_dt = simulation_time.world_seconds();
    let daylight = world_time.iter().next().is_none_or(WorldTime::is_day);
    for (
        site,
        mut under,
        materials,
        planned_access,
        inherited_capital,
        project_accounting,
        player_project,
        house_appearance,
    ) in pending.iter_mut()
    {
        let Ok((settlement, settlement_id)) = settlements.get(under.settlement) else {
            // Its settlement vanished; drop the site rather than leaving a
            // building belonging to nowhere.
            if let Some(builder) = under.builder {
                if let Ok(mut activity) = activities.get_mut(builder) {
                    activity.set_if_neq(CharacterActivity::Idle);
                }
            }
            release_builder(&mut commands, &mut intents, under.builder, None);
            commands.entity(site).despawn();
            continue;
        };

        // Settlement builders go home at night. Directly commanded heroes are
        // outside that employment schedule, so their private work order keeps
        // progressing until the player changes it or the building completes.
        let player_commanded = under.builder.is_some_and(|builder| {
            player_assignments.get(builder).is_ok_and(|assignment| {
                assignment.site == site && assignment.settlement == under.settlement
            })
        });
        if !daylight && !player_commanded {
            continue;
        }
        if !player_commanded
            && under
                .builder
                .is_some_and(|builder| home_routines.get(builder).is_ok())
        {
            continue;
        }

        match under.stage {
            BuildStage::Supplying => {
                let Some(builder) = under.builder else {
                    // A death or departure does not erase a permitted plot,
                    // its delivered Wood, or inherited-business escrow. The
                    // recovery/succession systems assign another builder.
                    continue;
                };
                if positions.get(builder).is_err() {
                    under.builder = None;
                    continue;
                }
                let required = under.kind.construction_wood_required();
                if materials.amount(Good::Wood) < required {
                    continue;
                }
                under.stage = BuildStage::Walking;
                commands
                    .entity(builder)
                    .remove::<ConstructionMaterialRoutine>()
                    .insert(MoveTarget(under.stand));
                info!(
                    "Village '{}': {} fully supplied ({required} wood)",
                    settlement.name,
                    under.kind.label(),
                );
            }
            BuildStage::Walking => {
                // Keep the supplied worksite intact while succession or the
                // orphan-construction pass finds a replacement builder.
                let Some(builder) = under.builder else {
                    continue;
                };
                let Ok(at) = positions.get(builder) else {
                    under.builder = None;
                    continue;
                };
                if route_failures
                    .get(builder)
                    .is_ok_and(|failed| failed.goal.distance_squared(under.stand) <= 0.01)
                {
                    under.failed_stand_routes = under.failed_stand_routes.saturating_add(1);
                    let attempt = u32::from(under.failed_stand_routes);
                    let angle =
                        under.rotation + (attempt % 12) as f32 * std::f32::consts::TAU / 12.0;
                    let footprint = under
                        .kind
                        .art_with_house(house_appearance)
                        .definition()
                        .footprint;
                    let radius =
                        footprint.x.max(footprint.y) * 0.5 + 2.0 + (attempt / 12) as f32 * 2.0;
                    let x = under.position.x + angle.sin() * radius;
                    let z = under.position.z + angle.cos() * radius;
                    under.stand = Vec3::new(
                        x,
                        terrain
                            .as_deref()
                            .map_or(under.position.y, |terrain| terrain.get_height(x, z)),
                        z,
                    );
                    if let Ok(mut site_view) = sites.get_mut(site) {
                        site_view.stand = under.stand;
                    }
                    commands
                        .entity(builder)
                        .remove::<NavigationRouteFailed>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>()
                        .insert(MoveTarget(under.stand));
                    warn!(
                        "Village '{}': builder could not reach the {} work point; trying perimeter approach {} at {:.1},{:.1}",
                        settlement.name,
                        under.kind.label(),
                        attempt,
                        under.stand.x,
                        under.stand.z,
                    );
                    continue;
                }
                if at.0.distance(under.stand) > BUILD_REACH {
                    if move_targets.get(builder).is_err() {
                        commands.entity(builder).insert(MoveTarget(under.stand));
                    }
                    continue;
                }

                // On site. Clear the plot before anything is raised on it.
                let earthwork_seconds = terrain.as_deref().map_or(0.0, |terrain| {
                    if under.kind == SettlementBuildingKind::Farmstead {
                        super::planning::farmstead_earthwork_effort(
                            terrain,
                            under.position,
                            under.rotation,
                        )
                        .map_or(0.0, |effort| 2.0 + effort * 6.0)
                    } else {
                        0.0
                    }
                });
                if let Some(terrain) = terrain.as_mut() {
                    clear_and_level(terrain, &mut deltas, &mut commands, &under);
                }
                // Carrying these two is what makes the world treat the plot as
                // built-on: the client stops scattering props inside it, the
                // server drops the tree colliders and marks it as an obstacle.
                // Attached NOW, at clearing, not at completion -- that is the
                // difference between a site being cleared and a building
                // appearing on top of standing trees.
                commands.entity(site).insert((
                    shared::building::PlacedBuilding {
                        building_type: under.kind.art_with_house(house_appearance),
                        rotation: under.rotation,
                    },
                    shared::building::BuildingPosition(under.position),
                ));
                under.stage = BuildStage::Raising {
                    seconds_left: BUILD_SECONDS + earthwork_seconds,
                };
                // Construction owns the builder now. Leaving the walk target
                // attached lets `step_units` run later in the fixed schedule
                // and overwrite the inward-facing rotation with the final
                // approach direction -- exactly the "back to the house" bug.
                commands.entity(builder).remove::<MoveTarget>();
                // One flip, one replication. The client runs its own clock from
                // here so the frame can rise out of the ground without the
                // server streaming a progress float at tick rate.
                if let Ok(mut site_view) = sites.get_mut(site) {
                    site_view.raising = true;
                }
                // Turn them to face the work. They arrive facing whichever way
                // they were walking, which is away from the plot as often as
                // not, and a builder hammering with their back to the house
                // reads as broken.
                //
                // The shipped character faces local -Z. Face that rendered
                // front toward the plot, then keep movement from overwriting it
                // by removing the completed walk target above.
                if let Some(builder) = under.builder {
                    if let Ok(mut facing) = facings.get_mut(builder) {
                        let to_work = under.position - under.stand;
                        if to_work.length_squared() > 1e-4 {
                            facing.0 = build_clip_facing(to_work);
                        }
                    }
                }
                info!(
                    "Village '{}': ground cleared for a {}",
                    settlement.name,
                    under.kind.label()
                );
            }
            BuildStage::Raising { seconds_left } => {
                if let Some(builder) = under.builder {
                    if let Ok(mut activity) = activities.get_mut(builder) {
                        activity.set_if_neq(CharacterActivity::Building);
                    }
                }
                let left = seconds_left - world_dt;
                if left > 0.0 {
                    under.stage = BuildStage::Raising { seconds_left: left };
                    continue;
                }
                let building_entity = commands
                    .spawn((
                        SettlementBuilding {
                            kind: under.kind,
                            settlement: settlement.name.clone(),
                            owner: under.owner.clone(),
                            quality: under.quality,
                            workers: Vec::new(),
                        },
                        shared::economy::GoodsInventory::new(under.kind.storage_bulk_capacity()),
                        PlayerPosition(under.position),
                        PlayerRotation(under.rotation),
                        // The finished building takes over the plot claim from the
                        // site, so the ground stays clear once the site despawns.
                        shared::building::PlacedBuilding {
                            building_type: under.kind.art_with_house(house_appearance),
                            rotation: under.rotation,
                        },
                        shared::building::BuildingPosition(under.position),
                        // Region tagging runs later in the shared village schedule;
                        // only the settlement's lightweight summary stays global.
                        Replicate::to_clients(NetworkTarget::All),
                    ))
                    .id();
                commands
                    .entity(building_entity)
                    .insert(shared::components::BuildingOf(*settlement_id));
                if let Some(appearance) = house_appearance {
                    commands.entity(building_entity).insert(*appearance);
                }
                if let Some(owner_id) = under.owner_id {
                    commands
                        .entity(building_entity)
                        .insert(shared::components::OwnedBy(owner_id));
                }
                if let Some(capital) = inherited_capital {
                    // Keep takeover money in an authoritative account on the
                    // completed entity immediately. The next economy pass
                    // atomically posts this escrow to the operating company.
                    // Deferring that conversion by one tick prevents a site
                    // despawn and a separately queued account insert from
                    // exposing a one-frame loss at schedule boundaries.
                    commands
                        .entity(building_entity)
                        .insert(InheritedBusinessCapital(capital.0));
                }
                if let Some(project_accounting) = project_accounting {
                    commands.entity(building_entity).insert(*project_accounting);
                    if let Some(company) = project_accounting.company {
                        commands
                            .entity(building_entity)
                            .insert(shared::components::OperatedBy(company));
                    }
                }
                if let Some(planned_access) = planned_access {
                    commands
                        .entity(building_entity)
                        .insert(planned_access.clone());
                }
                info!(
                    "Village '{}': {} completed ({:.0}% ground)",
                    settlement.name,
                    under.kind.label(),
                    under.quality * 100.0
                );
                if let Some(builder) = under.builder {
                    if player_project.is_some() {
                        // The hero's explicit construction order ends with the
                        // building. Road access remains a civic repair backlog;
                        // completing a private windmill must not silently turn
                        // the player into a municipal road worker.
                        commands
                            .entity(building_entity)
                            .insert(crate::world::village_roads::RoadRepairBacklog);
                        commands
                            .entity(builder)
                            .remove::<MoveTarget>()
                            .remove::<ConstructionMaterialRoutine>()
                            .remove::<PlayerConstructionAssignment>();
                        if let Ok(mut activity) = activities.get_mut(builder) {
                            activity.set_if_neq(CharacterActivity::Idle);
                        }
                    } else {
                        // The person who raised the building owns the last piece of
                        // work too: joining its authored door to the village path
                        // network. `plan_requested_roads` adopts them next in the
                        // chained schedule and releases them if no route is viable.
                        commands.entity(building_entity).insert(RoadRequest {
                            builder,
                            settlement: under.settlement,
                            completed_site: site,
                            attempt: 0,
                        });
                        commands
                            .entity(builder)
                            .remove::<MoveTarget>()
                            .remove::<ConstructionMaterialRoutine>();
                    }
                } else {
                    release_builder(&mut commands, &mut intents, None, Some(under.settlement));
                }
                commands.entity(site).despawn();
            }
        }
    }
}

/// Put a builder back to ordinary residency.
fn release_builder(
    commands: &mut Commands,
    intents: &mut Query<&mut VillagerIntent>,
    builder: Option<Entity>,
    settlement: Option<Entity>,
) {
    let Some(builder) = builder else { return };
    commands
        .entity(builder)
        .remove::<MoveTarget>()
        .remove::<ConstructionMaterialRoutine>()
        .remove::<PlayerConstructionAssignment>();
    if let Ok(mut intent) = intents.get_mut(builder) {
        *intent = match settlement {
            Some(settlement) => VillagerIntent::Resident { settlement },
            None => VillagerIntent::Idle,
        };
    }
}

/// Level the plot and publish the change so clients see the same ground.
///
/// The flatten itself already existed in the shared terrain-editing primitives,
/// as did the client's ingestion of replicated deltas. The server now authors
/// and publishes the edit when embodied construction clears the plot.
fn clear_and_level(
    terrain: &mut WorldTerrain,
    deltas: &mut PublishedTerrainDeltas,
    commands: &mut Commands,
    under: &UnderConstruction,
) {
    let affected = level_construction_ground(terrain, under);

    publish_terrain_chunks(terrain, deltas, commands, affected);
}

fn publish_terrain_chunks(
    terrain: &WorldTerrain,
    deltas: &mut PublishedTerrainDeltas,
    commands: &mut Commands,
    affected: impl IntoIterator<Item = shared::terrain::ChunkCoord>,
) {
    for coord in affected {
        let Some(data) = terrain.get_delta_chunk(coord) else {
            continue;
        };
        let chunk = shared::terrain::TerrainDeltaChunk::from_delta_data(coord, data);
        match deltas.by_chunk.get(&coord) {
            // Update in place: a second building in the same chunk must not
            // spawn a second authority for that chunk's heights.
            Some(entity) => {
                commands.entity(*entity).insert(chunk);
            }
            None => {
                let entity = commands
                    .spawn((chunk, Replicate::to_clients(NetworkTarget::All)))
                    .id();
                deltas.by_chunk.insert(coord, entity);
            }
        }
    }
}

fn level_building_ground(
    terrain: &mut WorldTerrain,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
) -> Vec<shared::terrain::ChunkCoord> {
    let def = kind.art().definition();
    // Level TO the plot's own height, so a building on a slope cuts a terrace
    // rather than the whole village drifting to one altitude.
    let ground = terrain.get_height(position.x, position.z);
    let footprint_center = def.world_footprint_center(position, rotation);
    let centre = Vec3::new(footprint_center.x, ground, footprint_center.y);
    terrain.apply_flatten_rect(
        centre,
        def.terrain_flat_half_extents(),
        rotation,
        def.terrain_blend_width(),
    )
}

fn level_construction_ground(
    terrain: &mut WorldTerrain,
    under: &UnderConstruction,
) -> Vec<shared::terrain::ChunkCoord> {
    let mut affected = level_building_ground(terrain, under.kind, under.position, under.rotation);

    // A Farmstead is one agricultural land claim: terrace both adjacent crop
    // plots to a shared working plane, while the farmyard keeps its own local
    // height and every outer edge blends back into the authored hillside.
    if let (Some(fields), Some(field_half)) = (
        under.kind.field_positions(under.position, under.rotation),
        under.kind.field_half_extents(),
    ) {
        let field_target = fields
            .iter()
            .map(|field| terrain.get_height(field.x, field.z))
            .sum::<f32>()
            / fields.len() as f32;
        let terrace_half = field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN);
        for field in fields {
            let field = Vec3::new(field.x, field_target, field.z);
            affected.extend(terrain.apply_flatten_rect(
                field,
                terrace_half,
                under.rotation,
                // The two crop plots share one agricultural terrace. Their
                // padded inner rectangles meet in the authored aisle, so grid
                // interpolation cannot leave a tilted strip beneath either
                // field model.
                1.0,
            ));
        }
    }
    if let (Some(pasture), Some(half)) = (
        under.kind.pasture_position(under.position, under.rotation),
        under.kind.pasture_half_extents(),
    ) {
        let target = terrain.get_height(pasture.x, pasture.z);
        affected.extend(terrain.apply_flatten_rect(
            Vec3::new(pasture.x, target, pasture.z),
            half + Vec2::splat(1.0),
            under.rotation,
            2.0,
        ));
    }

    affected.sort_unstable_by_key(|coord| (coord.x, coord.z));
    affected.dedup();
    affected
}

fn market_ground_needs_leveling(terrain: &WorldTerrain, position: Vec3, rotation: f32) -> bool {
    let def = SettlementBuildingKind::Market.art().definition();
    let center = def.world_footprint_center(position, rotation);
    let target = terrain.get_height(position.x, position.z);
    let half = def.footprint * 0.5;

    // Seven samples per axis cover the visible slab through its local +/-6 m
    // edges, so an old 9 x 7 blockout terrace cannot pass merely because its
    // centre happens to be level. The construction-only apron is intentionally
    // outside this check: it protects interpolation beneath the visible floor,
    // but its outermost blend-side edge need not itself be perfectly level.
    (0..=6).any(|z| {
        (0..=6).any(|x| {
            let local = Vec2::new(
                -half.x + 2.0 * half.x * x as f32 / 6.0,
                -half.y + 2.0 * half.y * z as f32 / 6.0,
            );
            let offset = shared::rotation::local_to_world_xz(local, rotation);
            (terrain.get_height(center.x + offset.x, center.y + offset.y) - target).abs() > 0.03
        })
    })
}

/// Repair completed marketplaces created before the authored 12 x 12 m
/// square replaced the old storage-hall blockout. Newly built markets already
/// pass because construction uses the same definition; loaded saves are
/// sampled once and republished only if their larger apron is not level.
pub(crate) fn ensure_market_ground_is_level(
    mut commands: Commands,
    mut terrain: Option<ResMut<WorldTerrain>>,
    mut deltas: ResMut<PublishedTerrainDeltas>,
    markets: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
        ),
        (
            With<shared::components::MarketLevel>,
            Without<MarketGroundLeveled>,
        ),
    >,
) {
    let Some(terrain) = terrain.as_mut() else {
        return;
    };

    for (entity, building, position, rotation) in markets.iter() {
        if building.kind == SettlementBuildingKind::Market
            && market_ground_needs_leveling(terrain, position.0, rotation.0)
        {
            let affected = level_building_ground(
                terrain,
                SettlementBuildingKind::Market,
                position.0,
                rotation.0,
            );
            publish_terrain_chunks(terrain, &mut deltas, &mut commands, affected);
            info!(
                "Re-levelled completed marketplace at {:.1},{:.1} for the authored square",
                position.0.x, position.0.z,
            );
        }
        commands.entity(entity).insert(MarketGroundLeveled);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delivery_egress_points, level_construction_ground, market_ground_needs_leveling,
        material_stock_may_supply, BuildStage, PlannedRoadAccess, UnderConstruction, WorldTerrain,
    };
    use bevy::prelude::{Entity, Vec2, Vec3};
    use shared::components::SettlementBuildingKind;

    #[test]
    fn abundant_market_wood_cannot_be_starved_by_one_priority_site() {
        assert!(material_stock_may_supply(true, 0, 60));
        assert!(!material_stock_may_supply(false, 59, 60));
        assert!(material_stock_may_supply(false, 60, 60));
        assert!(material_stock_may_supply(false, 387, 60));
    }

    #[test]
    fn material_carrier_leaves_by_the_reserved_corridor_in_forward_order() {
        let terrain = WorldTerrain::default();
        let access = PlannedRoadAccess {
            settlement_id: shared::components::SettlementId(1),
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(0.0, 2.0),
                Vec2::new(10.0, 2.0),
                Vec2::new(20.0, 4.0),
            ],
            half_width: 1.0,
        };

        let (exit, waypoints) = delivery_egress_points(&terrain, Some(&access)).unwrap();
        assert_eq!(Vec2::new(exit.x, exit.z), Vec2::new(20.0, 4.0));
        assert_eq!(waypoints.len(), 2);
        assert_eq!(
            Vec2::new(waypoints[0].position.x, waypoints[0].position.z),
            Vec2::new(10.0, 2.0)
        );
        assert_eq!(
            Vec2::new(waypoints[1].position.x, waypoints[1].position.z),
            Vec2::new(20.0, 4.0)
        );
    }

    #[test]
    fn farmstead_earthworks_level_both_crop_plots() {
        let mut terrain = WorldTerrain::default();
        let kind = SettlementBuildingKind::Farmstead;
        let position = (-1800..=1800)
            .step_by(40)
            .flat_map(|x| (-1200..=1200).step_by(40).map(move |z| (x, z)))
            .find_map(|(x, z)| {
                let position =
                    Vec3::new(x as f32, terrain.get_height(x as f32, z as f32), z as f32);
                let effort = crate::world::village::planning::farmstead_earthwork_effort(
                    &terrain, position, 0.0,
                )?;
                (effort > 0.08).then_some(position)
            })
            .expect("deterministic world contains moderate terraceable farm ground");
        let under = UnderConstruction {
            kind,
            position,
            rotation: 0.0,
            owner: None,
            owner_id: None,
            builder: None,
            settlement: Entity::PLACEHOLDER,
            settlement_id: shared::components::SettlementId(1),
            stand: position,
            failed_stand_routes: 0,
            stage: BuildStage::Walking,
            quality: 0.5,
        };

        assert!(!level_construction_ground(&mut terrain, &under).is_empty());
        let half = kind.field_half_extents().unwrap();
        for field in kind.field_positions(position, 0.0).unwrap() {
            let target = terrain.get_height(field.x, field.z);
            for corner in [
                Vec2::new(-half.x, -half.y),
                Vec2::new(-half.x, half.y),
                Vec2::new(half.x, -half.y),
                Vec2::new(half.x, half.y),
            ] {
                let height = terrain.get_height(field.x + corner.x, field.z + corner.y);
                assert!(
                    (height - target).abs() < 0.08,
                    "field corner remained {:.2} m from its terrace height",
                    height - target,
                );
            }
        }
    }

    #[test]
    fn market_earthworks_keep_the_full_visible_square_above_the_terrain() {
        let mut terrain = WorldTerrain::default();
        let kind = SettlementBuildingKind::Market;
        let position = (-1800..=1800)
            .step_by(40)
            .flat_map(|x| (-1200..=1200).step_by(40).map(move |z| (x, z)))
            .find_map(|(x, z)| {
                let x = x as f32;
                let z = z as f32;
                let target = terrain.get_height(x, z);
                if target <= 1.0 {
                    return None;
                }
                let uneven = [-6.0, 0.0, 6.0].into_iter().any(|dz| {
                    [-6.0, 0.0, 6.0]
                        .into_iter()
                        .any(|dx| (terrain.get_height(x + dx, z + dz) - target).abs() > 0.12)
                });
                uneven.then_some(Vec3::new(x, target, z))
            })
            .expect("deterministic world contains uneven dry market ground");
        let under = UnderConstruction {
            kind,
            position,
            rotation: 0.37,
            owner: None,
            owner_id: None,
            builder: None,
            settlement: Entity::PLACEHOLDER,
            settlement_id: shared::components::SettlementId(1),
            stand: position,
            failed_stand_routes: 0,
            stage: BuildStage::Walking,
            quality: 0.5,
        };

        assert!(market_ground_needs_leveling(
            &terrain,
            position,
            under.rotation
        ));
        assert!(!level_construction_ground(&mut terrain, &under).is_empty());
        assert!(!market_ground_needs_leveling(
            &terrain,
            position,
            under.rotation
        ));

        let target = terrain.get_height(position.x, position.z);
        for z in 0..=6 {
            for x in 0..=6 {
                let local = Vec2::new(-6.0 + 2.0 * x as f32, -6.0 + 2.0 * z as f32);
                let offset = shared::rotation::local_to_world_xz(local, under.rotation);
                let height = terrain.get_height(position.x + offset.x, position.z + offset.y);
                assert!(
                    (height - target).abs() < 0.03,
                    "market slab sample remained {:.2} m from the levelled plane",
                    height - target,
                );
            }
        }
    }
}
