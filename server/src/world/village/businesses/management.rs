use super::super::*;

use super::market_response::{MarketResponseSite, MarketResponses};
use super::NEW_BUSINESS_DAYS;
use crate::world::village::commerce::payroll_claims::PrivatePayrollClaims;
use shared::economy::{BusinessManagementPolicy, sustainable_unit_price};

#[cfg(test)]
#[path = "management_policy_tests.rs"]
mod policy_tests;

#[cfg(test)]
#[path = "management_demand_cycle_tests.rs"]
mod demand_cycle_tests;

const INSOLVENT_DAYS_BEFORE_CLOSURE: u16 = 5;
const AUTOPILOT_UNSOLD_EXIT_DAYS: u16 = 7;
const LIQUIDATION_EMPTY_DAYS: u16 = 2;
const LIQUIDATION_DAILY_MARKDOWN_BPS: u16 = 1_500;
const OWNER_PERSONAL_FLOOR: u64 = 4 * PENNIES_PER_COIN;
const OWNER_RESCUE_LIMIT: u64 = 2 * PENNIES_PER_COIN;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MarketPriceSignals {
    pub best_competitor: Option<u64>,
    pub last_clearing_price: u64,
    pub sold_units: u64,
    pub unavailable_units: u64,
    pub unaffordable_units: u64,
}

fn automatic_owner_should_close(
    day: u32,
    account: &BusinessAccount,
    sale: &BusinessSalePolicy,
    condition: &BusinessCondition,
    total_stock: u32,
    market: MarketPriceSignals,
) -> bool {
    let old_enough = day.saturating_sub(condition.opened_day)
        >= NEW_BUSINESS_DAYS.saturating_add(u32::from(AUTOPILOT_UNSOLD_EXIT_DAYS));
    old_enough
        && matches!(
            condition.state,
            BusinessState::Operating | BusinessState::CashTight | BusinessState::Distressed
        )
        && sale.days_without_sales >= AUTOPILOT_UNSOLD_EXIT_DAYS
        // Buyers who reached a real listing but could not afford it are
        // evidence for a price change, not evidence that demand vanished.
        && market.unaffordable_units == 0
        && total_stock > sale.company_reserve_units
        && account.previous_day.profit() < 0
}

fn price_step(value: u64, basis_points: u16, increase: bool) -> u64 {
    let movement = value
        .saturating_mul(u64::from(basis_points))
        .div_ceil(BASIS_POINTS)
        .max(1);
    if increase {
        value.saturating_add(movement)
    } else {
        value.saturating_sub(movement).max(1)
    }
}

fn contribute_rescue_capital(
    wallet: &mut Wallet,
    company: &mut shared::economy::CompanyAccount,
    account: &mut BusinessAccount,
) -> bool {
    let rescue = wallet
        .balance()
        .saturating_sub(OWNER_PERSONAL_FLOOR)
        .min(OWNER_RESCUE_LIMIT)
        .min(u64::MAX.saturating_sub(company.cash))
        .min(u64::MAX.saturating_sub(account.contributed_capital));
    if rescue == 0 || !wallet.debit(rescue) {
        return false;
    }
    company.credit(rescue);
    account.contributed_capital += rescue;
    true
}

fn request_closing_staff_release(commands: &mut Commands, business: Entity, workers: &[Entity]) {
    commands
        .entity(business)
        .insert(BusinessStaffingPolicy::new(0));
    for worker in workers {
        commands
            .entity(*worker)
            .insert(super::super::worker_activity::EmploymentReleaseRequested);
    }
}

pub(crate) fn review_automatic_price(
    account: &mut BusinessAccount,
    sale: &mut BusinessSalePolicy,
    condition: BusinessState,
    total_stock: u32,
    good: Good,
    market: MarketPriceSignals,
    replacement_input_cost: u64,
    market_fee_bps: u16,
) {
    if !sale.automatic_pricing {
        return;
    }
    let previous = account.previous_day;
    let output_basis = previous.produced_units.max(previous.sold_units).max(1);
    let observed_cost = previous
        .wage_expense
        .checked_div(u64::from(output_basis))
        .unwrap_or(0)
        .saturating_add(replacement_input_cost);
    if observed_cost > 0 {
        account.estimated_unit_cost = if account.estimated_unit_cost == 0 {
            observed_cost
        } else {
            account
                .estimated_unit_cost
                .saturating_mul(3)
                .saturating_add(observed_cost)
                / 4
        };
    }

    let current = if sale.asking_unit_price <= 1 {
        market
            .best_competitor
            .unwrap_or(market.last_clearing_price)
            .max(good.base_price())
    } else {
        sale.asking_unit_price
    };
    if previous.sold_units == 0 {
        sale.days_without_sales = sale.days_without_sales.saturating_add(1);
    } else {
        sale.days_without_sales = 0;
    }

    let affordability_pressure =
        market.unaffordable_units > 0 && total_stock > sale.company_reserve_units;
    let scarce_and_selling = !affordability_pressure
        && previous.sold_units > 0
        && previous.sold_units >= previous.produced_units.max(1)
        && total_stock <= sale.company_reserve_units.saturating_add(2)
        && market.unavailable_units >= market.unaffordable_units;
    let empty_shortage = !affordability_pressure
        && market.unavailable_units > 0
        && total_stock <= sale.company_reserve_units;
    let production_basis = previous.produced_units.max(1);
    let accumulating_surplus = previous.produced_units > 0
        && previous.sold_units.saturating_mul(2) <= previous.produced_units
        && total_stock
            > sale
                .company_reserve_units
                .saturating_add(production_basis.saturating_mul(2));
    let extreme_surplus = accumulating_surplus
        && total_stock
            > sale
                .company_reserve_units
                .saturating_add(production_basis.saturating_mul(5));
    let stale_surplus = sale.days_without_sales >= 2 && total_stock > sale.company_reserve_units;
    let mut desired = if scarce_and_selling || empty_shortage {
        price_step(current, sale.max_daily_price_change_bps, true)
    } else if stale_surplus || accumulating_surplus {
        // Some sales do not prove a price is clearing the market. When output
        // is piling up much faster than it sells, an automatic owner responds
        // to that inventory carrying cost; an extreme glut provokes a larger
        // voluntary markdown. The sustainable-cost floor remains authoritative.
        let markdown = if extreme_surplus {
            sale.max_daily_price_change_bps.saturating_mul(2)
        } else {
            sale.max_daily_price_change_bps
        };
        price_step(current, markdown, false)
    } else {
        current
    };

    // A seller with stock should react to real rejected buyers much faster
    // than the ordinary inventory-smoothing cadence. This is still a bounded
    // daily decision, but a Cautious owner cannot preserve a high margin for
    // weeks while residents stand at the counter unable to pay it.
    if affordability_pressure {
        let affordability_step = sale
            .max_daily_price_change_bps
            .saturating_mul(3)
            .clamp(1_500, 3_000);
        desired = desired.min(price_step(current, affordability_step, false));
    }

    // Continuously inspect the order book. Weak-selling owners move toward a
    // rival's cheaper live offer and seek a one-penny undercut where their
    // replacement cost permits it. The bounded move prevents one odd listing
    // from causing a discontinuous town-wide price crash.
    let rival_sales = market.sold_units > u64::from(previous.sold_units);
    let weak_sales = previous.sold_units == 0
        || previous.sold_units < previous.produced_units
        || rival_sales
        || total_stock > sale.company_reserve_units.saturating_add(2);
    if weak_sales {
        // A past sale is not an offer buyers can still accept. In an empty
        // market that stale price used to cancel every scarcity increase,
        // permanently trapping a closed supplier below its restart cost.
        if let Some(reference) = market.best_competitor.filter(|price| *price < current) {
            let target = reference.saturating_sub(1).max(1);
            let competitive_step = sale
                .max_daily_price_change_bps
                .saturating_mul(4)
                .clamp(1_500, 3_000);
            let bounded = price_step(current, competitive_step, false).max(target);
            desired = desired.min(bounded);
        }
    }

    let sustainable = sustainable_unit_price(
        account.estimated_unit_cost,
        market_fee_bps,
        if affordability_pressure {
            0
        } else {
            sale.target_margin_bps
        },
    );
    if matches!(
        condition,
        BusinessState::New
            | BusinessState::Operating
            | BusinessState::CashTight
            | BusinessState::Mothballed
    ) {
        desired = desired.max(sustainable);
    }
    // A clearance discount can realise value from existing stock. Applied to
    // an empty workplace it instead cancels every scarcity increase above,
    // leaving a distressed producer unable to quote a profitable restart even
    // when buyers are waiting. Empty shelves keep the normal demand-led price
    // decision; funding, inputs and the operating margin still govern work.
    if total_stock > 0
        && matches!(
            condition,
            BusinessState::Distressed | BusinessState::Insolvent
        )
    {
        desired = price_step(desired, sale.max_daily_price_change_bps, false);
    }
    sale.asking_unit_price = desired.max(sale.minimum_unit_price).max(1);
}

/// Review every business once per world day. Price changes, owner withdrawals,
/// rescue capital and closure are intentionally absent from frame-rate logic.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn review_business_management(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    halls: Query<
        (
            Entity,
            &shared::components::SettlementId,
            Option<&SettlementEconomy>,
        ),
        With<Settlement>,
    >,
    mut markets: Query<&mut MootMarket, With<Settlement>>,
    mut companies: ParamSet<(
        Query<(
            Entity,
            &shared::components::CompanyId,
            &shared::economy::CompanyAccount,
            Option<&shared::components::CompanyOwnership>,
        )>,
        Query<&mut shared::economy::CompanyAccount>,
    )>,
    mut businesses: Query<(
        Entity,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &SettlementBuilding,
        &GoodsInventory,
        &mut BusinessAccount,
        &mut BusinessSalePolicy,
        &BusinessWagePolicy,
        Option<&mut BusinessProcurementPolicy>,
        &BusinessManagementPolicy,
        &mut BusinessCondition,
        Option<&shared::components::OwnedBy>,
        Option<&shared::components::OperatedBy>,
        Option<&mut BusinessLiquidation>,
        Option<&BusinessForSale>,
    )>,
    mut payroll_claims: Query<&mut PrivatePayrollClaims>,
    mut villagers: Query<
        (
            Entity,
            &shared::components::PersonId,
            Option<&shared::components::EmployedAt>,
            &mut Wallet,
            &mut Occupation,
            &mut WorkStatus,
        ),
        With<Occupation>,
    >,
    mut hero_owners: Query<
        (Entity, &shared::components::PersonId, &mut Wallet),
        (With<shared::components::Hero>, Without<Occupation>),
    >,
    collections: Query<&MarketCollectionRoutine>,
    deliveries: Query<&InternalDeliveryRoutine>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if businesses
        .iter()
        .all(|(_, _, _, _, _, _, _, _, _, _, condition, ..)| condition.last_review_day == day)
    {
        // New sites still get a same-day first review. Quiet simulation ticks
        // do not rebuild settlement, shareholder and workforce hash maps for
        // decisions every existing company has already made today.
        return;
    }
    let hall_by_settlement: HashMap<shared::components::SettlementId, Entity> =
        halls.iter().map(|(entity, id, _)| (*id, entity)).collect();
    let responses = MarketResponses::build(
        businesses.iter().map(
            |(
                _,
                id,
                building_of,
                building,
                _,
                _,
                sale,
                wage,
                _,
                management,
                condition,
                _,
                _,
                _,
                _,
            )| {
                let mut sale = *sale;
                sale.target_margin_bps = management.strategy.target_margin_bps();
                MarketResponseSite {
                    id: *id,
                    settlement: building_of.0,
                    kind: building.kind,
                    quality: building.quality,
                    state: condition.state,
                    assigned_workers: building.workers.len().min(usize::from(u8::MAX)) as u8,
                    daily_wage: wage.daily_wage,
                    sale,
                    autopilot: management.autopilot,
                }
            },
        ),
        |settlement| {
            hall_by_settlement
                .get(&settlement)
                .and_then(|entity| markets.get(*entity).ok())
        },
    );
    let unmet_food_by_settlement: HashMap<shared::components::SettlementId, u32> = halls
        .iter()
        .map(|(_, id, economy)| (*id, economy.map_or(0, |economy| economy.unmet_food)))
        .collect();
    let people_by_id: HashMap<shared::components::PersonId, Entity> = villagers
        .iter()
        .map(|(entity, id, ..)| (*id, entity))
        .collect();
    let heroes_by_id: HashMap<shared::components::PersonId, Entity> = hero_owners
        .iter()
        .map(|(entity, id, _)| (*id, entity))
        .collect();
    let companies_by_id: HashMap<
        shared::components::CompanyId,
        (
            Entity,
            shared::economy::CompanyAccount,
            Option<shared::components::PersonId>,
        ),
    > = companies
        .p0()
        .iter()
        .map(|(entity, id, account, ownership)| {
            let sole_owner = ownership.and_then(|ownership| {
                let shares = ownership.shares();
                (shares.len() == 1).then(|| shares[0].shareholder)
            });
            (*id, (entity, *account, sole_owner))
        })
        .collect();
    let mut workers_by_business: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    for (entity, _, employed, ..) in villagers.iter() {
        if let Some(employed) = employed {
            workers_by_business
                .entry(employed.0)
                .or_default()
                .push(entity);
        }
    }
    let mut responsive_food_businesses = HashMap::<shared::components::SettlementId, u64>::new();
    for (_, _, building_of, building, _, _, _, _, _, _, condition, ..) in businesses.iter_mut() {
        if condition.state.accepts_new_workers() || condition.state == BusinessState::Mothballed {
            if let Some(output) = super::super::business_output(building.kind) {
                if output.is_edible() {
                    *responsive_food_businesses.entry(building_of.0).or_default() += 1;
                }
            }
        }
    }

    for (
        business_entity,
        building_id,
        building_of,
        building,
        inventory,
        mut account,
        mut sale,
        wage,
        mut procurement,
        management,
        mut condition,
        owner,
        operated_by,
        liquidation,
        for_sale,
    ) in businesses.iter_mut()
    {
        account.roll_to_day(day);
        if condition.opened_day == u32::MAX {
            condition.opened_day = day;
        }
        if condition.last_review_day == day {
            continue;
        }
        let elapsed = if condition.last_review_day == u32::MAX {
            1
        } else {
            day.saturating_sub(condition.last_review_day).max(1)
        }
        .min(u32::from(u16::MAX)) as u16;
        condition.last_review_day = day;

        // A property awaiting a buyer or deliberately retired shell is not an
        // operating firm. Reopening always goes through an explicit takeover.
        if matches!(
            condition.state,
            BusinessState::ForSale | BusinessState::Closed
        ) || (for_sale.is_some() && liquidation.is_none())
        {
            request_closing_staff_release(
                &mut commands,
                business_entity,
                workers_by_business
                    .get(building_id)
                    .map_or(&[], Vec::as_slice),
            );
            sale.collection_enabled = false;
            continue;
        }

        let Some(good) = super::super::business_output(building.kind)
            .or_else(|| (building.kind == SettlementBuildingKind::Tavern).then_some(Good::Bread))
        else {
            if building.kind == SettlementBuildingKind::StorageHall {
                // A depot is company infrastructure: its wages are visible in
                // the site ledger and consolidated company profit, but the
                // absence of direct product revenue does not independently
                // declare the building insolvent.
                condition.state = BusinessState::Operating;
            }
            continue;
        };
        let Some(hall) = hall_by_settlement.get(&building_of.0).copied() else {
            continue;
        };
        let Ok(mut market) = markets.get_mut(hall) else {
            continue;
        };
        let seller = MarketSeller::Business(*building_id);

        if let Some(mut liquidation) = liquidation {
            condition.state = BusinessState::Liquidating;
            // Arrears already belong to the employees who earned them.
            // Moving the named claims changes their lifecycle owner without
            // adding to or redistributing the site's aggregate liability.
            if let Ok(mut claims) = payroll_claims.get_mut(business_entity) {
                liquidation.wage_claims.extend(claims.take());
            }
            let workers: &[Entity] = workers_by_business
                .get(building_id)
                .map_or(&[], Vec::as_slice);
            request_closing_staff_release(&mut commands, business_entity, workers);
            // End the shift through its existing physical return/deposit/exit
            // sequence. The shared employment pass releases an empty worker
            // once those owners are clear; bankruptcy cannot teleport a fisher
            // off their pier or orphan the goods a processor is carrying.
            liquidation.staff_released = workers.is_empty();
            condition.liquidation_days = condition.liquidation_days.saturating_add(elapsed);
            sale.collection_enabled = true;
            sale.company_reserve_days = 0;
            sale.company_reserve_units = 0;
            sale.max_units_per_collection = sale.max_units_per_collection.max(32);
            // Once costs are sunk, liquidation follows the real order book.
            // One penny is the only universal bound; an authored base value is
            // not a reason to leave food stranded above buyers' means.
            sale.minimum_unit_price = 1;
            sale.asking_unit_price = price_step(
                sale.asking_unit_price.max(1),
                LIQUIDATION_DAILY_MARKDOWN_BPS,
                false,
            )
            .max(sale.minimum_unit_price)
            .max(1);
            market.markdown_seller(seller, LIQUIDATION_DAILY_MARKDOWN_BPS);

            // Liquidation receipts pay former employees before any other
            // claimant. Claims survive job release and are addressed by stable
            // PersonId, so the payment does not depend on display names.
            liquidation
                .wage_claims
                .sort_unstable_by_key(|claim| claim.worker);
            if !liquidation.wage_claims.is_empty() {
                let rotation = day as usize % liquidation.wage_claims.len();
                liquidation.wage_claims.rotate_left(rotation);
            }
            for claim in &mut liquidation.wage_claims {
                if claim.pennies == 0 {
                    continue;
                }
                let mut wallet = if let Some(worker) = people_by_id.get(&claim.worker).copied() {
                    let Ok((_, _, _, wallet, _, _)) = villagers.get_mut(worker) else {
                        continue;
                    };
                    wallet
                } else if let Some(hero) = heroes_by_id.get(&claim.worker).copied() {
                    let Ok((_, _, wallet)) = hero_owners.get_mut(hero) else {
                        continue;
                    };
                    wallet
                } else {
                    // Death/estate settlement owns genuine default. A person
                    // temporarily absent from these worker queries does not
                    // transfer the claim to a replacement or erase the debt.
                    continue;
                };
                let Some(company_entity) = operated_by
                    .and_then(|operation| companies_by_id.get(&operation.0))
                    .map(|(entity, _, _)| *entity)
                else {
                    continue;
                };
                let mut company_accounts = companies.p1();
                let Ok(mut company) = company_accounts.get_mut(company_entity) else {
                    continue;
                };
                let payment = claim
                    .pennies
                    .min(company.cash)
                    .min(account.wage_arrears)
                    .min(u64::MAX.saturating_sub(wallet.balance()));
                if payment == 0 || !company.debit(payment) {
                    continue;
                }
                wallet.credit(payment);
                let settled = account.settle_wage_claim(payment);
                claim.pennies = claim.pennies.saturating_sub(settled);
            }
            liquidation.wage_claims.retain(|claim| claim.pennies > 0);

            let physical_units = Good::ALL
                .into_iter()
                .map(|held| inventory.amount(held))
                .fold(0u32, u32::saturating_add);
            let listed_units = market.seller_total_listed_units(seller);
            let in_transit = collections.iter().any(|routine| {
                routine.seller == *building_id || routine.business == business_entity
            }) || deliveries.iter().any(|routine| {
                routine.supplier == business_entity
                    || routine.receiver == business_entity
                    || routine.supplier_id == *building_id
                    || routine.receiver_id == *building_id
            });
            if physical_units == 0 && listed_units == 0 && !in_transit && workers.is_empty() {
                liquidation.empty_days = liquidation.empty_days.saturating_add(elapsed);
            } else {
                liquidation.empty_days = 0;
            }
            liquidation.last_review_day = day;

            let remaining_company_cash = operated_by
                .and_then(|operation| companies_by_id.get(&operation.0))
                .map_or(0, |(entity, _, _)| {
                    companies
                        .p1()
                        .get(*entity)
                        .map_or(0, |company| company.cash)
                });
            let funded_unpaid_wages = account.wage_arrears > 0 && remaining_company_cash > 0;
            if liquidation.empty_days >= LIQUIDATION_EMPTY_DAYS && !funded_unpaid_wages {
                // No further stock can fund these claims. Record the genuine
                // default instead of keeping a dead firm and its food alive
                // forever. Taxes have already had their ordinary daily chance
                // to collect after the senior wage reserve. A funded claim
                // with a temporarily unavailable/full recipient wallet stays
                // payable instead of becoming a default just because the
                // last crate was sold.
                if account.wage_arrears > 0 {
                    let outstanding_wages = account.wage_arrears;
                    account.write_off_wage_claim(outstanding_wages);
                }
                if account.tax_arrears > 0 {
                    account.defaulted_taxes =
                        account.defaulted_taxes.saturating_add(account.tax_arrears);
                    account.tax_arrears = 0;
                }
                condition.state = BusinessState::ForSale;
                sale.collection_enabled = false;
                commands
                    .entity(business_entity)
                    .insert(BusinessForSale {
                        previous_owner: owner
                            .map_or(shared::components::PersonId::UNASSIGNED, |owner| owner.0),
                        asking_price: super::super::mortality::takeover_price(building.kind),
                        listed_day: day,
                        reason: liquidation.reason,
                    })
                    .remove::<BusinessLiquidation>();
                info!(
                    "Liquidation completed for {} in '{}'; the property is now for sale",
                    building.kind.label(),
                    building.settlement,
                );
            }
            continue;
        }

        let worker_count = workers_by_business.get(building_id).map_or(0, Vec::len) as u64;
        let daily_payroll = wage.daily_wage.saturating_mul(worker_count);
        let company_reading = operated_by
            .and_then(|operation| companies_by_id.get(&operation.0))
            .map(|(_, account, _)| *account);
        let free_cash = company_reading.map_or(0, |company| {
            company
                .cash
                .saturating_sub(company.wage_arrears)
                .saturating_sub(company.tax_arrears)
        });
        let company_supports_site = free_cash > 0;
        let old_state = condition.state;
        if (account.wage_arrears > 0 || account.tax_arrears > 0) && free_cash == 0 {
            condition.insolvent_days = if company_supports_site {
                0
            } else {
                condition.insolvent_days.saturating_add(elapsed)
            };
            condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
            condition.state = if company_supports_site {
                BusinessState::Distressed
            } else if condition.insolvent_days >= INSOLVENT_DAYS_BEFORE_CLOSURE {
                BusinessState::Liquidating
            } else {
                BusinessState::Insolvent
            };
        } else if account.wage_arrears > 0 || account.tax_arrears > 0 {
            condition.insolvent_days = 0;
            condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
            condition.state = BusinessState::Distressed;
        } else if daily_payroll > 0 && free_cash < daily_payroll.saturating_mul(2) {
            condition.insolvent_days = 0;
            condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
            condition.state = BusinessState::CashTight;
        } else if old_state == BusinessState::Mothballed {
            condition.insolvent_days = 0;
            condition.cash_tight_days = 0;
            condition.state = BusinessState::Mothballed;
        } else {
            condition.insolvent_days = 0;
            condition.cash_tight_days = 0;
            condition.state = if day.saturating_sub(condition.opened_day) < NEW_BUSINESS_DAYS {
                BusinessState::New
            } else {
                BusinessState::Operating
            };
        }
        if matches!(
            condition.state,
            BusinessState::Operating | BusinessState::CashTight
        ) {
            condition.operating_days = condition.operating_days.saturating_add(elapsed);
        }
        let listed = market.seller_listed_units(seller, good);
        let pool = *market.pool(good);
        let generic_food_shortage = if good.is_edible() {
            u64::from(
                unmet_food_by_settlement
                    .get(&building_of.0)
                    .copied()
                    .unwrap_or_default(),
            )
            .div_ceil(
                responsive_food_businesses
                    .get(&building_of.0)
                    .copied()
                    .unwrap_or(1)
                    .max(1),
            )
        } else {
            0
        };
        let market_signals = MarketPriceSignals {
            best_competitor: market.best_competing_price(seller, good),
            last_clearing_price: pool.bid,
            sold_units: pool
                .day
                .consumer_units
                .saturating_add(pool.previous_day.consumer_units),
            unavailable_units: pool
                .day
                .unavailable_units
                .saturating_add(pool.previous_day.unavailable_units)
                .max(generic_food_shortage),
            unaffordable_units: pool
                .day
                .unaffordable_units
                .saturating_add(pool.previous_day.unaffordable_units),
        };
        let replacement_input_cost = processing_recipe(building.kind).map_or(0, |recipe| {
            u64::from(recipe.input_units)
                .saturating_mul(market.suggested_price(recipe.input))
                .div_ceil(u64::from(recipe.output_units.max(1)))
        });
        let total_output_stock = inventory.amount(good).saturating_add(listed);
        let mut voluntary_closure = false;
        if management.autopilot {
            // Until a site has real production history, the owner estimates
            // costs from its actual land/water quality and opening roster.
            // This is a private pricing decision, not a regulated floor:
            // manual owners remain free to quote any price they choose.
            if account.current_day.produced_units == 0 && account.previous_day.produced_units == 0 {
                if let Some(estimated) = estimated_staffed_unit_cost(
                    building.kind,
                    building.quality,
                    automatic_opening_positions(building.kind),
                    wage.daily_wage,
                    |input| market.suggested_price(input),
                ) {
                    account.estimated_unit_cost = estimated;
                }
            }
            sale.target_margin_bps = management.strategy.target_margin_bps();
            sale.max_daily_price_change_bps = management.strategy.daily_price_step_bps();
            // Active downstream requests are reserved before public collection,
            // so an automatic owner does not need a second speculative output
            // hoard. Manual owners may still select one explicitly.
            sale.company_reserve_days = 0;
            review_automatic_price(
                &mut account,
                &mut sale,
                condition.state,
                total_output_stock,
                good,
                market_signals,
                replacement_input_cost,
                market.market_fee_bps(),
            );
            if sale.automatic_pricing && total_output_stock == 0 {
                if let Some((quote, ceiling)) = responses.restart_quote_for(*building_id) {
                    // Empty shelves have no inventory to clear. Quote within
                    // the overlap between real batch cost and demonstrated
                    // buying capacity; otherwise a scarcity rise above every
                    // funded bid can itself prevent production indefinitely.
                    sale.asking_unit_price = sale.asking_unit_price.clamp(quote, ceiling);
                }
            }
            voluntary_closure = condition.state != BusinessState::Mothballed
                && automatic_owner_should_close(
                    day,
                    &account,
                    &sale,
                    &condition,
                    total_output_stock,
                    market_signals,
                );
            if voluntary_closure {
                condition.state = BusinessState::Liquidating;
            }
            if let Some(procurement) = procurement.as_deref_mut() {
                if procurement.automatic {
                    if let Some(recipe) = processing_recipe(building.kind) {
                        let mut rule = procurement.rule(recipe.input);
                        if rule.enabled {
                            rule.set_coverage_days(
                                if matches!(
                                    condition.state,
                                    BusinessState::CashTight
                                        | BusinessState::Distressed
                                        | BusinessState::Insolvent
                                ) {
                                    1
                                } else {
                                    management.strategy.input_coverage_days()
                                },
                            );
                            if let Some(maximum) = maximum_viable_input_unit_price(
                                building.kind,
                                sale.asking_unit_price,
                                wage.daily_wage,
                                market.market_fee_bps(),
                                sale.target_margin_bps,
                            ) {
                                rule.maximum_unit_price = maximum;
                            }
                            procurement.set_rule(recipe.input, rule);
                        }

                        // A processor's empty input shelf is still an order.
                        // Record it once in this daily review so downstream
                        // scarcity can travel upstream through ordinary market
                        // prices (bread -> flour -> wheat). The physical
                        // porter/strategic purchase paths remain the only code
                        // which actually moves or pays for goods.
                        let wanted = rule.target_units.saturating_sub(
                            inventory
                                .amount(recipe.input)
                                .saturating_add(market.seller_listed_units(seller, recipe.input)),
                        );
                        if wanted > 0 && market.listed_units(recipe.input) == 0 {
                            let _ = market.purchase_recording_demand(
                                recipe.input,
                                wanted,
                                free_cash,
                                Some(rule.maximum_unit_price),
                                Some(seller),
                            );
                        }
                    }
                }
            }
        }
        sale.last_review_day = day;
        market.reprice(seller, good, sale.asking_unit_price);

        let owner_entity = owner.and_then(|owner| people_by_id.get(&owner.0).copied());
        let hero_owner_entity = owner.and_then(|owner| heroes_by_id.get(&owner.0).copied());
        if condition.state == BusinessState::Insolvent && management.rescue_with_personal_savings {
            // Automatic gifts to a shared company would enrich its other
            // shareholders without a loan or new share issue. Only its actual
            // sole owner may contribute, and the receiving treasury must exist.
            if let Some((company_entity, _, _)) = operated_by
                .and_then(|operation| companies_by_id.get(&operation.0))
                .filter(|(_, _, sole_owner)| {
                    sole_owner.is_some() && *sole_owner == owner.map(|owner| owner.0)
                })
            {
                if let Ok(mut company) = companies.p1().get_mut(*company_entity) {
                    let rescued = if let Some(owner_entity) = owner_entity {
                        villagers
                            .get_mut(owner_entity)
                            .is_ok_and(|(_, _, _, mut wallet, _, _)| {
                                contribute_rescue_capital(&mut wallet, &mut company, &mut account)
                            })
                    } else if let Some(owner_entity) = hero_owner_entity {
                        hero_owners
                            .get_mut(owner_entity)
                            .is_ok_and(|(_, _, mut wallet)| {
                                contribute_rescue_capital(&mut wallet, &mut company, &mut account)
                            })
                    } else {
                        false
                    };
                    if rescued {
                        condition.state = BusinessState::Distressed;
                        condition.insolvent_days = 0;
                    }
                }
            }
        }

        if condition.state == BusinessState::Liquidating {
            sale.collection_enabled = true;
            sale.company_reserve_days = 0;
            sale.company_reserve_units = 0;
            sale.max_units_per_collection = sale.max_units_per_collection.max(32);
            market.reprice(seller, good, sale.asking_unit_price.max(1));
            let claims = payroll_claims
                .get_mut(business_entity)
                .map_or_else(|_| Vec::new(), |mut claims| claims.take());
            let workers: &[Entity] = workers_by_business
                .get(building_id)
                .map_or(&[], Vec::as_slice);
            let mut liquidation = if voluntary_closure {
                BusinessLiquidation::voluntary_closure(day, claims)
            } else {
                BusinessLiquidation::insolvency(day, claims)
            };
            liquidation.staff_released = workers.is_empty();
            commands.entity(business_entity).insert(liquidation);
            request_closing_staff_release(&mut commands, business_entity, workers);
        }

        if old_state != condition.state {
            info!(
                "Business {} in '{}' changed from {} to {}",
                building.kind.label(),
                building.settlement,
                old_state.label(),
                condition.state.label(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquidating_tavern_releases_its_real_pantry_to_the_market_path() {
        let mut app = App::new();
        app.add_systems(Update, review_business_management);
        let mut clock = WorldTime::new_default();
        clock.day = 8;
        app.world_mut().spawn(clock);

        let settlement_id = shared::components::SettlementId(501);
        app.world_mut().spawn((
            settlement_id,
            Settlement {
                name: "Meadow".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 20,
                treasury: 0,
            },
            MootMarket::founding(),
        ));
        let company_id = shared::components::CompanyId(502);
        app.world_mut()
            .spawn((company_id, shared::economy::CompanyAccount::default()));

        let mut pantry = GoodsInventory::new(100);
        assert_eq!(pantry.add(Good::Meat, 2), 2);
        let tavern = app
            .world_mut()
            .spawn((
                shared::components::BuildingId(503),
                shared::components::BuildingOf(settlement_id),
                shared::components::OperatedBy(company_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Tavern,
                    settlement: "Meadow".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                pantry,
                BusinessAccount::default(),
                BusinessSalePolicy {
                    collection_enabled: false,
                    ..default()
                },
                BusinessWagePolicy::default(),
                BusinessManagementPolicy::default(),
                BusinessCondition {
                    state: BusinessState::Liquidating,
                    opened_day: 1,
                    ..default()
                },
                BusinessLiquidation::insolvency(7, Vec::new()),
            ))
            .id();

        app.update();

        let entity = app.world().entity(tavern);
        assert!(
            entity
                .get::<BusinessSalePolicy>()
                .unwrap()
                .collection_enabled
        );
        assert_eq!(
            entity.get::<BusinessCondition>().unwrap().state,
            BusinessState::Liquidating
        );
        assert_eq!(
            entity.get::<GoodsInventory>().unwrap().amount(Good::Meat),
            2,
            "management exposes the pantry; the physical porter moves it later"
        );
        assert!(entity.contains::<BusinessLiquidation>());
    }

    #[test]
    fn selling_out_raises_price_but_stale_stock_lowers_it() {
        let mut account = BusinessAccount {
            estimated_unit_cost: 20,
            previous_day: shared::economy::BusinessDayLedger {
                day: 2,
                gross_revenue: 80,
                wage_expense: 40,
                produced_units: 2,
                sold_units: 2,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Wheat);
        let opening = sale.asking_unit_price;
        review_automatic_price(
            &mut account,
            &mut sale,
            BusinessState::Operating,
            1,
            Good::Wheat,
            MarketPriceSignals {
                last_clearing_price: opening,
                ..default()
            },
            0,
            500,
        );
        assert!(sale.asking_unit_price > opening);

        let high = sale.asking_unit_price;
        account.previous_day = shared::economy::BusinessDayLedger {
            day: 3,
            produced_units: 4,
            ..default()
        };
        sale.days_without_sales = 1;
        review_automatic_price(
            &mut account,
            &mut sale,
            BusinessState::Distressed,
            20,
            Good::Wheat,
            MarketPriceSignals {
                last_clearing_price: high,
                ..default()
            },
            0,
            500,
        );
        assert!(sale.asking_unit_price < high);
    }

    #[test]
    fn partial_sales_do_not_hide_a_growing_inventory_glut() {
        let mut account = BusinessAccount {
            estimated_unit_cost: 20,
            previous_day: shared::economy::BusinessDayLedger {
                day: 4,
                gross_revenue: 600,
                produced_units: 12,
                sold_units: 2,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Bread);
        sale.asking_unit_price = 600;
        sale.max_daily_price_change_bps = 500;

        review_automatic_price(
            &mut account,
            &mut sale,
            BusinessState::Operating,
            80,
            Good::Bread,
            MarketPriceSignals {
                last_clearing_price: 600,
                ..default()
            },
            0,
            500,
        );

        assert_eq!(sale.days_without_sales, 0);
        assert_eq!(sale.asking_unit_price, 540);
    }

    #[test]
    fn bulk_procurement_is_not_mistaken_for_one_days_unit_cost() {
        let mut account = BusinessAccount {
            previous_day: shared::economy::BusinessDayLedger {
                day: 5,
                input_expense: 10_000,
                wage_expense: 100,
                produced_units: 10,
                sold_units: 10,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);

        review_automatic_price(
            &mut account,
            &mut sale,
            BusinessState::Operating,
            1,
            Good::Flour,
            MarketPriceSignals {
                last_clearing_price: Good::Flour.base_price(),
                ..default()
            },
            80,
            500,
        );

        assert_eq!(account.estimated_unit_cost, 90);
        assert!(sale.asking_unit_price < 200);
    }

    #[test]
    fn empty_mothballed_processor_restores_a_viable_ask_when_buyers_are_waiting() {
        let mut account = BusinessAccount {
            estimated_unit_cost: 100,
            previous_day: shared::economy::BusinessDayLedger {
                day: 8,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        sale.asking_unit_price = 30;

        review_automatic_price(
            &mut account,
            &mut sale,
            BusinessState::Mothballed,
            0,
            Good::Flour,
            MarketPriceSignals {
                unavailable_units: 12,
                ..default()
            },
            80,
            500,
        );

        assert!(sale.asking_unit_price > 30);
        assert!(
            sale.asking_unit_price
                >= sustainable_unit_price(account.estimated_unit_cost, 500, sale.target_margin_bps)
        );
    }

    #[test]
    fn historical_clearance_does_not_cancel_empty_market_price_discovery() {
        let mut account = BusinessAccount::default();
        let mut sale = BusinessSalePolicy::for_good(Good::Wood);
        sale.asking_unit_price = 4;
        for _ in 0..4 {
            let previous = sale.asking_unit_price;
            review_automatic_price(
                &mut account,
                &mut sale,
                BusinessState::Mothballed,
                0,
                Good::Wood,
                MarketPriceSignals {
                    last_clearing_price: 4,
                    unavailable_units: 6,
                    ..default()
                },
                0,
                500,
            );
            assert!(sale.asking_unit_price > previous);
        }
    }

    #[test]
    fn automatic_owner_exits_a_stocked_branch_after_a_week_without_sales() {
        let account = BusinessAccount {
            previous_day: shared::economy::BusinessDayLedger {
                day: 12,
                wage_expense: 100,
                produced_units: 2,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Flour);
        sale.days_without_sales = AUTOPILOT_UNSOLD_EXIT_DAYS;
        let condition = BusinessCondition {
            state: BusinessState::Operating,
            opened_day: 2,
            ..default()
        };

        assert!(automatic_owner_should_close(
            12,
            &account,
            &sale,
            &condition,
            8,
            MarketPriceSignals::default(),
        ));
        assert!(!automatic_owner_should_close(
            11,
            &account,
            &sale,
            &condition,
            8,
            MarketPriceSignals::default(),
        ));

        let profitable = BusinessAccount {
            previous_day: shared::economy::BusinessDayLedger {
                day: 12,
                internal_revenue: 200,
                wage_expense: 100,
                produced_units: 2,
                ..default()
            },
            ..default()
        };
        assert!(!automatic_owner_should_close(
            12,
            &profitable,
            &sale,
            &condition,
            8,
            MarketPriceSignals::default(),
        ));
    }

    #[test]
    fn distressed_empty_shelves_can_answer_real_demand_without_a_phantom_clearance() {
        for condition in [BusinessState::Distressed, BusinessState::Insolvent] {
            let mut account = BusinessAccount::default();
            let mut sale = BusinessSalePolicy::for_good(Good::Bread);
            sale.asking_unit_price = 26;
            for _ in 0..3 {
                let previous = sale.asking_unit_price;
                review_automatic_price(
                    &mut account,
                    &mut sale,
                    condition,
                    0,
                    Good::Bread,
                    MarketPriceSignals {
                        unavailable_units: 4,
                        ..default()
                    },
                    25,
                    500,
                );
                assert!(
                    sale.asking_unit_price > previous,
                    "scarcity must not be cancelled by discounting nonexistent stock"
                );
            }

            // No goods and no buyers do not justify a synthetic price rise.
            let no_demand_price = sale.asking_unit_price;
            review_automatic_price(
                &mut account,
                &mut sale,
                condition,
                0,
                Good::Bread,
                MarketPriceSignals::default(),
                25,
                500,
            );
            assert_eq!(sale.asking_unit_price, no_demand_price);

            // Real inventory still receives the distressed owner's clearance
            // markdown, including when it would sell below replacement cost.
            sale.asking_unit_price = 180;
            review_automatic_price(
                &mut account,
                &mut sale,
                condition,
                8,
                Good::Bread,
                MarketPriceSignals::default(),
                25,
                500,
            );
            assert!(sale.asking_unit_price < 180);
        }
    }

    #[test]
    fn rejected_buyers_accelerate_a_cautious_owners_markdown() {
        let mut account = BusinessAccount {
            estimated_unit_cost: 40,
            previous_day: shared::economy::BusinessDayLedger {
                produced_units: 4,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Bread);
        sale.asking_unit_price = 180;
        sale.max_daily_price_change_bps =
            shared::economy::BusinessStrategy::Conservative.daily_price_step_bps();
        sale.target_margin_bps =
            shared::economy::BusinessStrategy::Conservative.target_margin_bps();

        review_automatic_price(
            &mut account,
            &mut sale,
            BusinessState::Operating,
            20,
            Good::Bread,
            MarketPriceSignals {
                unaffordable_units: 8,
                ..default()
            },
            0,
            500,
        );

        assert_eq!(sale.asking_unit_price, 153);
    }

    #[test]
    fn weak_seller_moves_below_a_cheaper_competitor_without_crossing_cost() {
        let mut account = BusinessAccount {
            estimated_unit_cost: 50,
            previous_day: shared::economy::BusinessDayLedger {
                produced_units: 6,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Bread);
        sale.asking_unit_price = 180;

        for _ in 0..4 {
            review_automatic_price(
                &mut account,
                &mut sale,
                BusinessState::Operating,
                20,
                Good::Bread,
                MarketPriceSignals {
                    best_competitor: Some(120),
                    sold_units: 3,
                    ..default()
                },
                0,
                500,
            );
        }

        assert!(sale.asking_unit_price < 120);
        assert!(sale.asking_unit_price >= sustainable_unit_price(50, 500, 1_500));
    }

    #[test]
    fn unaffordable_demand_prevents_a_no_demand_exit() {
        let account = BusinessAccount {
            previous_day: shared::economy::BusinessDayLedger {
                wage_expense: 100,
                ..default()
            },
            ..default()
        };
        let mut sale = BusinessSalePolicy::for_good(Good::Bread);
        sale.days_without_sales = AUTOPILOT_UNSOLD_EXIT_DAYS;
        let condition = BusinessCondition {
            state: BusinessState::Operating,
            opened_day: 1,
            ..default()
        };

        assert!(!automatic_owner_should_close(
            20,
            &account,
            &sale,
            &condition,
            20,
            MarketPriceSignals {
                unaffordable_units: 10,
                ..default()
            },
        ));
    }
}
