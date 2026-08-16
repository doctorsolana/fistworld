//! Company-owned inter-settlement routes and buyer-funded civic contracts.
//!
//! The first controlled loop moves a real civic construction order from one
//! public market to another. A completed private Storage Hall (the warehouse)
//! and one of its ordinary Company Porters are mandatory. The carrier never
//! owns the Stone: destination escrow pays the source seller at collection,
//! the destination owns the in-transit cargo, and the carrier earns its
//! freight fee only after physical delivery.

use super::*;

use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::{
    CharacterObjective, CivicHallUpgradeWorksite, CivicTradeContract, CompanyTradeRoute,
    ConstructionSite, TradeContractId, TradeContractStatus, TradeRouteHistory, TradeRouteId,
    TradeRouteStatus, TradeRouteTrip,
};
use shared::economy::{MarketSeller, FOUNDING_DAILY_WAGE};

/// Three pennies per bulk makes one eight-Stone Town Works load worth 1.44
/// coin to the carrier: enough to cover one ordinary daily wage while keeping
/// freight materially smaller than the Stone purchase itself.
pub const CONTRACT_DELIVERY_PENNIES_PER_BULK: u64 = 3;
/// A public contract must at least cover the carrier's fixed cost of sending
/// one employee and its wagon. Without a call-out minimum, a partly supplied
/// Town Works can post a perfectly real three-Stone order whose per-bulk
/// freight is forever below one daily wage, so no rational company can accept
/// it. Larger loads continue to use the ordinary per-bulk rate.
pub const CONTRACT_MINIMUM_DELIVERY_PENNIES: u64 = 200;
const JOURNEY_PENNIES_PER_100_METRES: u64 = 5;
const ROUTE_REACH: f32 = 1.5;
const ROUTE_MANAGEMENT_INTERVAL_WORLD_SECONDS: f64 = 5.0;

#[derive(Component, Debug, Clone, Copy)]
pub struct TradeRouteRoutine {
    pub route: TradeRouteId,
    phase: TradeRoutePhase,
    departed_day: u32,
    departed_world_seconds: f64,
    source_purchase_cost: u64,
    source_market_fees: u64,
    cargo_units: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TradeRoutePhase {
    GoingToOrigin,
    InTransit,
    ReturningToOrigin,
    ReturningToWarehouse,
}

impl TradeRouteRoutine {
    pub(crate) const fn objective(&self) -> CharacterObjective {
        match self.phase {
            TradeRoutePhase::GoingToOrigin => CharacterObjective::GoingToTradeRoutePickup,
            TradeRoutePhase::InTransit => CharacterObjective::HaulingInterSettlementCargo,
            TradeRoutePhase::ReturningToOrigin | TradeRoutePhase::ReturningToWarehouse => {
                CharacterObjective::ReturningFromTradeRoute
            }
        }
    }
}

#[derive(Clone, Copy)]
struct HallSnapshot {
    position: Vec3,
    rotation: f32,
}

#[derive(Clone, Copy)]
struct WarehouseSnapshot {
    id: shared::components::BuildingId,
    settlement: shared::components::SettlementId,
    company: shared::components::CompanyId,
    position: Vec3,
    rotation: f32,
    can_operate: bool,
}

fn hall_entrance(hall: HallSnapshot) -> Vec3 {
    SettlementBuildingKind::Hall.entrance_position(hall.position, hall.rotation)
}

fn absolute_world_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

fn contract_delivery_pennies_per_bulk(good: Good, units: u32) -> u64 {
    let total_bulk = u64::from(units.max(1)).saturating_mul(u64::from(good.bulk_per_unit()));
    CONTRACT_DELIVERY_PENNIES_PER_BULK.max(CONTRACT_MINIMUM_DELIVERY_PENNIES.div_ceil(total_bulk))
}

/// The Town Works is the buyer. It checks its own exchange first; only the
/// uncovered part becomes an import order. Escrow is removed from treasury at
/// posting, so a town cannot promise the same coin to payroll or two carriers.
#[allow(clippy::type_complexity)]
pub fn post_civic_import_contracts(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    projects: Query<(
        &CivicHallUpgradeWorksite,
        &shared::components::BuildingOf,
        &GoodsInventory,
        &ConstructionSite,
    )>,
    mut halls: Query<(
        &shared::components::SettlementId,
        &mut Settlement,
        &GoodsInventory,
        &MootMarket,
    )>,
    contracts: Query<&CivicTradeContract>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let active: HashSet<_> = contracts
        .iter()
        .filter(|contract| contract.status.is_active())
        .map(|contract| (contract.destination, contract.good))
        .collect();

    let source_offers: Vec<_> = halls
        .iter()
        .flat_map(|(settlement, _, inventory, market)| {
            market.listings().iter().filter_map(move |listing| {
                let MarketSeller::Business(_) = listing.seller else {
                    return None;
                };
                let physical = inventory.amount(listing.good);
                (listing.units > 0 && physical > 0).then_some((
                    *settlement,
                    listing.seller,
                    listing.good,
                    listing.unit_price,
                    listing.units.min(physical),
                ))
            })
        })
        .collect();

    for (project, building_of, inventory, site) in projects.iter() {
        if site.raising || active.contains(&(building_of.0, project.material)) {
            continue;
        }
        let remaining = project
            .material_required
            .saturating_sub(inventory.amount(project.material));
        if remaining == 0 {
            continue;
        }
        let Some((_, mut destination, destination_store, destination_market)) = halls
            .iter_mut()
            .find(|(settlement, ..)| **settlement == building_of.0)
        else {
            continue;
        };
        let local_private_units = destination_market
            .listings()
            .iter()
            .filter(|listing| {
                listing.good == project.material
                    && matches!(listing.seller, MarketSeller::Business(_))
            })
            .map(|listing| listing.units)
            .fold(0u32, u32::saturating_add)
            .min(destination_store.amount(project.material));
        let import_units = remaining.saturating_sub(local_private_units);
        if import_units == 0 {
            continue;
        }

        let source_offer = source_offers
            .iter()
            .copied()
            .filter(|(origin, _, good, _, _)| *origin != building_of.0 && *good == project.material)
            .min_by_key(|(origin, seller, _, price, _)| (*price, *origin, *seller));
        let maximum_unit_price = source_offer
            .map(|(_, _, _, price, _)| price)
            .unwrap_or_else(|| project.material.base_price());
        let offered_units = source_offer
            .map(|(_, _, _, _, units)| units)
            .unwrap_or(import_units);
        let carry_units =
            (shared::economy::capacity::PORTER / project.material.bulk_per_unit()).max(1);
        let candidate_units = import_units.min(offered_units).min(carry_units);
        let Some((units, delivery_fee_per_bulk, reserved_cash)) =
            (1..=candidate_units).rev().find_map(|units| {
                let delivery_fee_per_bulk =
                    contract_delivery_pennies_per_bulk(project.material, units);
                let per_unit = maximum_unit_price.saturating_add(
                    delivery_fee_per_bulk
                        .saturating_mul(u64::from(project.material.bulk_per_unit())),
                );
                let reserved_cash = per_unit.saturating_mul(u64::from(units));
                (reserved_cash <= destination.treasury).then_some((
                    units,
                    delivery_fee_per_bulk,
                    reserved_cash,
                ))
            })
        else {
            continue;
        };
        destination.treasury -= reserved_cash;
        commands.spawn((
            CivicTradeContract {
                origin: source_offer.map(|(origin, ..)| origin),
                destination: building_of.0,
                good: project.material,
                source_seller: source_offer.map(|(_, seller, ..)| seller),
                requested_units: units,
                delivered_units: 0,
                maximum_unit_price,
                delivery_fee_per_bulk,
                reserved_cash,
                escrow_cash: reserved_cash,
                spent_on_goods: 0,
                spent_on_freight: 0,
                created_day: day,
                last_attempt_day: u32::MAX,
                status: TradeContractStatus::Open,
            },
            Replicate::to_clients(NetworkTarget::All),
        ));
        if let Some((origin, ..)) = source_offer {
            info!(
                "Settlement '{}': posted a buyer-funded import contract for {} {} from settlement #{} ({} coin reserved)",
                destination.name,
                units,
                project.material.label(),
                origin.0,
                shared::economy::format_money(reserved_cash),
            );
        } else {
            info!(
                "Settlement '{}': posted the first cash-backed tender for {} {} at up to {} each ({} coin reserved)",
                destination.name,
                units,
                project.material.label(),
                shared::economy::format_money(maximum_unit_price),
                shared::economy::format_money(reserved_cash),
            );
        }
    }
}

/// Translate open contracts into ordinary company assets. A completed and
/// staffed Storage Hall is mandatory, and the route's expected fee must cover
/// one normal wage plus a small distance allowance. No company is tagged as a
/// special merchant type.
#[allow(clippy::type_complexity)]
pub fn manage_company_trade_routes(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut next_review_world_seconds: Local<f64>,
    halls: Query<
        (
            Entity,
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &GoodsInventory,
            &MootMarket,
        ),
        With<Settlement>,
    >,
    warehouses: Query<(
        Entity,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &shared::components::OperatedBy,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&BusinessCondition>,
    )>,
    mut contracts: Query<(Entity, &TradeContractId, &mut CivicTradeContract)>,
    mut routes: Query<(Entity, &TradeRouteId, &mut CompanyTradeRoute)>,
    porters: Query<
        (
            Entity,
            &shared::components::PersonId,
            &CompanyPorter,
            &GoodsInventory,
            Option<&TradeRouteRoutine>,
            Option<&InternalDeliveryRoutine>,
            Option<&MarketCollectionRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&MootQueueTicket>,
            Option<&MootMealRoutine>,
            Option<&strategic::StrategicPerson>,
            Option<&strategic::StrategicTravel>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let now = absolute_world_seconds(clock);
    if now + f64::EPSILON < *next_review_world_seconds {
        return;
    }
    *next_review_world_seconds = now + ROUTE_MANAGEMENT_INTERVAL_WORLD_SECONDS;
    let day = clock.day;
    let hall_snapshots: HashMap<_, _> = halls
        .iter()
        .map(|(_entity, id, position, rotation, _, _)| {
            (
                *id,
                HallSnapshot {
                    position: position.0,
                    rotation: rotation.map_or(0.0, |rotation| rotation.0),
                },
            )
        })
        .collect();
    let source_offers: Vec<_> = halls
        .iter()
        .flat_map(|(_, settlement, _, _, inventory, market)| {
            market.listings().iter().filter_map(move |listing| {
                let MarketSeller::Business(_) = listing.seller else {
                    return None;
                };
                let physical = inventory.amount(listing.good);
                (listing.units > 0 && physical > 0).then_some((
                    *settlement,
                    listing.seller,
                    listing.good,
                    listing.unit_price,
                    listing.units.min(physical),
                ))
            })
        })
        .collect();
    let warehouse_snapshots: Vec<_> = warehouses
        .iter()
        .filter(|(_, _, _, _, building, ..)| building.kind == SettlementBuildingKind::StorageHall)
        .map(
            |(_entity, id, building_of, company, _, position, rotation, condition)| {
                WarehouseSnapshot {
                    id: *id,
                    settlement: building_of.0,
                    company: company.0,
                    position: position.0,
                    rotation: rotation.0,
                    can_operate: condition.is_none_or(|condition| condition.state.can_operate()),
                }
            },
        )
        .collect();

    // Repair assignments whose worker disappeared or lost the job. Cargo is
    // protected by the worker's carried inventory while alive; before pickup,
    // an abandoned assignment can safely return to the open board.
    let active_route_ids: HashSet<_> = porters
        .iter()
        .filter_map(|(_, _, _, _, routine, ..)| routine.map(|routine| routine.route))
        .collect();
    for (_, route_id, mut route) in routes.iter_mut() {
        if route.assigned_caravaner.is_some() && !active_route_ids.contains(route_id) {
            route.assigned_caravaner = None;
            if let Some(contract_id) = route.active_contract {
                if let Some((_, _, mut contract)) =
                    contracts.iter_mut().find(|(_, id, _)| **id == contract_id)
                {
                    match contract.status {
                        TradeContractStatus::Assigned | TradeContractStatus::Open => {
                            contract.status = TradeContractStatus::Open;
                            route.status = TradeRouteStatus::WaitingForPorter;
                        }
                        TradeContractStatus::InTransit => {
                            // Never purchase a second load merely because an
                            // embodied caravaner disappeared. Cargo recovery
                            // can later resume this route explicitly; until
                            // then both the destination's ownership claim and
                            // the remaining escrow stay intact and auditable.
                            route.status = TradeRouteStatus::Mothballed;
                            warn!(
                                "Company #{} mothballed route #{}: porter vanished with buyer-owned cargo in transit",
                                route.company.0, route_id.0,
                            );
                        }
                        TradeContractStatus::Fulfilled | TradeContractStatus::Cancelled => {
                            route.active_contract = None;
                            route.status = TradeRouteStatus::Idle;
                        }
                    }
                } else {
                    route.active_contract = None;
                    route.status = TradeRouteStatus::Idle;
                }
            } else {
                route.status = TradeRouteStatus::Idle;
            }
        }
    }

    let route_contracts: HashSet<_> = routes
        .iter()
        .filter_map(|(_, _, route)| route.active_contract)
        .collect();
    for (_, contract_id, mut contract) in contracts.iter_mut() {
        if contract.status != TradeContractStatus::Open
            || route_contracts.contains(contract_id)
            || contract.last_attempt_day == day
        {
            continue;
        }
        // Route formation is a daily company decision, not a per-frame poll.
        // A missing warehouse or employee can change on a later day without
        // making every Activity tick rescan all companies in the meantime.
        contract.last_attempt_day = day;
        if contract.origin.is_none() || contract.source_seller.is_none() {
            let Some((origin, seller, _, _, _)) = source_offers
                .iter()
                .copied()
                .filter(|(origin, _, good, price, units)| {
                    *origin != contract.destination
                        && *good == contract.good
                        && *price <= contract.maximum_unit_price
                        && *units >= contract.remaining_units()
                })
                .min_by_key(|(origin, seller, _, price, _)| (*price, *origin, *seller))
            else {
                continue;
            };
            contract.origin = Some(origin);
            contract.source_seller = Some(seller);
            info!(
                "Civic trade contract #{} bound its {} tender to seller {:?} in settlement #{}",
                contract_id.0,
                contract.good.label(),
                seller,
                origin.0,
            );
        }
        let Some(origin_id) = contract.origin else {
            continue;
        };
        let (Some(origin), Some(destination)) = (
            hall_snapshots.get(&origin_id),
            hall_snapshots.get(&contract.destination),
        ) else {
            continue;
        };
        let fee = u64::from(contract.remaining_units())
            .saturating_mul(u64::from(contract.good.bulk_per_unit()))
            .saturating_mul(contract.delivery_fee_per_bulk);
        let distance = Vec2::new(
            destination.position.x - origin.position.x,
            destination.position.z - origin.position.z,
        )
        .length();
        let journey_allowance =
            ((distance / 100.0).ceil() as u64).saturating_mul(JOURNEY_PENNIES_PER_100_METRES);
        let Some(warehouse) = warehouse_snapshots
            .iter()
            .copied()
            .filter(|warehouse| warehouse.can_operate && warehouse.settlement == origin_id)
            .filter(|warehouse| {
                porters.iter().any(|(_, _, porter, _, ..)| {
                    porter.company == warehouse.company && porter.storage_hall == warehouse.id
                })
            })
            .filter(|_| fee >= FOUNDING_DAILY_WAGE.saturating_add(journey_allowance))
            .min_by_key(|warehouse| (warehouse.company, warehouse.id))
        else {
            continue;
        };

        let reusable = routes
            .iter_mut()
            .find(|(_, _, route)| {
                route.company == warehouse.company
                    && route.warehouse == warehouse.id
                    && route.origin == origin_id
                    && route.destination == contract.destination
                    && route.good == contract.good
                    && route.active_contract.is_none()
                    && matches!(
                        route.status,
                        TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                    )
            })
            .map(|(_, _, route)| route);
        if let Some(mut route) = reusable {
            route.cargo_target = contract.remaining_units();
            route.maximum_purchase_price = contract.maximum_unit_price;
            route.active_contract = Some(*contract_id);
            route.status = TradeRouteStatus::WaitingForPorter;
        } else {
            commands.spawn((
                CompanyTradeRoute {
                    company: warehouse.company,
                    warehouse: warehouse.id,
                    origin: origin_id,
                    destination: contract.destination,
                    good: contract.good,
                    cargo_target: contract.remaining_units(),
                    maximum_purchase_price: contract.maximum_unit_price,
                    minimum_destination_price: 0,
                    automatic: true,
                    active_contract: Some(*contract_id),
                    assigned_caravaner: None,
                    status: TradeRouteStatus::WaitingForPorter,
                    completed_trips: 0,
                    lifetime_units: 0,
                    lifetime_delivery_revenue: 0,
                },
                TradeRouteHistory::default(),
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
        contract.status = TradeContractStatus::Assigned;
    }

    // Dispatch only after a route has received its stable id (normally the
    // update after creation). The same porter remains an ordinary employee of
    // the warehouse and therefore continues through normal payroll.
    for (_, route_id, mut route) in routes.iter_mut() {
        if route.status != TradeRouteStatus::WaitingForPorter
            || route.assigned_caravaner.is_some()
            || route.active_contract.is_none()
        {
            continue;
        }
        let Some(origin) = hall_snapshots.get(&route.origin).copied() else {
            continue;
        };
        let candidate = porters
            .iter()
            .filter(
                |(
                    _,
                    _,
                    porter,
                    inventory,
                    routine,
                    internal,
                    market,
                    home,
                    road,
                    queue,
                    meal,
                    _,
                    strategic_travel,
                )| {
                    porter.company == route.company
                        && porter.storage_hall == route.warehouse
                        && inventory.is_empty()
                        && routine.is_none()
                        && internal.is_none()
                        && market.is_none()
                        && home.is_none()
                        && road.is_none()
                        && queue.is_none()
                        && meal.is_none()
                        && strategic_travel.is_none()
                },
            )
            .min_by_key(|(_, person, ..)| **person);
        let Some((porter_entity, person, ..)) = candidate else {
            continue;
        };
        route.assigned_caravaner = Some(*person);
        route.status = TradeRouteStatus::GoingToOrigin;
        commands
            .entity(porter_entity)
            .remove::<strategic::StrategicPerson>()
            .remove::<strategic::PendingStrategicDemotion>()
            .insert((
                TradeRouteRoutine {
                    route: *route_id,
                    phase: TradeRoutePhase::GoingToOrigin,
                    departed_day: day,
                    departed_world_seconds: now,
                    source_purchase_cost: 0,
                    source_market_fees: 0,
                    cargo_units: 0,
                },
                MoveTarget(hall_entrance(origin)),
            ));
        info!(
            "Company #{} dispatched porter #{} on route #{} from settlement #{} to #{}",
            route.company.0, person.0, route_id.0, route.origin.0, route.destination.0,
        );
    }
}

/// Advance one embodied collection, outward haul and return journey. The
/// route performs no all-town scans per porter; stable ids join each bounded
/// route/contract set, while the normal navigation queue owns pathfinding.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_company_trade_routes(
    _simulation_time: crate::world::simulation_time::SimulationTime,
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut Settlement,
            &mut GoodsInventory,
            &mut MootMarket,
            Option<&mut shared::economy::CivicAccount>,
        ),
        (
            With<Settlement>,
            Without<SettlementBuilding>,
            Without<CharacterKind>,
        ),
    >,
    mut projects: Query<
        (
            &CivicHallUpgradeWorksite,
            &shared::components::BuildingOf,
            &ConstructionSite,
            &mut GoodsInventory,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    mut warehouses: Query<
        (
            &shared::components::BuildingId,
            &shared::components::OperatedBy,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &mut BusinessAccount,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut contracts: Query<(&TradeContractId, &mut CivicTradeContract)>,
    mut routes: Query<(
        &TradeRouteId,
        &mut CompanyTradeRoute,
        &mut TradeRouteHistory,
    )>,
    mut porters: Query<
        (
            Entity,
            &shared::components::PersonId,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            &mut TradeRouteRoutine,
            Option<&NavigationRouteFailed>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let now = absolute_world_seconds(clock);
    if porters.is_empty() {
        return;
    }

    for (
        porter_entity,
        person,
        position,
        mut activity,
        mut carrier,
        move_target,
        mut routine,
        route_failed,
    ) in porters.iter_mut()
    {
        let Some((_, mut route, mut history)) =
            routes.iter_mut().find(|(id, ..)| **id == routine.route)
        else {
            commands.entity(porter_entity).remove::<TradeRouteRoutine>();
            continue;
        };

        // Returning is deliberately independent of the contract entity. A
        // fulfilled/cancelled contract may be archived or removed while the
        // embodied porter is still walking home; that must never strand the
        // employee in a permanent route routine.
        let warehouse =
            warehouses
                .iter_mut()
                .find_map(|(id, company, building, at, rotation, _)| {
                    (*id == route.warehouse
                        && company.0 == route.company
                        && building.kind == SettlementBuildingKind::StorageHall)
                        .then_some(WarehouseSnapshot {
                            id: *id,
                            settlement: route.origin,
                            company: company.0,
                            position: at.0,
                            rotation: rotation.0,
                            can_operate: true,
                        })
                });
        let Some(warehouse) = warehouse else {
            continue;
        };
        if routine.phase == TradeRoutePhase::ReturningToOrigin {
            let Some(origin) = halls.iter().find_map(|(id, at, rotation, ..)| {
                (*id == route.origin).then_some(HallSnapshot {
                    position: at.0,
                    rotation: rotation.map_or(0.0, |rotation| rotation.0),
                })
            }) else {
                continue;
            };
            let target = hall_entrance(origin);
            if ground_distance(position.0, target) > ROUTE_REACH {
                ensure_move_target(&mut commands, porter_entity, move_target, target);
                continue;
            }
            routine.phase = TradeRoutePhase::ReturningToWarehouse;
            commands
                .entity(porter_entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        }
        if routine.phase == TradeRoutePhase::ReturningToWarehouse {
            let target = SettlementBuildingKind::StorageHall
                .entrance_position(warehouse.position, warehouse.rotation);
            if ground_distance(position.0, target) > ROUTE_REACH {
                ensure_move_target(&mut commands, porter_entity, move_target, target);
                continue;
            }
            route.assigned_caravaner = None;
            route.active_contract = None;
            route.status = TradeRouteStatus::Idle;
            *activity = CharacterActivity::Idle;
            commands
                .entity(porter_entity)
                .remove::<TradeRouteRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            debug!(
                "Company #{} route #{} porter #{} returned to its warehouse",
                route.company.0, routine.route.0, person.0,
            );
            continue;
        }
        let Some(contract_id) = route.active_contract else {
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::ReturningToOrigin;
            continue;
        };
        let Some((_, mut contract)) = contracts.iter_mut().find(|(id, _)| **id == contract_id)
        else {
            route.active_contract = None;
            route.status = TradeRouteStatus::Returning;
            routine.phase = TradeRoutePhase::ReturningToOrigin;
            continue;
        };

        let origin = halls.iter().find_map(|(id, at, rotation, ..)| {
            (*id == route.origin).then_some(HallSnapshot {
                position: at.0,
                rotation: rotation.map_or(0.0, |rotation| rotation.0),
            })
        });
        let destination = halls.iter().find_map(|(id, at, rotation, ..)| {
            (*id == route.destination).then_some(HallSnapshot {
                position: at.0,
                rotation: rotation.map_or(0.0, |rotation| rotation.0),
            })
        });
        let Some((origin, destination)) = origin.zip(destination) else {
            continue;
        };

        if route_failed.is_some() {
            // The shared navigation retry system owns its exponential
            // backoff and wakes this exact target after geometry changes or
            // the retry deadline. Clearing all route state here made an
            // impossible overland journey launch another full search every
            // update. Keep the transaction and buyer-owned cargo intact while
            // the caravan waits; future bridges/ships can make it reachable.
            continue;
        }

        match routine.phase {
            TradeRoutePhase::GoingToOrigin => {
                let target = hall_entrance(origin);
                if ground_distance(position.0, target) > ROUTE_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, target);
                    continue;
                }
                commands.entity(porter_entity).remove::<MoveTarget>();
                route.status = TradeRouteStatus::Loading;
                let wanted = contract
                    .remaining_units()
                    .min(route.cargo_target)
                    .min(carrier.free_bulk() / route.good.bulk_per_unit().max(1));
                let delivery_reserve = u64::from(wanted)
                    .saturating_mul(u64::from(route.good.bulk_per_unit()))
                    .saturating_mul(contract.delivery_fee_per_bulk);
                let purchase_budget = contract.escrow_cash.saturating_sub(delivery_reserve);
                let Some((source_id, _, _, _, mut source_store, mut source_market, _)) =
                    halls.iter_mut().find(|(id, ..)| **id == route.origin)
                else {
                    continue;
                };
                let requested = wanted.min(source_store.amount(route.good));
                let purchase = source_market.purchase_from_seller(
                    contract
                        .source_seller
                        .expect("a dispatched contract must have a bound seller"),
                    route.good,
                    requested,
                    purchase_budget,
                    Some(route.maximum_purchase_price),
                );
                if purchase.trade.units == 0 {
                    // A named offer is not a reservation: local households or
                    // processors may legitimately buy it before the wagon
                    // arrives. Close and refund this stale contract so the
                    // civic buyer can post a fresh order against the current
                    // best real seller on the next planning pass.
                    if let Some((_, _, _, mut destination_settlement, _, _, _)) =
                        halls.iter_mut().find(|(id, ..)| **id == route.destination)
                    {
                        destination_settlement.treasury = destination_settlement
                            .treasury
                            .saturating_add(contract.escrow_cash);
                        contract.escrow_cash = 0;
                    }
                    contract.status = TradeContractStatus::Cancelled;
                    contract.last_attempt_day = day;
                    route.status = TradeRouteStatus::Returning;
                    routine.phase = TradeRoutePhase::ReturningToWarehouse;
                    ensure_move_target(
                        &mut commands,
                        porter_entity,
                        move_target,
                        SettlementBuildingKind::StorageHall
                            .entrance_position(warehouse.position, warehouse.rotation),
                    );
                    continue;
                }
                let removed = source_store.remove(route.good, purchase.trade.units);
                let loaded = carrier.add(route.good, removed);
                debug_assert_eq!(loaded, purchase.trade.units);
                contract.escrow_cash = contract.escrow_cash.saturating_sub(purchase.trade.pennies);
                contract.spent_on_goods = contract
                    .spent_on_goods
                    .saturating_add(purchase.trade.pennies);
                contract.status = TradeContractStatus::InTransit;
                routine.source_purchase_cost = purchase.trade.pennies;
                routine.source_market_fees = purchase
                    .fills
                    .iter()
                    .map(|fill| fill.market_fee)
                    .fold(0u64, u64::saturating_add);
                routine.cargo_units = loaded;
                business_events.record_market_purchase(day, *source_id, purchase.fills);
                if let Some((_, _, _, _, _, _, Some(mut civic))) =
                    halls.iter_mut().find(|(id, ..)| **id == route.destination)
                {
                    civic.record_material_expense(day.saturating_add(1), purchase.trade.pennies);
                }
                route.status = TradeRouteStatus::InTransit;
                routine.phase = TradeRoutePhase::InTransit;
                *activity = CharacterActivity::Idle;
                let target = projects
                    .iter_mut()
                    .find(|(project, building_of, _, _)| {
                        building_of.0 == route.destination && project.material == route.good
                    })
                    .map_or_else(|| hall_entrance(destination), |(_, _, site, _)| site.stand);
                ensure_move_target(&mut commands, porter_entity, move_target, target);
            }
            TradeRoutePhase::InTransit => {
                let target = projects
                    .iter_mut()
                    .find(|(project, building_of, _, _)| {
                        building_of.0 == route.destination && project.material == route.good
                    })
                    .map_or_else(|| hall_entrance(destination), |(_, _, site, _)| site.stand);
                if ground_distance(position.0, target) > ROUTE_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, target);
                    continue;
                }
                let carried = carrier.amount(route.good).min(routine.cargo_units);
                let destination_project =
                    projects.iter_mut().find(|(project, building_of, _, _)| {
                        building_of.0 == route.destination && project.material == route.good
                    });
                let delivered = if let Some((_, _, _, mut store)) = destination_project {
                    store.add(route.good, carried)
                } else {
                    // The destination already owns paid cargo in transit. If
                    // local suppliers happened to finish Town Works before the
                    // wagon arrived, retain the shipment as civic reserve at
                    // the Hall instead of leaving the porter in a permanent
                    // wait for a project which will never respawn.
                    halls
                        .iter_mut()
                        .find(|(id, ..)| **id == route.destination)
                        .map_or(0, |(_, _, _, _, mut store, _, _)| {
                            store.add(route.good, carried)
                        })
                };
                if delivered == 0 {
                    // The destination still owns the load. A genuinely full
                    // worksite/Hall may clear later, so preserve the cargo on
                    // the porter rather than deleting it or converting it into
                    // carrier property.
                    continue;
                }
                carrier.remove(route.good, delivered);
                contract.delivered_units = contract.delivered_units.saturating_add(delivered);
                let freight = u64::from(delivered)
                    .saturating_mul(u64::from(route.good.bulk_per_unit()))
                    .saturating_mul(contract.delivery_fee_per_bulk)
                    .min(contract.escrow_cash);
                contract.escrow_cash -= freight;
                contract.spent_on_freight = contract.spent_on_freight.saturating_add(freight);
                if let Some((_, _, _, mut destination_settlement, _, _, civic)) =
                    halls.iter_mut().find(|(id, ..)| **id == route.destination)
                {
                    if let Some(mut civic) = civic {
                        civic.record_freight_expense(day.saturating_add(1), freight);
                    }
                    if contract.delivered_units >= contract.requested_units {
                        destination_settlement.treasury = destination_settlement
                            .treasury
                            .saturating_add(contract.escrow_cash);
                        contract.escrow_cash = 0;
                        contract.status = TradeContractStatus::Fulfilled;
                    } else {
                        contract.status = TradeContractStatus::Open;
                    }
                }
                if let Some(company_entity) = company_entities
                    .iter()
                    .find_map(|(entity, id)| (*id == route.company).then_some(entity))
                {
                    if let Ok(mut account) = company_accounts.get_mut(company_entity) {
                        account.credit(freight);
                    }
                }
                if let Some((_, _, _, _, _, mut account)) = warehouses
                    .iter_mut()
                    .find(|(id, ..)| **id == route.warehouse)
                {
                    account.record_service_revenue(day, freight);
                }
                route.completed_trips = route.completed_trips.saturating_add(1);
                route.lifetime_units = route.lifetime_units.saturating_add(delivered);
                route.lifetime_delivery_revenue =
                    route.lifetime_delivery_revenue.saturating_add(freight);
                history.record(TradeRouteTrip {
                    departed_day: routine.departed_day,
                    completed_day: day,
                    units: delivered,
                    source_purchase_cost: routine.source_purchase_cost,
                    source_market_fees: routine.source_market_fees,
                    delivery_revenue: freight,
                    travel_world_seconds: (now - routine.departed_world_seconds)
                        .max(0.0)
                        .min(f64::from(u32::MAX)) as u32,
                });
                info!(
                    "Company #{} route #{} delivered {} {} to settlement #{} and earned {} coin",
                    route.company.0,
                    routine.route.0,
                    delivered,
                    route.good.label(),
                    route.destination.0,
                    shared::economy::format_money(freight),
                );
                route.status = TradeRouteStatus::Returning;
                routine.phase = TradeRoutePhase::ReturningToOrigin;
                routine.cargo_units = 0;
                ensure_move_target(
                    &mut commands,
                    porter_entity,
                    move_target,
                    hall_entrance(origin),
                );
            }
            TradeRoutePhase::ReturningToOrigin | TradeRoutePhase::ReturningToWarehouse => {
                unreachable!("returning routes are handled above")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{
        BuildingId, BuildingOf, CivicHallLevel, CompanyId, OperatedBy, SettlementId, SettlementTier,
    };
    use shared::economy::{BusinessAccount, CivicAccount, CompanyAccount};

    const SOURCE: SettlementId = SettlementId(1);
    const DESTINATION: SettlementId = SettlementId(2);
    const SELLER_COMPANY: CompanyId = CompanyId(10);
    const CARRIER_COMPANY: CompanyId = CompanyId(11);
    const QUARRY: BuildingId = BuildingId(20);
    const WAREHOUSE: BuildingId = BuildingId(21);
    const CONTRACT: TradeContractId = TradeContractId(30);
    const ROUTE: TradeRouteId = TradeRouteId(31);
    const PORTER: shared::components::PersonId = shared::components::PersonId(40);

    fn hall(
        id: SettlementId,
        name: &str,
        at: Vec3,
        treasury: u64,
        market: MootMarket,
        inventory: GoodsInventory,
    ) -> impl Bundle {
        (
            id,
            Settlement {
                name: name.to_string(),
                tier: SettlementTier::Village,
                residents: 30,
                treasury,
            },
            PlayerPosition(at),
            PlayerRotation(0.0),
            market,
            inventory,
            CivicAccount::default(),
        )
    }

    #[test]
    fn partial_stone_load_still_covers_the_fixed_carrier_callout() {
        let per_bulk = contract_delivery_pennies_per_bulk(Good::Stone, 3);
        let freight = per_bulk.saturating_mul(u64::from(3 * Good::Stone.bulk_per_unit()));
        assert!(freight >= CONTRACT_MINIMUM_DELIVERY_PENNIES);
        assert!(freight >= FOUNDING_DAILY_WAGE);
    }

    #[test]
    fn town_works_posts_an_escrowed_order_against_real_remote_stock() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());

        let mut source_market = MootMarket::founding();
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        app.world_mut().spawn(hall(
            SOURCE,
            "Stonefield",
            Vec3::ZERO,
            0,
            source_market,
            source_stock,
        ));
        let destination = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Meadowford",
                Vec3::new(40.0, 0.0, 0.0),
                10_000,
                MootMarket::founding(),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Town,
                material: Good::Stone,
                material_required: 8,
            },
            BuildingOf(DESTINATION),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Meadowford".to_string(),
                raising: false,
                stand: Vec3::new(40.0, 0.0, -5.2),
                rotation: 0.0,
            },
            GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
        ));

        app.update();

        let contract = app
            .world_mut()
            .query::<&CivicTradeContract>()
            .single(app.world())
            .expect("Town Works should be the first remote Stone buyer");
        assert_eq!(contract.origin, Some(SOURCE));
        assert_eq!(contract.destination, DESTINATION);
        assert_eq!(contract.source_seller, Some(MarketSeller::Business(QUARRY)));
        assert_eq!(contract.requested_units, 8);
        assert_eq!(contract.status, TradeContractStatus::Open);
        assert_eq!(contract.delivery_fee_per_bulk, 5);
        assert_eq!(contract.reserved_cash, 2_240);
        assert_eq!(contract.escrow_cash, 2_240);
        assert_eq!(
            app.world().get::<Settlement>(destination).unwrap().treasury,
            7_760,
            "contract cash must leave the spendable treasury immediately"
        );
    }

    #[test]
    fn town_works_posts_a_cash_backed_tender_before_any_supplier_exists() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());

        let destination = app
            .world_mut()
            .spawn(hall(
                DESTINATION,
                "Meadowford",
                Vec3::new(40.0, 0.0, 0.0),
                10_000,
                MootMarket::founding(),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Town,
                material: Good::Stone,
                material_required: 8,
            },
            BuildingOf(DESTINATION),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Meadowford".to_string(),
                raising: false,
                stand: Vec3::new(40.0, 0.0, -5.2),
                rotation: 0.0,
            },
            GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
        ));

        app.update();

        let contract = app
            .world_mut()
            .query::<&CivicTradeContract>()
            .single(app.world())
            .expect("Town Works must advertise demand before a quarry exists");
        assert_eq!(contract.origin, None);
        assert_eq!(contract.source_seller, None);
        assert_eq!(contract.requested_units, 8);
        assert_eq!(contract.maximum_unit_price, Good::Stone.base_price());
        assert_eq!(contract.delivery_fee_per_bulk, 5);
        assert_eq!(contract.escrow_cash, 2_240);
        assert_eq!(
            app.world().get::<Settlement>(destination).unwrap().treasury,
            7_760
        );
    }

    #[test]
    fn ai_route_waits_for_a_staffed_warehouse_and_reviews_only_daily() {
        let mut app = App::new();
        app.add_systems(Update, manage_company_trade_routes);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut source_market = MootMarket::founding();
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        let source_hall = app
            .world_mut()
            .spawn(hall(
                SOURCE,
                "Stonefield",
                Vec3::ZERO,
                0,
                source_market,
                source_stock,
            ))
            .id();
        app.world_mut().spawn(hall(
            DESTINATION,
            "Meadowford",
            Vec3::new(40.0, 0.0, 0.0),
            0,
            MootMarket::founding(),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ));
        let contract_entity = app
            .world_mut()
            .spawn((
                CONTRACT,
                CivicTradeContract {
                    origin: None,
                    destination: DESTINATION,
                    good: Good::Stone,
                    source_seller: None,
                    requested_units: 8,
                    delivered_units: 0,
                    maximum_unit_price: 250,
                    delivery_fee_per_bulk: CONTRACT_DELIVERY_PENNIES_PER_BULK,
                    reserved_cash: 2_144,
                    escrow_cash: 2_144,
                    spent_on_goods: 0,
                    spent_on_freight: 0,
                    created_day: 0,
                    last_attempt_day: u32::MAX,
                    status: TradeContractStatus::Open,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .get::<CivicTradeContract>(contract_entity)
                .unwrap()
                .last_attempt_day,
            0
        );
        assert_eq!(
            app.world()
                .get::<CivicTradeContract>(contract_entity)
                .unwrap()
                .origin,
            Some(SOURCE),
            "the cash-backed tender should bind to the first complete real offer"
        );
        assert_eq!(
            app.world_mut()
                .query::<&CompanyTradeRoute>()
                .iter(app.world())
                .count(),
            0
        );

        app.world_mut().spawn((
            WAREHOUSE,
            BuildingOf(SOURCE),
            OperatedBy(CARRIER_COMPANY),
            SettlementBuilding {
                kind: SettlementBuildingKind::StorageHall,
                settlement: "Stonefield".to_string(),
                owner: Some("Carrier".to_string()),
                quality: 1.0,
                workers: vec!["Caravaner".to_string()],
            },
            PlayerPosition(Vec3::new(-12.0, 0.0, 0.0)),
            PlayerRotation(0.0),
        ));
        app.world_mut().spawn((
            CharacterKind::Villager,
            PORTER,
            CompanyPorter {
                settlement: source_hall,
                settlement_id: SOURCE,
                company: CARRIER_COMPANY,
                storage_hall: WAREHOUSE,
            },
            GoodsInventory::new(shared::economy::capacity::PORTER),
        ));

        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&CompanyTradeRoute>()
                .iter(app.world())
                .count(),
            0,
            "adding capacity later in the same day must not create a per-frame decision loop"
        );
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        let route = app
            .world_mut()
            .query::<&CompanyTradeRoute>()
            .single(app.world())
            .expect("the staffed warehouse should accept the viable contract next day");
        assert_eq!(route.company, CARRIER_COMPANY);
        assert_eq!(route.warehouse, WAREHOUSE);
        assert_eq!(route.good, Good::Stone);
        assert_eq!(route.active_contract, Some(CONTRACT));
    }

    #[test]
    fn physical_contract_trip_pays_seller_then_carrier_and_returns_porter() {
        let mut app = App::new();
        app.init_resource::<BusinessEventQueue>().add_systems(
            Update,
            (run_company_trade_routes, apply_business_events).chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());

        let source_at = Vec3::ZERO;
        let destination_at = Vec3::new(40.0, 0.0, 0.0);
        let warehouse_at = Vec3::new(-12.0, 0.0, 0.0);
        let project_stand = Vec3::new(40.0, 0.0, -5.2);
        let source_entrance = SettlementBuildingKind::Hall.entrance_position(source_at, 0.0);
        let warehouse_entrance =
            SettlementBuildingKind::StorageHall.entrance_position(warehouse_at, 0.0);

        let mut source_market = MootMarket::founding();
        source_market.consign(MarketSeller::Business(QUARRY), Good::Stone, 8, 250);
        let mut source_stock = GoodsInventory::new(shared::economy::capacity::HALL);
        source_stock.add(Good::Stone, 8);
        let source_hall = app
            .world_mut()
            .spawn(hall(
                SOURCE,
                "Stonefield",
                source_at,
                0,
                source_market,
                source_stock,
            ))
            .id();
        app.world_mut().spawn(hall(
            DESTINATION,
            "Meadowford",
            destination_at,
            1_000,
            MootMarket::founding(),
            GoodsInventory::new(shared::economy::capacity::HALL),
        ));
        let project = app
            .world_mut()
            .spawn((
                CivicHallUpgradeWorksite {
                    target: CivicHallLevel::Town,
                    material: Good::Stone,
                    material_required: 8,
                },
                BuildingOf(DESTINATION),
                ConstructionSite {
                    kind: SettlementBuildingKind::Hall,
                    settlement: "Meadowford".to_string(),
                    raising: false,
                    stand: project_stand,
                    rotation: 0.0,
                },
                GoodsInventory::new(8 * Good::Stone.bulk_per_unit()),
            ))
            .id();

        let seller_company = app
            .world_mut()
            .spawn((SELLER_COMPANY, CompanyAccount::default()))
            .id();
        let carrier_company = app
            .world_mut()
            .spawn((CARRIER_COMPANY, CompanyAccount::default()))
            .id();
        let quarry = app
            .world_mut()
            .spawn((
                QUARRY,
                OperatedBy(SELLER_COMPANY),
                BusinessAccount::default(),
            ))
            .id();
        let warehouse = app
            .world_mut()
            .spawn((
                WAREHOUSE,
                BuildingOf(SOURCE),
                OperatedBy(CARRIER_COMPANY),
                SettlementBuilding {
                    kind: SettlementBuildingKind::StorageHall,
                    settlement: "Stonefield".to_string(),
                    owner: Some("Carrier".to_string()),
                    quality: 1.0,
                    workers: vec!["Caravaner".to_string()],
                },
                PlayerPosition(warehouse_at),
                PlayerRotation(0.0),
                BusinessAccount::default(),
            ))
            .id();
        let contract_entity = app
            .world_mut()
            .spawn((
                CONTRACT,
                CivicTradeContract {
                    origin: Some(SOURCE),
                    destination: DESTINATION,
                    good: Good::Stone,
                    source_seller: Some(MarketSeller::Business(QUARRY)),
                    requested_units: 8,
                    delivered_units: 0,
                    maximum_unit_price: 250,
                    delivery_fee_per_bulk: CONTRACT_DELIVERY_PENNIES_PER_BULK,
                    reserved_cash: 2_144,
                    escrow_cash: 2_144,
                    spent_on_goods: 0,
                    spent_on_freight: 0,
                    created_day: 0,
                    last_attempt_day: 0,
                    status: TradeContractStatus::Assigned,
                },
            ))
            .id();
        let route = app
            .world_mut()
            .spawn((
                ROUTE,
                CompanyTradeRoute {
                    company: CARRIER_COMPANY,
                    warehouse: WAREHOUSE,
                    origin: SOURCE,
                    destination: DESTINATION,
                    good: Good::Stone,
                    cargo_target: 8,
                    maximum_purchase_price: 250,
                    minimum_destination_price: 0,
                    automatic: true,
                    active_contract: Some(CONTRACT),
                    assigned_caravaner: Some(PORTER),
                    status: TradeRouteStatus::GoingToOrigin,
                    completed_trips: 0,
                    lifetime_units: 0,
                    lifetime_delivery_revenue: 0,
                },
                TradeRouteHistory::default(),
            ))
            .id();
        let porter = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PORTER,
                PlayerPosition(source_entrance),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::PORTER),
                TradeRouteRoutine {
                    route: ROUTE,
                    phase: TradeRoutePhase::GoingToOrigin,
                    departed_day: 0,
                    departed_world_seconds: 0.0,
                    source_purchase_cost: 0,
                    source_market_fees: 0,
                    cargo_units: 0,
                },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .get::<GoodsInventory>(source_hall)
                .unwrap()
                .amount(Good::Stone),
            0
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(porter)
                .unwrap()
                .amount(Good::Stone),
            8
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(seller_company)
                .unwrap()
                .cash,
            1_900,
            "source firm receives the listing gross less its ordinary 5% market fee"
        );
        assert_eq!(
            app.world().get::<Settlement>(source_hall).unwrap().treasury,
            100
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(quarry)
                .unwrap()
                .gross_revenue,
            2_000
        );

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = project_stand;
        app.update();

        assert_eq!(
            app.world()
                .get::<GoodsInventory>(project)
                .unwrap()
                .amount(Good::Stone),
            8
        );
        assert_eq!(
            app.world()
                .get::<CompanyAccount>(carrier_company)
                .unwrap()
                .cash,
            144
        );
        assert_eq!(
            app.world()
                .get::<BusinessAccount>(warehouse)
                .unwrap()
                .gross_revenue,
            144
        );
        assert_eq!(
            app.world()
                .get::<CivicTradeContract>(contract_entity)
                .unwrap()
                .status,
            TradeContractStatus::Fulfilled
        );
        let route_state = app.world().get::<CompanyTradeRoute>(route).unwrap();
        assert_eq!(route_state.status, TradeRouteStatus::Returning);
        assert_eq!(route_state.completed_trips, 1);
        assert_eq!(
            app.world()
                .get::<TradeRouteHistory>(route)
                .unwrap()
                .trips()
                .len(),
            1
        );

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = source_entrance;
        app.update();
        assert_eq!(
            app.world().get::<TradeRouteRoutine>(porter).unwrap().phase,
            TradeRoutePhase::ReturningToWarehouse,
        );

        app.world_mut().get_mut::<PlayerPosition>(porter).unwrap().0 = warehouse_entrance;
        app.update();

        assert!(app.world().get::<TradeRouteRoutine>(porter).is_none());
        let route_state = app.world().get::<CompanyTradeRoute>(route).unwrap();
        assert_eq!(route_state.status, TradeRouteStatus::Idle);
        assert_eq!(route_state.active_contract, None);
        assert_eq!(route_state.assigned_caravaner, None);
    }
}
