//! Authoritative permit review, applicant selection, payment and worksite creation.

use super::demand::{
    development_pipeline_has_capacity, has_planned_food_extractor, next_civic_need,
    processing_upstream_is_complete, should_try_complementary_fishing,
};
use super::fishing::{advance_incremental_fishing_search, find_incremental_fishing_site};
use super::funding::{
    debit_company_expansion, CompanyExpansionFunds, NPC_PERSONAL_INVESTMENT_RESERVE,
};
use super::market_signals::accumulate_business_signals;
use super::plots::{
    find_site_with_plan_diagnostics, SiteSearchRejections, MAX_SETTLEMENT_SEARCH_RADIUS,
};
use super::road_access::{planned_road_access_path, road_access_blockers_for_plot};
use super::terrain::site_quality;
use crate::world::village::development_market::{
    investor_score, investor_threshold, minimum_startup_capital, private_opportunities,
    replicated_opportunity_board, DevelopmentMarketSignals, DevelopmentOpportunity,
};
use crate::world::village::*;

/// Optional fine-grained permit telemetry used by the deterministic lab.
///
/// Production does not insert this resource, so ordinary server ticks pay no
/// vector-growth cost. Keeping the probes at the actual search calls lets a
/// stress run distinguish site geometry from the rest of the civic/economy
/// schedule instead of guessing from one aggregate core duration.
#[derive(Resource, Default)]
pub struct PermitPlanningDiagnostics {
    pub primary_site_milliseconds: Vec<f64>,
    pub fishing_site_milliseconds: Vec<f64>,
    pub final_access_milliseconds: Vec<f64>,
}

/// A resident applies for a permit, and it is approved if it is valid.
///
/// Distinct decisions may be in flight together. Planned kinds count as already
/// had, each applicant can hold only one active build, and pending plots reserve
/// their ground, so concurrency cannot duplicate or overlap construction.
/// Needed housing approval is free. A business permit belongs to a company:
/// retained company cash pays the actual civic fee, while a first-time sole
/// founder explicitly capitalises a company before its site is approved.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub fn consider_permits(
    simulation_time: crate::world::simulation_time::SimulationTime,
    world_time: Query<&WorldTime>,
    mut clock: ResMut<VillageClock>,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
    mut commands: Commands,
    mut planning: PermitPlanningResources,
    mut settlements: Query<(
        Entity,
        &mut Settlement,
        &PlayerPosition,
        &shared::components::SettlementId,
        Option<&mut shared::economy::CivicAccount>,
        Option<&SettlementPolicies>,
        Option<&MootMarket>,
        Option<&shared::components::SettlementOpportunityBoard>,
    )>,
    economies: Query<&SettlementEconomy>,
    developments: Query<&shared::components::SettlementDevelopment>,
    mut buildings: ParamSet<(
        Query<(
            Entity,
            &SettlementBuilding,
            &shared::components::BuildingOf,
            Option<&shared::components::OwnedBy>,
            Option<&BusinessCondition>,
            Option<&GoodsInventory>,
            Option<&BusinessAccount>,
            Option<&shared::components::BuildingId>,
            Option<&shared::components::OperatedBy>,
            Option<&BusinessWagePolicy>,
            Option<&BusinessStaffingPolicy>,
        )>,
        Query<(
            Entity,
            &shared::components::CompanyId,
            &mut shared::economy::CompanyAccount,
        )>,
    )>,
    pending: Query<(&UnderConstruction, &GoodsInventory)>,
    road_requests: Query<&RoadRequest>,
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
        Option<&CharacterAttributes>,
        Option<&shared::components::LivesAt>,
        Option<&strategic::StrategicPerson>,
        Option<&shared::components::CivicEmployment>,
    )>,
    mut wallets: Query<&mut Wallet>,
) {
    let civic_day = world_time
        .iter()
        .next()
        .map_or(1, |clock| clock.day.saturating_add(1));
    clock.permit += simulation_time.world_seconds();
    if clock.permit < PERMIT_INTERVAL {
        return;
    }
    clock.permit = 0.0;
    clock.permit_round = clock.permit_round.wrapping_add(1);
    let permit_round = clock.permit_round;

    let Some(terrain) = planning.terrain.as_deref() else {
        return;
    };

    // A Master normally controls one craft company. If inheritance or player
    // activity leaves an NPC in charge of several, choose the oldest stable
    // CompanyId deterministically instead of depending on ECS iteration order.
    let mut company_by_master =
        HashMap::<shared::components::PersonId, shared::components::CompanyId>::new();
    let mut company_strategies =
        HashMap::<shared::components::CompanyId, shared::economy::BusinessStrategy>::new();
    for (id, leadership, policy, _) in planning.companies.iter() {
        company_strategies.insert(*id, policy.strategy);
        company_by_master
            .entry(leadership.master)
            .and_modify(|current| {
                if *id < *current {
                    *current = *id;
                }
            })
            .or_insert(*id);
    }
    let company_reserve_days: HashMap<shared::components::CompanyId, u8> = planning
        .companies
        .iter()
        .map(|(id, _, policy, _)| (*id, policy.payroll_reserve_days))
        .collect();
    // A sole proprietor can inject more of their own money without changing
    // anybody else's economic interest. Once shares are split, an automatic
    // personal top-up would enrich the other shareholders for free; co-owned
    // firms therefore expand only from retained cash until explicit company
    // loans or primary share issuance are introduced.
    let personal_capital_companies: HashSet<shared::components::CompanyId> = planning
        .companies
        .iter()
        .filter_map(|(id, leadership, _, ownership)| {
            ownership
                .is_none_or(|ownership| {
                    ownership.share_count(leadership.master)
                        == shared::components::COMPANY_TOTAL_SHARES
                })
                .then_some(*id)
        })
        .collect();
    let mut company_funds = HashMap::<shared::components::CompanyId, CompanyExpansionFunds>::new();
    let mut protected_payroll = HashMap::<shared::components::CompanyId, u64>::new();
    {
        let business_read = buildings.p0();
        for (_, building, _, _, _, _, account, _, operated_by, wage, staffing) in
            business_read.iter()
        {
            let (Some(_account), Some(operated_by)) = (account, operated_by) else {
                continue;
            };
            let reserve_days = u64::from(
                company_reserve_days
                    .get(&operated_by.0)
                    .copied()
                    .unwrap_or(1),
            );
            let fallback_wage = BusinessWagePolicy {
                daily_wage: FOUNDING_DAILY_WAGE,
                automatic: true,
                ..default()
            };
            let enabled_positions = staffing
                .copied()
                .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
                .target_for(building.kind);
            let payroll = u64::from(enabled_positions)
                .saturating_mul(wage.unwrap_or(&fallback_wage).daily_wage)
                .saturating_mul(reserve_days);
            let protected = protected_payroll.entry(operated_by.0).or_default();
            *protected = protected.saturating_add(payroll);
        }
    }
    {
        let mut accounts = buildings.p1();
        for (entity, company_id, account) in accounts.iter_mut() {
            let reserve_requirement = account
                .wage_arrears
                .saturating_add(account.tax_arrears)
                .saturating_add(
                    protected_payroll
                        .get(company_id)
                        .copied()
                        .unwrap_or_default(),
                );
            company_funds.insert(
                *company_id,
                CompanyExpansionFunds {
                    available: shared::economy::company_expansion_cash(
                        &account,
                        protected_payroll
                            .get(company_id)
                            .copied()
                            .unwrap_or_default(),
                    ),
                    reserve_shortfall: reserve_requirement.saturating_sub(account.cash),
                    entity: Some(entity),
                },
            );
        }
    }

    for (
        settlement_entity,
        mut settlement,
        hall,
        settlement_id,
        mut civic_account,
        policies,
        market,
        current_board,
    ) in settlements.iter_mut()
    {
        let policies = policies.copied().unwrap_or_default();
        // Count what stands AND what is already approved, or three residents
        // deciding on successive permit ticks all build the same thing.
        let mut have: HashMap<SettlementBuildingKind, usize> = HashMap::new();
        let mut completed: HashMap<SettlementBuildingKind, usize> = HashMap::new();
        let business_read = buildings.p0();
        for (_, building, _, _, _, _, _, _, _, _, _) in
            business_read
                .iter()
                .filter(|(_, _, building_of, _, condition, _, _, _, _, _, _)| {
                    building_of.0 == *settlement_id
                        && !condition
                            .is_some_and(|condition| !condition.state.counts_as_active_capacity())
                })
        {
            *have.entry(building.kind).or_default() += 1;
            *completed.entry(building.kind).or_default() += 1;
        }
        for (under, _) in pending.iter() {
            if under.settlement == settlement_entity {
                *have.entry(under.kind).or_default() += 1;
            }
        }
        // The hall is always there; it is the founding act, not a need.
        have.insert(SettlementBuildingKind::Hall, 1);

        let active_worksites = pending
            .iter()
            .filter(|(under, _)| under.settlement == settlement_entity)
            .count()
            + planning
                .hall_upgrades
                .iter()
                .filter(|(_, building_of, _)| building_of.0 == *settlement_id)
                .count();
        let active_connectors = road_requests
            .iter()
            .filter(|request| request.settlement == settlement_entity)
            .count()
            + roads
                .iter()
                .filter(|(road, road_of)| road_of.0 == *settlement_id && !road.is_complete())
                .count();
        let pipeline_has_capacity = development_pipeline_has_capacity(
            settlement.residents,
            active_worksites,
            active_connectors,
        );

        let count = |kind| have.get(&kind).copied().unwrap_or(0);
        let mut signals = DevelopmentMarketSignals {
            residents: settlement.residents,
            farms: count(SettlementBuildingKind::Farmstead),
            fishers: count(SettlementBuildingKind::FishermansHut),
            livestock_farms: count(SettlementBuildingKind::LivestockFarm),
            windmills: count(SettlementBuildingKind::Windmill),
            bakeries: count(SettlementBuildingKind::Bakery),
            storage_halls: count(SettlementBuildingKind::StorageHall),
            completed_windmills: completed
                .get(&SettlementBuildingKind::Windmill)
                .copied()
                .unwrap_or(0),
            completed_bakeries: completed
                .get(&SettlementBuildingKind::Bakery)
                .copied()
                .unwrap_or(0),
            lumber_huts: count(SettlementBuildingKind::LumberjackHut),
            stone_quarries: count(SettlementBuildingKind::StoneQuarry),
            taverns: count(SettlementBuildingKind::Tavern),
            houses: count(SettlementBuildingKind::House),
            // A Village does not create a speculative Stone shortage merely
            // because Town is its eventual next tier. Demand begins when its
            // physical Town Works exists, or when another settlement posts a
            // cash-backed tender which this town could supply.
            town_hall_stone_demand: planning
                .trade_contracts
                .iter()
                .filter(|contract| {
                    contract.origin.is_none()
                        && contract.destination != *settlement_id
                        && contract.good == Good::Stone
                        && contract.status.is_active()
                })
                .map(|contract| contract.remaining_units())
                .fold(0u32, u32::saturating_add),
            ..Default::default()
        };
        signals.export_contract_bulk = planning
            .trade_contracts
            .iter()
            .filter(|contract| {
                contract.origin == Some(*settlement_id) && contract.status.is_active()
            })
            .map(|contract| {
                contract
                    .remaining_units()
                    .saturating_mul(contract.good.bulk_per_unit())
            })
            .fold(0u32, u32::saturating_add);
        if let Some(merchant_demand) = planning.merchant_demand.as_deref() {
            signals.merchant_export_bulk = merchant_demand.bulk(*settlement_id);
            signals.merchant_export_units = std::array::from_fn(|index| {
                merchant_demand.units(*settlement_id, Good::ALL[index])
            });
            signals.recent_wheat_export_demand = merchant_demand.units(*settlement_id, Good::Wheat);
        }
        for (_, building, building_of, _, condition, inventory, account, _, _, _, staffing) in
            business_read.iter()
        {
            if building_of.0 != *settlement_id {
                continue;
            }
            accumulate_business_signals(
                &mut signals,
                building,
                condition,
                inventory,
                account,
                staffing,
                market,
            );
        }
        if let Some(market) = market {
            signals.wheat_stock = signals
                .wheat_stock
                .saturating_add(market.listed_units(Good::Wheat));
            signals.flour_stock = signals
                .flour_stock
                .saturating_add(market.listed_units(Good::Flour));
            signals.bread_stock = signals
                .bread_stock
                .saturating_add(market.listed_units(Good::Bread));
            signals.meat_stock = signals
                .meat_stock
                .saturating_add(market.listed_units(Good::Meat));
            signals.wood_stock = signals
                .wood_stock
                .saturating_add(market.listed_units(Good::Wood));
            signals.stone_stock = signals
                .stone_stock
                .saturating_add(market.listed_units(Good::Stone));
        }
        for (under, inventory) in pending.iter() {
            if under.settlement == settlement_entity {
                if matches!(
                    under.kind,
                    SettlementBuildingKind::Farmstead
                        | SettlementBuildingKind::FishermansHut
                        | SettlementBuildingKind::LivestockFarm
                ) {
                    let anticipated =
                        crate::world::village::rated_daily_production(under.kind, under.quality)
                            .map_or(0, |capacity| capacity.output_units);
                    match under.kind {
                        SettlementBuildingKind::Farmstead => {
                            signals.anticipated_wheat_output =
                                signals.anticipated_wheat_output.saturating_add(anticipated);
                            signals.pending_wheat_output =
                                signals.pending_wheat_output.saturating_add(anticipated);
                        }
                        SettlementBuildingKind::FishermansHut => {
                            signals.anticipated_fish_output =
                                signals.anticipated_fish_output.saturating_add(anticipated);
                            signals.pending_fish_output =
                                signals.pending_fish_output.saturating_add(anticipated);
                        }
                        SettlementBuildingKind::LivestockFarm => {
                            signals.anticipated_meat_output =
                                signals.anticipated_meat_output.saturating_add(anticipated);
                            signals.pending_meat_output =
                                signals.pending_meat_output.saturating_add(anticipated);
                        }
                        _ => {}
                    }
                }
                signals.construction_wood_demand = signals.construction_wood_demand.saturating_add(
                    under
                        .kind
                        .construction_wood_required()
                        .saturating_sub(inventory.amount(Good::Wood)),
                );
            }
        }
        for (upgrade, building_of, inventory) in planning.hall_upgrades.iter() {
            if building_of.0 != *settlement_id {
                continue;
            }
            let remaining = upgrade
                .material_required
                .saturating_sub(inventory.amount(upgrade.material));
            match upgrade.material {
                Good::Wood => {
                    signals.construction_wood_demand =
                        signals.construction_wood_demand.saturating_add(remaining);
                }
                Good::Stone => {
                    signals.town_hall_stone_demand =
                        signals.town_hall_stone_demand.saturating_add(remaining);
                }
                _ => {}
            }
        }

        // Index ownership once. Recounting every building for every resident
        // and every opportunity makes permit review O(people × opportunities
        // × buildings), which is exactly the wrong curve for a mature town.
        let mut holding_counts = HashMap::<shared::components::PersonId, usize>::new();
        let mut private_site_counts = HashMap::<shared::components::PersonId, usize>::new();
        let mut storage_holders = HashSet::<shared::components::PersonId>::new();
        let mut holdings_by_kind =
            HashSet::<(shared::components::PersonId, SettlementBuildingKind)>::new();
        for (_, building, building_of, owner, _, _, _, _, _, _, _) in business_read.iter() {
            if building_of.0 == *settlement_id {
                if let Some(owner) = owner {
                    *holding_counts.entry(owner.0).or_default() += 1;
                    holdings_by_kind.insert((owner.0, building.kind));
                    if building.kind == SettlementBuildingKind::StorageHall {
                        storage_holders.insert(owner.0);
                    } else if is_private_business(building.kind) {
                        *private_site_counts.entry(owner.0).or_default() += 1;
                    }
                }
            }
        }
        for (under, _) in pending.iter() {
            if under.settlement_id == *settlement_id {
                if let Some(owner) = under.owner_id {
                    *holding_counts.entry(owner).or_default() += 1;
                    holdings_by_kind.insert((owner, under.kind));
                    if under.kind == SettlementBuildingKind::StorageHall {
                        storage_holders.insert(owner);
                    } else if is_private_business(under.kind) {
                        *private_site_counts.entry(owner).or_default() += 1;
                    }
                }
            }
        }
        let holdings = |who: shared::components::PersonId| -> usize {
            holding_counts.get(&who).copied().unwrap_or(0)
        };
        // Local-overflow depots belong to established branches. A real
        // regional opportunity may instead support a standalone merchant:
        // its Storage Hall, porter and route are the whole young company.
        // Empty speculative depots remain unattractive in isolated towns.
        let export_warehouse_needed =
            signals.export_contract_bulk > 0 || signals.merchant_export_bulk > 0;
        let may_found_storage = |who: shared::components::PersonId| -> bool {
            (export_warehouse_needed
                || (company_by_master.contains_key(&who)
                    && private_site_counts.get(&who).copied().unwrap_or(0) >= 2))
                && !storage_holders.contains(&who)
        };
        let mut blocked_portfolios = HashSet::<shared::components::PersonId>::new();
        for (owner, condition, for_sale) in planning.portfolios.iter() {
            if for_sale.is_some()
                || condition.is_some_and(|condition| condition.state.blocks_owner_expansion())
            {
                blocked_portfolios.insert(owner.0);
            }
        }
        for (under, _) in pending.iter() {
            if under.settlement_id == *settlement_id && is_private_business(under.kind) {
                if let Some(owner) = under.owner_id {
                    blocked_portfolios.insert(owner);
                }
            }
        }

        // Copy the small facts used by subjective scoring once per resident.
        // Names and mutable intent stay in the authoritative query and are
        // touched only after geography has selected one actual applicant.
        let permit_candidates: Vec<_> = villagers
            .iter()
            .filter_map(
                |(
                    entity,
                    person_id,
                    _,
                    intent,
                    status,
                    attributes,
                    lives_at,
                    strategic,
                    civic_job,
                )| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
                        .then_some((
                            entity,
                            *person_id,
                            status.copied(),
                            attributes.copied(),
                            lives_at.is_some(),
                            strategic.is_some(),
                            civic_job.copied(),
                            planning.permit_busy.get(entity).is_ok(),
                        ))
                },
            )
            .collect();
        let investment_balance = |entity: Entity,
                                  person: shared::components::PersonId,
                                  kind: SettlementBuildingKind|
         -> u64 {
            let personal = wallets
                .get(entity)
                .map(|wallet| wallet.balance())
                .unwrap_or(shared::economy::STARTING_VILLAGER_MONEY);
            if !is_private_business(kind) {
                return personal;
            }
            let personal = personal.saturating_sub(NPC_PERSONAL_INVESTMENT_RESERVE);
            let Some(company) = company_by_master.get(&person) else {
                return personal;
            };
            let retained = company_funds
                .get(company)
                .map_or(0, |funds| funds.available);
            if personal_capital_companies.contains(company) {
                let repair = company_funds
                    .get(company)
                    .map_or(0, |funds| funds.reserve_shortfall);
                personal.saturating_sub(repair).saturating_add(retained)
            } else {
                retained
            }
        };
        let investment_strategy =
            |person: shared::components::PersonId, attributes: Option<&CharacterAttributes>| {
                company_by_master
                    .get(&person)
                    .and_then(|company| company_strategies.get(company))
                    .copied()
                    .unwrap_or_else(|| {
                        crate::world::village::automatic_owner_strategy(Some(person), attributes)
                    })
            };

        let mut opportunities = private_opportunities(
            signals,
            economies.get(settlement_entity).ok(),
            market,
            &policies,
        );
        opportunities.retain(|opportunity| {
            opportunity
                .kind
                .is_player_permit_available_at(settlement.tier)
        });
        if clock
            .failed_fishing_terrain_versions
            .get(&settlement_entity)
            .is_some_and(|version| *version == terrain.modification_version())
        {
            // Geography is market information once the bounded shoreline
            // survey has actually proved it. Do not keep advertising and
            // subsidising an impossible fishing firm in an inland town; a
            // terrain edit changes the version and reopens the opportunity.
            opportunities
                .retain(|opportunity| opportunity.kind != SettlementBuildingKind::FishermansHut);
        }
        let opportunity_is_deferred = |kind| {
            clock
                .deferred_opportunities
                .get(&(settlement_entity, kind))
                .is_some_and(|until| permit_round < *until)
        };
        let mut selected: Option<(DevelopmentOpportunity, f32)> = None;
        for opportunity in opportunities.iter().copied() {
            if opportunity_is_deferred(opportunity.kind)
                || !processing_upstream_is_complete(opportunity.kind, &completed)
            {
                continue;
            }
            let best_applicant_score = permit_candidates
                .iter()
                .filter_map(
                    |(
                        entity,
                        person_id,
                        status,
                        attributes,
                        housed,
                        strategic,
                        civic_job,
                        busy,
                    )| {
                        if *strategic
                            || civic_job.is_some()
                            || *busy
                            || (opportunity.kind == SettlementBuildingKind::House && *housed)
                            || (opportunity.kind != SettlementBuildingKind::House
                                && blocked_portfolios.contains(person_id))
                            || (opportunity.requires_independent_owner
                                && holdings_by_kind.contains(&(*person_id, opportunity.kind)))
                            || (opportunity.kind == SettlementBuildingKind::StorageHall
                                && !may_found_storage(*person_id))
                        {
                            return None;
                        }
                        let holding_count = holdings(*person_id);
                        let fee = permit_price_with_subsidy(
                            opportunity.kind,
                            holding_count,
                            opportunity.civic_priority,
                            policies.business_permit_subsidy_bps,
                        );
                        let balance = investment_balance(*entity, *person_id, opportunity.kind);
                        let startup_capital = minimum_startup_capital(opportunity.kind, market);
                        if balance < fee.saturating_add(startup_capital) {
                            return None;
                        }
                        if opportunity.kind == SettlementBuildingKind::House {
                            let personal = (person_id.0.wrapping_mul(31) % 11) as f32 - 5.0;
                            return Some(opportunity.score + personal - holding_count as f32 * 4.0);
                        }
                        let strategy = investment_strategy(*person_id, attributes.as_ref());
                        let mut score = investor_score(
                            opportunity,
                            strategy,
                            0.5,
                            market,
                            holding_count,
                            person_id.0,
                        );
                        if status.is_some_and(|status| status == WorkStatus::Chilling)
                            && holding_count > 0
                        {
                            score += 5.0;
                        }
                        (score >= investor_threshold(strategy)).then_some(score)
                    },
                )
                .max_by(f32::total_cmp);
            if let Some(applicant_score) = best_applicant_score {
                let replace = selected.is_none_or(|(_, score)| applicant_score > score);
                if replace {
                    selected = Some((opportunity, applicant_score));
                }
            }
        }

        let civic_need =
            next_civic_need(settlement.tier, &have).filter(|kind| !opportunity_is_deferred(*kind));
        let civic_opportunity = civic_need.map(|kind| DevelopmentOpportunity {
            kind,
            score: 62.0,
            civic_priority: true,
            requires_independent_owner: false,
        });
        let board =
            replicated_opportunity_board(&opportunities, civic_opportunity, settlement.tier);
        if current_board != Some(&board) {
            commands.entity(settlement_entity).insert(board);
        }
        if !pipeline_has_capacity {
            // Continue publishing every player permit and its current signal
            // while municipal crews are saturated. Only automatic NPC/public
            // approvals wait here; a player's private hero does not consume
            // this development pipeline.
            continue;
        }
        let mut selected_opportunity = selected.map(|(opportunity, _)| opportunity);
        // Tier infrastructure is a real public choice. It waits behind an
        // acute private shortage, then outranks low-score speculation.
        if civic_need.is_some()
            && selected_opportunity.is_none_or(|opportunity| opportunity.score < 62.0)
        {
            selected_opportunity = civic_opportunity;
        }
        let Some(selected_opportunity) = selected_opportunity else {
            continue;
        };
        let requested = Some(selected_opportunity.kind);
        let planned_farms = have
            .get(&SettlementBuildingKind::Farmstead)
            .copied()
            .unwrap_or(0);
        let planned_fishers = have
            .get(&SettlementBuildingKind::FishermansHut)
            .copied()
            .unwrap_or(0);
        // Livestock is a complete food extractor too. Omitting it here made
        // the old founding-shore fallback think a town with an operating
        // pasture still had no food source, allowing a coastal Fisherman's Hut
        // search to replace an independently selected House or other permit.
        let initial_food_request = !has_planned_food_extractor(&have);
        let may_add_complementary_fishing =
            should_try_complementary_fishing(requested, planned_farms, planned_fishers);

        if requested.is_none() && !may_add_complementary_fishing {
            continue;
        }

        let missing = selected_opportunity.kind;

        // Site selection can inspect thousands of terrain/prop samples in a
        // mature settlement. Do not repeat that work every permit interval
        // while the only eligible Reeve is already building a civic project,
        // or while no private resident can pay for the requested firm. The
        // actual applicant is selected again after geography chooses the exact
        // kind, so this is only a cheap admission guard, never an approval.
        let has_eligible_applicant = permit_candidates.iter().any(
            |(entity, person_id, _, _, housed, strategic, civic_job, busy)| {
                if *strategic || *busy || (missing == SettlementBuildingKind::House && *housed) {
                    return false;
                }
                if missing != SettlementBuildingKind::House
                    && !missing.is_civic()
                    && blocked_portfolios.contains(person_id)
                {
                    return false;
                }
                if missing == SettlementBuildingKind::StorageHall && !may_found_storage(*person_id)
                {
                    return false;
                }
                if missing.is_civic() {
                    if !civic_job.is_some_and(|job| {
                        job.settlement == *settlement_id
                            && job.role == shared::components::CivicRole::Reeve
                    }) {
                        return false;
                    }
                } else if civic_job.is_some() {
                    return false;
                }
                let fee = permit_price_with_subsidy(
                    missing,
                    holdings(*person_id),
                    selected_opportunity.civic_priority,
                    policies.business_permit_subsidy_bps,
                );
                investment_balance(*entity, *person_id, missing) >= fee
            },
        );
        if !has_eligible_applicant {
            clock
                .deferred_opportunities
                .insert((settlement_entity, missing), permit_round.saturating_add(2));
            debug!(
                "Village '{}': nobody is currently eligible for its next {} permit",
                settlement.name,
                missing.label()
            );
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
            .chain(pending.iter().filter_map(|(under, _)| {
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
                        .map(|half| half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN);
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
                .filter(|(under, _)| under.settlement == settlement_entity)
                .flat_map(|(under, _)| {
                    let radius = under
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN);
                    under
                        .kind
                        .field_positions(under.position, under.rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );
        // The fenced pasture is a permanent land use even though it is not a
        // solid building. Reserve it from permits and roads from approval day.
        occupied.extend(
            placed
                .iter()
                .filter_map(|(building, building_of, position, rotation)| {
                    if building_of.0 != *settlement_id {
                        return None;
                    }
                    let rotation = rotation.map_or(0.0, |rotation| rotation.0);
                    Some((
                        building.kind.pasture_position(position.0, rotation)?,
                        building.kind.pasture_half_extents()?.length() + 2.0,
                    ))
                }),
        );
        occupied.extend(pending.iter().filter_map(|(under, _)| {
            if under.settlement != settlement_entity {
                return None;
            }
            Some((
                under
                    .kind
                    .pasture_position(under.position, under.rotation)?,
                under.kind.pasture_half_extents()?.length() + 2.0,
            ))
        }));

        let village_roads: Vec<_> = roads
            .iter()
            .filter_map(|(road, road_of)| (road_of.0 == *settlement_id).then_some(road))
            .collect();
        let existing_accesses: Vec<_> = planning
            .planned_road_accesses
            .iter()
            .filter(|access| access.settlement_id == *settlement_id)
            .cloned()
            .collect();
        let mut access_blockers: Vec<_> = placed
            .iter()
            .filter(|(_, building_of, _, _)| building_of.0 == *settlement_id)
            .flat_map(|(building, _, position, rotation)| {
                road_access_blockers_for_plot(
                    building.kind,
                    position.0,
                    rotation.map_or(0.0, |rotation| rotation.0),
                )
            })
            .collect();
        access_blockers.extend(
            pending
                .iter()
                .filter(|(under, _)| under.settlement == settlement_entity)
                .flat_map(|(under, _)| {
                    road_access_blockers_for_plot(under.kind, under.position, under.rotation)
                }),
        );
        let search_signature = FailedSiteSearch {
            kind: missing,
            occupied_plots: occupied.len(),
            roads: village_roads.len(),
            terrain_version: terrain.modification_version(),
        };
        let repeated_failed_search = clock
            .failed_site_searches
            .get(&settlement_entity)
            .is_some_and(|failed| *failed == search_signature);
        if repeated_failed_search {
            // This opportunity has already exhausted unchanged geometry. Do
            // not let it occupy every four-second review: other households and
            // investors may proceed, while a completed plot/road or terrain
            // edit naturally invalidates the failed-search signature.
            clock
                .deferred_opportunities
                .insert((settlement_entity, missing), permit_round.saturating_add(4));
            continue;
        }
        // Fishing is a first-class private opportunity. The founding food
        // search may still discover a shoreline before the town has enough
        // evidence to rank farm and fish separately, but later Fisher permits
        // always use the authored hut-and-pier geography rather than the
        // generic land-building search.
        let fishing_attempted = requested == Some(SettlementBuildingKind::FishermansHut)
            || initial_food_request
            || may_add_complementary_fishing;
        let fishing_started = fishing_attempted
            .then(|| {
                planning
                    .diagnostics
                    .as_ref()
                    .map(|_| std::time::Instant::now())
            })
            .flatten();
        let coastal_site = fishing_attempted
            .then(|| {
                find_incremental_fishing_site(
                    terrain,
                    hall.0,
                    &occupied,
                    &village_roads,
                    settlement_entity,
                    &mut clock,
                )
            })
            .flatten();
        if let (Some(diagnostics), Some(started)) =
            (planning.diagnostics.as_deref_mut(), fishing_started)
        {
            diagnostics
                .fishing_site_milliseconds
                .push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let primary_started = planning
            .diagnostics
            .as_ref()
            .map(|_| std::time::Instant::now());
        let mut primary_rejections = SiteSearchRejections::default();
        let ordinary_site = requested.and_then(|kind| {
            if kind == SettlementBuildingKind::FishermansHut {
                return None;
            }
            let bounded_resource_search = matches!(
                kind,
                SettlementBuildingKind::Farmstead
                    | SettlementBuildingKind::LumberjackHut
                    | SettlementBuildingKind::Windmill
                    | SettlementBuildingKind::StoneQuarry
            );
            find_site_with_plan_diagnostics(
                terrain,
                hall.0,
                kind,
                &occupied,
                &village_roads,
                &existing_accesses,
                &access_blockers,
                developments.get(settlement_entity).ok(),
                planning.colliders.as_deref(),
                planning.derived.as_deref(),
                bounded_resource_search
                    .then(|| {
                        clock
                            .site_search_radii
                            .get(&(settlement_entity, kind))
                            .copied()
                    })
                    .flatten(),
                bounded_resource_search.then_some(1),
                Some(&mut primary_rejections),
            )
            .map(|(position, rotation)| (kind, position, rotation))
        });
        if let (Some(diagnostics), Some(started)) =
            (planning.diagnostics.as_deref_mut(), primary_started)
        {
            diagnostics
                .primary_site_milliseconds
                .push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let mut approved_site = if let Some((position, rotation, quality)) = coastal_site {
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

        // Food demand belongs to the settlement, not to one hard-coded trade.
        // Once the best sampled Farmstead plots fill, a coastal settlement
        // must be allowed to add another fishing business instead of logging
        // "nowhere to put a Farmstead" forever while residents go hungry.
        // Prefer the requested Farmstead when it is viable; fishing is the
        // geography-aware fallback, so inland settlements retain their normal
        // behaviour and no speculative hut appears without real food demand.
        if approved_site.is_none()
            && requested == Some(SettlementBuildingKind::Farmstead)
            && !fishing_attempted
        {
            let fishing_started = planning
                .diagnostics
                .as_ref()
                .map(|_| std::time::Instant::now());
            approved_site = find_incremental_fishing_site(
                terrain,
                hall.0,
                &occupied,
                &village_roads,
                settlement_entity,
                &mut clock,
            )
            .map(|(position, rotation, quality)| {
                (
                    SettlementBuildingKind::FishermansHut,
                    position,
                    rotation,
                    quality,
                )
            });
            if let (Some(diagnostics), Some(started)) =
                (planning.diagnostics.as_deref_mut(), fishing_started)
            {
                diagnostics
                    .fishing_site_milliseconds
                    .push(started.elapsed().as_secs_f64() * 1_000.0);
            }
        }

        let Some((kind, position, rotation, quality)) = approved_site else {
            if requested == Some(SettlementBuildingKind::FishermansHut)
                && !clock
                    .failed_fishing_terrain_versions
                    .get(&settlement_entity)
                    .is_some_and(|version| *version == terrain.modification_version())
            {
                // `find_incremental_fishing_site` deliberately scans one ring
                // per review. A miss before the final ring is resumable work,
                // not a failed geometry signature; caching it here would make
                // an explicit Fisher opportunity inspect exactly one ring for
                // the rest of the settlement's life.
                clock.failed_site_searches.remove(&settlement_entity);
                clock.deferred_opportunities.insert(
                    (settlement_entity, SettlementBuildingKind::FishermansHut),
                    permit_round.saturating_add(2),
                );
                continue;
            }
            if let Some(searched_kind) = requested.filter(|kind| {
                matches!(
                    kind,
                    SettlementBuildingKind::Farmstead
                        | SettlementBuildingKind::LumberjackHut
                        | SettlementBuildingKind::Windmill
                        | SettlementBuildingKind::StoneQuarry
                )
            }) {
                let cursor = clock
                    .site_search_radii
                    .entry((settlement_entity, searched_kind))
                    .or_insert_with(|| searched_kind.preferred_ring().0);
                if *cursor < MAX_SETTLEMENT_SEARCH_RADIUS {
                    // One outward ring per decision is a deliberate server
                    // budget. Resume at the next ring on the next permit tick
                    // instead of turning one mature-city search into a hitch.
                    *cursor = (*cursor + 6.0).min(MAX_SETTLEMENT_SEARCH_RADIUS);
                    clock.failed_site_searches.remove(&settlement_entity);
                    clock.deferred_opportunities.insert(
                        (settlement_entity, searched_kind),
                        permit_round.saturating_add(2),
                    );
                    continue;
                }
            }
            info!(
                "Village '{}': nowhere to put a {} yet ({})",
                settlement.name,
                missing.label(),
                primary_rejections.summary(),
            );
            clock
                .failed_site_searches
                .insert(settlement_entity, search_signature);
            clock
                .deferred_opportunities
                .insert((settlement_entity, missing), permit_round.saturating_add(4));
            continue;
        };
        if !processing_upstream_is_complete(kind, &completed) {
            // Alternative-site selection can provisionally pretend an
            // unavailable request exists to discover the next useful plot.
            // Never let that bookkeeping fiction authorize a processor whose
            // physical input source has not actually been completed.
            continue;
        }
        let access_started = planning
            .diagnostics
            .as_ref()
            .map(|_| std::time::Instant::now());
        let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall.0, 0.0);
        let connected_road_keys = crate::world::village_roads::hall_connected_road_keys(
            Vec2::new(hall_door3.x, hall_door3.z),
            &village_roads,
        );
        let planned_access = planned_road_access_path(
            terrain,
            hall.0,
            kind,
            position,
            rotation,
            &village_roads,
            &access_blockers,
            &connected_road_keys,
        );
        if let (Some(diagnostics), Some(started)) =
            (planning.diagnostics.as_deref_mut(), access_started)
        {
            diagnostics
                .final_access_milliseconds
                .push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let Some(planned_access_points) = planned_access else {
            if kind == SettlementBuildingKind::FishermansHut
                && advance_incremental_fishing_search(&mut clock, settlement_entity)
            {
                clock.failed_site_searches.remove(&settlement_entity);
            } else if matches!(
                kind,
                SettlementBuildingKind::Farmstead
                    | SettlementBuildingKind::LumberjackHut
                    | SettlementBuildingKind::Windmill
                    | SettlementBuildingKind::StoneQuarry
            ) {
                let cursor = clock
                    .site_search_radii
                    .entry((settlement_entity, kind))
                    .or_insert_with(|| kind.preferred_ring().0);
                if *cursor < MAX_SETTLEMENT_SEARCH_RADIUS {
                    *cursor = (*cursor + 6.0).min(MAX_SETTLEMENT_SEARCH_RADIUS);
                    clock.failed_site_searches.remove(&settlement_entity);
                } else {
                    clock
                        .failed_site_searches
                        .insert(settlement_entity, search_signature);
                }
            } else {
                clock
                    .failed_site_searches
                    .insert(settlement_entity, search_signature);
            }
            continue;
        };
        if existing_accesses.iter().any(|access| {
            let footprint_radius = kind.placement_definition().root_footprint_radius() + 0.45;
            access.intersects_circle(Vec2::new(position.x, position.z), footprint_radius)
        }) {
            // Fishing uses its own shoreline search and alternative selection,
            // so repeat the generic access proof here. Ordinary plots already
            // passed it inside `find_site_with_plan`; this is deliberately a
            // cheap safety net rather than approving an inaccessible coast.
            if kind == SettlementBuildingKind::FishermansHut
                && advance_incremental_fishing_search(&mut clock, settlement_entity)
            {
                clock.failed_site_searches.remove(&settlement_entity);
            } else {
                clock
                    .failed_site_searches
                    .insert(settlement_entity, search_signature);
            }
            continue;
        }
        clock.failed_site_searches.remove(&settlement_entity);
        if matches!(
            kind,
            SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::Windmill
                | SettlementBuildingKind::StoneQuarry
        ) {
            clock.site_search_radii.insert(
                (settlement_entity, kind),
                Vec2::new(position.x - hall.0.x, position.z - hall.0.z).length(),
            );
        }

        // A resident applies only after geography has selected the actual
        // permit kind. This prevents an impossible lumber permit from applying
        // lumber prices or eligibility rules to a fallback house or farm.
        // The applicant is the resident whose strategy, expected margin and
        // personal judgement most strongly support this exact site.
        let applicant = villagers
            .iter()
            .filter(|(_, _, _, intent, _, _, _, strategic, _)| {
                strategic.is_none()
                    && matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
            })
            .filter_map(
                |(
                    entity,
                    person_id,
                    name,
                    _,
                    status,
                    attributes,
                    lives_at,
                    _,
                    civic_job,
                )| {
                // Owning a building does not create a second job, but its
                // construction still needs the applicant's physical time.
                // Never tear somebody out of a field, workplace doorway or
                // fishing pier mid-shift. Employed residents become eligible
                // again after their bounded work routine ends for the day.
                if planning.permit_busy.get(entity).is_ok() {
                    return None;
                }
                if kind == SettlementBuildingKind::House && lives_at.is_some() {
                    return None;
                }
                if kind != SettlementBuildingKind::House
                    && !kind.is_civic()
                    && blocked_portfolios.contains(person_id)
                {
                    return None;
                }
                if kind == SettlementBuildingKind::StorageHall
                    && !may_found_storage(*person_id)
                {
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
                let actual_opportunity = if kind == selected_opportunity.kind {
                    selected_opportunity
                } else if kind == SettlementBuildingKind::FishermansHut
                    && selected_opportunity.kind == SettlementBuildingKind::Farmstead
                {
                    opportunities
                        .iter()
                        .copied()
                        .find(|opportunity| opportunity.kind == kind)
                        .unwrap_or(DevelopmentOpportunity {
                            kind,
                            requires_independent_owner: false,
                            ..selected_opportunity
                        })
                } else {
                    opportunities
                        .iter()
                        .copied()
                        .find(|opportunity| opportunity.kind == kind)
                        .unwrap_or(DevelopmentOpportunity {
                            kind,
                            score: selected_opportunity.score,
                            civic_priority: selected_opportunity.civic_priority,
                            requires_independent_owner: false,
                        })
                };
                if actual_opportunity.requires_independent_owner
                    && holdings_by_kind.contains(&(*person_id, kind))
                {
                    return None;
                }
                let holding_count = holdings(*person_id);
                let fee = permit_price_with_subsidy(
                    kind,
                    holding_count,
                    actual_opportunity.civic_priority,
                    policies.business_permit_subsidy_bps,
                );
                let balance = investment_balance(entity, *person_id, kind);
                let startup_capital = minimum_startup_capital(kind, market);
                if balance < fee.saturating_add(startup_capital) {
                    return None;
                }
                let mut decision_score = actual_opportunity.score;
                if !kind.is_civic() && kind != SettlementBuildingKind::House {
                    let strategy = investment_strategy(*person_id, attributes);
                    decision_score = investor_score(
                        actual_opportunity,
                        strategy,
                        quality,
                        market,
                        holding_count,
                        person_id.0,
                    );
                    if status.is_some_and(|status| *status == WorkStatus::Chilling)
                        && holding_count > 0
                    {
                        decision_score += 5.0;
                    }
                    if decision_score < investor_threshold(strategy) {
                        return None;
                    }
                } else if kind == SettlementBuildingKind::House {
                    decision_score -= holding_count as f32 * 4.0;
                }
                Some((
                    entity,
                    *person_id,
                    name.0.clone(),
                    holding_count,
                    decision_score,
                    actual_opportunity.civic_priority,
                    startup_capital,
                ))
            },
            )
            .max_by(|a, b| {
                a.4.total_cmp(&b.4)
                    .then_with(|| b.3.cmp(&a.3))
                    .then_with(|| b.1.cmp(&a.1))
            });
        let Some((
            builder,
            applicant_id,
            applicant,
            applicant_holdings,
            _,
            demand_subsidized,
            startup_capital,
        )) = applicant
        else {
            clock
                .deferred_opportunities
                .insert((settlement_entity, kind), permit_round.saturating_add(3));
            debug!(
                "Village '{}': no resident currently chooses its offered {} permit",
                settlement.name,
                kind.label()
            );
            continue;
        };

        // Charge the actual approved kind. The coastal substitution has the
        // same base as a Farmstead today, but keeping this exact lets their
        // prices diverge later without charging for a building never granted.
        let fee = permit_price_with_subsidy(
            kind,
            applicant_holdings,
            demand_subsidized,
            policies.business_permit_subsidy_bps,
        );
        let prudent_company_cash = fee.saturating_add(startup_capital);
        let mut operating_company = is_private_business(kind)
            .then(|| company_by_master.get(&applicant_id).copied())
            .flatten();
        let company_paid = if let Some(company) = operating_company {
            if company_funds
                .get(&company)
                .is_some_and(|funds| funds.available >= prudent_company_cash)
            {
                let mut accounts = buildings.p1();
                debit_company_expansion(
                    company_funds
                        .get_mut(&company)
                        .expect("checked company expansion funds"),
                    &mut accounts,
                    fee,
                )
            } else {
                false
            }
        } else {
            false
        };
        let mut contributed_capital = 0;
        if !company_paid {
            if operating_company
                .is_some_and(|company| !personal_capital_companies.contains(&company))
            {
                // Defensive recheck: the applicant scoring already excludes
                // this case, but another approved permit may have reduced the
                // pooled expansion balance earlier in the same update.
                continue;
            }
            if let Some(company) = operating_company {
                let Some(funds) = company_funds.get_mut(&company) else {
                    continue;
                };
                let shortfall = funds.personal_contribution_required(prudent_company_cash);
                if let Ok(mut wallet) = wallets.get_mut(builder) {
                    if wallet
                        .balance()
                        .saturating_sub(NPC_PERSONAL_INVESTMENT_RESERVE)
                        < shortfall
                        || !wallet.debit(shortfall)
                    {
                        continue;
                    }
                } else {
                    commands.entity(builder).insert(Wallet::new(
                        shared::economy::STARTING_VILLAGER_MONEY.saturating_sub(shortfall),
                    ));
                }
                let Some(company_entity) = funds.entity else {
                    continue;
                };
                let mut accounts = buildings.p1();
                let Ok((_, _, mut account)) = accounts.get_mut(company_entity) else {
                    continue;
                };
                account.credit(shortfall);
                if !account.debit(fee) {
                    continue;
                }
                account.contributed_capital = account.contributed_capital.saturating_add(shortfall);
                funds.record_personal_contribution(shortfall, fee);
                contributed_capital = shortfall;
            } else {
                if let Ok(mut wallet) = wallets.get_mut(builder) {
                    if wallet
                        .balance()
                        .saturating_sub(NPC_PERSONAL_INVESTMENT_RESERVE)
                        < prudent_company_cash
                        || !wallet.debit(prudent_company_cash)
                    {
                        continue;
                    }
                } else {
                    // Old/test villagers without a wallet migrate into the live
                    // rule with the same founding endowment, minus this capital.
                    commands.entity(builder).insert(Wallet::new(
                        shared::economy::STARTING_VILLAGER_MONEY
                            .saturating_sub(prudent_company_cash),
                    ));
                }
                if is_private_business(kind) {
                    let company = planning.ids.company();
                    commands.spawn(crate::world::village::new_company_bundle(
                        company,
                        format!("{} & Company", applicant),
                        civic_day.saturating_sub(1),
                        applicant_id,
                        startup_capital,
                        prudent_company_cash,
                    ));
                    operating_company = Some(company);
                    contributed_capital = prudent_company_cash;
                }
            }
        }
        settlement.treasury = settlement.treasury.saturating_add(fee);
        if let Some(account) = civic_account.as_deref_mut() {
            account.record_permit_income(civic_day, fee);
        }

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
            kind.placement_definition().footprint.y,
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
                    failed_stand_routes: 0,
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
                PlannedRoadAccess {
                    settlement_id: *settlement_id,
                    points: planned_access_points,
                    half_width: RoadClass::Lane.initial_reserved_width() * 0.5,
                },
                PlayerPosition(position),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();
        if kind == SettlementBuildingKind::House {
            commands
                .entity(site)
                .insert(shared::components::HouseAppearance::for_new_house(
                    settlement.tier,
                    position,
                ));
        }
        if is_private_business(kind) {
            commands.entity(site).insert((
                BusinessProjectAccounting {
                    company: operating_company,
                    contributed_capital,
                    capital_expenditure: fee,
                },
                shared::components::OperatedBy(
                    operating_company.expect("private permit formed or selected a company"),
                ),
            ));
        }

        if !kind.is_civic() {
            // Constructing an approved private plot is the applicant's one
            // daytime occupation until the shell is finished. An off-shift
            // employee may choose entrepreneurship, but they must resign the
            // old position instead of becoming a farmer/porter and builder at
            // once. Otherwise the two routines repeatedly replace each
            // other's MoveTarget and can strand the worksite forever.
            commands
                .entity(builder)
                .insert((Occupation(None), WorkStatus::LookingForWork))
                .remove::<shared::components::EmployedAt>()
                .remove::<CompanyPorter>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<QuarryRoutine>()
                .remove::<ProcessingRoutine>()
                .remove::<InternalDeliveryRoutine>()
                .remove::<MarketCollectionRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<PierTraversal>()
                .remove::<WorkerOffDuty>()
                .remove::<HomeRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
        }

        // Approval reserves the plot and money immediately, but a tactical
        // applicant first joins the Moot forecourt line to collect the stamped
        // permit. Focused unit tests without the shared runtime retain the
        // direct seam; both paths begin identical physical material work.
        if let Some(queue_clock) = queue_clock.as_deref_mut() {
            moot_services::wait_for_permit(
                &mut commands,
                queue_clock,
                builder,
                settlement_entity,
                site,
            );
        } else {
            commands.entity(builder).remove::<MoveTarget>().insert((
                ConstructionMaterialRoutine::new(site),
                CharacterActivity::Idle,
            ));
        }
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
