//! Material supply, builder work and terrain publication for approved sites.

use super::*;

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
    mut commands: Commands,
    settlements: Query<
        (
            Entity,
            &Settlement,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    sites: Query<(Entity, &UnderConstruction, &PlayerPosition), Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut markets: Query<&mut MootMarket>,
    mut builders: Query<
        (
            Entity,
            &shared::components::PersonId,
            &CharacterName,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut ConstructionMaterialRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
            Option<&mut Wallet>,
            Option<&ambient::AmbientRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    if world_time
        .iter()
        .next()
        .is_some_and(|clock| !clock.is_day())
    {
        return;
    }
    let dt = simulation_time.world_seconds();
    let now = simulation_time.elapsed_real_seconds_f64();
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
    for (site_entity, site, _) in sites.iter() {
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
        let candidate = (remaining, site_entity.to_bits(), site_entity);
        let priority = material_priority
            .entry(site.settlement)
            .or_insert(candidate);
        if (candidate.0, candidate.1) < (priority.0, priority.1) {
            *priority = candidate;
        }
    }

    for (
        builder,
        person_id,
        name,
        position,
        intent,
        home_routine,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_failed,
        mut wallet,
        ambient_routine,
    ) in builders.iter_mut()
    {
        if home_routine.is_some() {
            continue;
        }
        if !matches!(intent, VillagerIntent::Building { site, .. } if *site == routine.site) {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((_, site, site_position)) = sites.get(routine.site) else {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if site.stage != BuildStage::Supplying {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>();
            continue;
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
            } else if matches!(routine.phase, ConstructionMaterialPhase::Delivering { .. }) {
                // A valid plot can still have one bad interaction side (a
                // steep rear bank, water edge or prop cluster). Delivery does
                // not require the builder to stand at the hammering anchor:
                // rotate around the site's accessible perimeter instead of
                // pinning the whole village to one failed endpoint forever.
                routine.failed_delivery_routes = routine.failed_delivery_routes.wrapping_add(1);
                let attempt = u32::from(routine.failed_delivery_routes);
                let angle = (attempt % 12) as f32 * std::f32::consts::TAU / 12.0;
                let radius = site.kind.clearance() + 1.5 + (attempt / 12) as f32 * 2.0;
                let x = site_position.0.x + angle.cos() * radius;
                let z = site_position.0.z + angle.sin() * radius;
                let destination = Vec3::new(x, terrain.get_height(x, z), z);
                debug!(
                    "Village supplier {} could not reach delivery anchor {:.1},{:.1}; trying perimeter approach {:.1},{:.1}",
                    name.0,
                    failed.goal.x,
                    failed.goal.z,
                    destination.x,
                    destination.z,
                );
                routine.phase = ConstructionMaterialPhase::Delivering { destination };
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .insert(MoveTarget(destination));
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
            *activity = CharacterActivity::Idle;
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .insert(MoveTarget(site.stand));
            continue;
        }

        let Some((hall_entity, settlement, _, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(entity, ..)| *entity == site.settlement)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map(|rotation| rotation.0).unwrap_or(0.0),
        );
        let carried_wood = inventories
            .get(builder)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);
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
                *activity = CharacterActivity::Idle;
                if carried_wood > 0 {
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, site.stand);
                    routine.phase = ConstructionMaterialPhase::Delivering {
                        destination: site.stand,
                    };
                    continue;
                }
                if carries_other_goods {
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                    routine.phase = ConstructionMaterialPhase::UnloadingAtHall {
                        hall: hall_entity,
                        entrance: hall_entrance,
                    };
                    continue;
                }
                let affordable = site.owner_id.is_none()
                    || match (markets.get(hall_entity), wallet.as_deref()) {
                        (Ok(market), Some(wallet)) => {
                            wallet.can_afford(market.pool(Good::Wood).ask)
                        }
                        // Compatibility for focused tests assembled without the
                        // finance initialiser. A live village always has both.
                        _ => true,
                    };
                let source = inventories
                    .get(hall_entity)
                    .is_ok_and(|inventory| {
                        now >= routine.store_retry_after
                            && inventory.amount(Good::Wood) > 0
                            && affordable
                    })
                    .then_some((hall_entity, hall_entrance));
                if let Some((source, entrance)) = source {
                    if !owns_material_priority {
                        if ambient_routine.is_some() {
                            continue;
                        }
                        if ground_distance(position.0, hall_entrance) > WORK_REACH {
                            ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                        }
                        continue;
                    }
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    routine.phase =
                        ConstructionMaterialPhase::CollectingFromStore { source, entrance };
                    continue;
                }

                if now < routine.tree_retry_after || tree_proof_used {
                    continue;
                }
                commands.entity(builder).remove::<ambient::AmbientRoutine>();
                tree_proof_used = true;

                let salt = stable_name_hash(&name.0) ^ routine.site.to_bits() as u32;
                let Some((tree, stand)) = find_tree_for_cycle(
                    &terrain,
                    derived.as_deref(),
                    obstacles.as_deref(),
                    site_position.0,
                    routine.cycle,
                    salt,
                ) else {
                    // No locally valid stand for this deterministic candidate.
                    // Apply the same real-time backoff as a failed global route,
                    // otherwise a sparse grove retries at the server tick rate.
                    postpone_construction_tree_search(&mut routine, now);
                    ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                    continue;
                };
                // Reject the permanent failure before it enters the ordinary
                // navigation queue. The terrain-only proof deliberately omits
                // transient props/buildings; it answers the cheap structural
                // question of whether hall and tree share walkable land.
                if !crate::world::village_roads::embodied_land_route_exists(
                    &terrain,
                    hall_entrance,
                    stand,
                ) {
                    let widened = postpone_construction_tree_search(&mut routine, now);
                    if widened {
                        warn!(
                            "Village supplier {} found no reachable timber after twelve candidates; waiting for market stock or terrain change",
                            name.0
                        );
                    }
                    ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                    continue;
                }
                routine.tree_retry_after = 0.0;
                ensure_move_target(&mut commands, builder, move_target, stand);
                routine.phase = ConstructionMaterialPhase::WalkingToTree { tree, stand };
            }
            ConstructionMaterialPhase::UnloadingAtHall { hall, entrance } => {
                *activity = CharacterActivity::Idle;
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
            ConstructionMaterialPhase::CollectingFromStore { source, entrance } => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                let remaining = required.saturating_sub(delivered);
                let moved = if let Ok([mut source_store, mut carrier]) =
                    inventories.get_many_mut([source, builder])
                {
                    let carry_room = carrier.free_bulk() / Good::Wood.bulk_per_unit();
                    let requested = remaining.min(carry_room);
                    if site.owner_id.is_none() {
                        source_store.transfer_to(&mut carrier, Good::Wood, requested)
                    } else {
                        match (markets.get_mut(source), wallet.as_deref_mut()) {
                            (Ok(mut market), Some(wallet)) => buy_from_moot(
                                Good::Wood,
                                requested,
                                wallet,
                                &mut source_store,
                                &mut carrier,
                                &mut market,
                            ),
                            _ => source_store.transfer_to(&mut carrier, Good::Wood, requested),
                        }
                    }
                } else {
                    0
                };
                if moved > 0 {
                    routine.failed_store_routes = 0;
                    routine.store_retry_after = 0.0;
                    commands.entity(builder).insert(MoveTarget(site.stand));
                    routine.phase = ConstructionMaterialPhase::Delivering {
                        destination: site.stand,
                    };
                } else {
                    routine.phase = ConstructionMaterialPhase::Seeking;
                }
            }
            ConstructionMaterialPhase::WalkingToTree { tree, stand } => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, stand) <= WORK_REACH {
                    routine.failed_tree_routes = 0;
                    routine.tree_retry_after = 0.0;
                    commands.entity(builder).remove::<MoveTarget>();
                    let to_tree = tree - position.0;
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.z);
                    }
                    *activity = CharacterActivity::Chopping;
                    routine.phase = ConstructionMaterialPhase::Chopping {
                        tree,
                        seconds_left: CHOP_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, builder, move_target, stand);
                }
            }
            ConstructionMaterialPhase::Chopping { tree, seconds_left } => {
                *activity = CharacterActivity::Chopping;
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
                    carrier.add(Good::Wood, remaining.min(3));
                }
                routine.cycle = routine.cycle.wrapping_add(1);
                *activity = CharacterActivity::Idle;
                commands.entity(builder).insert(MoveTarget(site.stand));
                routine.phase = ConstructionMaterialPhase::Delivering {
                    destination: site.stand,
                };
            }
            ConstructionMaterialPhase::Delivering { destination } => {
                *activity = CharacterActivity::Idle;
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
                    commands
                        .entity(builder)
                        .remove::<ConstructionMaterialRoutine>()
                        .insert(MoveTarget(site.stand));
                } else {
                    routine.phase = ConstructionMaterialPhase::Seeking;
                }
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
    home_routines: Query<(), With<HomeRoutine>>,
    mut intents: Query<&mut VillagerIntent>,
    mut pending: Query<(Entity, &mut UnderConstruction, &GoodsInventory)>,
    mut sites: Query<&mut shared::components::ConstructionSite>,
    mut facings: Query<&mut PlayerRotation>,
) {
    let world_dt = simulation_time.world_seconds();
    let daylight = world_time.iter().next().is_none_or(WorldTime::is_day);
    for (site, mut under, materials) in pending.iter_mut() {
        let Ok((settlement, settlement_id)) = settlements.get(under.settlement) else {
            // Its settlement vanished; drop the site rather than leaving a
            // building belonging to nowhere.
            release_builder(&mut commands, &mut intents, under.builder, None);
            commands.entity(site).despawn();
            continue;
        };

        // People go home at night. An approved site remains exactly where it
        // was, but no ground is cleared and no raising timer advances without
        // daylight and its builder's time.
        if !daylight {
            continue;
        }
        if under
            .builder
            .is_some_and(|builder| home_routines.get(builder).is_ok())
        {
            continue;
        }

        match under.stage {
            BuildStage::Supplying => {
                let Some(builder) = under.builder else {
                    commands.entity(site).despawn();
                    continue;
                };
                if positions.get(builder).is_err() {
                    commands.entity(site).despawn();
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
                // No builder left (they were despawned): the permit lapses
                // rather than the building appearing by itself.
                let Some(builder) = under.builder else {
                    commands.entity(site).despawn();
                    continue;
                };
                let Ok(at) = positions.get(builder) else {
                    commands.entity(site).despawn();
                    continue;
                };
                if at.0.distance(under.stand) > BUILD_REACH {
                    if move_targets.get(builder).is_err() {
                        commands.entity(builder).insert(MoveTarget(under.stand));
                    }
                    continue;
                }

                // On site. Clear the plot before anything is raised on it.
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
                        building_type: under.kind.art(),
                        rotation: under.rotation,
                    },
                    shared::building::BuildingPosition(under.position),
                ));
                under.stage = BuildStage::Raising {
                    seconds_left: BUILD_SECONDS,
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
                            building_type: under.kind.art(),
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
                if let Some(owner_id) = under.owner_id {
                    commands
                        .entity(building_entity)
                        .insert(shared::components::OwnedBy(owner_id));
                }
                info!(
                    "Village '{}': {} completed ({:.0}% ground)",
                    settlement.name,
                    under.kind.label(),
                    under.quality * 100.0
                );
                if let Some(builder) = under.builder {
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
        .remove::<ConstructionMaterialRoutine>();
    if let Ok(mut intent) = intents.get_mut(builder) {
        *intent = match settlement {
            Some(settlement) => VillagerIntent::Resident { settlement },
            None => VillagerIntent::Idle,
        };
    }
}

/// Level the plot and publish the change so clients see the same ground.
///
/// The flatten itself already existed in the shared terrain-editing primitives
/// uses -- and so did the client's ingest of replicated deltas. What did not
/// exist was anything on the SERVER writing one, so this is the missing half of
/// a road that was already built from both ends.
fn clear_and_level(
    terrain: &mut WorldTerrain,
    deltas: &mut PublishedTerrainDeltas,
    commands: &mut Commands,
    under: &UnderConstruction,
) {
    let def = under.kind.art().definition();
    // Level TO the plot's own height, so a building on a slope cuts a terrace
    // rather than the whole village drifting to one altitude.
    let ground = terrain.get_height(under.position.x, under.position.z);
    let centre = Vec3::new(under.position.x, ground, under.position.z);
    let affected = terrain.apply_flatten_rect(
        centre,
        def.footprint * 0.5,
        under.rotation,
        def.flatten_radius,
    );

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
