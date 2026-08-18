//! Vacancy matching, skill requirements and durable workplace assignment.

use super::*;

const UNPAID_DAYS_BEFORE_PRIVATE_RESIGNATION: u64 = 3;
const JOB_SWITCH_MINIMUM_RAISE_BPS: u64 = 2_000;
const JOB_SWITCH_MINIMUM_RAISE_PENNIES: u64 = 10;

fn materially_better_wage(current: u64, candidate: u64) -> bool {
    let percentage_raise = current
        .saturating_mul(JOB_SWITCH_MINIMUM_RAISE_BPS)
        .div_ceil(BASIS_POINTS);
    candidate >= current.saturating_add(percentage_raise.max(JOB_SWITCH_MINIMUM_RAISE_PENNIES))
}

/// Equal-wage founding businesses must form a usable production chain before
/// a two-seat workplace monopolises a very small Hamlet's labour. This is only
/// a tie-breaker: an owner can still recruit differently by changing wages.
const fn founding_job_priority(kind: SettlementBuildingKind) -> u8 {
    match kind {
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut => 0,
        SettlementBuildingKind::Windmill => 1,
        SettlementBuildingKind::LumberjackHut => 2,
        SettlementBuildingKind::Bakery => 3,
        _ => 4,
    }
}

fn signed_profit(revenue: u64, cost: u64) -> i64 {
    if revenue >= cost {
        revenue.saturating_sub(cost).min(i64::MAX as u64) as i64
    } else {
        -(cost.saturating_sub(revenue).min(i64::MAX as u64) as i64)
    }
}

#[allow(clippy::too_many_arguments)]
fn marginal_operating_plan(
    day: u32,
    building: &SettlementBuilding,
    inventory: &GoodsInventory,
    account: &BusinessAccount,
    sale: &BusinessSalePolicy,
    wage: &BusinessWagePolicy,
    management: &BusinessManagementPolicy,
    market: Option<&MootMarket>,
    seller: Option<MarketSeller>,
    responsive_sellers: usize,
    generic_output_demand: u32,
    external_output_bid: Option<u64>,
    opening_trial: bool,
) -> BusinessOperatingPlan {
    let positions = building.kind.positions();
    let Some(capacity) = rated_daily_production(building.kind, building.quality) else {
        return BusinessOperatingPlan {
            day,
            target_output_units: 0,
            produced_output_units: 0,
            optimal_positions: 0,
            marginal_daily_profit: 0,
        };
    };
    let output = capacity.output;
    let listed = market.zip(seller).map_or(0, |(market, seller)| {
        market.seller_listed_units(seller, output)
    });
    let stock = inventory.amount(output).saturating_add(listed);
    let own_current = account.current_day.sold_units;
    let own_previous = account.previous_day.sold_units;
    let proven_daily_sales = own_current.max(own_previous);
    let (unavailable, demand_variation, fee_bps, mut output_quote, input_quote) =
        market.map_or((0, 0, 0, output.base_price(), 0), |market| {
            let pool = market.pool(output);
            let sellers = u64::try_from(responsive_sellers.max(1)).unwrap_or(u64::MAX);
            let unavailable = pool
                .day
                .unavailable_units
                .max(pool.previous_day.unavailable_units)
                .div_ceil(sellers)
                .min(u64::from(u32::MAX)) as u32;
            let demand_variation = pool
                .day
                .requested_units()
                .abs_diff(pool.previous_day.requested_units())
                .div_ceil(sellers)
                .min(u64::from(u32::MAX)) as u32;
            let input_quote = capacity
                .input
                .map_or(0, |(input, _)| market.suggested_price(input));
            (
                unavailable,
                demand_variation,
                market.market_fee_bps(),
                sale.asking_unit_price.max(sale.minimum_unit_price).max(1),
                input_quote,
            )
        });
    // An empty shelf has no live ask from which a closed producer can value
    // reopening. Failed purchases are nevertheless real demand: use the
    // exchange's current scarcity/last-sale quote when buyers requested units
    // which no listing could supply. This remains a price signal rather than a
    // production order; the owner still rejects the shift when that revenue
    // cannot cover inputs and wages.
    if unavailable > 0 {
        if let Some(market) = market {
            output_quote = output_quote.max(market.suggested_price(output));
        }
    }
    // A funded inter-settlement tender is a real bid, not merely an abstract
    // quantity shortage. The seller remains free to choose any ask at or
    // below the buyer's ceiling; this price is used only to answer whether a
    // rational shift could pay for itself. Without it, a mothballed quarry
    // evaluates the remote order at its stale local ask and may reject a
    // profitable 2.50-coin bid as though it were worth only 0.23 coin.
    if generic_output_demand > 0 {
        output_quote = output_quote.max(external_output_bid.unwrap_or_default());
    }
    let strategy_buffer = match management.strategy {
        shared::economy::BusinessStrategy::Growth
        | shared::economy::BusinessStrategy::Opportunistic => demand_variation,
        shared::economy::BusinessStrategy::Balanced => demand_variation.div_ceil(2),
        shared::economy::BusinessStrategy::HighMargin
        | shared::economy::BusinessStrategy::Cautious => 0,
    };
    let one_worker_capacity = capacity
        .output_units
        .saturating_mul(1)
        .div_ceil(u32::from(positions.max(1)))
        .max(1);
    // A completely empty food market cannot express a good-specific order:
    // households have nothing to select or buy. SettlementEconomy retains the
    // real missed-ration count, so edible producers treat their allocated
    // share as demand even when the order book has no listing. `max` avoids
    // counting the same hungry household twice when a failed market purchase
    // already recorded the shortage against this exact output.
    let unmet_output_demand = unavailable.max(generic_output_demand);
    let desired_dispatch = proven_daily_sales
        .saturating_add(unmet_output_demand)
        .saturating_add(strategy_buffer);
    let mut output_gap = desired_dispatch.saturating_sub(stock);
    if opening_trial && stock == 0 {
        let proving_batch = processing_recipe(building.kind)
            .map_or(one_worker_capacity, |recipe| recipe.output_units);
        output_gap = output_gap.max(proving_batch);
    }
    if let Some(recipe) = processing_recipe(building.kind) {
        output_gap = output_gap
            .div_ceil(recipe.output_units)
            .saturating_mul(recipe.output_units);
    }

    let net_unit_revenue =
        output_quote.saturating_mul(BASIS_POINTS.saturating_sub(u64::from(fee_bps))) / BASIS_POINTS;
    let mut best_positions = 0u8;
    let mut best_profit = 0i64;
    let mut best_units = 0u32;
    for candidate in 1..=positions {
        let mut possible = capacity
            .output_units
            .saturating_mul(u32::from(candidate))
            .div_ceil(u32::from(positions.max(1)));
        possible = possible.min(output_gap);
        let input_cost = if let Some(recipe) = processing_recipe(building.kind) {
            let cycles = possible / recipe.output_units;
            possible = cycles.saturating_mul(recipe.output_units);
            u64::from(cycles)
                .saturating_mul(u64::from(recipe.input_units))
                .saturating_mul(input_quote)
        } else {
            0
        };
        let revenue = u64::from(possible).saturating_mul(net_unit_revenue);
        let payroll = u64::from(candidate).saturating_mul(wage.daily_wage);
        let profit = signed_profit(revenue, input_cost.saturating_add(payroll));
        if profit > best_profit {
            best_profit = profit;
            best_positions = candidate;
            best_units = possible;
        }
    }
    // A processor deliberately opens with one physical recipe batch so a new
    // bakery cannot dump a full shift into an untested market. That learning
    // batch is smaller than the worker's normal daily throughput, however, so
    // charging the entire daily wage only against those first units can make a
    // viable mill refuse ever to begin. Admit the one-position trial when the
    // same worker's rated day has positive contribution, while retaining the
    // small production budget above.
    if opening_trial && best_positions == 0 && output_gap > 0 {
        let mut rated_units = one_worker_capacity;
        let rated_input_cost = if let Some(recipe) = processing_recipe(building.kind) {
            let cycles = rated_units / recipe.output_units;
            rated_units = cycles.saturating_mul(recipe.output_units);
            u64::from(cycles)
                .saturating_mul(u64::from(recipe.input_units))
                .saturating_mul(input_quote)
        } else {
            0
        };
        let rated_revenue = u64::from(rated_units).saturating_mul(net_unit_revenue);
        let rated_profit = signed_profit(
            rated_revenue,
            rated_input_cost.saturating_add(wage.daily_wage),
        );
        if rated_profit > 0 {
            best_positions = 1;
            best_units = output_gap.min(one_worker_capacity);
            best_profit = rated_profit;
        }
    }
    BusinessOperatingPlan {
        day,
        target_output_units: best_units,
        produced_output_units: 0,
        optimal_positions: best_positions,
        marginal_daily_profit: best_profit,
    }
}

/// Cheap daily operating decision used by NPC/autopilot sites. Demand,
/// staffing and production are solved together once, then cached for both the
/// tactical and strategic simulations. Positions move only one step per day;
/// a mature firm with a genuinely empty order book contracts before it is
/// mothballed, and can reopen from the same signal without a new building.
#[allow(clippy::type_complexity)]
pub fn review_automatic_staffing(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut last_day: Local<Option<u32>>,
    merchant_demand: Option<Res<super::trade_routes::RegionalMerchantDemand>>,
    trade_contracts: Query<&shared::components::CivicTradeContract>,
    halls: Query<
        (
            &shared::components::SettlementId,
            &MootMarket,
            Option<&SettlementEconomy>,
        ),
        With<Settlement>,
    >,
    mut buildings: ParamSet<(
        Query<(
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
            &SettlementBuilding,
            &GoodsInventory,
            Option<&BusinessCondition>,
            Option<&shared::components::OperatedBy>,
            Option<&BusinessAccount>,
            Option<&BusinessProcurementPolicy>,
        )>,
        Query<(
            Entity,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
            &SettlementBuilding,
            &GoodsInventory,
            &BusinessManagementPolicy,
            Option<&mut BusinessCondition>,
            &BusinessAccount,
            &BusinessSalePolicy,
            &BusinessWagePolicy,
            &mut BusinessStaffingPolicy,
            Option<&shared::components::OperatedBy>,
            Option<&shared::economy::TavernService>,
        )>,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    if *last_day == Some(day) {
        return;
    }
    *last_day = Some(day);
    let markets: HashMap<_, _> = halls
        .iter()
        .map(|(id, market, economy)| (*id, (market, economy)))
        .collect();
    let mut responsive_sellers = HashMap::<(shared::components::SettlementId, Good), usize>::new();
    let mut food_restart_leaders =
        HashMap::<(shared::components::SettlementId, Good), shared::components::BuildingId>::new();
    let mut trade_restart_leaders =
        HashMap::<(shared::components::SettlementId, Good), shared::components::BuildingId>::new();
    let mut carrier_restart_leaders =
        HashMap::<shared::components::SettlementId, shared::components::BuildingId>::new();
    let mut branch_backlog = HashMap::<
        (
            shared::components::SettlementId,
            shared::components::CompanyId,
        ),
        u32,
    >::new();
    {
        let read = buildings.p0();
        for (
            building_id,
            building_of,
            building,
            inventory,
            condition,
            operated_by,
            _,
            procurement,
        ) in read.iter()
        {
            let responds_to_demand = condition.is_some_and(|condition| {
                condition.state.accepts_new_workers()
                    || condition.state == BusinessState::Mothballed
            });
            if responds_to_demand {
                if building.kind == SettlementBuildingKind::StorageHall {
                    carrier_restart_leaders
                        .entry(building_of.0)
                        .and_modify(|current| *current = (*current).min(*building_id))
                        .or_insert(*building_id);
                }
                if let Some(output) = business_output(building.kind) {
                    *responsive_sellers
                        .entry((building_of.0, output))
                        .or_default() += 1;
                    trade_restart_leaders
                        .entry((building_of.0, output))
                        .and_modify(|current| *current = (*current).min(*building_id))
                        .or_insert(*building_id);
                    if output.is_edible() {
                        food_restart_leaders
                            .entry((building_of.0, output))
                            .and_modify(|current| *current = (*current).min(*building_id))
                            .or_insert(*building_id);
                    }
                }
            }
            if building.kind != SettlementBuildingKind::StorageHall {
                if let (Some(company), Some(output)) = (operated_by, business_output(building.kind))
                {
                    let bulk = inventory
                        .amount(output)
                        .saturating_mul(output.bulk_per_unit());
                    let procurement_bulk = procurement.map_or(0, |procurement| {
                        Good::ALL
                            .into_iter()
                            .map(|good| {
                                let rule = procurement.rule(good);
                                if rule.enabled {
                                    rule.target_units
                                        .saturating_sub(inventory.amount(good))
                                        .saturating_mul(good.bulk_per_unit())
                                } else {
                                    0
                                }
                            })
                            .fold(0u32, u32::saturating_add)
                    });
                    let workload = bulk.saturating_add(procurement_bulk);
                    *branch_backlog
                        .entry((building_of.0, company.0))
                        .or_default() += workload;
                }
            }
        }
    }
    // A buyer-funded inter-settlement tender is a real order even before the
    // source market has a listing. Route formation deliberately waits for
    // physical stock, so the production planner must expose that demand to one
    // deterministic producer in every eligible source settlement. Otherwise a
    // previously mothballed Quarry sees an empty *local* order book forever and
    // the public tender can never acquire the first unit which would bind it.
    // Once a tender is bound, only its named seller receives the order.
    let mut external_trade_demand = HashMap::<shared::components::BuildingId, (u32, u64)>::new();
    let mut external_carrier_bulk = HashMap::<shared::components::BuildingId, u32>::new();
    for contract in trade_contracts
        .iter()
        .filter(|contract| contract.status.is_active() && contract.remaining_units() > 0)
    {
        if let Some(origin) = contract.origin {
            if let Some(storage) = carrier_restart_leaders.get(&origin) {
                let bulk = contract
                    .remaining_units()
                    .saturating_mul(contract.good.bulk_per_unit());
                let demand = external_carrier_bulk.entry(*storage).or_default();
                *demand = demand.saturating_add(bulk);
            }
        }
        if let Some(MarketSeller::Business(seller)) = contract.source_seller {
            let demand = external_trade_demand.entry(seller).or_default();
            demand.0 = demand.0.saturating_add(contract.remaining_units());
            demand.1 = demand.1.max(contract.maximum_unit_price);
            continue;
        }
        if let Some(origin) = contract.origin {
            if let Some(leader) = trade_restart_leaders.get(&(origin, contract.good)) {
                let demand = external_trade_demand.entry(*leader).or_default();
                demand.0 = demand.0.saturating_add(contract.remaining_units());
                demand.1 = demand.1.max(contract.maximum_unit_price);
            }
            continue;
        }
        for ((settlement, good), leader) in &trade_restart_leaders {
            if *settlement == contract.destination || *good != contract.good {
                continue;
            }
            let demand = external_trade_demand.entry(*leader).or_default();
            demand.0 = demand.0.saturating_add(contract.remaining_units());
            demand.1 = demand.1.max(contract.maximum_unit_price);
        }
    }
    // A demonstrated speculative export opportunity needs one available
    // porter before an autonomous company can launch its first route. Feed
    // the aggregate, already-funded market signal to only the stable
    // lowest-id Storage Hall in that settlement. Without this seam a new
    // warehouse dismisses its founding porter after one quiet day, while the
    // route manager refuses to create a route until a porter exists: a
    // permanent route-before-worker deadlock.
    if let Some(merchant_demand) = merchant_demand.as_deref() {
        for (settlement, storage) in &carrier_restart_leaders {
            let bulk = merchant_demand.bulk(*settlement);
            if bulk > 0 {
                external_carrier_bulk
                    .entry(*storage)
                    .and_modify(|current| *current = (*current).max(bulk))
                    .or_insert(bulk);
            }
        }
    }
    // When every edible listing is gone, route the settlement's generic
    // missed-ration signal to one cheapest viable staple and one deterministic
    // site. Concentrating the restart order lets a worker cover the fixed
    // daily wage; splitting a fifteen-ration shortage among five idle mills
    // can otherwise make every three-unit fragment individually unprofitable.
    let mut food_restart_goods = HashMap::<shared::components::SettlementId, (Good, u64)>::new();
    for (settlement_id, good) in food_restart_leaders.keys().copied() {
        let quote = markets
            .get(&settlement_id)
            .map_or(good.base_price(), |(market, _)| {
                market.suggested_price(good)
            });
        food_restart_goods
            .entry(settlement_id)
            .and_modify(|current| {
                if (quote, good.index()) < (current.1, current.0.index()) {
                    *current = (good, quote);
                }
            })
            .or_insert((good, quote));
    }

    for (
        entity,
        building_id,
        building_of,
        building,
        inventory,
        management,
        condition,
        account,
        sale,
        wage,
        mut staffing,
        operated_by,
        tavern_service,
    ) in buildings.p1().iter_mut()
    {
        let state = condition
            .as_deref()
            .map_or(BusinessState::Operating, |condition| condition.state);
        let has_operated = condition
            .as_deref()
            .is_some_and(|condition| condition.operating_days > 0)
            || account.gross_revenue > 0
            || account.operating_expenses > 0
            || account.current_day.produced_units > 0
            || account.previous_day.produced_units > 0;
        if !management.autopilot {
            commands
                .entity(entity)
                .insert(BusinessOperatingPlan::uncapped(
                    day,
                    staffing.target_for(building.kind),
                ));
            continue;
        }
        if matches!(
            state,
            BusinessState::Insolvent
                | BusinessState::Liquidating
                | BusinessState::ForSale
                | BusinessState::Closed
        ) {
            staffing.enabled_positions = 0;
            commands.entity(entity).insert(BusinessOperatingPlan {
                day,
                target_output_units: 0,
                produced_output_units: 0,
                optimal_positions: 0,
                marginal_daily_profit: 0,
            });
            continue;
        }

        if building.kind == SettlementBuildingKind::StorageHall {
            let throughput = account
                .current_day
                .purchased_input_units
                .saturating_add(account.current_day.sold_units)
                .max(
                    account
                        .previous_day
                        .purchased_input_units
                        .saturating_add(account.previous_day.sold_units),
                );
            let backlog = operated_by.map_or(inventory.used_bulk(), |company| {
                branch_backlog
                    .get(&(building_of.0, company.0))
                    .copied()
                    .unwrap_or_default()
                    .saturating_add(inventory.used_bulk())
            });
            let measured_load = backlog
                .max(
                    u32::try_from(throughput)
                        .unwrap_or(u32::MAX)
                        .min(u32::MAX / Good::Wood.bulk_per_unit())
                        .saturating_mul(Good::Wood.bulk_per_unit()),
                )
                // A bound, cash-backed export contract is physical work even
                // before the first collection. Assign it only to the stable
                // lowest-id source warehouse so one route hires one porter rather
                // than waking every depot in town. Route management will bind an
                // unbound tender to a real seller/origin first; the next daily
                // staffing review then exposes this ordinary paid position.
                .max(
                    external_carrier_bulk
                        .get(building_id)
                        .copied()
                        .unwrap_or_default(),
                );
            let optimal = if measured_load == 0 {
                u8::from(state == BusinessState::New)
            } else {
                u8::try_from(measured_load.div_ceil(shared::economy::capacity::PORTER))
                    .unwrap_or(u8::MAX)
                    .clamp(1, building.kind.positions())
            };
            staffing.enabled_positions = staffing
                .enabled_positions
                .min(building.kind.positions())
                .saturating_sub(u8::from(staffing.enabled_positions > optimal))
                .saturating_add(u8::from(staffing.enabled_positions < optimal))
                .min(building.kind.positions());
            commands.entity(entity).insert(BusinessOperatingPlan {
                day,
                target_output_units: 0,
                produced_output_units: 0,
                optimal_positions: optimal,
                marginal_daily_profit: 0,
            });
            continue;
        }

        if building.kind == SettlementBuildingKind::Tavern {
            let observed_visits = tavern_service.map_or(0, |service| {
                service
                    .current_day
                    .planned_visits
                    .max(service.previous_day.planned_visits)
                    .max(
                        service
                            .current_day
                            .served_meals
                            .saturating_add(service.current_day.unmet_visits()),
                    )
                    .max(
                        service
                            .previous_day
                            .served_meals
                            .saturating_add(service.previous_day.unmet_visits()),
                    )
            });
            // A new dining room needs one Innkeeper before demand can be
            // observed at all. Thereafter real attempted patronage determines
            // whether a second physical position is worth advertising.
            let desired = if observed_visits == 0 {
                1
            } else {
                u8::try_from(
                    observed_visits.div_ceil(shared::economy::TAVERN_MEALS_PER_INNKEEPER_DAY),
                )
                .unwrap_or(u8::MAX)
                .clamp(1, building.kind.positions())
            };
            let desired = if matches!(state, BusinessState::CashTight | BusinessState::Distressed) {
                desired.min(1)
            } else {
                desired
            };
            let current = staffing.enabled_positions.min(building.kind.positions());
            staffing.enabled_positions = if current < desired {
                current.saturating_add(1)
            } else if current > desired {
                current.saturating_sub(1)
            } else {
                current
            };
            commands.entity(entity).insert(BusinessOperatingPlan {
                day,
                target_output_units: 0,
                produced_output_units: 0,
                optimal_positions: desired,
                marginal_daily_profit: 0,
            });
            continue;
        }

        let output = business_output(building.kind);
        let (market, settlement_economy) = markets
            .get(&building_of.0)
            .copied()
            .map_or((None, None), |(market, economy)| (Some(market), economy));
        let seller = output.map(|_| MarketSeller::Business(*building_id));
        let external_trade = external_trade_demand
            .get(building_id)
            .copied()
            .unwrap_or_default();
        let generic_output_demand = output.map_or(0, |good| {
            let selected_good = food_restart_goods
                .get(&building_of.0)
                .map(|(selected, _)| *selected);
            let selected_site = food_restart_leaders.get(&(building_of.0, good)).copied();
            let local_restart =
                if selected_good == Some(good) && selected_site == Some(*building_id) {
                    settlement_economy.map_or(0, |economy| economy.unmet_food)
                } else {
                    0
                };
            local_restart.max(external_trade.0)
        });
        let opening_trial = state == BusinessState::New
            && account.gross_revenue == 0
            && account.current_day.produced_units == 0
            && account.previous_day.produced_units == 0;
        let mut plan = marginal_operating_plan(
            day,
            building,
            inventory,
            account,
            sale,
            wage,
            management,
            market,
            seller,
            output.map_or(1, |good| {
                responsive_sellers
                    .get(&(building_of.0, good))
                    .copied()
                    .unwrap_or(1)
            }),
            generic_output_demand,
            (external_trade.0 > 0).then_some(external_trade.1),
            opening_trial,
        );
        let has_unavailable_demand = output.is_some_and(|good| {
            market.is_some_and(|market| {
                let pool = market.pool(good);
                pool.day.unavailable_units > 0 || pool.previous_day.unavailable_units > 0
            })
        });
        let mature_and_unwanted = !matches!(state, BusinessState::New | BusinessState::Mothballed)
            && plan.optimal_positions == 0
            && sale.days_without_sales >= 2
            && !has_unavailable_demand
            && has_operated;
        let current = staffing.enabled_positions.min(building.kind.positions());
        let mut desired = plan.optimal_positions;
        if matches!(state, BusinessState::CashTight | BusinessState::Distressed) {
            desired = desired.min(1);
        }
        if state == BusinessState::Mothballed {
            if desired > 0 {
                if let Some(mut condition) = condition {
                    condition.state = BusinessState::Operating;
                }
                staffing.enabled_positions = 1.min(building.kind.positions());
            } else {
                staffing.enabled_positions = 0;
            }
        } else {
            staffing.enabled_positions = if current < desired {
                current.saturating_add(1)
            } else if current > desired {
                current.saturating_sub(1)
            } else {
                current
            };
            if mature_and_unwanted && staffing.enabled_positions == 0 {
                if let Some(mut condition) = condition {
                    condition.state = BusinessState::Mothballed;
                }
            }
        }
        let staffed_capacity =
            rated_daily_production(building.kind, building.quality).map_or(0, |capacity| {
                capacity
                    .output_units
                    .saturating_mul(u32::from(staffing.enabled_positions))
                    / u32::from(building.kind.positions().max(1))
            });
        plan.target_output_units = plan.target_output_units.min(staffed_capacity);
        commands.entity(entity).insert(plan);
    }
}

/// Residents take vacant positions in their own settlement.
///
/// This is the smallest honest version of WORLD-DESIGN section 1a's rule that
/// production is people in jobs rather than population times a multiplier. A
/// position is held by a durable PersonId, so renaming somebody cannot vacate,
/// duplicate or transfer their job.
///
/// Founding workplaces have no skill minimum. If a future specialist building
/// carries [`WorkforceRequirements`], only matching residents can fill it.
/// Among jobs a resident can do, the highest daily wage recruits first and
/// distance breaks the choice of which resident takes that offer.
pub fn fill_vacancies(
    mut commands: Commands,
    mut buildings: Query<(
        Entity,
        &mut SettlementBuilding,
        &PlayerPosition,
        Option<&BusinessWagePolicy>,
        Option<&WorkforceRequirements>,
        Option<&BusinessCondition>,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&shared::components::OperatedBy>,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &PlayerPosition,
        &mut Occupation,
        Option<&WorkStatus>,
        Option<&CharacterAttributes>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        &shared::components::PersonId,
    )>,
    settlements: Query<(Entity, &Settlement, &shared::components::SettlementId)>,
    companies: Query<(
        &shared::components::CompanyId,
        &shared::components::CompanyLeadership,
    )>,
    staffing_policies: Query<(&shared::components::BuildingId, &BusinessStaffingPolicy)>,
    active_builders: Query<
        (),
        Or<(
            With<ConstructionMaterialRoutine>,
            With<RoadBuilderRoutine>,
            With<moot_services::PermitPickupRoutine>,
        )>,
    >,
) {
    let staffing_targets: HashMap<shared::components::BuildingId, usize> = staffing_policies
        .iter()
        .map(|(building, policy)| (*building, usize::from(policy.enabled_positions)))
        .collect();
    let workplace_kinds: HashMap<shared::components::BuildingId, SettlementBuildingKind> =
        buildings
            .iter()
            .map(|(_, building, _, _, _, _, building_id, _, _)| (*building_id, building.kind))
            .collect();
    let mut assigned_entities = HashSet::new();
    let mut people_by_id = HashMap::new();
    let mut stable_workers_by_building: HashMap<
        shared::components::BuildingId,
        Vec<(shared::components::PersonId, Entity, String)>,
    > = HashMap::new();
    for (entity, name, _, _, mut occupation, _, _, employment, civic_job, person_id) in
        villagers.iter_mut()
    {
        people_by_id.insert(*person_id, entity);
        let invalid_public_assignment = employment.is_some_and(|employment| {
            workplace_kinds
                .get(&employment.0)
                .is_some_and(|kind| !is_private_business(*kind))
        });
        if invalid_public_assignment {
            // Market and Church are civic service buildings, but they do not
            // yet have a funded municipal workplace contract. Earlier
            // builds advertised their architectural positions as if they were
            // private paid jobs, leaving residents employed without any
            // payroll path. Taverns are real private firms and therefore do
            // not enter this cleanup path.
            occupation.0 = None;
            commands
                .entity(entity)
                .insert(WorkStatus::LookingForWork)
                .remove::<shared::components::EmployedAt>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .remove::<WorkerOffDuty>();
            continue;
        }
        if employment.is_some() || civic_job.is_some() {
            assigned_entities.insert(entity);
        }
        if employment.is_some() && civic_job.is_some() {
            // The public post owns the working day. Commands are applied before
            // routine assignment later in the shared chained schedule.
            commands
                .entity(entity)
                .remove::<shared::components::EmployedAt>();
        } else if let Some(employment) = employment {
            stable_workers_by_building
                .entry(employment.0)
                .or_default()
                .push((*person_id, entity, name.0.clone()));
        }
    }
    for workers in stable_workers_by_building.values_mut() {
        workers.sort_unstable_by_key(|(person_id, entity, _)| (person_id.0, entity.to_bits()));
    }
    let mut worker_counts: HashMap<shared::components::BuildingId, usize> =
        stable_workers_by_building
            .iter()
            .map(|(building_id, workers)| (*building_id, workers.len()))
            .collect();

    // Stable employment is authoritative and the readable roster is derived
    // from it. This preserves two different people with the same display name
    // and cleans civic/business double assignment without guessing by name.
    for (_, mut building, _, _, _, _, building_id, _, _) in buildings.iter_mut() {
        let roster: Vec<String> = stable_workers_by_building
            .get(building_id)
            .into_iter()
            .flatten()
            .map(|(_, _, name)| name.clone())
            .collect();
        if building.workers != roster {
            building.workers = roster;
        }
    }
    let master_by_company: HashMap<_, _> = companies
        .iter()
        .map(|(company, leadership)| (*company, leadership.master))
        .collect();
    if !villagers.iter().any(
        |(entity, _, intent, _, occupation, status, _, employed_at, civic_job, _)| {
            intent.is_settled()
                && occupation.0.is_none()
                && employed_at.is_none()
                && civic_job.is_none()
                && !assigned_entities.contains(&entity)
                && active_builders.get(entity).is_err()
                && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
        },
    ) {
        return;
    }

    for (settlement_entity, settlement, settlement_id) in settlements.iter() {
        loop {
            // Best offer in this settlement. Skill requirements remain absent
            // on every founding business, but are part of the vacancy now so
            // later specialist buildings do not need a parallel job system.
            let mut vacancies: Vec<_> = buildings
                .iter()
                .filter(
                    |(_, building, _, _, _, condition, building_id, building_of, _)| {
                        is_private_business(building.kind)
                            && building_of.0 == *settlement_id
                            && !condition
                                .is_some_and(|condition| !condition.state.accepts_new_workers())
                            && worker_counts.get(*building_id).copied().unwrap_or(0)
                                < staffing_targets
                                    .get(*building_id)
                                    .copied()
                                    .unwrap_or(building.kind.positions() as usize)
                                    .min(building.kind.positions() as usize)
                    },
                )
                .map(
                    |(entity, building, at, wage, requirements, _, building_id, _, operated_by)| {
                        (
                            entity,
                            *building_id,
                            building.kind,
                            at.0,
                            wage.map_or(FOUNDING_DAILY_WAGE, |policy| policy.daily_wage),
                            requirements.copied(),
                            worker_counts.get(building_id).copied().unwrap_or(0),
                            operated_by.copied(),
                        )
                    },
                )
                .collect();
            vacancies.sort_by(|a, b| {
                b.4.cmp(&a.4)
                    .then_with(|| usize::from(a.6 > 0).cmp(&usize::from(b.6 > 0)))
                    .then_with(|| founding_job_priority(a.2).cmp(&founding_job_priority(b.2)))
                    .then_with(|| a.3.x.total_cmp(&b.3.x))
                    .then_with(|| a.3.z.total_cmp(&b.3.z))
            });

            // A small founding firm should normally begin as the familiar
            // owner-master-worker shop. Protect an otherwise eligible Company
            // Master from being recruited by a neighbouring business while
            // one of their own sites has a vacancy. This is deliberately a
            // first-refusal preference, not an exemption from skill rules or
            // the one-person/one-job invariant. If a company has several open
            // sites, its best ordinary vacancy (the ordering above) wins.
            let mut protected_master_sites = HashMap::new();
            let mut master_by_site = HashMap::new();
            for vacancy in &vacancies {
                let Some(company) = vacancy.7.map(|operated_by| operated_by.0) else {
                    continue;
                };
                let Some(master) = master_by_company.get(&company).copied() else {
                    continue;
                };
                if protected_master_sites.contains_key(&master) {
                    continue;
                }
                let Some(master_entity) = people_by_id.get(&master).copied() else {
                    continue;
                };
                let eligible = villagers.get(master_entity).is_ok_and(
                    |(
                        entity,
                        _,
                        intent,
                        _,
                        occupation,
                        status,
                        attributes,
                        employed_at,
                        civic_job,
                        _,
                    )| {
                        intent.is_settled()
                            && intent.settlement() == Some(settlement_entity)
                            && occupation.0.is_none()
                            && employed_at.is_none()
                            && civic_job.is_none()
                            && !assigned_entities.contains(&entity)
                            && active_builders.get(entity).is_err()
                            && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
                            && vacancy.5.is_none_or(|requirements| {
                                attributes
                                    .is_some_and(|attributes| requirements.is_met_by(*attributes))
                            })
                    },
                );
                if eligible {
                    protected_master_sites.insert(master, vacancy.1);
                    master_by_site.insert(vacancy.1, master);
                }
            }
            vacancies.sort_by(|a, b| {
                usize::from(master_by_site.contains_key(&b.1))
                    .cmp(&usize::from(master_by_site.contains_key(&a.1)))
                    .then_with(|| b.4.cmp(&a.4))
                    .then_with(|| usize::from(a.6 > 0).cmp(&usize::from(b.6 > 0)))
                    .then_with(|| founding_job_priority(a.2).cmp(&founding_job_priority(b.2)))
                    .then_with(|| a.3.x.total_cmp(&b.3.x))
                    .then_with(|| a.3.z.total_cmp(&b.3.z))
            });

            // Usually the best-paid offer finds somebody in one people scan.
            // Only an unfillable specialist offer falls through to the next;
            // ordinary jobs never scan the whole population once per building.
            let mut placement = None;
            for (vacancy_entity, vacancy_id, kind, plot, offered_wage, requirements, _, _) in
                vacancies
            {
                let preferred_master = master_by_site
                    .get(&vacancy_id)
                    .and_then(|master| people_by_id.get(master))
                    .and_then(|entity| villagers.get(*entity).ok())
                    .map(|(entity, name, _, _, _, _, _, _, _, _)| (entity, name.0.clone()));
                let taker = preferred_master.or_else(|| {
                    villagers
                        .iter()
                        .filter(
                            |(
                                entity,
                                _,
                                intent,
                                _,
                                occupation,
                                status,
                                attributes,
                                employed_at,
                                civic_job,
                                person_id,
                            )| {
                                intent.is_settled()
                                    && intent.settlement() == Some(settlement_entity)
                                    && occupation.0.is_none()
                                    && employed_at.is_none()
                                    && civic_job.is_none()
                                    && !assigned_entities.contains(entity)
                                    && !protected_master_sites.contains_key(*person_id)
                                    && active_builders.get(*entity).is_err()
                                    && status
                                        .is_none_or(|status| *status == WorkStatus::LookingForWork)
                                    && requirements.is_none_or(|requirements| {
                                        attributes.is_some_and(|attributes| {
                                            requirements.is_met_by(*attributes)
                                        })
                                    })
                            },
                        )
                        .min_by(|a, b| {
                            a.3 .0
                                .distance_squared(plot)
                                .total_cmp(&b.3 .0.distance_squared(plot))
                        })
                        .map(|(entity, name, _, _, _, _, _, _, _, _)| (entity, name.0.clone()))
                });
                if let Some((taker_entity, taker)) = taker {
                    placement = Some((
                        vacancy_entity,
                        vacancy_id,
                        kind,
                        offered_wage,
                        taker_entity,
                        taker,
                    ));
                    break;
                }
            }
            let Some((vacancy_entity, vacancy_id, kind, offered_wage, taker_entity, taker)) =
                placement
            else {
                break;
            };

            let Ok((_, mut building, _, _, _, _, building_id, _, _)) =
                buildings.get_mut(vacancy_entity)
            else {
                break;
            };
            if worker_counts.get(&vacancy_id).copied().unwrap_or(0)
                >= staffing_targets
                    .get(&vacancy_id)
                    .copied()
                    .unwrap_or(building.kind.positions() as usize)
                    .min(building.kind.positions() as usize)
            {
                break;
            }
            building.workers.push(taker.clone());
            *worker_counts.entry(vacancy_id).or_default() += 1;
            assigned_entities.insert(taker_entity);
            if let Ok((_, _, _, _, mut occupation, _, _, _, _, _)) = villagers.get_mut(taker_entity)
            {
                let title = kind.trade().unwrap_or("Villager").to_string();
                if occupation.0.as_deref() != Some(title.as_str()) {
                    occupation.0 = Some(title);
                }
            }
            let mut employee = commands.entity(taker_entity);
            employee.insert(WorkStatus::Employed);
            employee.insert(shared::components::EmployedAt(*building_id));
            info!(
                "Village '{}': {taker} took work as a {} for {} coin/day",
                settlement.name,
                kind.trade().unwrap_or("hand"),
                shared::economy::format_money(offered_wage),
            );
        }
    }
}

/// Release positions closed by an operator before the vacancy matcher runs.
/// The lowest stable PersonIds retain their jobs, making retries and save/load
/// deterministic. Released workers re-enter the ordinary labour market and
/// all job-specific movement state is cleared in the same ordered pass.
pub fn enforce_staffing_targets(
    mut commands: Commands,
    buildings: Query<(
        &shared::components::BuildingId,
        &SettlementBuilding,
        &BusinessStaffingPolicy,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &shared::components::EmployedAt,
        &mut Occupation,
        &mut WorkStatus,
        &GoodsInventory,
        Has<InternalDeliveryRoutine>,
        Has<MarketCollectionRoutine>,
        Has<TradeRouteRoutine>,
    )>,
) {
    let targets: HashMap<_, _> = buildings
        .iter()
        .map(|(id, building, policy)| (*id, policy.target_for(building.kind) as usize))
        .collect();
    let mut workers: HashMap<
        shared::components::BuildingId,
        Vec<(shared::components::PersonId, Entity)>,
    > = HashMap::new();
    for (entity, person, employment, ..) in villagers.iter() {
        if targets.contains_key(&employment.0) {
            workers
                .entry(employment.0)
                .or_default()
                .push((*person, entity));
        }
    }
    for (building, roster) in workers.iter_mut() {
        roster.sort_unstable_by_key(|(person, entity)| (*person, entity.to_bits()));
        let target = targets.get(building).copied().unwrap_or(roster.len());
        for (_, entity) in roster.iter().skip(target) {
            let Ok((_, _, _, mut occupation, mut status, carrier, internal, market, trade_route)) =
                villagers.get_mut(*entity)
            else {
                continue;
            };
            // Closing a porter position is graceful. Keep the employment until
            // an already-promised shipment reaches its destination and the
            // carrier is empty; otherwise the same person can be hired into a
            // second job while physically holding somebody else's goods.
            if internal || market || trade_route || !carrier.is_empty() {
                continue;
            }
            occupation.0 = None;
            *status = WorkStatus::LookingForWork;
            commands
                .entity(*entity)
                .remove::<shared::components::EmployedAt>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<QuarryRoutine>()
                .remove::<ProcessingRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<PierTraversal>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .remove::<WorkerOffDuty>();
        }
    }
}

#[derive(Clone, Copy)]
struct JobOfferSnapshot {
    building: shared::components::BuildingId,
    settlement: shared::components::SettlementId,
    kind: SettlementBuildingKind,
    wage: u64,
    owner: Option<shared::components::PersonId>,
    arrears: u64,
    accepts_workers: bool,
    target: usize,
}

/// Let employees react to the same wage offers businesses already manage.
///
/// The decision is daily and requires a material 20% raise, so ordinary
/// ten-penny wage reviews do not produce constant churn. Three days of unpaid
/// wages override that inertia. Owners do not abandon their own shop through
/// this employee path; its business lifecycle remains their decision.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn review_worker_job_choices(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    businesses: Query<(
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &SettlementBuilding,
        Option<&BusinessWagePolicy>,
        Option<&BusinessAccount>,
        Option<&BusinessStaffingPolicy>,
        Option<&shared::components::OwnedBy>,
        Option<&BusinessCondition>,
    )>,
    mut workers: Query<(
        Entity,
        &shared::components::PersonId,
        &shared::components::EmployedAt,
        &mut Occupation,
        &mut WorkStatus,
        &GoodsInventory,
        Has<InternalDeliveryRoutine>,
        Has<MarketCollectionRoutine>,
        Has<TradeRouteRoutine>,
        Has<WorkplaceDoorTransit>,
    )>,
    mut last_day: Local<Option<u32>>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    if *last_day == Some(day) {
        return;
    }
    *last_day = Some(day);

    let mut worker_counts = HashMap::<shared::components::BuildingId, usize>::new();
    for (_, _, employment, ..) in workers.iter() {
        *worker_counts.entry(employment.0).or_default() += 1;
    }
    let offers: Vec<_> = businesses
        .iter()
        .filter(|(_, _, building, ..)| is_private_business(building.kind))
        .map(
            |(id, building_of, building, wage, account, staffing, owner, condition)| {
                let accepts_workers =
                    condition.is_none_or(|condition| condition.state.accepts_new_workers());
                JobOfferSnapshot {
                    building: *id,
                    settlement: building_of.0,
                    kind: building.kind,
                    wage: wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage),
                    owner: owner.map(|owner| owner.0),
                    arrears: account.map_or(0, |account| account.wage_arrears),
                    accepts_workers,
                    target: staffing.map_or_else(
                        || usize::from(building.kind.positions()),
                        |staffing| usize::from(staffing.target_for(building.kind)),
                    ),
                }
            },
        )
        .collect();
    let offers_by_building: HashMap<_, _> = offers
        .iter()
        .map(|offer| (offer.building, *offer))
        .collect();
    let mut vacancies: HashMap<_, usize> = offers
        .iter()
        .filter(|offer| offer.accepts_workers)
        .map(|offer| {
            (
                offer.building,
                offer
                    .target
                    .saturating_sub(worker_counts.get(&offer.building).copied().unwrap_or(0)),
            )
        })
        .collect();
    let mut ordered_workers: Vec<_> = workers
        .iter()
        .map(|(entity, person, ..)| (*person, entity))
        .collect();
    ordered_workers.sort_unstable_by_key(|(person, entity)| (*person, entity.to_bits()));

    for (person_id, worker) in ordered_workers {
        let Ok((
            _,
            _,
            employment,
            mut occupation,
            mut status,
            inventory,
            internal_delivery,
            market_collection,
            trade_route,
            door_transit,
        )) = workers.get_mut(worker)
        else {
            continue;
        };
        let Some(current) = offers_by_building.get(&employment.0).copied() else {
            continue;
        };
        if current.owner == Some(person_id)
            || !inventory.is_empty()
            || internal_delivery
            || market_collection
            || trade_route
            || door_transit
        {
            continue;
        }
        let coworkers = worker_counts
            .get(&current.building)
            .copied()
            .unwrap_or(1)
            .max(1) as u64;
        let estimated_personal_arrears = current.arrears.div_ceil(coworkers);
        let chronically_unpaid = estimated_personal_arrears
            >= current
                .wage
                .saturating_mul(UNPAID_DAYS_BEFORE_PRIVATE_RESIGNATION);
        let alternative = offers
            .iter()
            .filter(|offer| {
                offer.building != current.building
                    && offer.settlement == current.settlement
                    && offer.accepts_workers
                    && vacancies.get(&offer.building).copied().unwrap_or(0) > 0
                    && (chronically_unpaid || materially_better_wage(current.wage, offer.wage))
            })
            .max_by_key(|offer| (offer.wage, std::cmp::Reverse(offer.building)));
        if !chronically_unpaid && alternative.is_none() {
            continue;
        }

        let mut employee = commands.entity(worker);
        employee
            .remove::<FarmerRoutine>()
            .remove::<FishingRoutine>()
            .remove::<LumberjackRoutine>()
            .remove::<QuarryRoutine>()
            .remove::<ProcessingRoutine>()
            .remove::<WorkplaceDoorTransit>()
            .remove::<BuildingDoorUse>()
            .remove::<PierTraversal>()
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .remove::<WorkerOffDuty>()
            .remove::<CompanyPorter>();
        if let Some(next) = alternative {
            employee.insert(shared::components::EmployedAt(next.building));
            occupation.0 = next.kind.trade().map(str::to_string);
            *status = WorkStatus::Employed;
            if let Some(slots) = vacancies.get_mut(&next.building) {
                *slots = slots.saturating_sub(1);
            }
            *vacancies.entry(current.building).or_default() += 1;
            info!(
                "Worker #{} moved from {} at {} to {} at {} coin/day",
                person_id.0,
                current.kind.label(),
                shared::economy::format_money(current.wage),
                next.kind.label(),
                shared::economy::format_money(next.wage),
            );
        } else {
            employee.remove::<shared::components::EmployedAt>();
            occupation.0 = None;
            *status = WorkStatus::LookingForWork;
            *vacancies.entry(current.building).or_default() += 1;
            info!(
                "Worker #{} left {} after three unpaid days",
                person_id.0,
                current.kind.label(),
            );
        }
    }
}

/// Derive private logistics authority from durable employment. Storage Hall
/// workers are ordinary one-job employees; this marker only grants their work
/// routine permission and disappears immediately when that employment ends.
pub fn sync_company_porters(
    mut commands: Commands,
    halls: Query<(Entity, &shared::components::SettlementId), With<Settlement>>,
    storage_halls: Query<(
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &shared::components::OperatedBy,
        &SettlementBuilding,
    )>,
    workers: Query<
        (
            Entity,
            Option<&shared::components::EmployedAt>,
            Option<&CompanyPorter>,
            Option<&shared::components::CivicEmployment>,
            &GoodsInventory,
            Has<InternalDeliveryRoutine>,
            Has<MarketCollectionRoutine>,
            Has<TradeRouteRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    let hall_by_settlement: HashMap<_, _> = halls
        .iter()
        .map(|(entity, settlement)| (*settlement, entity))
        .collect();
    let storage_by_id: HashMap<_, _> = storage_halls
        .iter()
        .filter(|(_, _, _, building)| building.kind == SettlementBuildingKind::StorageHall)
        .filter_map(|(id, building_of, company, _)| {
            hall_by_settlement.get(&building_of.0).copied().map(|hall| {
                (
                    *id,
                    CompanyPorter {
                        settlement: hall,
                        settlement_id: building_of.0,
                        company: company.0,
                        storage_hall: *id,
                    },
                )
            })
        })
        .collect();
    for (entity, employment, current, civic_job, carrier, internal, market, trade_route) in
        workers.iter()
    {
        let wanted = employment
            .and_then(|employment| storage_by_id.get(&employment.0))
            .copied()
            .filter(|_| civic_job.is_none());
        if wanted == current.copied() {
            continue;
        }
        if let Some(wanted) = wanted {
            commands.entity(entity).insert(wanted);
        } else if current.is_some() {
            // Employment normally disappears only after the graceful staffing
            // boundary above. Keep this fallback for death, sale and unusual
            // lifecycle changes so an in-flight shipment can still complete.
            if internal || market || trade_route || !carrier.is_empty() {
                continue;
            }
            commands
                .entity(entity)
                .remove::<CompanyPorter>()
                .remove::<InternalDeliveryRoutine>()
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workplace(
        id: u64,
        settlement: shared::components::SettlementId,
        kind: SettlementBuildingKind,
        wage: u64,
        wage_arrears: u64,
    ) -> impl Bundle {
        let mut account = BusinessAccount::default();
        account.wage_arrears = wage_arrears;
        (
            shared::components::BuildingId(id),
            shared::components::BuildingOf(settlement),
            SettlementBuilding {
                kind,
                settlement: "Workford".into(),
                owner: None,
                quality: 1.0,
                workers: Vec::new(),
            },
            BusinessWagePolicy {
                daily_wage: wage,
                ..default()
            },
            BusinessStaffingPolicy::new(1),
            BusinessCondition {
                state: BusinessState::Operating,
                ..default()
            },
            account,
        )
    }

    fn employee(person: u64, workplace: u64) -> impl Bundle {
        (
            shared::components::PersonId(person),
            shared::components::EmployedAt(shared::components::BuildingId(workplace)),
            Occupation(Some("Worker".into())),
            WorkStatus::Employed,
            GoodsInventory::new(shared::economy::capacity::VILLAGER),
        )
    }

    #[test]
    fn a_job_offer_needs_a_material_raise_to_overcome_inertia() {
        assert!(!materially_better_wage(100, 119));
        assert!(materially_better_wage(100, 120));
        assert!(!materially_better_wage(40, 49));
        assert!(materially_better_wage(40, 50));
    }

    #[test]
    fn employee_moves_directly_to_a_materially_better_open_job() {
        let mut app = App::new();
        app.add_systems(Update, review_worker_job_choices);
        app.world_mut().spawn(WorldTime::new_default());
        let settlement = shared::components::SettlementId(1);
        app.world_mut().spawn(workplace(
            10,
            settlement,
            SettlementBuildingKind::Farmstead,
            100,
            0,
        ));
        app.world_mut().spawn(workplace(
            11,
            settlement,
            SettlementBuildingKind::Windmill,
            120,
            0,
        ));
        let worker = app.world_mut().spawn(employee(1, 10)).id();

        app.update();

        assert_eq!(
            app.world().get::<shared::components::EmployedAt>(worker),
            Some(&shared::components::EmployedAt(
                shared::components::BuildingId(11)
            ))
        );
        assert_eq!(
            app.world().get::<Occupation>(worker).unwrap().0.as_deref(),
            SettlementBuildingKind::Windmill.trade(),
        );
    }

    #[test]
    fn chronically_unpaid_employee_quits_when_no_alternative_exists() {
        let mut app = App::new();
        app.add_systems(Update, review_worker_job_choices);
        app.world_mut().spawn(WorldTime::new_default());
        let settlement = shared::components::SettlementId(1);
        app.world_mut().spawn(workplace(
            10,
            settlement,
            SettlementBuildingKind::Farmstead,
            100,
            300,
        ));
        let worker = app.world_mut().spawn(employee(1, 10)).id();

        app.update();

        assert!(app
            .world()
            .get::<shared::components::EmployedAt>(worker)
            .is_none());
        assert_eq!(
            app.world().get::<WorkStatus>(worker),
            Some(&WorkStatus::LookingForWork)
        );
        assert_eq!(app.world().get::<Occupation>(worker).unwrap().0, None);
    }

    #[test]
    fn marginal_plan_hires_only_the_worker_whose_output_can_sell() {
        let building = SettlementBuilding {
            kind: SettlementBuildingKind::Farmstead,
            settlement: "Workford".into(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        };
        let mut account = BusinessAccount::default();
        account.current_day.sold_units = 2;
        let plan = marginal_operating_plan(
            4,
            &building,
            &GoodsInventory::new(building.kind.storage_bulk_capacity()),
            &account,
            &BusinessSalePolicy::for_good(Good::Wheat),
            &BusinessWagePolicy::default(),
            &BusinessManagementPolicy::default(),
            None,
            None,
            1,
            0,
            None,
            false,
        );
        assert_eq!(plan.target_output_units, 2);
        assert_eq!(plan.optimal_positions, 1);
        assert!(plan.marginal_daily_profit > 0);
    }

    #[test]
    fn existing_stock_exhausts_the_production_budget() {
        let building = SettlementBuilding {
            kind: SettlementBuildingKind::Farmstead,
            settlement: "Workford".into(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        };
        let mut inventory = GoodsInventory::new(building.kind.storage_bulk_capacity());
        inventory.add(Good::Wheat, 5);
        let mut account = BusinessAccount::default();
        account.current_day.sold_units = 2;
        let plan = marginal_operating_plan(
            4,
            &building,
            &inventory,
            &account,
            &BusinessSalePolicy::for_good(Good::Wheat),
            &BusinessWagePolicy::default(),
            &BusinessManagementPolicy::default(),
            None,
            None,
            1,
            0,
            None,
            false,
        );
        assert_eq!(plan.target_output_units, 0);
        assert_eq!(plan.optimal_positions, 0);
    }

    #[test]
    fn processor_opening_trial_is_one_recipe_batch_not_a_full_shift() {
        let building = SettlementBuilding {
            kind: SettlementBuildingKind::Bakery,
            settlement: "Workford".into(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        };
        let plan = marginal_operating_plan(
            1,
            &building,
            &GoodsInventory::new(building.kind.storage_bulk_capacity()),
            &BusinessAccount::default(),
            &BusinessSalePolicy::for_good(Good::Bread),
            &BusinessWagePolicy::default(),
            &BusinessManagementPolicy::default(),
            None,
            None,
            1,
            0,
            None,
            true,
        );
        assert_eq!(
            plan.target_output_units,
            processing_recipe(SettlementBuildingKind::Bakery)
                .unwrap()
                .output_units
        );
        assert_eq!(plan.optimal_positions, 1);

        let mill = SettlementBuilding {
            kind: SettlementBuildingKind::Windmill,
            settlement: "Workford".into(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        };
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(shared::components::BuildingId(99)),
            Good::Wheat,
            10,
            Good::Wheat.base_price(),
        );
        let mill_plan = marginal_operating_plan(
            1,
            &mill,
            &GoodsInventory::new(mill.kind.storage_bulk_capacity()),
            &BusinessAccount::default(),
            &BusinessSalePolicy::for_good(Good::Flour),
            &BusinessWagePolicy::default(),
            &BusinessManagementPolicy::default(),
            Some(&market),
            Some(MarketSeller::Business(shared::components::BuildingId(100))),
            1,
            0,
            None,
            true,
        );
        assert_eq!(mill_plan.target_output_units, 1);
        assert_eq!(mill_plan.optimal_positions, 1);
        assert!(mill_plan.marginal_daily_profit > 0);
    }

    #[test]
    fn generic_food_shortage_restarts_a_mill_when_the_edible_market_is_empty() {
        let mill = SettlementBuilding {
            kind: SettlementBuildingKind::Windmill,
            settlement: "Workford".into(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        };
        let mut market = MootMarket::founding();
        market.consign(
            MarketSeller::Business(shared::components::BuildingId(99)),
            Good::Wheat,
            10,
            Good::Wheat.base_price(),
        );
        let plan = marginal_operating_plan(
            8,
            &mill,
            &GoodsInventory::new(mill.kind.storage_bulk_capacity()),
            &BusinessAccount::default(),
            &BusinessSalePolicy::for_good(Good::Flour),
            &BusinessWagePolicy::default(),
            &BusinessManagementPolicy::default(),
            Some(&market),
            Some(MarketSeller::Business(shared::components::BuildingId(100))),
            1,
            5,
            None,
            false,
        );

        assert_eq!(plan.target_output_units, 5);
        assert_eq!(plan.optimal_positions, 1);
        assert!(plan.marginal_daily_profit > 0);
    }

    #[test]
    fn unwanted_firm_mothballs_and_real_shortage_reopens_it() {
        let mut app = App::new();
        app.add_systems(Update, review_automatic_staffing);
        let mut clock = WorldTime::new_default();
        clock.day = 4;
        let clock_entity = app.world_mut().spawn(clock).id();
        let settlement_id = shared::components::SettlementId(7);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Workford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 4,
                    treasury: 0,
                },
                MootMarket::founding(),
            ))
            .id();
        let mut account = BusinessAccount::default();
        account.gross_revenue = 1;
        account.previous_day.produced_units = 6;
        let mut sale = BusinessSalePolicy::for_good(Good::Wheat);
        sale.days_without_sales = 3;
        let business = app
            .world_mut()
            .spawn((
                shared::components::BuildingId(70),
                shared::components::BuildingOf(settlement_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Workford".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                GoodsInventory::new(SettlementBuildingKind::Farmstead.storage_bulk_capacity()),
                BusinessManagementPolicy::default(),
                BusinessCondition {
                    state: BusinessState::Operating,
                    ..default()
                },
                account,
                sale,
                BusinessWagePolicy::default(),
                BusinessStaffingPolicy::new(1),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessCondition>(business)
                .unwrap()
                .state,
            BusinessState::Mothballed
        );
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(business)
                .unwrap()
                .enabled_positions,
            0
        );

        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .day = 5;
        app.world_mut()
            .get_mut::<MootMarket>(hall)
            .unwrap()
            .purchase_recording_demand(Good::Wheat, 3, u64::MAX, None, None);
        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessCondition>(business)
                .unwrap()
                .state,
            BusinessState::Operating
        );
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(business)
                .unwrap()
                .enabled_positions,
            1
        );
    }

    #[test]
    fn empty_civic_material_shelf_values_reopening_at_the_market_quote() {
        let lumber = SettlementBuilding {
            kind: SettlementBuildingKind::LumberjackHut,
            settlement: "Timbermoot".into(),
            owner: None,
            quality: 1.0,
            workers: Vec::new(),
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Wood);
        sale.asking_unit_price = 1;
        sale.minimum_unit_price = 1;
        let mut market = MootMarket::founding();
        market.purchase_recording_demand(Good::Wood, 4, u64::MAX, None, None);

        let plan = marginal_operating_plan(
            8,
            &lumber,
            &GoodsInventory::new(lumber.kind.storage_bulk_capacity()),
            &BusinessAccount::default(),
            &sale,
            &BusinessWagePolicy::default(),
            &BusinessManagementPolicy::default(),
            Some(&market),
            Some(MarketSeller::Business(shared::components::BuildingId(70))),
            1,
            0,
            None,
            false,
        );

        assert_eq!(plan.optimal_positions, 1);
        assert!(plan.target_output_units > 0);
        assert!(plan.marginal_daily_profit > 0);
    }

    #[test]
    fn insolvent_food_shell_cannot_capture_the_only_hunger_restart_order() {
        let mut app = App::new();
        app.add_systems(Update, review_automatic_staffing);
        let mut clock = WorldTime::new_default();
        clock.day = 8;
        app.world_mut().spawn(clock);
        let settlement_id = shared::components::SettlementId(7);
        app.world_mut().spawn((
            settlement_id,
            Settlement {
                name: "Workford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 10,
                treasury: 0,
            },
            MootMarket::founding(),
            SettlementEconomy {
                unmet_food: 10,
                observed_days: 2,
                ..default()
            },
        ));
        let spawn_fishery = |world: &mut World, id: u64, state: BusinessState| -> Entity {
            let mut sale = BusinessSalePolicy::for_good(Good::Food);
            sale.asking_unit_price = PENNIES_PER_COIN;
            world
                .spawn((
                    shared::components::BuildingId(id),
                    shared::components::BuildingOf(settlement_id),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::FishermansHut,
                        settlement: "Workford".into(),
                        owner: None,
                        quality: 1.0,
                        workers: Vec::new(),
                    },
                    GoodsInventory::new(
                        SettlementBuildingKind::FishermansHut.storage_bulk_capacity(),
                    ),
                    BusinessManagementPolicy::default(),
                    BusinessCondition { state, ..default() },
                    BusinessAccount::default(),
                    sale,
                    BusinessWagePolicy::default(),
                    BusinessStaffingPolicy::new(0),
                ))
                .id()
        };
        let insolvent = spawn_fishery(app.world_mut(), 69, BusinessState::Insolvent);
        let viable = spawn_fishery(app.world_mut(), 70, BusinessState::Mothballed);

        app.update();

        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(insolvent)
                .unwrap()
                .enabled_positions,
            0
        );
        assert_eq!(
            app.world().get::<BusinessCondition>(viable).unwrap().state,
            BusinessState::Operating
        );
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(viable)
                .unwrap()
                .enabled_positions,
            1
        );
    }

    #[test]
    fn cash_backed_remote_tender_reopens_a_mothballed_quarry() {
        let mut app = App::new();
        app.add_systems(Update, review_automatic_staffing);
        let mut clock = WorldTime::new_default();
        clock.day = 28;
        app.world_mut().spawn(clock);

        let source = shared::components::SettlementId(7);
        let destination = shared::components::SettlementId(8);
        app.world_mut().spawn((
            source,
            Settlement {
                name: "Stonefield".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 30,
                treasury: 0,
            },
            MootMarket::founding(),
        ));
        let quarry_id = shared::components::BuildingId(70);
        let mut stale_sale = BusinessSalePolicy::for_good(Good::Stone);
        stale_sale.asking_unit_price = 23;
        stale_sale.minimum_unit_price = 1;
        let quarry = app
            .world_mut()
            .spawn((
                quarry_id,
                shared::components::BuildingOf(source),
                SettlementBuilding {
                    kind: SettlementBuildingKind::StoneQuarry,
                    settlement: "Stonefield".into(),
                    owner: None,
                    quality: 0.84,
                    workers: Vec::new(),
                },
                GoodsInventory::new(SettlementBuildingKind::StoneQuarry.storage_bulk_capacity()),
                BusinessManagementPolicy::default(),
                BusinessCondition {
                    state: BusinessState::Mothballed,
                    ..default()
                },
                BusinessAccount::default(),
                stale_sale,
                BusinessWagePolicy::default(),
                BusinessStaffingPolicy::new(0),
            ))
            .id();
        app.world_mut()
            .spawn(shared::components::CivicTradeContract {
                origin: None,
                destination,
                good: Good::Stone,
                source_seller: None,
                requested_units: 7,
                delivered_units: 0,
                maximum_unit_price: Good::Stone.base_price(),
                delivery_fee_per_bulk: 5,
                reserved_cash: 1_960,
                escrow_cash: 1_960,
                spent_on_goods: 0,
                spent_on_freight: 0,
                created_day: 27,
                last_attempt_day: u32::MAX,
                status: shared::components::TradeContractStatus::Open,
            });

        app.update();

        assert_eq!(
            app.world().get::<BusinessCondition>(quarry).unwrap().state,
            BusinessState::Operating
        );
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(quarry)
                .unwrap()
                .enabled_positions,
            1
        );
        assert!(
            app.world()
                .get::<BusinessOperatingPlan>(quarry)
                .unwrap()
                .target_output_units
                > 0
        );
    }

    #[test]
    fn bound_export_contract_keeps_exactly_one_source_warehouse_staffed() {
        let mut app = App::new();
        app.add_systems(Update, review_automatic_staffing);
        let mut clock = WorldTime::new_default();
        clock.day = 12;
        let clock_entity = app.world_mut().spawn(clock).id();

        let source = shared::components::SettlementId(7);
        let destination = shared::components::SettlementId(8);
        app.world_mut().spawn((
            source,
            Settlement {
                name: "Stonefield".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 30,
                treasury: 0,
            },
            MootMarket::founding(),
        ));
        let spawn_warehouse = |world: &mut World, id: u64, company: u64| {
            world
                .spawn((
                    shared::components::BuildingId(id),
                    shared::components::BuildingOf(source),
                    shared::components::OperatedBy(shared::components::CompanyId(company)),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::StorageHall,
                        settlement: "Stonefield".into(),
                        owner: None,
                        quality: 1.0,
                        workers: Vec::new(),
                    },
                    GoodsInventory::new(
                        SettlementBuildingKind::StorageHall.storage_bulk_capacity(),
                    ),
                    BusinessManagementPolicy::default(),
                    BusinessCondition {
                        state: BusinessState::Operating,
                        ..default()
                    },
                    BusinessAccount::default(),
                    BusinessSalePolicy::for_good(Good::Stone),
                    BusinessWagePolicy::default(),
                    BusinessStaffingPolicy::new(0),
                ))
                .id()
        };
        let first = spawn_warehouse(app.world_mut(), 70, 20);
        let second = spawn_warehouse(app.world_mut(), 71, 21);
        let contract = app
            .world_mut()
            .spawn(shared::components::CivicTradeContract {
                origin: Some(source),
                destination,
                good: Good::Stone,
                source_seller: Some(MarketSeller::Business(shared::components::BuildingId(90))),
                requested_units: 4,
                delivered_units: 0,
                maximum_unit_price: Good::Stone.base_price(),
                delivery_fee_per_bulk: 9,
                reserved_cash: 888,
                escrow_cash: 888,
                spent_on_goods: 0,
                spent_on_freight: 0,
                created_day: 11,
                last_attempt_day: u32::MAX,
                status: shared::components::TradeContractStatus::Open,
            })
            .id();

        app.update();

        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(first)
                .unwrap()
                .enabled_positions,
            1
        );
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(second)
                .unwrap()
                .enabled_positions,
            0
        );

        app.world_mut().despawn(contract);
        for warehouse in [first, second] {
            app.world_mut()
                .get_mut::<BusinessStaffingPolicy>(warehouse)
                .unwrap()
                .enabled_positions = 0;
        }
        let mut merchant_demand = super::trade_routes::RegionalMerchantDemand::default();
        merchant_demand.advertise(source, Good::Bread, 24);
        app.world_mut().insert_resource(merchant_demand);
        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .day = 13;

        app.update();

        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(first)
                .unwrap()
                .enabled_positions,
            1,
            "a funded merchant opportunity must keep one real porter position open before the route exists"
        );
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(second)
                .unwrap()
                .enabled_positions,
            0,
            "one export opportunity must not wake every warehouse in the settlement"
        );
    }

    #[test]
    fn tavern_staffing_follows_attempted_service_demand() {
        let mut app = App::new();
        app.add_systems(Update, review_automatic_staffing);
        let mut clock = WorldTime::new_default();
        clock.day = 2;
        let clock_entity = app.world_mut().spawn(clock).id();

        let settlement = shared::components::SettlementId(81);
        app.world_mut().spawn((
            settlement,
            Settlement {
                name: "Meadow".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 35,
                treasury: 0,
            },
            MootMarket::founding(),
        ));
        let mut service = shared::economy::TavernService::default();
        service.previous_day.planned_visits = 12;
        let tavern = app
            .world_mut()
            .spawn((
                shared::components::BuildingId(82),
                shared::components::BuildingOf(settlement),
                shared::components::OperatedBy(shared::components::CompanyId(83)),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Tavern,
                    settlement: "Meadow".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                GoodsInventory::new(SettlementBuildingKind::Tavern.storage_bulk_capacity()),
                BusinessManagementPolicy::default(),
                BusinessCondition {
                    state: BusinessState::Operating,
                    ..default()
                },
                BusinessAccount::default(),
                BusinessSalePolicy::default(),
                BusinessWagePolicy::default(),
                BusinessStaffingPolicy::new(0),
                service,
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(tavern)
                .unwrap()
                .enabled_positions,
            1
        );

        app.world_mut()
            .get_mut::<WorldTime>(clock_entity)
            .unwrap()
            .day = 3;
        app.update();
        assert_eq!(
            app.world()
                .get::<BusinessStaffingPolicy>(tavern)
                .unwrap()
                .enabled_positions,
            2
        );
    }
}
