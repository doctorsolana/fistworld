//! Settlement demand, permits and geography-aware site selection.

use super::*;

/// What a settlement wants next, counting completed and already-approved
/// structures alike.
///
/// The founding order remains legible, but needs no longer stop forever after
/// three buildings: housing follows real bed pressure and observed food
/// shortages can request more Farmsteads up to the available population.
pub fn next_need(
    existing: &HashMap<SettlementBuildingKind, usize>,
    residents: u32,
    economy: Option<&SettlementEconomy>,
) -> Option<SettlementBuildingKind> {
    let count = |kind| existing.get(&kind).copied().unwrap_or(0);
    shared::economy::next_settlement_building(
        count(SettlementBuildingKind::Farmstead),
        count(SettlementBuildingKind::FishermansHut),
        count(SettlementBuildingKind::LumberjackHut),
        count(SettlementBuildingKind::House),
        residents,
        economy,
    )
}

/// Tier infrastructure is requested only after survival/housing shortages are
/// satisfied. The charter influences where it goes, not whether demand and the
/// current tier justify it.
fn next_civic_need(
    tier: shared::components::SettlementTier,
    existing: &HashMap<SettlementBuildingKind, usize>,
) -> Option<SettlementBuildingKind> {
    let has = |kind| existing.get(&kind).copied().unwrap_or(0) > 0;
    match tier {
        shared::components::SettlementTier::Village => {
            if !has(SettlementBuildingKind::Market) {
                Some(SettlementBuildingKind::Market)
            } else if !has(SettlementBuildingKind::Tavern) {
                Some(SettlementBuildingKind::Tavern)
            } else {
                None
            }
        }
        shared::components::SettlementTier::Town => {
            (!has(SettlementBuildingKind::Church)).then_some(SettlementBuildingKind::Church)
        }
        _ => None,
    }
}

/// A resident applies for a permit, and it is approved if it is valid.
///
/// Distinct decisions may be in flight together. Planned kinds count as already
/// had, each applicant can hold only one active build, and pending plots reserve
/// their ground, so concurrency cannot duplicate or overlap construction.
/// Needed housing approval is free. A business permit always moves personal
/// coin into the settlement treasury, discounted when the settlement requested
/// that trade and progressively dearer for residents with several holdings.
#[allow(clippy::too_many_arguments)]
pub fn consider_permits(
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut clock: ResMut<VillageClock>,
    mut commands: Commands,
    planning: PermitPlanningResources,
    mut settlements: Query<(
        Entity,
        &mut Settlement,
        &PlayerPosition,
        &shared::components::SettlementId,
    )>,
    economies: Query<&SettlementEconomy>,
    developments: Query<&shared::components::SettlementDevelopment>,
    buildings: Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        Option<&shared::components::OwnedBy>,
    )>,
    pending: Query<&UnderConstruction>,
    placed: Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    // ONE query, read then written. Two -- a read of `&VillagerIntent` and a
    // write of `&mut VillagerIntent` -- is a genuine conflict Bevy refuses at
    // runtime, and iterating a mutable query gives read-only items anyway.
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &mut VillagerIntent,
        Option<&WorkStatus>,
        Option<&strategic::StrategicPerson>,
        Option<&shared::components::CivicEmployment>,
    )>,
    mut wallets: Query<&mut Wallet>,
) {
    clock.permit += simulation_time.world_seconds();
    if clock.permit < PERMIT_INTERVAL {
        return;
    }
    clock.permit = 0.0;

    let Some(terrain) = planning.terrain.as_deref() else {
        return;
    };

    for (settlement_entity, mut settlement, hall, settlement_id) in settlements.iter_mut() {
        // Count what stands AND what is already approved, or three residents
        // deciding on successive permit ticks all build the same thing.
        let mut have: HashMap<SettlementBuildingKind, usize> = HashMap::new();
        for (building, _, _) in buildings
            .iter()
            .filter(|(_, building_of, _)| building_of.0 == *settlement_id)
        {
            *have.entry(building.kind).or_default() += 1;
        }
        for under in pending.iter() {
            if under.settlement == settlement_entity {
                *have.entry(under.kind).or_default() += 1;
            }
        }
        // The hall is always there; it is the founding act, not a need.
        have.insert(SettlementBuildingKind::Hall, 1);

        let requested = next_need(
            &have,
            settlement.residents,
            economies.get(settlement_entity).ok(),
        )
        .or_else(|| next_civic_need(settlement.tier, &have));
        let planned_farms = have
            .get(&SettlementBuildingKind::Farmstead)
            .copied()
            .unwrap_or(0);
        let planned_fishers = have
            .get(&SettlementBuildingKind::FishermansHut)
            .copied()
            .unwrap_or(0);
        let initial_food_request = planned_farms + planned_fishers == 0;
        let may_add_complementary_fishing =
            requested.is_none() && planned_farms > 0 && planned_fishers == 0;

        if requested.is_none() && !may_add_complementary_fishing {
            continue;
        }

        // Occupied ground, so a new building does not land on an old one.
        let mut occupied: Vec<(Vec3, f32)> = placed
            .iter()
            .filter(|(_, building_of, _, _)| building_of.0 == *settlement_id)
            .map(|(building, _, position, _)| (position.0, building.kind.clearance()))
            .chain(std::iter::once((
                hall.0,
                SettlementBuildingKind::Hall.clearance(),
            )))
            .chain(pending.iter().filter_map(|under| {
                (under.settlement == settlement_entity)
                    .then_some((under.position, under.kind.clearance()))
            }))
            .collect();

        // A Farmstead owns more ground than its cabin. Reserve the separate
        // crop plot as well, including while construction is pending, so a
        // later building cannot be approved on top of the wheat rows.
        occupied.extend(
            placed
                .iter()
                .filter(|(_, building_of, _, _)| building_of.0 == *settlement_id)
                .flat_map(|(building, _, position, rotation)| {
                    let rotation = rotation.map_or(0.0, |rotation| rotation.0);
                    let radius = building
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_EDGE_CLEARANCE);
                    building
                        .kind
                        .field_positions(position.0, rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );
        occupied.extend(
            pending
                .iter()
                .filter(|under| under.settlement == settlement_entity)
                .flat_map(|under| {
                    let radius = under
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_EDGE_CLEARANCE);
                    under
                        .kind
                        .field_positions(under.position, under.rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );

        let village_roads: Vec<_> = roads
            .iter()
            .filter_map(|(road, road_of)| (road_of.0 == *settlement_id).then_some(road))
            .collect();
        let missing = requested.unwrap_or(SettlementBuildingKind::FishermansHut);
        let search_signature = FailedSiteSearch {
            kind: missing,
            residents: settlement.residents,
            occupied_plots: occupied.len(),
            roads: village_roads.len(),
            terrain_version: terrain.modification_version(),
        };
        if clock
            .failed_site_searches
            .get(&settlement_entity)
            .is_some_and(|failed| *failed == search_signature)
        {
            continue;
        }
        // A food permit becomes a fishing workplace when the authored hut and
        // pier can form a convincing land-to-water pair. Inland settlements
        // keep the farmstead path unchanged.
        let coastal_site = (initial_food_request || may_add_complementary_fishing)
            .then(|| find_fishing_site(terrain, hall.0, &occupied, &village_roads))
            .flatten();
        let ordinary_site = requested.and_then(|kind| {
            find_site_with_plan(
                terrain,
                hall.0,
                kind,
                &occupied,
                &village_roads,
                developments.get(settlement_entity).ok(),
                planning.colliders.as_deref(),
                planning.derived.as_deref(),
            )
            .map(|(position, rotation)| (kind, position, rotation))
        });
        let mut approved_site = if let Some((position, rotation, quality)) =
            coastal_site.filter(|_| initial_food_request || may_add_complementary_fishing)
        {
            Some((
                SettlementBuildingKind::FishermansHut,
                position,
                rotation,
                quality,
            ))
        } else if let Some((kind, position, rotation)) = ordinary_site {
            Some((
                kind,
                position,
                rotation,
                site_quality(terrain, kind, position),
            ))
        } else {
            None
        };

        // One geographically impossible business must not freeze the entire
        // permit queue. Treat the unavailable preferred kind as provisionally
        // satisfied, ask the same shortage model what would come next, and
        // approve that real need if it has a viable plot. The unavailable kind
        // remains absent, so it will be reconsidered after settlement geometry
        // or terrain changes instead of being silently granted.
        if approved_site.is_none() {
            if let Some(unavailable) = requested {
                let mut assumed_have = have.clone();
                *assumed_have.entry(unavailable).or_default() += 1;
                let alternative = next_need(
                    &assumed_have,
                    settlement.residents,
                    economies.get(settlement_entity).ok(),
                )
                .or_else(|| next_civic_need(settlement.tier, &assumed_have));
                if let Some(alternative) = alternative.filter(|kind| *kind != unavailable) {
                    approved_site = find_site_with_plan(
                        terrain,
                        hall.0,
                        alternative,
                        &occupied,
                        &village_roads,
                        developments.get(settlement_entity).ok(),
                        planning.colliders.as_deref(),
                        planning.derived.as_deref(),
                    )
                    .map(|(position, rotation)| {
                        (
                            alternative,
                            position,
                            rotation,
                            site_quality(terrain, alternative, position),
                        )
                    });
                }
            }
        }

        let Some((kind, position, rotation, quality)) = approved_site else {
            info!(
                "Village '{}': nowhere to put a {} yet",
                settlement.name,
                missing.label()
            );
            clock
                .failed_site_searches
                .insert(settlement_entity, search_signature);
            continue;
        };
        clock.failed_site_searches.remove(&settlement_entity);

        // A resident applies only after geography has selected the actual
        // permit kind. This prevents an impossible lumber permit from applying
        // lumber prices or eligibility rules to a fallback house or farm.
        // The applicant is whoever holds the fewest completed and approved
        // plots, with stable name ordering for reproducible runs.
        let holdings = |who: shared::components::PersonId| -> usize {
            buildings
                .iter()
                .filter(|(_, building_of, owner)| {
                    building_of.0 == *settlement_id && owner.is_some_and(|owner| owner.0 == who)
                })
                .count()
                + pending
                    .iter()
                    .filter(|under| {
                        under.settlement_id == *settlement_id && under.owner_id == Some(who)
                    })
                    .count()
        };
        let applicant = villagers
            .iter()
            .filter(|(_, _, _, intent, _, strategic, _)| {
                strategic.is_none()
                    && matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
            })
            .filter_map(|(entity, person_id, name, _, status, _, civic_job)| {
                // Owning a building does not create a second job, but its
                // construction still needs the applicant's physical time.
                // Never tear somebody out of a field, workplace doorway or
                // fishing pier mid-shift. Employed residents become eligible
                // again after their bounded work routine ends for the day.
                if planning.permit_busy.get(entity).is_ok() {
                    return None;
                }
                if kind.is_civic() {
                    if !civic_job.is_some_and(|job| {
                        job.settlement == *settlement_id
                            && job.role == shared::components::CivicRole::Reeve
                    }) {
                        return None;
                    }
                } else if civic_job.is_some() {
                    return None;
                }
                let holding_count = holdings(*person_id);
                let fee = permit_price(kind, holding_count, requested.is_some());
                let balance = wallets
                    .get(entity)
                    .map(|wallet| wallet.balance())
                    .unwrap_or(shared::economy::STARTING_VILLAGER_MONEY);
                let wealthy_investor = kind != SettlementBuildingKind::House
                    && status.is_some_and(|status| *status == WorkStatus::Chilling)
                    && holding_count > 0;
                (balance >= fee).then_some((
                    entity,
                    *person_id,
                    name.0.clone(),
                    holding_count,
                    u8::from(!wealthy_investor),
                ))
            })
            .min_by(|a, b| {
                a.4.cmp(&b.4)
                    .then_with(|| a.3.cmp(&b.3))
                    .then_with(|| a.1.cmp(&b.1))
            });
        let Some((builder, applicant_id, applicant, applicant_holdings, _)) = applicant else {
            info!(
                "Village '{}': nobody can afford its next {} permit",
                settlement.name,
                kind.label()
            );
            continue;
        };

        // Charge the actual approved kind. The coastal substitution has the
        // same base as a Farmstead today, but keeping this exact lets their
        // prices diverge later without charging for a building never granted.
        let fee = permit_price(kind, applicant_holdings, requested.is_some());
        if let Ok(mut wallet) = wallets.get_mut(builder) {
            if !wallet.debit(fee) {
                continue;
            }
        } else {
            // Old/test villagers without a wallet migrate into the live rule
            // with the same founding endowment, minus this real fee.
            commands.entity(builder).insert(Wallet::new(
                shared::economy::STARTING_VILLAGER_MONEY.saturating_sub(fee),
            ));
        }
        settlement.treasury = settlement.treasury.saturating_add(fee);

        // Auto-approved after payment. A permit still builds nothing: private
        // owners buy or gather the Wood, while civic work draws physical Wood
        // from the settlement's common hall inventory.
        // How good this ground is for this trade, sampled where it will stand
        // rather than at the hall. A farmstead on the settlement's best soil is
        // worth more than one behind the woodshed, and that has to be decided
        // by the plot, not the village.
        // Beside the plot, in front of it. The builder must not stand where the
        // building is about to rise.
        let stand = shared::components::builder_stand_position(
            position,
            rotation,
            kind.art().definition().footprint.y,
        );

        let site = commands
            .spawn((
                UnderConstruction {
                    kind,
                    position,
                    rotation,
                    // Progression amenities are public works. The applicant
                    // supplies builder time, while the settlement owns the
                    // completed structure and its common-stock material bill.
                    owner: (!kind.is_civic()).then_some(applicant.clone()),
                    owner_id: (!kind.is_civic()).then_some(applicant_id),
                    builder: Some(builder),
                    settlement: settlement_entity,
                    settlement_id: *settlement_id,
                    stand,
                    stage: BuildStage::Supplying,
                    quality,
                },
                // The replicated half, so the panel can show it as approved.
                shared::components::ConstructionSite {
                    kind,
                    settlement: settlement.name.clone(),
                    raising: false,
                    stand,
                    rotation,
                },
                GoodsInventory::new(kind.construction_storage_bulk()),
                PlayerPosition(position),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();

        // The permit does not build anything. Its builder first sources every
        // required wood bundle and carries it into this site's bounded pile.
        commands.entity(builder).remove::<MoveTarget>().insert((
            ConstructionMaterialRoutine {
                site,
                cycle: 0,
                failed_tree_routes: 0,
                failed_store_routes: 0,
                failed_delivery_routes: 0,
                tree_retry_after: 0.0,
                store_retry_after: 0.0,
                phase: ConstructionMaterialPhase::Seeking,
            },
            CharacterActivity::Idle,
        ));
        commands
            .entity(builder)
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .remove::<ambient::AmbientRoutine>();
        if let Ok(mut intent) = villagers
            .get_mut(builder)
            .map(|(_, _, _, intent, ..)| intent)
        {
            *intent = VillagerIntent::Building {
                settlement: settlement_entity,
                site,
            };
        }
        info!(
            "Village '{}': {applicant} paid {} coin for a {} permit at {:.0},{:.0}",
            settlement.name,
            shared::economy::format_money(fee),
            kind.label(),
            position.x,
            position.z
        );
    }
}

/// How well this ground suits what is being built, 0..1.
///
/// Reads the SAME `BiomeField::resources` the grass density and the economy
/// read, so a farmstead standing in thick grass really is standing on good
/// soil — the thing you can see is the thing the number says.
///
/// Slope is passed as zero deliberately. `find_site` already rejected anything
/// above `MAX_BUILD_SLOPE`, which is below every slope threshold inside
/// `biome()` and `resources()`, so at a legal plot the slope term cannot change
/// the answer. Inventing a second slope formula here would only create
/// something to disagree with the siting gate about.
pub fn site_quality(terrain: &WorldTerrain, kind: SettlementBuildingKind, at: Vec3) -> f32 {
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        // Hand-authored maps carry no biome field. Neutral rather than zero: a
        // building that works nowhere is worse than one that works averagely.
        return 0.5;
    };
    let profile = field.resources(at.x, at.z, at.y, 0.0);
    kind.yield_quality(&profile)
}

/// Steepness at a point, as a rise over the sampling distance.
pub(super) fn slope_at(terrain: &WorldTerrain, x: f32, z: f32) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(x, z);
    let dx = (terrain.get_height(x + STEP, z) - here).abs();
    let dz = (terrain.get_height(x, z + STEP) - here).abs();
    dx.max(dz) / STEP
}

/// Steepest ground a building will accept.
const MAX_BUILD_SLOPE: f32 = 0.30;

/// How far above the waterline anything a settlement builds must stand, in metres.
///
/// Not zero: ground exactly at the waterline is shoreline, and a farmstead with
/// its doorstep in the lake reads as a bug even though the maths permitted it.
pub const FREEBOARD: f32 = 1.5;

/// Find a dry Fisherman's Hut plot whose authored rear pier reaches genuine
/// open water.
///
/// This is intentionally geometry-led rather than biome-led. A northern rock
/// coast and a southern dry coast are both viable if there is a safe hut pad,
/// a dry route around the hut, and water beneath the working end of the pier.
/// The returned rotation points the hut's local +Z (its `Anchor_Pier` side)
/// seaward.
pub fn find_fishing_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32, f32)> {
    const BEARINGS: usize = 24;
    const FACINGS: usize = 24;
    const RING_STEP: f32 = 4.0;

    let water = terrain.water_level()?;
    let kind = SettlementBuildingKind::FishermansHut;
    let (min_radius, max_radius) = kind.preferred_ring();
    let clearance = kind.clearance();
    let mut radius = min_radius;

    while radius <= max_radius {
        let mut best_at_radius: Option<(Vec3, f32, f32)> = None;
        for i in 0..BEARINGS {
            let turn = (i as f32 + (radius / RING_STEP) * 0.5) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let x = hall.x + angle.cos() * radius;
            let z = hall.z + angle.sin() * radius;
            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(candidate.x - other.x, candidate.z - other.z).length()
                    < clearance + other_clearance
            }) {
                continue;
            }
            let footprint_radius = kind.art().definition().footprint.length() * 0.5 + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                continue;
            }

            for facing in 0..FACINGS {
                let rotation = facing as f32 / FACINGS as f32 * std::f32::consts::TAU;
                if shared::components::minimum_building_water_clearance(
                    terrain, candidate, kind, rotation,
                ) < shared::components::SETTLEMENT_FREEBOARD
                {
                    continue;
                }
                if !crate::world::village_roads::doorway_road_apron_is_dry(
                    terrain, kind, candidate, rotation,
                ) {
                    continue;
                }

                // The side-route anchor is where a fisher rounds the solid
                // building on the way from its front door to its rear pier.
                // It must remain dry and reasonably level with the hut pad.
                let Some(nets) = kind.nets_position(candidate, rotation) else {
                    continue;
                };
                let nets_ground = terrain.get_height(nets.x, nets.z);
                if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
                    continue;
                }

                let Some(fish_spot) = kind.fishing_position(candidate, rotation) else {
                    continue;
                };
                let quality = fishing_water_quality(terrain, fish_spot, rotation, water);
                if quality <= 0.0 {
                    continue;
                }
                let stand = shared::components::builder_stand_position(
                    candidate,
                    rotation,
                    kind.art().definition().footprint.y,
                );
                if !crate::world::village_roads::embodied_land_route_exists(terrain, hall, stand) {
                    continue;
                }
                let replace = best_at_radius
                    .as_ref()
                    .is_none_or(|(_, _, best_quality)| quality > *best_quality);
                if replace {
                    best_at_radius = Some((candidate, rotation, quality));
                }
            }
        }
        if best_at_radius.is_some() {
            return best_at_radius;
        }
        radius += RING_STEP;
    }
    None
}

/// Score water around the working end of the authored pier. Zero means the
/// three seaward samples are not all submerged, so the layout would visibly
/// terminate on land. Non-zero values reward deeper, broader water without
/// making oceans categorically better than rivers or lakes.
fn fishing_water_quality(
    terrain: &WorldTerrain,
    fish_spot: Vec3,
    rotation: f32,
    water: f32,
) -> f32 {
    let mut depth_score = 0.0;
    let mut samples = 0.0;
    for forward in [-1.0_f32, 0.75, 2.5] {
        for side in [-1.4_f32, 0.0, 1.4] {
            let offset = shared::rotation::local_to_world_xz(Vec2::new(side, forward), rotation);
            let ground = terrain.get_height(fish_spot.x + offset.x, fish_spot.z + offset.y);
            let depth = water - ground;
            // The outer row is load-bearing: a pier whose tip merely touches a
            // shallow puddle is not a fishing site.
            if forward >= 2.5 && depth < 0.18 {
                return 0.0;
            }
            depth_score += (depth / 2.5).clamp(0.0, 1.0);
            samples += 1.0;
        }
    }
    // Require the actual authored standing point to be above water, too.
    let tip_depth = water - terrain.get_height(fish_spot.x, fish_spot.z);
    if tip_depth < 0.12 {
        return 0.0;
    }
    (0.35 + 0.65 * depth_score / samples).clamp(0.35, 1.0)
}

/// Deterministic fallback search for a legal building plot.
///
/// Walks outward in rings from the hall, sampling a fixed number of bearings
/// per ring, and takes the first spot that is flat enough and clear of what is
/// already there. Deterministic on purpose: the same village in the same state
/// makes the same choice, so a bug is reproducible rather than a story about
/// what happened once.
///
/// Production permits call `find_site_with_plan` with the settlement charter,
/// obstacle indexes and adjacency context. This public no-context wrapper is a
/// conservative baseline used by geometry and road regression tests; it still
/// treats completed roads as occupied infrastructure.
pub fn find_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32)> {
    find_site_with_plan(terrain, hall, kind, occupied, roads, None, None, None)
}

#[derive(Clone, Copy, Debug)]
struct PlannedPlotCandidate {
    /// Building centre relative to the Moot Hall.
    local: Vec2,
    /// Point on the intended lane, street, avenue, or neighbourhood green that
    /// the building's door should face.
    frontage: Vec2,
}

fn plan_axis(plan: &shared::components::SettlementDevelopment) -> (Vec2, Vec2) {
    let fraction = (plan.plan_seed.rotate_right(29) & 0xffff) as f32 / 65_535.0;
    let angle = fraction * std::f32::consts::TAU;
    let axis = Vec2::new(angle.cos(), angle.sin());
    (axis, Vec2::new(-axis.y, axis.x))
}

fn plan_center_offset(
    plan: &shared::components::SettlementDevelopment,
    axis: Vec2,
    side: Vec2,
) -> Vec2 {
    use shared::components::SettlementCenterStyle as Center;
    let handedness = if plan.plan_seed.rotate_right(17) & 1 == 0 {
        1.0
    } else {
        -1.0
    };
    match plan.center {
        Center::Green => side * handedness * 8.0,
        Center::Square => Vec2::ZERO,
        Center::Avenue => axis * 6.0,
        Center::Courtyard => (axis + side * handedness) * 5.0,
    }
}

/// Convert one deterministic search sample into a recognisable piece of the
/// settlement's street grammar. Buildings are placed BESIDE an implied street
/// and face that street. The road builder later surveys from the authored door
/// to the existing network, turning this inexpensive plan into real terrain-
/// aware paths without pre-baking a whole city.
fn planned_plot_candidate(
    plan: &shared::components::SettlementDevelopment,
    kind: SettlementBuildingKind,
    radius: f32,
    bearing: usize,
    base: Vec2,
) -> PlannedPlotCandidate {
    use shared::components::SettlementLayoutStyle as Style;

    let (axis, side) = plan_axis(plan);
    let centre = plan_center_offset(plan, axis, side);
    let handedness = if plan.plan_seed & 1 == 0 { 1.0 } else { -1.0 };
    let side_sign = if bearing & 1 == 0 { 1.0 } else { -1.0 };

    match plan.layout {
        Style::Organic => {
            // Three gently wandering lanes. Houses occupy alternating verges
            // instead of forming a ring around the hall.
            let branch = bearing % 3;
            let branch_angle = (plan.plan_seed.rotate_right(7) & 0xffff) as f32 / 65_535.0
                * std::f32::consts::TAU
                + branch as f32 * std::f32::consts::TAU / 3.0
                + (radius * 0.085 + branch as f32).sin() * 0.18;
            let direction = Vec2::new(branch_angle.cos(), branch_angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let bend = normal
                * (radius * 0.11 + bearing as f32 + (plan.plan_seed & 31) as f32 * 0.03).sin()
                * 5.5;
            let frontage = centre + direction * radius + bend;
            let setback = 8.5 + (bearing / 6) as f32 * 3.0;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * setback,
                frontage,
            }
        }
        Style::Radial => {
            // Buildings front the sides of several spokes, not the Moot Hall.
            let branches = 5 + (plan.plan_seed.rotate_right(13) & 1) as usize;
            let branch = (bearing / 2) % branches;
            let angle =
                axis.y.atan2(axis.x) + branch as f32 * std::f32::consts::TAU / branches as f32;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let frontage = centre + direction * radius;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * 9.0,
                frontage,
            }
        }
        Style::Grid => {
            // Seed-rotated orthogonal streets. Each sample is a building set
            // back from the nearest grid line with its facade parallel to it.
            const BLOCK: f32 = 26.0;
            const FRONTAGE_STEP: f32 = 11.0;
            const SETBACK: f32 = 9.0;
            let along = base.dot(axis);
            let across = base.dot(side);
            if bearing & 1 == 0 {
                let street_across = (across / BLOCK).round() * BLOCK;
                let frontage = centre
                    + axis * ((along / FRONTAGE_STEP).round() * FRONTAGE_STEP)
                    + side * street_across;
                let verge = if (across - street_across).abs() > 0.5 {
                    (across - street_across).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + side * verge * SETBACK,
                    frontage,
                }
            } else {
                let street_along = (along / BLOCK).round() * BLOCK;
                let frontage = centre
                    + axis * street_along
                    + side * ((across / FRONTAGE_STEP).round() * FRONTAGE_STEP);
                let verge = if (along - street_along).abs() > 0.5 {
                    (along - street_along).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + axis * verge * SETBACK,
                    frontage,
                }
            }
        }
        Style::Avenue => {
            // A long civic spine with buildings on both sides. Farther rings
            // extend the avenue rather than inflating another circle.
            let mut along = base.dot(axis) * 1.35;
            if along.abs() < 10.0 {
                along = side_sign * radius * 0.8;
            }
            let avenue = centre + axis * along;
            let verge = if base.dot(side).abs() > 0.5 {
                base.dot(side).signum()
            } else {
                side_sign * handedness
            };
            PlannedPlotCandidate {
                local: avenue + side * verge * (12.0 + (bearing / 8) as f32 * 5.0),
                frontage: avenue,
            }
        }
        Style::Polycentric => {
            // Three persistent neighbourhood centres. Workplaces sit in the
            // looser outer clusters while homes/civic buildings fill the near
            // neighbourhoods.
            let district = bearing % 3;
            let district_angle =
                axis.y.atan2(axis.x) + handedness * district as f32 * std::f32::consts::TAU / 3.0;
            let district_direction = Vec2::new(district_angle.cos(), district_angle.sin());
            let district_distance = match kind {
                SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::FishermansHut => 44.0,
                _ => 30.0,
            };
            let district_centre = centre + district_direction * district_distance;
            let orbit_angle = district_angle
                + std::f32::consts::FRAC_PI_2
                + (bearing / 3) as f32 * 0.75
                + radius * 0.035;
            let orbit = Vec2::new(orbit_angle.cos(), orbit_angle.sin())
                * (9.0 + ((radius / 6.0) as usize & 1) as f32 * 5.0);
            PlannedPlotCandidate {
                local: district_centre + orbit,
                frontage: district_centre,
            }
        }
    }
}

fn rotation_facing_frontage(building: Vec2, frontage: Vec2) -> f32 {
    // Building doors are authored on local -Z. Rotating local -Z toward the
    // target requires the vector FROM the target back to the building.
    let outward = building - frontage;
    outward.x.atan2(outward.y)
}

fn closest_point_on_segment(point: Vec2, start: Vec2, end: Vec2) -> Vec2 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= 1e-6 {
        return start;
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    start + segment * t
}

fn nearest_completed_road_frontage(candidate: Vec2, roads: &[&VillageRoad]) -> Option<Vec2> {
    roads
        .iter()
        .filter(|road| road.is_complete())
        .flat_map(|road| road.built_points().windows(2))
        .map(|pair| closest_point_on_segment(candidate, pair[0], pair[1]))
        .min_by(|a, b| {
            a.distance_squared(candidate)
                .total_cmp(&b.distance_squared(candidate))
        })
        .filter(|frontage| frontage.distance_squared(candidate) <= 48.0 * 48.0)
}

pub(super) fn find_site_with_plan(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    development: Option<&shared::components::SettlementDevelopment>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<(Vec3, f32)> {
    const BEARINGS: usize = 12;
    const RING_STEP: f32 = 6.0;
    const RESOURCE_PLOT_SHORTLIST: usize = 6;

    let (mut min_radius, max_radius) = kind.preferred_ring();
    if let Some(plan) = development {
        use shared::components::SettlementCenterStyle as Center;
        if kind == SettlementBuildingKind::House {
            // Preserve the selected civic centre from the first cabin onward.
            min_radius = min_radius.max(match plan.center {
                Center::Green => 24.0,
                Center::Square => 27.0,
                Center::Avenue => 19.0,
                Center::Courtyard => 26.0,
            });
        }
    }
    let clearance = kind.clearance();
    let resource_scored = matches!(
        kind,
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::LumberjackHut
    );
    let mut best_resource_plots: Vec<(f32, Vec3, f32)> = Vec::new();

    let mut radius = min_radius;
    while radius <= max_radius {
        'candidate: for i in 0..BEARINGS {
            // Offset each ring's bearings so successive rings do not line every
            // building up on the same spokes.
            let seeded_turn = development.map_or(0.0, |plan| {
                ((plan.plan_seed.rotate_right(9) & 0xffff) as f32 / 65_535.0) * 0.92
            });
            let turn = (i as f32 + (radius / RING_STEP) * 0.5 + seeded_turn) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let base = Vec2::new(angle.cos(), angle.sin()) * radius;
            let planned = development.map_or(
                PlannedPlotCandidate {
                    local: base,
                    frontage: Vec2::ZERO,
                },
                |plan| planned_plot_candidate(plan, kind, radius, i, base),
            );
            let local = planned.local;
            let x = hall.x + local.x;
            let z = hall.z + local.y;

            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            // Prefer real completed frontage once a street exists. Before
            // that, face the implied street from the seed grammar. This is the
            // rotation used by water, field, collision, door, and road checks.
            let candidate2 = Vec2::new(x, z);
            let planned_frontage =
                Vec2::new(hall.x + planned.frontage.x, hall.z + planned.frontage.y);
            let frontage =
                nearest_completed_road_frontage(candidate2, roads).unwrap_or(planned_frontage);
            let rotation = rotation_facing_frontage(candidate2, frontage);
            // Test every part of the rotated footprint and the authored door
            // against the LOCAL water surface. Comparing the centre to the
            // global ocean plane misses inland rivers entirely.
            if shared::components::minimum_building_water_clearance(
                terrain, candidate, kind, rotation,
            ) < FREEBOARD
            {
                continue;
            }
            if !crate::world::village_roads::doorway_road_apron_is_dry(
                terrain, kind, candidate, rotation,
            ) {
                continue;
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
                    kind, candidate, rotation, colliders, derived,
                )
            }) {
                continue;
            }
            let clashes = occupied.iter().any(|(other, other_clearance)| {
                let flat = Vec2::new(candidate.x - other.x, candidate.z - other.z).length();
                flat < clearance + other_clearance
            });
            if clashes {
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    if shared::components::minimum_rotated_rect_water_clearance(
                        terrain,
                        field,
                        field_half + Vec2::splat(shared::components::FARM_FIELD_EDGE_CLEARANCE),
                        rotation,
                    ) < FREEBOARD
                    {
                        continue 'candidate;
                    }
                    if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                        !crate::world::village_roads::rotated_rect_is_clear_of_props(
                            Vec2::new(field.x, field.z),
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_EDGE_CLEARANCE,
                            colliders,
                            derived,
                        )
                    }) {
                        continue 'candidate;
                    }
                    let field_clearance =
                        field_half.length() + shared::components::FARM_FIELD_EDGE_CLEARANCE;
                    if occupied.iter().any(|(other, other_clearance)| {
                        Vec2::new(field.x - other.x, field.z - other.z).length()
                            < field_clearance + other_clearance
                    }) {
                        continue 'candidate;
                    }
                }
            }
            let footprint_radius = kind.art().definition().footprint.length() * 0.5 + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    let field_center = Vec2::new(field.x, field.z);
                    if roads.iter().any(|road| {
                        road.intersects_rotated_rect(
                            field_center,
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_EDGE_CLEARANCE,
                        )
                    }) {
                        continue 'candidate;
                    }
                }
            }
            if resource_scored {
                // A layout grammar says which street this plot belongs to;
                // geography still decides whether a farm or timber workplace
                // is worth building there. A small travel penalty prevents a
                // negligible quality gain from sending the very first worker
                // to the edge of the full 120 m search band.
                let quality = site_quality(terrain, kind, candidate);
                let travel = candidate2.distance(Vec2::new(hall.x, hall.z));
                let score = quality * 100.0 - travel / max_radius.max(1.0) * 9.0;
                best_resource_plots.push((score, candidate, rotation));
                best_resource_plots.sort_by(|a, b| {
                    b.0.total_cmp(&a.0)
                        .then_with(|| a.1.x.total_cmp(&b.1.x))
                        .then_with(|| a.1.z.total_cmp(&b.1.z))
                });
                best_resource_plots.truncate(RESOURCE_PLOT_SHORTLIST);
            } else {
                return Some((candidate, rotation));
            }
        }
        radius += RING_STEP;
    }
    best_resource_plots
        .into_iter()
        .find(|(_, candidate, rotation)| {
            let builder_stand = shared::components::builder_stand_position(
                *candidate,
                *rotation,
                kind.art().definition().footprint.y,
            );
            if !crate::world::village_roads::embodied_land_route_exists(
                terrain,
                hall,
                builder_stand,
            ) {
                return false;
            }
            kind != SettlementBuildingKind::LumberjackHut
                || lumber_plot_has_reachable_tree(
                    terrain,
                    kind.entrance_position(*candidate, *rotation),
                )
        })
        .map(|(_, candidate, rotation)| (candidate, rotation))
}
