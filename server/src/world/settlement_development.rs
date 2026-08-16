//! Append-only settlement plans, tier progression and material road upgrades.
//!
//! A charter biases future choices; it never owns a mutable list of final plots.
//! Demand can therefore add an unexpected farm without moving any structure
//! that already exists.

use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::building::PlacedBuilding;
use shared::components::{
    BuildingOf, CharacterActivity, CharacterKind, CivicEmployment, CivicHallLevel,
    CivicHallUpgradeWorksite, CivicRole, CivicTradeContract, ConstructionSite, MarketLevel,
    MootAdministration, PlayerPosition, PlayerRotation, RoadClass, RoadSurface, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementDevelopment, SettlementId,
    SettlementPolicies, SettlementProgressGate, SettlementTier, VillageRoad, WorldTime,
};
use shared::economy::{
    CivicAccount, Good, GoodsInventory, MarketSeller, MootMarket, SettlementEconomy,
    CITY_MIN_PROSPERITY, CITY_MIN_RESIDENTS, CITY_REQUIRED_DAYS, TOWN_HALL_STONE_REQUIRED,
    TOWN_MIN_MARKET_VOLUME, TOWN_MIN_PROSPERITY, TOWN_MIN_RESIDENTS, TOWN_REQUIRED_DAYS,
    VILLAGE_HALL_WOOD_REQUIRED, VILLAGE_MIN_PROSPERITY, VILLAGE_MIN_RESIDENTS,
    VILLAGE_REQUIRED_SECURE_DAYS,
};

pub fn ensure_settlement_developments(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    settlements: Query<(Entity, &Settlement, &PlayerPosition), Without<SettlementDevelopment>>,
) {
    let day = clock.iter().next().map_or(0, |clock| clock.day);
    for (entity, settlement, position) in settlements.iter() {
        let development = SettlementDevelopment::from_foundation(&settlement.name, position.0, day);
        info!(
            "Settlement '{}': charter {} / {} centre / seed {}",
            settlement.name,
            development.layout.label(),
            development.center.label(),
            development.plan_seed,
        );
        commands.entity(entity).insert(development);
    }
}

/// Keep the physical Hall rung explicit. Village-to-Town promotion now changes
/// the tier only after its material project finishes, so this system performs
/// an art swap without replacing the authoritative settlement entity.
pub fn sync_civic_hall_levels(
    mut commands: Commands,
    settlements: Query<(Entity, &Settlement, Option<&CivicHallLevel>)>,
) {
    for (entity, settlement, current) in settlements.iter() {
        let desired = CivicHallLevel::for_tier(settlement.tier);
        if current.is_some_and(|current| *current == desired) {
            continue;
        }
        if let Some(previous) = current {
            info!(
                "Settlement '{}': civic building upgraded from {} to {}",
                settlement.name,
                previous.label(),
                desired.label()
            );
        }
        commands.entity(entity).insert(desired);
    }
}

/// Server-only clock and procurement cadence for an in-place Hall project.
/// The stable project and its material inventory replicate; this ticking state
/// deliberately does not.
#[derive(Component, Debug, Clone, Copy)]
pub struct CivicHallUpgradeRuntime {
    last_procurement_day: u32,
    raise_seconds_left: f32,
    builder: Option<Entity>,
}

/// The named civic worker physically constructing an in-place Hall upgrade.
/// Entity references stay server-local; the replicated activity/objective and
/// worksite are the stable public explanation of the job.
#[derive(Component, Debug, Clone, Copy)]
pub struct CivicHallBuilderRoutine {
    pub project: Entity,
}

fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Buy privately consigned materials into the visible Hall worksite and complete
/// the in-place upgrade with civic labour. No material is withdrawn for free:
/// `MootMarket::purchase` identifies every seller and the normal business
/// event pass credits them and accounts for the market fee.
#[allow(clippy::type_complexity)]
pub fn run_civic_hall_upgrade_projects(
    simulation_time: crate::world::simulation_time::SimulationTime,
    clock: Query<&WorldTime>,
    mut commands: Commands,
    mut business_events: ResMut<crate::world::village::BusinessEventQueue>,
    import_contracts: Query<&CivicTradeContract>,
    mut sets: ParamSet<(
        Query<(
            Entity,
            &CivicHallUpgradeWorksite,
            &BuildingOf,
            &ConstructionSite,
            &PlayerPosition,
            &GoodsInventory,
            &CivicHallUpgradeRuntime,
        )>,
        Query<
            (
                &SettlementId,
                &mut Settlement,
                &mut MootMarket,
                &mut GoodsInventory,
                Option<&MootAdministration>,
                Option<&SettlementPolicies>,
                Option<&mut CivicAccount>,
                &mut SettlementDevelopment,
            ),
            Without<CivicHallUpgradeWorksite>,
        >,
        Query<
            (
                &mut ConstructionSite,
                &mut GoodsInventory,
                &mut CivicHallUpgradeRuntime,
            ),
            With<CivicHallUpgradeWorksite>,
        >,
    )>,
    mut civic_workers: Query<
        (
            Entity,
            &CivicEmployment,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            Option<&crate::player::hero::MoveTarget>,
            Option<&crate::world::village::HomeRoutine>,
            Option<&crate::world::village_roads::RoadBuilderRoutine>,
            Option<&crate::world::village::MarketCollectionRoutine>,
            Option<&crate::world::village::MootQueueTicket>,
            Option<&crate::world::village_roads::NavigationRouteFailed>,
            Option<&CivicHallBuilderRoutine>,
        ),
        (
            With<CharacterKind>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let day = clock.day;
    let daylight = clock.is_day();
    let active_imports: HashSet<_> = import_contracts
        .iter()
        .filter(|contract| contract.status.is_active())
        .map(|contract| (contract.destination, contract.good))
        .collect();
    let projects: Vec<_> = sets
        .p0()
        .iter()
        .map(
            |(entity, project, building_of, site, position, inventory, runtime)| {
                (
                    entity,
                    *project,
                    building_of.0,
                    site.raising,
                    site.stand,
                    position.0,
                    inventory.amount(project.material),
                    runtime.last_procurement_day,
                    runtime.raise_seconds_left,
                    runtime.builder,
                )
            },
        )
        .collect();

    for (
        project_entity,
        project,
        settlement_id,
        raising,
        stand,
        hall_position,
        staged_material,
        last_procurement_day,
        raise_seconds_left,
        assigned_builder,
    ) in projects
    {
        let mut ready_to_build = staged_material >= project.material_required;
        if !raising {
            let mut purchased_units = 0;
            if staged_material < project.material_required
                && last_procurement_day != day
                && !active_imports.contains(&(settlement_id, project.material))
            {
                let remaining = project.material_required.saturating_sub(staged_material);
                if let Some((
                    _,
                    mut settlement,
                    mut market,
                    mut hall_store,
                    administration,
                    policies,
                    mut civic_account,
                    _,
                )) = sets
                    .p1()
                    .iter_mut()
                    .find(|(candidate, ..)| **candidate == settlement_id)
                {
                    let carry_batch = (shared::economy::capacity::VILLAGER
                        / project.material.bulk_per_unit())
                    .max(1);
                    let requested = remaining
                        .min(carry_batch)
                        .min(hall_store.amount(project.material));
                    let budget = crate::world::village::civic::civic_discretionary_budget(
                        &settlement,
                        administration,
                        policies,
                    );
                    let purchase = market.purchase(
                        project.material,
                        requested,
                        budget,
                        None,
                        Some(MarketSeller::Treasury(settlement_id)),
                    );
                    if purchase.trade.units > 0 && settlement.treasury >= purchase.trade.pennies {
                        purchased_units = hall_store.remove(project.material, purchase.trade.units);
                        debug_assert_eq!(purchased_units, purchase.trade.units);
                        settlement.treasury -= purchase.trade.pennies;
                        if let Some(account) = civic_account.as_deref_mut() {
                            account.record_material_expense(
                                day.saturating_add(1),
                                purchase.trade.pennies,
                            );
                        }
                        business_events.record_market_purchase(day, settlement_id, purchase.fills);
                    }
                }
            }

            if let Ok((_, mut store, mut runtime)) = sets.p2().get_mut(project_entity) {
                if runtime.last_procurement_day != day {
                    runtime.last_procurement_day = day;
                }
                if purchased_units > 0 {
                    let staged = store.add(project.material, purchased_units);
                    debug_assert_eq!(staged, purchased_units);
                }
                ready_to_build = store.amount(project.material) >= project.material_required;
                if ready_to_build && staged_material < project.material_required {
                    info!(
                        "Settlement {:?}: staged all {} {} for the {}",
                        settlement_id,
                        project.material_required,
                        project.material.label(),
                        project.target.label(),
                    );
                }
            }
            if !ready_to_build {
                continue;
            }
        }

        // A waiting material pile is not construction. One real paid civic
        // worker must be free, walk to the front stand, face the Hall and do
        // the same replicated hammer work shown for an ordinary building.
        if !daylight && !raising {
            if let Some(builder) = assigned_builder {
                commands
                    .entity(builder)
                    .remove::<CivicHallBuilderRoutine>()
                    .remove::<crate::player::hero::MoveTarget>()
                    .remove::<crate::world::village_roads::TravelRoute>()
                    .remove::<crate::world::village_roads::NavigationRoutePending>()
                    .remove::<crate::world::village_roads::NavigationRouteFailed>();
                if let Ok((_, _, _, _, mut activity, ..)) = civic_workers.get_mut(builder) {
                    *activity = CharacterActivity::Idle;
                }
                if let Ok((_, _, mut runtime)) = sets.p2().get_mut(project_entity) {
                    runtime.builder = None;
                }
            }
            continue;
        }

        let previous_builder = assigned_builder;
        let assigned_builder = previous_builder.filter(|builder| {
            civic_workers.get(*builder).is_ok_and(
                |(_, employment, _, _, _, _, home, road, collection, queue, _, routine)| {
                    employment.settlement == settlement_id
                        && matches!(
                            employment.role,
                            CivicRole::MootSteward | CivicRole::RoadSteward | CivicRole::CityWorker
                        )
                        && home.is_none()
                        && road.is_none()
                        && collection.is_none()
                        && queue.is_none()
                        && routine.is_some_and(|routine| routine.project == project_entity)
                },
            )
        });
        if assigned_builder.is_none() {
            if let Some(previous_builder) = previous_builder {
                commands
                    .entity(previous_builder)
                    .remove::<CivicHallBuilderRoutine>()
                    .remove::<crate::player::hero::MoveTarget>()
                    .remove::<crate::world::village_roads::TravelRoute>()
                    .remove::<crate::world::village_roads::NavigationRoutePending>()
                    .remove::<crate::world::village_roads::NavigationRouteFailed>();
            }
            if let Ok((_, _, mut runtime)) = sets.p2().get_mut(project_entity) {
                runtime.builder = None;
            }
        }
        let Some(builder) = assigned_builder else {
            let candidate = civic_workers
                .iter_mut()
                .filter_map(
                    |(
                        entity,
                        employment,
                        _,
                        _,
                        _,
                        _,
                        home,
                        road,
                        collection,
                        queue,
                        _,
                        routine,
                    )| {
                        (employment.settlement == settlement_id
                            && matches!(
                                employment.role,
                                CivicRole::MootSteward
                                    | CivicRole::RoadSteward
                                    | CivicRole::CityWorker
                            )
                            && home.is_none()
                            && road.is_none()
                            && collection.is_none()
                            && queue.is_none()
                            && routine.is_none())
                        .then_some(entity)
                    },
                )
                .min_by_key(|entity| entity.to_bits());
            if let Some(candidate) = candidate {
                commands.entity(candidate).insert((
                    CivicHallBuilderRoutine {
                        project: project_entity,
                    },
                    crate::player::hero::MoveTarget(stand),
                    CharacterActivity::Idle,
                ));
                if let Ok((_, _, mut runtime)) = sets.p2().get_mut(project_entity) {
                    runtime.builder = Some(candidate);
                }
            }
            continue;
        };

        let Ok((
            _,
            _,
            position,
            mut facing,
            mut activity,
            move_target,
            _,
            _,
            _,
            _,
            route_failed,
            _,
        )) = civic_workers.get_mut(builder)
        else {
            continue;
        };
        let work_reach = if route_failed.is_some() { 3.0 } else { 1.35 };
        if route_failed.is_some() && ground_distance(position.0, stand) > work_reach {
            commands
                .entity(builder)
                .remove::<CivicHallBuilderRoutine>()
                .remove::<crate::player::hero::MoveTarget>()
                .remove::<crate::world::village_roads::TravelRoute>()
                .remove::<crate::world::village_roads::NavigationRoutePending>()
                .remove::<crate::world::village_roads::NavigationRouteFailed>();
            *activity = CharacterActivity::Idle;
            if let Ok((_, _, mut runtime)) = sets.p2().get_mut(project_entity) {
                runtime.builder = None;
            }
            continue;
        }
        if ground_distance(position.0, stand) > work_reach {
            *activity = CharacterActivity::Idle;
            crate::world::village::ensure_move_target(&mut commands, builder, move_target, stand);
            continue;
        }
        commands
            .entity(builder)
            .remove::<crate::player::hero::MoveTarget>()
            .remove::<crate::world::village_roads::TravelRoute>()
            .remove::<crate::world::village_roads::NavigationRoutePending>()
            .remove::<crate::world::village_roads::NavigationRouteFailed>();
        let hall_direction = hall_position - position.0;
        if hall_direction.length_squared() > 1e-4 {
            // Character art faces local -Z, matching ordinary building crews.
            facing.0 = f32::atan2(-hall_direction.x, -hall_direction.z);
        }
        *activity = CharacterActivity::Building;

        if !raising {
            if let Ok((mut site, _, mut runtime)) = sets.p2().get_mut(project_entity) {
                site.raising = true;
                runtime.raise_seconds_left = shared::components::SETTLEMENT_RAISE_SECONDS;
            }
            info!(
                "Settlement {:?}: a civic worker began physically raising the {}",
                settlement_id,
                project.target.label(),
            );
            continue;
        }

        let seconds_left = (raise_seconds_left - simulation_time.world_seconds()).max(0.0);
        if seconds_left > 0.0 {
            if let Ok((_, _, mut runtime)) = sets.p2().get_mut(project_entity) {
                runtime.raise_seconds_left = seconds_left;
            }
            continue;
        }

        if let Some((_, mut settlement, _, _, _, _, _, mut development)) = sets
            .p1()
            .iter_mut()
            .find(|(candidate, ..)| **candidate == settlement_id)
        {
            let destination_tier = match project.target {
                CivicHallLevel::Village => SettlementTier::Village,
                CivicHallLevel::Town => SettlementTier::Town,
                CivicHallLevel::Moot => settlement.tier,
            };
            if destination_tier > settlement.tier {
                settlement.tier = destination_tier;
                development.progress_days = 0;
                development.next_gate = match destination_tier {
                    SettlementTier::Village => SettlementProgressGate::Marketplace,
                    SettlementTier::Town => SettlementProgressGate::Church,
                    _ => development.next_gate,
                };
                info!(
                    "Settlement '{}' completed its {}-funded {} and advanced to {}",
                    settlement.name,
                    project.material.label(),
                    project.target.label(),
                    destination_tier.label(),
                );
            }
        }
        commands
            .entity(builder)
            .remove::<CivicHallBuilderRoutine>()
            .remove::<crate::player::hero::MoveTarget>()
            .remove::<crate::world::village_roads::TravelRoute>()
            .remove::<crate::world::village_roads::NavigationRoutePending>()
            .remove::<crate::world::village_roads::NavigationRouteFailed>();
        *activity = CharacterActivity::Idle;
        commands.entity(project_entity).despawn();
    }
}

/// Upgrade a completed marketplace's physical finish without replacing its
/// authoritative building entity. Inventory, ownership, jobs and road links
/// all remain attached while the local art/navigation record changes from the
/// earthen square to its identically sized paved counterpart.
pub fn sync_market_levels(
    mut commands: Commands,
    settlements: Query<(&SettlementId, &Settlement)>,
    markets: Query<(
        Entity,
        &SettlementBuilding,
        &BuildingOf,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&PlacedBuilding>,
    )>,
) {
    for (entity, building, building_of, rotation, current, placed) in markets.iter() {
        if building.kind != SettlementBuildingKind::Market {
            continue;
        }
        let Some((_, settlement)) = settlements
            .iter()
            .find(|(settlement_id, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        let desired = MarketLevel::for_tier(settlement.tier);
        let desired_art = desired.building_type();
        if !current.is_some_and(|current| *current == desired) {
            if let Some(previous) = current {
                info!(
                    "Settlement '{}': marketplace upgraded from {} to {}",
                    settlement.name,
                    previous.label(),
                    desired.label()
                );
            }
            commands.entity(entity).insert(desired);
        }
        if !placed.is_some_and(|placed| {
            placed.building_type == desired_art && placed.rotation.to_bits() == rotation.0.to_bits()
        }) {
            commands.entity(entity).insert(PlacedBuilding {
                building_type: desired_art,
                rotation: rotation.0,
            });
        }
    }
}

fn has_building(
    buildings: &Query<(&SettlementBuilding, &shared::components::BuildingOf)>,
    settlement: shared::components::SettlementId,
    kind: SettlementBuildingKind,
) -> bool {
    buildings
        .iter()
        .any(|(building, building_of)| building_of.0 == settlement && building.kind == kind)
}

fn spawn_civic_hall_worksite(
    commands: &mut Commands,
    settlement_id: SettlementId,
    settlement_name: &str,
    hall_position: Vec3,
    rotation: f32,
    target: CivicHallLevel,
    material: Good,
    material_required: u32,
    day: u32,
) {
    let footprint_depth = target.building_type().definition().footprint.y;
    let stand =
        shared::components::builder_stand_position(hall_position, rotation, footprint_depth);
    commands.spawn((
        CivicHallUpgradeWorksite {
            target,
            material,
            material_required,
        },
        CivicHallUpgradeRuntime {
            last_procurement_day: day.saturating_sub(1),
            raise_seconds_left: shared::components::SETTLEMENT_RAISE_SECONDS,
            builder: None,
        },
        ConstructionSite {
            kind: SettlementBuildingKind::Hall,
            settlement: settlement_name.to_string(),
            raising: false,
            stand,
            rotation,
        },
        GoodsInventory::new(material_required.saturating_mul(material.bulk_per_unit())),
        BuildingOf(settlement_id),
        PlayerPosition(hall_position),
        PlayerRotation(rotation),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

/// Keep the promotion ledger current and promote only after all visible
/// requirements remain true for the advertised number of whole days.
pub fn update_settlement_developments(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    mut settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &mut Settlement,
        &SettlementEconomy,
        &MootMarket,
        &mut SettlementDevelopment,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    buildings: Query<(&SettlementBuilding, &shared::components::BuildingOf)>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    hall_projects: Query<(
        &CivicHallUpgradeWorksite,
        &BuildingOf,
        &ConstructionSite,
        &GoodsInventory,
    )>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };

    for (
        _settlement_entity,
        settlement_id,
        mut settlement,
        economy,
        market,
        mut development,
        hall_position,
        hall_rotation,
    ) in settlements.iter_mut()
    {
        let mut dirt = 0u16;
        let mut stone = 0u16;
        let mut committed = 0u32;
        let mut stone_needed = 0u32;
        for (road, _) in roads
            .iter()
            .filter(|(road, road_of)| road_of.0 == *settlement_id && road.is_complete())
        {
            committed = committed.saturating_add(road.stone_committed);
            match road.surface {
                RoadSurface::Dirt => {
                    dirt = dirt.saturating_add(1);
                    if settlement.tier >= SettlementTier::Town
                        && road.class == RoadClass::Main
                        && stone_needed == 0
                    {
                        stone_needed = road.stone_required().saturating_sub(road.stone_committed);
                    }
                }
                RoadSurface::Stone => stone = stone.saturating_add(1),
            }
        }
        if development.dirt_roads != dirt {
            development.dirt_roads = dirt;
        }
        if development.stone_roads != stone {
            development.stone_roads = stone;
        }
        if development.stone_committed != committed {
            development.stone_committed = committed;
        }
        if development.stone_needed != stone_needed {
            development.stone_needed = stone_needed;
        }

        let elapsed_days = day.saturating_sub(development.last_progress_day);
        if development.last_progress_day != day {
            development.last_progress_day = day;
        }

        if matches!(
            settlement.tier,
            SettlementTier::Hamlet | SettlementTier::Village
        ) {
            if let Some((project, _, site, inventory)) = hall_projects
                .iter()
                .find(|(_, building_of, ..)| building_of.0 == *settlement_id)
            {
                development.next_gate = if site.raising {
                    SettlementProgressGate::CivicHallConstruction
                } else {
                    SettlementProgressGate::CivicHallMaterials
                };
                development.progress_days =
                    inventory.amount(project.material).min(u32::from(u16::MAX)) as u16;
                development.required_days =
                    project.material_required.min(u32::from(u16::MAX)) as u16;
                continue;
            }
        }

        let (gate, all_met, required_days) = match settlement.tier {
            SettlementTier::Ruins => (SettlementProgressGate::FoodSecurity, false, 0),
            SettlementTier::Hamlet => {
                let progress_days = economy.food_secure_days;
                let required_days = VILLAGE_REQUIRED_SECURE_DAYS;
                let next_gate = if settlement.residents < VILLAGE_MIN_RESIDENTS {
                    SettlementProgressGate::Population
                } else if economy.prosperity < VILLAGE_MIN_PROSPERITY {
                    SettlementProgressGate::Prosperity
                } else {
                    SettlementProgressGate::FoodSecurity
                };
                if development.progress_days != progress_days {
                    development.progress_days = progress_days;
                }
                if development.required_days != required_days {
                    development.required_days = required_days;
                }
                if development.next_gate != next_gate {
                    development.next_gate = next_gate;
                }
                if settlement.residents >= VILLAGE_MIN_RESIDENTS
                    && economy.prosperity >= VILLAGE_MIN_PROSPERITY
                    && economy.food_secure_days >= VILLAGE_REQUIRED_SECURE_DAYS
                {
                    spawn_civic_hall_worksite(
                        &mut commands,
                        *settlement_id,
                        &settlement.name,
                        hall_position.0,
                        hall_rotation.map_or(0.0, |rotation| rotation.0),
                        CivicHallLevel::Village,
                        Good::Wood,
                        VILLAGE_HALL_WOOD_REQUIRED,
                        day,
                    );
                    development.progress_days = 0;
                    development.required_days = VILLAGE_HALL_WOOD_REQUIRED as u16;
                    development.next_gate = SettlementProgressGate::CivicHallMaterials;
                    info!(
                        "Settlement '{}' secured Hamlet requirements and opened a {}-Wood Village Hall worksite",
                        settlement.name, VILLAGE_HALL_WOOD_REQUIRED
                    );
                }
                continue;
            }
            SettlementTier::Village => {
                let market_built =
                    has_building(&buildings, *settlement_id, SettlementBuildingKind::Market);
                let tavern_built =
                    has_building(&buildings, *settlement_id, SettlementBuildingKind::Tavern);
                let gate = if settlement.residents < TOWN_MIN_RESIDENTS {
                    SettlementProgressGate::Population
                } else if !market_built {
                    SettlementProgressGate::Marketplace
                } else if !tavern_built {
                    SettlementProgressGate::Tavern
                } else if market.total_volume() < TOWN_MIN_MARKET_VOLUME {
                    SettlementProgressGate::Trade
                } else if economy.prosperity < TOWN_MIN_PROSPERITY {
                    SettlementProgressGate::Prosperity
                } else {
                    SettlementProgressGate::Sustaining
                };
                (
                    gate,
                    gate == SettlementProgressGate::Sustaining,
                    TOWN_REQUIRED_DAYS,
                )
            }
            SettlementTier::Town => {
                let church_built =
                    has_building(&buildings, *settlement_id, SettlementBuildingKind::Church);
                let gate = if settlement.residents < CITY_MIN_RESIDENTS {
                    SettlementProgressGate::Population
                } else if !church_built {
                    SettlementProgressGate::Church
                } else if economy.prosperity < CITY_MIN_PROSPERITY {
                    SettlementProgressGate::Prosperity
                } else {
                    SettlementProgressGate::Sustaining
                };
                (
                    gate,
                    gate == SettlementProgressGate::Sustaining,
                    CITY_REQUIRED_DAYS,
                )
            }
            SettlementTier::City => (SettlementProgressGate::Complete, false, 0),
        };

        if development.next_gate != gate {
            development.next_gate = gate;
        }
        if development.required_days != required_days {
            development.required_days = required_days;
        }
        let progress_days = if all_met {
            development
                .progress_days
                .saturating_add(elapsed_days.min(u32::from(u16::MAX)) as u16)
        } else {
            0
        };
        if development.progress_days != progress_days {
            development.progress_days = progress_days;
        }

        if required_days > 0 && development.progress_days >= required_days {
            if settlement.tier == SettlementTier::Village {
                spawn_civic_hall_worksite(
                    &mut commands,
                    *settlement_id,
                    &settlement.name,
                    hall_position.0,
                    hall_rotation.map_or(0.0, |rotation| rotation.0),
                    CivicHallLevel::Town,
                    Good::Stone,
                    TOWN_HALL_STONE_REQUIRED,
                    day,
                );
                development.progress_days = 0;
                development.required_days = TOWN_HALL_STONE_REQUIRED as u16;
                development.next_gate = SettlementProgressGate::CivicHallMaterials;
                info!(
                    "Settlement '{}' sustained the Town requirements and opened a {}-Stone Town Hall worksite",
                    settlement.name, TOWN_HALL_STONE_REQUIRED
                );
                continue;
            }
            let previous = settlement.tier;
            settlement.tier = match settlement.tier {
                SettlementTier::Town => SettlementTier::City,
                other => other,
            };
            if settlement.tier != previous {
                development.progress_days = 0;
                development.next_gate = match settlement.tier {
                    SettlementTier::Town => SettlementProgressGate::Church,
                    SettlementTier::City => SettlementProgressGate::Complete,
                    _ => development.next_gate,
                };
                info!(
                    "Settlement '{}' advanced from {} to {}",
                    settlement.name,
                    previous.label(),
                    settlement.tier.label()
                );
            }
        }
    }
}

/// Upgrade one unit of a principal road per elapsed day. Stone is removed from
/// the bounded hall inventory first, and the surface flips only when the whole
/// road has been paid for.
pub fn upgrade_town_roads(
    clock: Query<&WorldTime>,
    mut halls: Query<(
        &Settlement,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
        &mut SettlementDevelopment,
        &mut GoodsInventory,
    )>,
    mut roads: Query<(Entity, &mut VillageRoad, &shared::components::RoadOf)>,
    civic_workers: Query<&CivicEmployment>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };

    for (settlement, settlement_id, hall, rotation, mut development, mut inventory) in
        halls.iter_mut()
    {
        let has_city_worker = civic_workers.iter().any(|employment| {
            employment.settlement == *settlement_id
                && matches!(
                    employment.role,
                    CivicRole::MootSteward | CivicRole::RoadSteward | CivicRole::CityWorker
                )
        });
        if settlement.tier < SettlementTier::Town || !has_city_worker {
            if development.last_road_work_day != day {
                development.last_road_work_day = day;
            }
            continue;
        }
        let elapsed = day.saturating_sub(development.last_road_work_day);
        if elapsed == 0 {
            continue;
        }
        development.last_road_work_day = day;

        // Old roads predate hierarchy metadata. Promote the hall connector
        // rather than leaving an upgraded save with no eligible main street.
        let has_main = roads.iter().any(|(_, road, road_of)| {
            road_of.0 == *settlement_id && road.is_complete() && road.class == RoadClass::Main
        });
        if !has_main {
            let door = SettlementBuildingKind::Hall
                .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
            let door = Vec2::new(door.x, door.z);
            let candidate = roads
                .iter()
                .filter(|(_, road, road_of)| road_of.0 == *settlement_id && road.is_complete())
                .min_by(|(_, a, _), (_, b, _)| {
                    let distance = |road: &VillageRoad| {
                        road.built_points()
                            .iter()
                            .map(|point| point.distance_squared(door))
                            .fold(f32::INFINITY, f32::min)
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .map(|(entity, _, _)| entity);
            if let Some(candidate) = candidate {
                if let Ok((_, mut road, _)) = roads.get_mut(candidate) {
                    road.class = RoadClass::Main;
                    road.widen_within_reservation(4.0);
                }
            }
        }

        let candidate = roads
            .iter()
            .filter(|(_, road, road_of)| {
                road_of.0 == *settlement_id
                    && road.is_complete()
                    && road.class == RoadClass::Main
                    && road.surface == RoadSurface::Dirt
            })
            .min_by_key(|(entity, _, _)| entity.to_bits())
            .map(|(entity, _, _)| entity);
        let Some(candidate) = candidate else { continue };
        let Ok((_, mut road, _)) = roads.get_mut(candidate) else {
            continue;
        };
        let required = road.stone_required();
        let remaining = required.saturating_sub(road.stone_committed);
        let requested = remaining.min(elapsed);
        let moved = inventory.remove(Good::Stone, requested);
        if moved > 0 {
            road.stone_committed = road.stone_committed.saturating_add(moved);
        }
        if road.stone_committed >= required {
            road.surface = RoadSurface::Stone;
            road.widen_within_reservation(4.0);
            info!(
                "Settlement '{}': completed a stone main road using {} Stone",
                settlement.name, required
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn development_test_app() -> App {
        let mut app = App::new();
        app.init_resource::<crate::world::identity::WorldIdAllocator>()
            .init_resource::<crate::world::identity::WorldIdentityIndex>()
            .add_systems(
                PreUpdate,
                (
                    crate::world::identity::assign_stable_world_ids,
                    crate::world::identity::rebuild_world_identity_index,
                    crate::world::identity::reconcile_stable_world_relationships,
                    crate::world::identity::reconcile_stable_road_relationships,
                )
                    .chain(),
            );
        app
    }

    #[test]
    fn charter_is_deterministic_and_uses_independent_wall_choices() {
        let a = SettlementDevelopment::from_foundation("Oakmead", Vec3::new(2.0, 0.0, 7.0), 0);
        let b = SettlementDevelopment::from_foundation("Oakmead", Vec3::new(2.0, 0.0, 7.0), 9);
        assert_eq!(a.plan_seed, b.plan_seed);
        assert_eq!(a.layout, b.layout);
        assert_ne!(a.inner_wall, a.outer_wall);
    }

    #[test]
    fn promotion_changes_the_hall_level_without_replacing_the_settlement() {
        let mut app = App::new();
        app.add_systems(Update, sync_civic_hall_levels);
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Doorstead".into(),
                tier: SettlementTier::Hamlet,
                residents: 0,
                treasury: 73,
            })
            .id();

        app.update();
        assert_eq!(
            app.world().get::<CivicHallLevel>(settlement),
            Some(&CivicHallLevel::Moot)
        );
        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .tier = SettlementTier::Village;
        app.update();
        assert_eq!(
            app.world().get::<CivicHallLevel>(settlement),
            Some(&CivicHallLevel::Village)
        );
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().treasury,
            73,
            "the authoritative Hall entity and its state must survive the art upgrade"
        );

        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .tier = SettlementTier::City;
        app.update();
        assert_eq!(
            app.world().get::<CivicHallLevel>(settlement),
            Some(&CivicHallLevel::Town)
        );
    }

    #[test]
    fn town_paves_the_existing_market_without_replacing_its_state() {
        let mut app = App::new();
        app.add_systems(Update, sync_market_levels);
        let settlement_id = SettlementId(17);
        let settlement = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Marketford".into(),
                    tier: SettlementTier::Village,
                    residents: 11,
                    treasury: 83,
                },
            ))
            .id();
        let market = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Market,
                    settlement: "Marketford".into(),
                    owner: None,
                    quality: 0.5,
                    workers: vec!["Alda".into()],
                },
                BuildingOf(settlement_id),
                PlayerRotation(0.37),
                PlacedBuilding {
                    building_type: shared::building::BuildingType::Market,
                    rotation: 0.37,
                },
                GoodsInventory::new(250),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<MarketLevel>(market),
            Some(&MarketLevel::Earthen)
        );
        assert_eq!(
            app.world()
                .get::<PlacedBuilding>(market)
                .unwrap()
                .building_type,
            shared::building::BuildingType::Market
        );

        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .tier = SettlementTier::Town;
        app.update();
        assert_eq!(
            app.world().get::<MarketLevel>(market),
            Some(&MarketLevel::Paved)
        );
        let placed = app.world().get::<PlacedBuilding>(market).unwrap();
        assert_eq!(
            placed.building_type,
            shared::building::BuildingType::MarketPaved
        );
        assert_eq!(placed.rotation, 0.37);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(market)
                .unwrap()
                .bulk_capacity(),
            250,
            "art promotion must not replace the market entity or inventory"
        );
    }

    #[test]
    fn stone_main_road_waits_for_physical_stone() {
        let mut app = development_test_app();
        app.add_systems(Update, upgrade_town_roads);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        app.world_mut()
            .entity_mut(clock)
            .get_mut::<WorldTime>()
            .unwrap()
            .day = 1;

        let mut development = SettlementDevelopment::from_foundation("Stoneford", Vec3::ZERO, 0);
        development.last_road_work_day = 0;
        let hall = app
            .world_mut()
            .spawn((
                shared::components::SettlementId(1),
                Settlement {
                    name: "Stoneford".into(),
                    tier: SettlementTier::Town,
                    residents: 12,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                MootAdministration {
                    city_workers: vec!["Mara".into()],
                    ..default()
                },
                development,
                GoodsInventory::new(100),
            ))
            .id();
        let road = app
            .world_mut()
            .spawn((
                VillageRoad {
                    settlement: "Stoneford".into(),
                    builder: "Mara".into(),
                    points: vec![Vec2::ZERO, Vec2::new(2.0, 0.0)],
                    built_through: 2,
                    width: 2.6,
                    reserved_width: RoadClass::Main.initial_reserved_width(),
                    surface: RoadSurface::Dirt,
                    class: RoadClass::Main,
                    stone_committed: 0,
                },
                shared::components::RoadOf(shared::components::SettlementId(1)),
            ))
            .id();
        app.world_mut().spawn(CivicEmployment {
            settlement: shared::components::SettlementId(1),
            role: CivicRole::CityWorker,
        });

        app.update();
        assert_eq!(app.world().get::<VillageRoad>(road).unwrap().width, 2.6);
        assert_eq!(
            app.world().get::<VillageRoad>(road).unwrap().surface,
            RoadSurface::Dirt
        );
        assert_eq!(
            app.world()
                .get::<VillageRoad>(road)
                .unwrap()
                .stone_committed,
            0
        );

        app.world_mut()
            .get_mut::<GoodsInventory>(hall)
            .unwrap()
            .add(Good::Stone, 1);
        app.world_mut()
            .entity_mut(clock)
            .get_mut::<WorldTime>()
            .unwrap()
            .day = 2;
        app.update();
        assert_eq!(app.world().get::<VillageRoad>(road).unwrap().width, 4.0);
        assert_eq!(
            app.world().get::<VillageRoad>(road).unwrap().surface,
            RoadSurface::Stone
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Stone),
            0
        );
    }

    #[test]
    fn village_promotion_is_sustained_and_uses_real_market_volume() {
        let mut app = development_test_app();
        app.add_systems(Update, update_settlement_developments);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut market = MootMarket::founding();
        let seller = shared::economy::MarketSeller::Business(shared::components::BuildingId(42));
        while market.total_volume() < TOWN_MIN_MARKET_VOLUME {
            market.consign(seller, Good::Wood, 1, Good::Wood.base_price());
            let sold = market.purchase(Good::Wood, 1, u64::MAX, None, None);
            assert_eq!(sold.trade.units, 1, "market must keep trading");
        }
        let mut economy = SettlementEconomy::default();
        economy.prosperity = TOWN_MIN_PROSPERITY;
        let settlement = app
            .world_mut()
            .spawn((
                SettlementId(71),
                Settlement {
                    name: "Tradeford".into(),
                    tier: SettlementTier::Village,
                    residents: TOWN_MIN_RESIDENTS - 1,
                    treasury: 0,
                },
                economy,
                market,
                SettlementDevelopment::from_foundation("Tradeford", Vec3::ZERO, 0),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        for kind in [
            SettlementBuildingKind::Market,
            SettlementBuildingKind::Tavern,
        ] {
            app.world_mut().spawn((
                SettlementBuilding {
                    kind,
                    settlement: "Tradeford".into(),
                    owner: None,
                    quality: 0.5,
                    workers: vec!["Worker".into()],
                },
                BuildingOf(SettlementId(71)),
            ));
        }

        for day in 1..=TOWN_REQUIRED_DAYS {
            app.world_mut()
                .entity_mut(clock)
                .get_mut::<WorldTime>()
                .unwrap()
                .day = u32::from(day);
            app.update();
        }
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().tier,
            SettlementTier::Village,
            "prosperity and trade cannot bypass the 30-resident Town gate"
        );
        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .residents = TOWN_MIN_RESIDENTS;
        for day in (TOWN_REQUIRED_DAYS + 1)..=(TOWN_REQUIRED_DAYS * 2) {
            app.world_mut()
                .entity_mut(clock)
                .get_mut::<WorldTime>()
                .unwrap()
                .day = u32::from(day);
            app.update();
        }
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().tier,
            SettlementTier::Village,
            "sustained economic gates open a physical Hall project; they no longer mint a Town instantly"
        );
        let projects = app
            .world_mut()
            .query::<(&CivicHallUpgradeWorksite, &BuildingOf, &GoodsInventory)>()
            .iter(app.world())
            .filter(|(_, building_of, _)| building_of.0 == SettlementId(71))
            .count();
        assert_eq!(projects, 1);
    }

    #[test]
    fn secure_hamlet_opens_a_wood_funded_village_hall_worksite() {
        let mut app = development_test_app();
        app.add_systems(Update, update_settlement_developments);
        app.world_mut().spawn(WorldTime::new_default());
        let mut economy = SettlementEconomy::default();
        economy.prosperity = VILLAGE_MIN_PROSPERITY;
        economy.food_secure_days = VILLAGE_REQUIRED_SECURE_DAYS;
        app.world_mut().spawn((
            SettlementId(74),
            Settlement {
                name: "Oakmoot".into(),
                tier: SettlementTier::Hamlet,
                residents: VILLAGE_MIN_RESIDENTS,
                treasury: 2_000,
            },
            economy,
            MootMarket::founding(),
            SettlementDevelopment::from_foundation("Oakmoot", Vec3::ZERO, 0),
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
        ));

        app.update();
        let (project, site, store) = app
            .world_mut()
            .query::<(
                &CivicHallUpgradeWorksite,
                &ConstructionSite,
                &GoodsInventory,
            )>()
            .single(app.world())
            .unwrap();
        assert_eq!(project.target, CivicHallLevel::Village);
        assert_eq!(project.material, Good::Wood);
        assert_eq!(project.material_required, VILLAGE_HALL_WOOD_REQUIRED);
        assert!(!site.raising);
        assert_eq!(store.amount(Good::Wood), 0);
    }

    #[test]
    fn town_promotion_reaches_city_after_real_amenity_and_sustained_pull() {
        let mut app = development_test_app();
        app.add_systems(Update, update_settlement_developments);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut economy = SettlementEconomy::default();
        economy.prosperity = CITY_MIN_PROSPERITY;
        let settlement = app
            .world_mut()
            .spawn((
                SettlementId(72),
                Settlement {
                    name: "Bellcross".into(),
                    tier: SettlementTier::Town,
                    residents: CITY_MIN_RESIDENTS,
                    treasury: 0,
                },
                economy,
                MootMarket::founding(),
                SettlementDevelopment::from_foundation("Bellcross", Vec3::ZERO, 0),
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        app.world_mut().spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Church,
                settlement: "Bellcross".into(),
                owner: None,
                quality: 0.5,
                workers: vec!["Keeper".into()],
            },
            BuildingOf(SettlementId(72)),
        ));

        for day in 1..=CITY_REQUIRED_DAYS {
            app.world_mut()
                .entity_mut(clock)
                .get_mut::<WorldTime>()
                .unwrap()
                .day = u32::from(day);
            app.update();
        }
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().tier,
            SettlementTier::City
        );
    }

    #[test]
    fn hall_upgrade_buys_private_stone_stages_it_and_only_then_promotes() {
        let mut app = development_test_app();
        app.init_resource::<crate::world::village::BusinessEventQueue>()
            .add_systems(
                Update,
                (
                    run_civic_hall_upgrade_projects,
                    crate::world::village::apply_business_events,
                )
                    .chain(),
            );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let settlement_id = SettlementId(73);
        let seller_id = shared::components::PersonId(901);
        let unit_price = Good::Stone.base_price();
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Person(seller_id),
            Good::Stone,
            TOWN_HALL_STONE_REQUIRED,
            unit_price,
        );
        let mut policies = SettlementPolicies::from_foundation("Quarryford", Vec3::ZERO);
        policies.civic_payroll_reserve_days = 0;
        let mut hall_store = GoodsInventory::new(shared::economy::capacity::HALL);
        hall_store.add(Good::Stone, TOWN_HALL_STONE_REQUIRED);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Quarryford".into(),
                    tier: SettlementTier::Village,
                    residents: TOWN_MIN_RESIDENTS,
                    treasury: 10_000,
                },
                market,
                hall_store,
                MootAdministration::default(),
                policies,
                CivicAccount::default(),
                SettlementDevelopment::from_foundation("Quarryford", Vec3::ZERO, 0),
            ))
            .id();
        let seller = app
            .world_mut()
            .spawn((seller_id, shared::economy::Wallet::default()))
            .id();
        let project = app
            .world_mut()
            .spawn((
                CivicHallUpgradeWorksite {
                    target: CivicHallLevel::Town,
                    material: Good::Stone,
                    material_required: TOWN_HALL_STONE_REQUIRED,
                },
                CivicHallUpgradeRuntime {
                    last_procurement_day: 0,
                    raise_seconds_left: shared::components::SETTLEMENT_RAISE_SECONDS,
                    builder: None,
                },
                ConstructionSite {
                    kind: SettlementBuildingKind::Hall,
                    settlement: "Quarryford".into(),
                    raising: false,
                    stand: Vec3::ZERO,
                    rotation: 0.0,
                },
                GoodsInventory::new(
                    TOWN_HALL_STONE_REQUIRED.saturating_mul(Good::Stone.bulk_per_unit()),
                ),
                BuildingOf(settlement_id),
                PlayerPosition(Vec3::ZERO),
            ))
            .id();
        let civic_builder = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CivicEmployment {
                    settlement: settlement_id,
                    role: CivicRole::MootSteward,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
            ))
            .id();

        for day in 1..=4 {
            app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
            app.update();
        }
        // Assignment is deferred, then the embodied worker reaches the stand
        // and flips the single replicated raising transition.
        app.update();
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(project)
                .unwrap()
                .amount(Good::Stone),
            TOWN_HALL_STONE_REQUIRED
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Stone),
            0
        );
        assert!(
            app.world()
                .get::<ConstructionSite>(project)
                .unwrap()
                .raising
        );
        assert_eq!(
            app.world()
                .get::<CivicHallBuilderRoutine>(civic_builder)
                .map(|routine| routine.project),
            Some(project)
        );
        assert_eq!(
            app.world().get::<CharacterActivity>(civic_builder),
            Some(&CharacterActivity::Building)
        );
        assert!(
            app.world()
                .get::<shared::economy::Wallet>(seller)
                .unwrap()
                .balance()
                > 0
        );
        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().tier,
            SettlementTier::Village,
            "materials alone do not skip the raising phase"
        );

        app.world_mut()
            .get_mut::<CivicHallUpgradeRuntime>(project)
            .unwrap()
            .raise_seconds_left = 0.0;
        app.update();
        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().tier,
            SettlementTier::Town
        );
        assert!(app.world().get_entity(project).is_err());
    }

    #[test]
    fn public_position_caps_expand_from_hamlet_to_village() {
        assert_eq!(SettlementTier::Hamlet.public_guard_positions(), 0);
        assert_eq!(SettlementTier::Hamlet.public_worker_positions(), 2);
        assert_eq!(SettlementTier::Village.public_guard_positions(), 2);
        assert_eq!(SettlementTier::Village.public_worker_positions(), 2);
    }
}
