//! Businesses, civic market work, payroll and employment-state reconciliation.
//!
//! Physical production belongs to `trades`; this module owns the commercial
//! decisions and transfers wrapped around that production.

use super::*;
use shared::economy::BusinessStrategy;

pub(crate) fn automatic_owner_strategy(
    owner: Option<shared::components::PersonId>,
    attributes: Option<&CharacterAttributes>,
) -> shared::economy::BusinessStrategy {
    use shared::economy::BusinessStrategy;
    if attributes.is_some_and(|attributes| attributes.intelligence() >= 75) {
        return BusinessStrategy::Cautious;
    }
    if attributes.is_some_and(|attributes| attributes.charm() >= 75) {
        return BusinessStrategy::HighMargin;
    }
    if attributes.is_some_and(|attributes| attributes.physique() >= 75) {
        return BusinessStrategy::Growth;
    }
    match owner.map_or(0, |owner| owner.0) % 5 {
        0 => BusinessStrategy::Balanced,
        1 => BusinessStrategy::Growth,
        2 => BusinessStrategy::HighMargin,
        3 => BusinessStrategy::Cautious,
        _ => BusinessStrategy::Opportunistic,
    }
}

pub(crate) fn business_output(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        SettlementBuildingKind::Windmill => Some(Good::Flour),
        SettlementBuildingKind::Bakery => Some(Good::Bread),
        _ => None,
    }
}

fn procurement_for(kind: SettlementBuildingKind) -> BusinessProcurementPolicy {
    match kind {
        SettlementBuildingKind::Windmill => BusinessProcurementPolicy::none().with_rule(
            Good::Wheat,
            BusinessInputRule {
                enabled: true,
                reorder_below: 2,
                target_units: 10,
                maximum_unit_price: Good::Wheat.base_price().saturating_mul(175) / 100,
            },
        ),
        SettlementBuildingKind::Bakery => BusinessProcurementPolicy::none().with_rule(
            Good::Flour,
            BusinessInputRule {
                enabled: true,
                reorder_below: 4,
                target_units: 12,
                maximum_unit_price: Good::Flour.base_price().saturating_mul(175) / 100,
            },
        ),
        _ => BusinessProcurementPolicy::none(),
    }
}

fn sale_policy_for(kind: SettlementBuildingKind, output: Good) -> BusinessSalePolicy {
    let mut policy = BusinessSalePolicy::for_good(output);
    if processing_recipe(kind).is_some() {
        // A generic two-unit shop reserve is harmless for extractors that
        // continuously create stock, but it deadlocks a new production chain:
        // the first mill batch is exactly the two Flour a bakery needs. Mills
        // and bakeries do not consume their own output, so every finished unit
        // is genuinely saleable and belongs in the market pipeline.
        policy.keep_units = 0;
    }
    policy
}

fn liquidation_price(good: Good, liquidation_days: u16) -> u64 {
    let mut price = good.base_price().max(1);
    for _ in 0..liquidation_days.min(10) {
        let movement = price.saturating_mul(1_500).div_ceil(BASIS_POINTS).max(1);
        price = price.saturating_sub(movement).max(1);
    }
    price.max(good.base_price().saturating_mul(25) / 100).max(1)
}

/// Add the small ledgers used by every productive workplace. Founding working
/// capital is transferred from the owner when possible, never minted.
pub fn ensure_business_economies(
    mut commands: Commands,
    halls: Query<(&shared::components::SettlementId, &MootMarket), With<Settlement>>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        &shared::components::BuildingOf,
        Option<&shared::components::OwnedBy>,
        Option<&BusinessAccount>,
        Option<&BusinessSalePolicy>,
        Option<&BusinessWagePolicy>,
        Option<&BusinessProcurementPolicy>,
        Option<&BusinessManagementPolicy>,
        Option<&BusinessCondition>,
        Option<&InheritedBusinessCapital>,
    )>,
    mut owners: Query<(
        &shared::components::PersonId,
        Option<&CharacterAttributes>,
        &mut Wallet,
    )>,
) {
    let market_snapshots: HashMap<shared::components::SettlementId, OpeningMarketSnapshot> = halls
        .iter()
        .map(|(settlement_id, market)| {
            let mut prices = [0; Good::COUNT];
            let mut scarce = [false; Good::COUNT];
            for good in Good::ALL {
                let pool = market.pool(good);
                prices[good.index()] = market
                    .suggested_price(good)
                    .max(pool.day.high_ask)
                    .max(pool.previous_day.high_ask);
                scarce[good.index()] = pool
                    .day
                    .unmet_units()
                    .saturating_add(pool.previous_day.unmet_units())
                    > 0
                    || market.listed_units(good) < pool.target_stock.max(2) / 2;
            }
            (
                *settlement_id,
                OpeningMarketSnapshot {
                    prices,
                    scarce,
                    market_fee_bps: market.market_fee_bps(),
                },
            )
        })
        .collect();
    for (
        entity,
        building,
        building_of,
        owner_id,
        account,
        policy,
        wage_policy,
        procurement,
        management,
        condition,
        inherited_capital,
    ) in buildings.iter()
    {
        let Some(output) = business_output(building.kind) else {
            continue;
        };
        let strategy = owners
            .iter()
            .find(|(person_id, ..)| owner_id.is_some_and(|owner| **person_id == owner.0))
            .map_or_else(
                || automatic_owner_strategy(owner_id.map(|owner| owner.0), None),
                |(person_id, attributes, _)| automatic_owner_strategy(Some(*person_id), attributes),
            );
        let mut entity_commands = commands.entity(entity);
        if account.is_none() {
            let opening_cash = if let Some(inherited_capital) = inherited_capital {
                inherited_capital.0
            } else {
                let mut opening_cash = 0;
                if let Some((_, _, mut wallet)) = owners
                    .iter_mut()
                    .find(|(person_id, _, _)| owner_id.is_some_and(|owner| **person_id == owner.0))
                {
                    let wanted = if processing_recipe(building.kind).is_some() {
                        6 * PENNIES_PER_COIN
                    } else {
                        2 * PENNIES_PER_COIN
                    };
                    // Processors need working capital but never strip the owner of
                    // their final two personal coins. A partially funded firm can
                    // still buy a first small input batch and prove demand.
                    let contribution = wallet
                        .balance()
                        .saturating_sub(2 * PENNIES_PER_COIN)
                        .min(wanted);
                    if contribution > 0 && wallet.debit(contribution) {
                        opening_cash = contribution;
                    }
                }
                opening_cash
            };
            entity_commands.insert(BusinessAccount::with_capital(opening_cash));
            if inherited_capital.is_some() {
                entity_commands.remove::<InheritedBusinessCapital>();
            }
        }
        if policy.is_none() {
            let mut sale = sale_policy_for(building.kind, output);
            sale.target_margin_bps = strategy.target_margin_bps();
            sale.max_daily_price_change_bps = strategy.daily_price_step_bps();
            sale.asking_unit_price = owner_opening_asking_price(
                building.kind,
                output,
                sale.minimum_unit_price,
                strategy,
                market_snapshots.get(&building_of.0),
            );
            entity_commands.insert(sale);
        }
        if wage_policy.is_none() {
            entity_commands.insert(BusinessWagePolicy::default());
        }
        if procurement.is_none() {
            entity_commands.insert(procurement_for(building.kind));
        }
        if management.is_none() {
            entity_commands.insert(BusinessManagementPolicy::for_strategy(strategy));
        }
        if condition.is_none() {
            entity_commands.insert(BusinessCondition::default());
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OpeningMarketSnapshot {
    prices: [u64; Good::COUNT],
    scarce: [bool; Good::COUNT],
    market_fee_bps: u16,
}

fn owner_opening_asking_price(
    kind: SettlementBuildingKind,
    output: Good,
    minimum_price: u64,
    strategy: BusinessStrategy,
    market: Option<&OpeningMarketSnapshot>,
) -> u64 {
    let local_price = market.map_or(output.base_price(), |market| {
        market.prices[output.index()].max(1)
    });
    let scarce = market.is_some_and(|market| market.scarce[output.index()]);
    let positioned_price = local_price
        .saturating_mul(u64::from(strategy.opening_market_position_bps(scarce)))
        .div_ceil(BASIS_POINTS);

    let sustainable = rated_daily_production(kind, 1.0).map_or(minimum_price, |capacity| {
        let input_cost = capacity.input.map_or(0, |(input, units)| {
            let input_price = market.map_or(input.base_price(), |market| {
                market.prices[input.index()].max(1)
            });
            u64::from(units).saturating_mul(input_price)
        });
        let payroll = u64::from(kind.positions()).saturating_mul(FOUNDING_DAILY_WAGE);
        let unit_cost = input_cost
            .saturating_add(payroll)
            .div_ceil(u64::from(capacity.output_units.max(1)));
        shared::economy::sustainable_unit_price(
            unit_cost,
            market.map_or(0, |market| market.market_fee_bps),
            strategy.target_margin_bps(),
        )
    });
    positioned_price.max(sustainable).max(minimum_price).max(1)
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn intermediate_goods_are_not_trapped_by_the_generic_shop_reserve() {
        assert_eq!(
            sale_policy_for(SettlementBuildingKind::Windmill, Good::Flour).keep_units,
            0
        );
        assert_eq!(
            sale_policy_for(SettlementBuildingKind::Bakery, Good::Bread).keep_units,
            0
        );
        assert_eq!(
            sale_policy_for(SettlementBuildingKind::Farmstead, Good::Wheat).keep_units,
            2
        );
    }

    #[test]
    fn entrants_choose_different_opening_prices_from_their_owner_strategy() {
        let mut prices = [0; Good::COUNT];
        prices[Good::Wheat.index()] = Good::Wheat.base_price();
        prices[Good::Flour.index()] = 6 * PENNIES_PER_COIN;
        let mut scarce = [false; Good::COUNT];
        scarce[Good::Flour.index()] = true;
        let market = OpeningMarketSnapshot {
            prices,
            scarce,
            market_fee_bps: 500,
        };
        let price = |strategy| {
            owner_opening_asking_price(
                SettlementBuildingKind::Windmill,
                Good::Flour,
                1,
                strategy,
                Some(&market),
            )
        };

        assert_eq!(price(BusinessStrategy::Growth), 540);
        assert_eq!(price(BusinessStrategy::Balanced), 600);
        assert_eq!(price(BusinessStrategy::Cautious), 600);
        assert_eq!(price(BusinessStrategy::HighMargin), 690);
        assert_eq!(price(BusinessStrategy::Opportunistic), 720);
    }
}

/// Fill the Reeve position and retire legacy standalone Market Porters. The
/// combined Moot Steward is staffed by the road domain and owns both hauling
/// and road work.
pub fn staff_moot_hall_roles(
    mut commands: Commands,
    mut halls: Query<(
        Entity,
        &Settlement,
        &mut MootAdministration,
        &shared::components::SettlementId,
        &SettlementPolicies,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        &mut WorkStatus,
        Option<&MarketPorter>,
        Option<&crate::world::village_roads::RoadSteward>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
    )>,
) {
    for (hall, settlement, mut administration, settlement_id, policies) in halls.iter_mut() {
        let mut steward = None;
        let mut reeve = None;
        let mut legacy_porters = Vec::new();
        for (entity, person_id, name, intent, _, _, _, _, _, civic_job) in villagers.iter() {
            if intent.settlement() != Some(hall) || !intent.counts_as_resident() {
                continue;
            }
            match civic_job.filter(|job| job.settlement == *settlement_id) {
                Some(job)
                    if matches!(
                        job.role,
                        shared::components::CivicRole::MootSteward
                            | shared::components::CivicRole::RoadSteward
                    ) =>
                {
                    let candidate = (entity, *person_id, name.0.clone());
                    if steward
                        .as_ref()
                        .is_none_or(|(_, current, _)| candidate.1 < *current)
                    {
                        steward = Some(candidate);
                    }
                }
                Some(job) if job.role == shared::components::CivicRole::Reeve => {
                    let candidate = (entity, *person_id, name.0.clone());
                    if reeve
                        .as_ref()
                        .is_none_or(|(_, current, _)| candidate.1 < *current)
                    {
                        reeve = Some(candidate);
                    }
                }
                Some(job) if job.role == shared::components::CivicRole::MarketPorter => {
                    legacy_porters.push(entity);
                }
                _ => {}
            }
        }

        // An old save may have two people in what is now one job. Release the
        // standalone porter cleanly; the Moot Steward receives the marker.
        for legacy in legacy_porters {
            if Some(legacy) == steward.as_ref().map(|(entity, ..)| *entity) {
                continue;
            }
            if let Ok((_, _, _, _, mut occupation, mut status, _, _, _, _)) =
                villagers.get_mut(legacy)
            {
                occupation.0 = None;
                *status = WorkStatus::LookingForWork;
            }
            commands
                .entity(legacy)
                .remove::<shared::components::CivicEmployment>()
                .remove::<MarketPorter>()
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
        }

        administration.road_steward = steward.as_ref().map(|(_, _, name)| name.clone());
        administration.market_porter = administration.road_steward.clone();
        administration.reeve = reeve.as_ref().map(|(_, _, name)| name.clone());
        if reeve.is_some()
            || usize::from(steward.is_some()) >= settlement.residents.saturating_sub(1) as usize
            || !crate::world::village::civic::can_afford_new_civic_hire(
                settlement,
                &administration,
                policies,
            )
        {
            continue;
        }
        let candidate = villagers
            .iter()
            .filter(
                |(_, _, _, intent, occupation, status, _, _, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                        && **status == WorkStatus::LookingForWork
                },
            )
            .min_by_key(|(_, person_id, ..)| **person_id)
            .map(|(entity, ..)| entity);
        let Some(candidate) = candidate else { continue };
        let Ok((_, _, name, _, mut occupation, mut status, _, _, _, _)) =
            villagers.get_mut(candidate)
        else {
            continue;
        };
        occupation.0 = Some("Reeve".to_string());
        *status = WorkStatus::Employed;
        administration.reeve = Some(name.0.clone());
        commands
            .entity(candidate)
            .insert(shared::components::CivicEmployment {
                settlement: *settlement_id,
                role: shared::components::CivicRole::Reeve,
            });
        info!(
            "Village '{}': {} took the Reeve position",
            settlement.name, name.0
        );
    }
}

/// Keep the compact work-state component aligned with real rosters. `Chilling`
/// is an intentional choice and is never silently converted back into job
/// seeking merely because the occupation title is empty.
pub fn reconcile_work_statuses(
    mut villagers: Query<
        (
            &Occupation,
            Option<&shared::components::EmployedAt>,
            Option<&shared::components::CivicEmployment>,
            &mut WorkStatus,
        ),
        With<CharacterKind>,
    >,
) {
    for (occupation, employed_at, civic_job, mut status) in villagers.iter_mut() {
        let next = if occupation.0.is_some() || employed_at.is_some() || civic_job.is_some() {
            WorkStatus::Employed
        } else if *status == WorkStatus::Chilling {
            WorkStatus::Chilling
        } else {
            WorkStatus::LookingForWork
        };
        if *status != next {
            *status = next;
        }
    }
}

/// Collect saleable stock with the Moot porter. Goods retain their business
/// owner at the hall and no payment occurs until a real buyer purchases them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_market_collections(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    road_requests: Query<&RoadRequest>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut businesses: Query<
        (
            Entity,
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            &PlayerRotation,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            Option<&BusinessCondition>,
            &shared::components::BuildingId,
            Option<&BusinessProcurementPolicy>,
            &mut BusinessAccount,
            &BusinessWagePolicy,
        ),
        Without<CharacterKind>,
    >,
    mut porters: Query<
        (
            Entity,
            &MarketPorter,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            Option<&mut MarketCollectionRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&MootQueueTicket>,
            Option<&MootMealRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    // Newly attached routines are deferred until the system ends, while the
    // underlying inventories mutate immediately. Keep an explicit reservation
    // ledger so two Moot Stewards cannot promise the same goods or place the
    // same processor order on one tick.
    let active_collections: Vec<_> = porters
        .iter()
        .filter_map(|(_, _, _, _, _, _, routine, ..)| routine.cloned())
        .collect();
    let mut reserved_output: HashMap<(Entity, Good), u32> = HashMap::new();
    let mut reserved_input: HashMap<(Entity, Good), u32> = HashMap::new();
    let mut reserved_hall_bulk: HashMap<Entity, u32> = HashMap::new();
    for routine in active_collections {
        match routine.phase {
            MarketCollectionPhase::GoingToBusiness => {
                *reserved_output
                    .entry((routine.business, routine.good))
                    .or_default() += routine.reserved_units;
                *reserved_hall_bulk.entry(routine.hall).or_default() += routine
                    .reserved_units
                    .saturating_mul(routine.good.bulk_per_unit());
            }
            MarketCollectionPhase::ReturningToHall => {
                *reserved_hall_bulk.entry(routine.hall).or_default() += routine
                    .reserved_units
                    .saturating_mul(routine.good.bulk_per_unit());
            }
            MarketCollectionPhase::DeliveringInput => {
                *reserved_input
                    .entry((routine.business, routine.good))
                    .or_default() += routine.reserved_units;
            }
        }
    }
    for (
        porter_entity,
        porter,
        position,
        mut activity,
        mut carrier,
        move_target,
        routine,
        home,
        road_work,
        shopping,
        queue_ticket,
        meal,
        route_failed,
    ) in porters.iter_mut()
    {
        let assigned_road_repair = road_requests
            .iter()
            .any(|request| request.builder == porter_entity);
        if home.is_some()
            || road_work.is_some()
            || shopping.is_some()
            || queue_ticket.is_some()
            || meal.is_some()
        {
            continue;
        }
        let Ok((settlement_id, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(porter.settlement)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );

        if let Some(failed) = route_failed {
            match routine.as_deref() {
                Some(active) if active.phase == MarketCollectionPhase::GoingToBusiness => {
                    warn!(
                        "Market porter could not reach business at {:.1},{:.1}; cancelling that collection so another offer can be tried",
                        failed.goal.x, failed.goal.z
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                Some(active) if active.phase == MarketCollectionPhase::ReturningToHall => {
                    // Once stock is physically aboard it must reach the hall.
                    // Clear the static failure and explicitly dirty the target
                    // so the bounded planner gets a fresh request next tick.
                    warn!(
                        "Market porter retrying a loaded return to the Moot Hall after route failure at {:.1},{:.1}",
                        failed.goal.x, failed.goal.z
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert(MoveTarget(hall_entrance));
                }
                Some(active) => {
                    debug_assert_eq!(active.phase, MarketCollectionPhase::DeliveringInput);
                    let target = businesses.get(active.business).ok().map(
                        |(_, building, _, at, rotation, ..)| {
                            building.kind.entrance_position(at.0, rotation.0)
                        },
                    );
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if let Some(target) = target {
                        porter_commands.insert(MoveTarget(target));
                    }
                }
                None => {
                    // A stale failure without a live transaction must never
                    // prevent this unique civic worker from accepting work.
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if carrier.used_bulk() > 0 {
                        porter_commands.insert(MoveTarget(hall_entrance));
                    } else {
                        porter_commands.remove::<MoveTarget>();
                    }
                }
            }
            continue;
        }

        let Some(mut routine) = routine else {
            if carrier.used_bulk() > 0 {
                // Recover an interrupted/legacy load. If its durable routine
                // vanished we no longer know a private claimant, so return it
                // as Treasury stock rather than trapping the unique porter in
                // an endless loaded-at-the-hall loop.
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                for good in Good::ALL {
                    let amount = carrier.amount(good);
                    let moved = carrier.transfer_to(&mut hall_store, good, amount);
                    if moved > 0 {
                        let price = market.suggested_price(good);
                        market.consign(
                            shared::economy::MarketSeller::Treasury(*settlement_id),
                            good,
                            moved,
                            price,
                        );
                    }
                }
                *activity = CharacterActivity::Indoors;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            }

            // The Moot Steward also owns road repair in a Hamlet. Let an
            // already-loaded market trip finish without losing private
            // goods, but do not immediately adopt another collection while a
            // completed building is explicitly waiting for this same person.
            // Otherwise a busy large-town market can starve its only road
            // worker forever after a connector resurvey.
            if assigned_road_repair {
                *activity = CharacterActivity::Idle;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            }

            // Input-consuming businesses use the same physical market and
            // porter as households. Protect wage liabilities plus the next
            // payroll day, then buy only when a rule's reorder point is crossed.
            let mut input_orders: Vec<(
                u32,
                Entity,
                shared::components::BuildingId,
                Good,
                u32,
                u64,
                u64,
                Vec3,
            )> = Vec::new();
            for (
                entity,
                building,
                building_of,
                at,
                rotation,
                inventory,
                _,
                condition,
                building_id,
                procurement,
                account,
                wage,
            ) in businesses.iter()
            {
                if building_of.0 != *settlement_id
                    || condition.is_some_and(|condition| !condition.state.can_operate())
                {
                    continue;
                }
                let Some(procurement) = procurement.filter(|policy| policy.needs_anything()) else {
                    continue;
                };
                // A brand-new processor must be allowed to buy its first
                // inputs before it has revenue. Once it has traded, protect
                // the next full staffed payroll day; two protected days can
                // lock a young processor below the cash needed to earn more.
                let payroll_reserve = if account.gross_revenue == 0
                    && account.current_day.produced_units == 0
                    && account.previous_day.produced_units == 0
                {
                    0
                } else {
                    wage.daily_wage
                        .saturating_mul(u64::from(building.kind.positions()))
                };
                let budget = account
                    .cash
                    .saturating_sub(account.wage_arrears)
                    .saturating_sub(payroll_reserve);
                if budget == 0 {
                    continue;
                }
                for good in Good::ALL {
                    let rule = procurement.rule(good);
                    let in_transit = reserved_input
                        .get(&(entity, good))
                        .copied()
                        .unwrap_or_default();
                    let held = inventory.amount(good).saturating_add(in_transit);
                    if !rule.enabled || held >= rule.reorder_below {
                        continue;
                    }
                    let carrier_room = carrier.free_bulk() / good.bulk_per_unit();
                    let store_room = inventory
                        .free_bulk()
                        .saturating_sub(in_transit.saturating_mul(good.bulk_per_unit()))
                        / good.bulk_per_unit();
                    let available_units = rule
                        .target_units
                        .saturating_sub(held)
                        .min(carrier_room)
                        .min(store_room)
                        .min(hall_store.amount(good));
                    let affordable = market.preview_purchase(
                        good,
                        available_units,
                        budget,
                        Some(rule.maximum_unit_price),
                        Some(shared::economy::MarketSeller::Business(*building_id)),
                    );
                    let units = viable_processing_input_purchase(
                        building.kind,
                        good,
                        held,
                        affordable.units,
                    );
                    if units == 0 {
                        continue;
                    }
                    input_orders.push((
                        rule.reorder_below.saturating_sub(held),
                        entity,
                        *building_id,
                        good,
                        units,
                        rule.maximum_unit_price,
                        budget,
                        building.kind.entrance_position(at.0, rotation.0),
                    ));
                }
            }
            input_orders.sort_unstable_by_key(|(shortage, entity, ..)| {
                (std::cmp::Reverse(*shortage), entity.to_bits())
            });
            if let Some((_, buyer_entity, buyer_id, good, wanted, max_price, budget, entrance)) =
                input_orders.into_iter().next()
            {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                let preview = market.preview_purchase(
                    good,
                    wanted,
                    budget,
                    Some(max_price),
                    Some(shared::economy::MarketSeller::Business(buyer_id)),
                );
                if preview.units > 0 {
                    let Ok((_, _, _, _, _, _, _, _, _, _, mut buyer, _)) =
                        businesses.get_mut(buyer_entity)
                    else {
                        continue;
                    };
                    if !buyer.buy_inputs(day, preview.pennies, preview.units) {
                        continue;
                    }
                    let purchase = market.purchase(
                        good,
                        preview.units,
                        preview.pennies,
                        Some(max_price),
                        Some(shared::economy::MarketSeller::Business(buyer_id)),
                    );
                    debug_assert_eq!(purchase.trade, preview);
                    let moved = hall_store.transfer_to(&mut carrier, good, purchase.trade.units);
                    debug_assert_eq!(moved, purchase.trade.units);
                    business_events.record_market_purchase(day, *settlement_id, purchase.fills);
                    *activity = CharacterActivity::Idle;
                    commands.entity(porter_entity).insert((
                        MarketCollectionRoutine {
                            business: buyer_entity,
                            seller: buyer_id,
                            hall: porter.settlement,
                            good,
                            reserved_units: moved,
                            unit_price: 0,
                            phase: MarketCollectionPhase::DeliveringInput,
                        },
                        MoveTarget(entrance),
                    ));
                    *reserved_input.entry((buyer_entity, good)).or_default() += moved;
                    continue;
                }
            }

            let mut offers: Vec<(Entity, shared::components::BuildingId, Good, u32, u64, Vec3)> =
                Vec::new();
            for (
                entity,
                building,
                building_of,
                at,
                rotation,
                inventory,
                policy,
                condition,
                building_id,
                _,
                _,
                _,
            ) in businesses.iter()
            {
                if building_of.0 != *settlement_id || !policy.collection_enabled {
                    continue;
                }
                let state = condition.map_or(BusinessState::Operating, |condition| condition.state);
                if !state.can_operate() && state != BusinessState::Liquidating {
                    continue;
                }
                let liquidating = state == BusinessState::Liquidating;
                for good in Good::ALL {
                    if !liquidating && business_output(building.kind) != Some(good) {
                        continue;
                    }
                    let already_reserved = reserved_output
                        .get(&(entity, good))
                        .copied()
                        .unwrap_or_default();
                    let surplus = inventory
                        .amount(good)
                        .saturating_sub(if liquidating { 0 } else { policy.keep_units })
                        .saturating_sub(already_reserved);
                    let carrier_room = carrier.free_bulk() / good.bulk_per_unit();
                    let hall_room = hall_store.free_bulk().saturating_sub(
                        reserved_hall_bulk
                            .get(&porter.settlement)
                            .copied()
                            .unwrap_or_default(),
                    ) / good.bulk_per_unit();
                    let units = surplus
                        .min(if liquidating {
                            policy.max_units_per_collection.max(32)
                        } else {
                            policy.max_units_per_collection
                        })
                        .min(carrier_room)
                        .min(hall_room);
                    if units == 0 {
                        continue;
                    }
                    let unit_price = if liquidating {
                        liquidation_price(
                            good,
                            condition.map_or(0, |condition| condition.liquidation_days),
                        )
                    } else {
                        policy
                            .asking_unit_price
                            .max(policy.minimum_unit_price)
                            .max(1)
                    };
                    offers.push((
                        entity,
                        *building_id,
                        good,
                        units,
                        unit_price,
                        building.kind.entrance_position(at.0, rotation.0),
                    ));
                }
            }
            offers.sort_unstable_by_key(|(entity, _, _, _, price, _)| (*price, entity.to_bits()));
            let Some((business, seller, good, offered, unit_price, entrance)) =
                offers.into_iter().next()
            else {
                *activity = CharacterActivity::Indoors;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            };
            *activity = CharacterActivity::Idle;
            commands.entity(porter_entity).insert((
                MarketCollectionRoutine {
                    business,
                    seller,
                    hall: porter.settlement,
                    good,
                    reserved_units: offered,
                    unit_price,
                    phase: MarketCollectionPhase::GoingToBusiness,
                },
                MoveTarget(entrance),
            ));
            *reserved_output.entry((business, good)).or_default() += offered;
            *reserved_hall_bulk.entry(porter.settlement).or_default() +=
                offered.saturating_mul(good.bulk_per_unit());
            continue;
        };

        if routine.hall != porter.settlement {
            commands
                .entity(porter_entity)
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
            continue;
        }
        match routine.phase {
            MarketCollectionPhase::GoingToBusiness => {
                let Ok((_, building, _, at, rotation, mut store, _, _, _, _, _, _)) =
                    businesses.get_mut(routine.business)
                else {
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                };
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let moved = store.transfer_to(&mut carrier, routine.good, routine.reserved_units);
                routine.reserved_units = moved;
                if routine.reserved_units == 0 {
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                *activity = CharacterActivity::Idle;
                commands
                    .entity(porter_entity)
                    .insert(MoveTarget(hall_entrance));
                routine.phase = MarketCollectionPhase::ReturningToHall;
            }
            MarketCollectionPhase::ReturningToHall => {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                let delivered =
                    carrier.transfer_to(&mut hall_store, routine.good, routine.reserved_units);
                if delivered > 0 {
                    market.consign(
                        shared::economy::MarketSeller::Business(routine.seller),
                        routine.good,
                        delivered,
                        routine.unit_price,
                    );
                }
                *activity = CharacterActivity::Indoors;
                commands
                    .entity(porter_entity)
                    .remove::<MarketCollectionRoutine>()
                    .remove::<MoveTarget>();
            }
            MarketCollectionPhase::DeliveringInput => {
                let Ok((_, building, _, at, rotation, mut store, _, _, _, _, _, _)) =
                    businesses.get_mut(routine.business)
                else {
                    // The purchased input remains physically aboard. Drop the
                    // dead destination so the generic recovery branch returns
                    // it to the hall on the next tick.
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .insert(MoveTarget(hall_entrance));
                    continue;
                };
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let delivered =
                    carrier.transfer_to(&mut store, routine.good, routine.reserved_units);
                if delivered == routine.reserved_units {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                }
            }
        }
    }
}

/// Pay daily wages from business cash and let a wealthy owner leave hands-on
/// work when a replacement is ready. Profit withdrawals are reviewed by the
/// management system after payroll. This is daily O(people + workplaces), not
/// per-frame decision search.
pub fn run_business_payroll_and_owner_leisure(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    settlements: Query<(Entity, &shared::components::SettlementId)>,
    mut businesses: Query<(
        Entity,
        &SettlementBuilding,
        &mut BusinessAccount,
        &mut BusinessWagePolicy,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&shared::components::OwnedBy>,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &shared::components::PersonId,
        Option<&shared::components::EmployedAt>,
        &mut Wallet,
        &mut Occupation,
        &mut WorkStatus,
    )>,
    mut processed_day: Local<Option<u32>>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *processed_day == Some(day) {
        return;
    }
    *processed_day = Some(day);
    let settlements_by_id: HashMap<shared::components::SettlementId, Entity> = settlements
        .iter()
        .map(|(entity, settlement_id)| (*settlement_id, entity))
        .collect();
    // Build one daily index. Looking up every roster name by scanning all
    // villagers made payroll O(businesses * population), and the wealthy-owner
    // check repeated that cost even on ticks with no day boundary.
    let mut people_by_id: HashMap<shared::components::PersonId, Entity> = HashMap::new();
    let mut workers_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    let mut available_replacements: HashMap<Entity, usize> = HashMap::new();
    for (entity, _name, intent, person_id, employed_at, _, occupation, status) in villagers.iter() {
        let Some(settlement) = intent.settlement() else {
            continue;
        };
        people_by_id.insert(*person_id, entity);
        if let Some(employment) = employed_at {
            workers_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
        if occupation.0.is_none() && employed_at.is_none() && *status == WorkStatus::LookingForWork
        {
            *available_replacements.entry(settlement).or_default() += 1;
        }
    }

    for (
        business_entity,
        building,
        mut account,
        mut wage_policy,
        building_id,
        building_of,
        owner_id,
    ) in businesses.iter_mut()
    {
        if business_output(building.kind).is_none() {
            continue;
        }
        let Some(settlement) = settlements_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let mut worker_entities: Vec<Entity> = workers_by_building
            .get(building_id)
            .cloned()
            .unwrap_or_default();
        worker_entities.sort_unstable_by_key(|entity| entity.to_bits());
        worker_entities.dedup();
        let worker_count = worker_entities.len();
        let owner_entity = owner_id.and_then(|owner| people_by_id.get(&owner.0).copied());
        if account.last_payroll_day == u32::MAX {
            account.last_payroll_day = day;
        }
        let elapsed = day.saturating_sub(account.last_payroll_day);
        if elapsed > 0 {
            account.last_payroll_day = day;
            wage_policy.daily_wage = wage_policy
                .daily_wage
                .clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
            let per_worker = wage_policy.daily_wage.saturating_mul(u64::from(elapsed));
            // The day boundary pays the shift which just finished. Record the
            // expense against that completed day so its wages and sales appear
            // in the same P&L rather than one calendar day apart.
            account.incur_wages(
                day.saturating_sub(1),
                per_worker.saturating_mul(worker_count as u64),
            );

            // Arrears are a real workplace liability, not merely a warning
            // counter. When later sales make cash available, distribute the
            // entire affordable obligation evenly so no alphabetically-early
            // worker is always paid while everybody else starves.
            if !worker_entities.is_empty() {
                let payment_budget = account.cash.min(account.wage_arrears);
                let worker_count = worker_entities.len() as u64;
                let equal_share = payment_budget / worker_count;
                let remainder = payment_budget % worker_count;
                let mut paid = 0_u64;
                for (index, worker) in worker_entities.iter().copied().enumerate() {
                    let payment = equal_share + u64::from((index as u64) < remainder);
                    let Ok((_, _, _, _, _, mut wallet, _, _)) = villagers.get_mut(worker) else {
                        continue;
                    };
                    wallet.credit(payment);
                    paid = paid.saturating_add(payment);
                }
                let settled = account.pay_wage_claim(paid);
                debug_assert_eq!(settled, paid);
            }

            review_automatic_wage_offer(
                &mut wage_policy,
                elapsed,
                worker_count,
                building.kind.positions() as usize,
                account.cash,
                account.wage_arrears,
            );
        }

        let Some(_owner_id) = owner_id else {
            continue;
        };
        let Some(owner_entity) = owner_entity else {
            continue;
        };
        let owner_is_worker =
            villagers
                .get(owner_entity)
                .is_ok_and(|(_, _, _, _, employed_at, _, _, _)| {
                    employed_at.copied() == Some(shared::components::EmployedAt(*building_id))
                });
        if !owner_is_worker {
            continue;
        }
        let replacement_exists = available_replacements
            .get(&settlement)
            .copied()
            .unwrap_or(0)
            > 0;
        let owner_is_wealthy = villagers
            .get(owner_entity)
            .is_ok_and(|(_, _, _, _, _, wallet, _, _)| wallet.balance() >= WEALTHY_OWNER_MONEY);
        let payroll_secure = account
            .cash
            .saturating_sub(account.wage_arrears)
            .saturating_sub(account.tax_arrears)
            >= wage_policy
                .daily_wage
                .saturating_mul(worker_count as u64)
                .saturating_mul(2);
        if !replacement_exists || !owner_is_wealthy || !payroll_secure {
            continue;
        }
        if let Ok((_, owner_name, _, _, _, _, mut occupation, mut status)) =
            villagers.get_mut(owner_entity)
        {
            occupation.0 = None;
            *status = WorkStatus::Chilling;
            commands
                .entity(owner_entity)
                .remove::<shared::components::EmployedAt>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<MoveTarget>();
            info!(
                "Village '{}': {} became a wealthy owner and left daily {} work",
                building.settlement,
                owner_name.0,
                building.kind.trade().unwrap_or("business")
            );
        }
        let _ = business_entity;
    }
}
