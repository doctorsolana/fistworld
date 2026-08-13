use super::super::*;

use shared::economy::{business_working_capital, sustainable_unit_price, BusinessManagementPolicy};

const INSOLVENT_DAYS_BEFORE_CLOSURE: u16 = 5;
const NEW_BUSINESS_DAYS: u32 = 3;
const LIQUIDATION_EMPTY_DAYS: u16 = 2;
const LIQUIDATION_DAILY_MARKDOWN_BPS: u16 = 1_500;
const OWNER_PERSONAL_FLOOR: u64 = 4 * PENNIES_PER_COIN;
const OWNER_RESCUE_LIMIT: u64 = 2 * PENNIES_PER_COIN;

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

pub(crate) fn review_automatic_price(
    account: &mut BusinessAccount,
    sale: &mut BusinessSalePolicy,
    condition: BusinessState,
    total_stock: u32,
    good: Good,
    market_reference: u64,
    market_fee_bps: u16,
) {
    if !sale.automatic_pricing {
        return;
    }
    let previous = account.previous_day;
    let output_basis = previous.produced_units.max(previous.sold_units).max(1);
    let observed_cost = previous
        .wage_expense
        .saturating_add(previous.input_expense)
        .checked_div(u64::from(output_basis))
        .unwrap_or(0);
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
        market_reference.max(good.base_price())
    } else {
        sale.asking_unit_price
    };
    if previous.sold_units == 0 {
        sale.days_without_sales = sale.days_without_sales.saturating_add(1);
    } else {
        sale.days_without_sales = 0;
    }

    let scarce_and_selling = previous.sold_units > 0
        && previous.sold_units >= previous.produced_units.max(1)
        && total_stock <= sale.keep_units.saturating_add(2);
    let stale_surplus = sale.days_without_sales >= 2 && total_stock > sale.keep_units;
    let mut desired = if scarce_and_selling {
        price_step(current, sale.max_daily_price_change_bps, true)
    } else if stale_surplus {
        price_step(current, sale.max_daily_price_change_bps, false)
    } else {
        current
    };

    let sustainable = sustainable_unit_price(
        account.estimated_unit_cost,
        market_fee_bps,
        sale.target_margin_bps,
    );
    if matches!(
        condition,
        BusinessState::New | BusinessState::Operating | BusinessState::CashTight
    ) {
        desired = desired.max(sustainable);
    }
    if matches!(
        condition,
        BusinessState::Distressed | BusinessState::Insolvent
    ) {
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
    halls: Query<(Entity, &shared::components::SettlementId), With<Settlement>>,
    mut markets: Query<&mut MootMarket, With<Settlement>>,
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
        Option<&mut BusinessLiquidation>,
        Option<&BusinessForSale>,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        Option<&shared::components::EmployedAt>,
        &mut Wallet,
        &mut Occupation,
        &mut WorkStatus,
    )>,
    collections: Query<&MarketCollectionRoutine>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    let hall_by_settlement: HashMap<shared::components::SettlementId, Entity> =
        halls.iter().map(|(entity, id)| (*id, entity)).collect();
    let people_by_id: HashMap<shared::components::PersonId, Entity> = villagers
        .iter()
        .map(|(entity, id, ..)| (*id, entity))
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
        liquidation,
        for_sale,
    ) in businesses.iter_mut()
    {
        if super::super::business_output(building.kind).is_none() {
            continue;
        }
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
            sale.collection_enabled = false;
            continue;
        }

        let Some(good) = super::super::business_output(building.kind) else {
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
            if !liquidation.staff_released {
                let workers = workers_by_business
                    .get(building_id)
                    .cloned()
                    .unwrap_or_default();
                let mut worker_ids: Vec<_> = workers
                    .iter()
                    .filter_map(|worker| villagers.get(*worker).ok().map(|(_, id, ..)| *id))
                    .collect();
                worker_ids.sort_unstable();
                worker_ids.dedup();
                if worker_ids.is_empty() {
                    account.defaulted_wages =
                        account.defaulted_wages.saturating_add(account.wage_arrears);
                    account.wage_arrears = 0;
                } else {
                    let each = account.wage_arrears / worker_ids.len() as u64;
                    let remainder = account.wage_arrears % worker_ids.len() as u64;
                    liquidation
                        .wage_claims
                        .extend(worker_ids.iter().enumerate().map(|(index, worker)| {
                            BusinessWageClaim {
                                worker: *worker,
                                pennies: each + u64::from((index as u64) < remainder),
                            }
                        }));
                }
                for worker in workers {
                    if let Ok((_, _, _, _, mut occupation, mut status)) = villagers.get_mut(worker)
                    {
                        occupation.0 = None;
                        *status = WorkStatus::LookingForWork;
                    }
                    commands
                        .entity(worker)
                        .remove::<shared::components::EmployedAt>()
                        .remove::<FarmerRoutine>()
                        .remove::<FishingRoutine>()
                        .remove::<LumberjackRoutine>()
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
                liquidation.staff_released = true;
            }
            condition.liquidation_days = condition.liquidation_days.saturating_add(elapsed);
            sale.collection_enabled = true;
            sale.keep_units = 0;
            sale.max_units_per_collection = sale.max_units_per_collection.max(32);
            sale.minimum_unit_price = good.base_price().saturating_mul(25) / 100;
            sale.asking_unit_price = price_step(
                sale.asking_unit_price.max(good.base_price()),
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
                if claim.pennies == 0 || account.cash == 0 {
                    continue;
                }
                let Some(worker) = people_by_id.get(&claim.worker).copied() else {
                    let written_off = account.write_off_wage_claim(claim.pennies);
                    claim.pennies = claim.pennies.saturating_sub(written_off);
                    continue;
                };
                let payment = claim.pennies.min(account.cash).min(account.wage_arrears);
                if payment == 0 {
                    continue;
                }
                if let Ok((_, _, _, mut wallet, _, _)) = villagers.get_mut(worker) {
                    wallet.credit(payment);
                    let settled = account.pay_wage_claim(payment);
                    claim.pennies = claim.pennies.saturating_sub(settled);
                }
            }
            liquidation.wage_claims.retain(|claim| claim.pennies > 0);

            let physical_units = Good::ALL
                .into_iter()
                .map(|held| inventory.amount(held))
                .fold(0u32, u32::saturating_add);
            let listed_units = market.seller_total_listed_units(seller);
            let in_transit = collections
                .iter()
                .any(|routine| routine.seller == *building_id);
            if physical_units == 0 && listed_units == 0 && !in_transit {
                liquidation.empty_days = liquidation.empty_days.saturating_add(elapsed);
            } else {
                liquidation.empty_days = 0;
            }
            liquidation.last_review_day = day;

            if liquidation.empty_days >= LIQUIDATION_EMPTY_DAYS {
                // No further stock can fund these claims. Record the genuine
                // default instead of keeping a dead firm and its food alive
                // forever. Taxes have already had their ordinary daily chance
                // to collect after the senior wage reserve.
                if account.wage_arrears > 0 {
                    let outstanding_wages = account.wage_arrears;
                    account.write_off_wage_claim(outstanding_wages);
                }
                if account.tax_arrears > 0 {
                    account.defaulted_taxes =
                        account.defaulted_taxes.saturating_add(account.tax_arrears);
                    account.tax_arrears = 0;
                }
                if let Some(owner_entity) =
                    owner.and_then(|owner| people_by_id.get(&owner.0).copied())
                {
                    if account.cash > 0 {
                        if let Ok((_, _, _, mut wallet, _, _)) = villagers.get_mut(owner_entity) {
                            wallet.credit(account.cash);
                            account.cash = 0;
                        }
                    }
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
        let free_cash = account
            .cash
            .saturating_sub(account.wage_arrears)
            .saturating_sub(account.tax_arrears);
        let old_state = condition.state;
        if (account.wage_arrears > 0 || account.tax_arrears > 0) && account.cash == 0 {
            condition.insolvent_days = condition.insolvent_days.saturating_add(elapsed);
            condition.cash_tight_days = condition.cash_tight_days.saturating_add(elapsed);
            condition.state = if condition.insolvent_days >= INSOLVENT_DAYS_BEFORE_CLOSURE {
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
        let market_reference = market.suggested_price(good);
        if management.autopilot {
            sale.target_margin_bps = management.strategy.target_margin_bps();
            sale.max_daily_price_change_bps = management.strategy.daily_price_step_bps();
            review_automatic_price(
                &mut account,
                &mut sale,
                condition.state,
                inventory.amount(good).saturating_add(listed),
                good,
                market_reference,
                market.market_fee_bps(),
            );
            if let Some(procurement) = procurement.as_deref_mut() {
                if procurement.automatic {
                    if let Some(recipe) = processing_recipe(building.kind) {
                        let mut rule = procurement.rule(recipe.input);
                        if rule.enabled {
                            if let Some(maximum) = maximum_viable_input_unit_price(
                                building.kind,
                                sale.asking_unit_price,
                                wage.daily_wage,
                                market.market_fee_bps(),
                                sale.target_margin_bps,
                            ) {
                                rule.maximum_unit_price = maximum;
                                procurement.set_rule(recipe.input, rule);
                            }
                        }
                    }
                }
            }
        }
        sale.last_review_day = day;
        market.reprice(seller, good, sale.asking_unit_price);

        let owner_entity = owner.and_then(|owner| people_by_id.get(&owner.0).copied());
        if condition.state == BusinessState::Insolvent && management.rescue_with_personal_savings {
            if let Some(owner_entity) = owner_entity {
                if let Ok((_, _, _, mut wallet, _, _)) = villagers.get_mut(owner_entity) {
                    let rescue = wallet
                        .balance()
                        .saturating_sub(OWNER_PERSONAL_FLOOR)
                        .min(OWNER_RESCUE_LIMIT);
                    if rescue > 0 && wallet.debit(rescue) {
                        account.contribute_capital(rescue);
                        condition.state = BusinessState::Distressed;
                        condition.insolvent_days = 0;
                    }
                }
            }
        }

        if management.automatic_withdrawals
            && matches!(
                condition.state,
                BusinessState::Operating | BusinessState::CashTight
            )
        {
            if let Some(owner_entity) = owner_entity {
                let procurement = procurement.as_deref().copied().unwrap_or_default();
                let reserve = business_working_capital(
                    building.kind.positions(),
                    wage,
                    management,
                    &procurement,
                    Some(&market),
                )
                .total();
                let draw = account.withdraw_owner(day, management.max_daily_withdrawal, reserve);
                if draw > 0 {
                    if let Ok((_, _, _, mut wallet, _, _)) = villagers.get_mut(owner_entity) {
                        wallet.credit(draw);
                    }
                }
            }
        }

        if condition.state == BusinessState::Liquidating {
            sale.collection_enabled = true;
            sale.keep_units = 0;
            sale.max_units_per_collection = sale.max_units_per_collection.max(32);
            market.reprice(seller, good, sale.asking_unit_price.max(1));
            let workers = workers_by_business
                .get(building_id)
                .cloned()
                .unwrap_or_default();
            let mut worker_ids: Vec<_> = workers
                .iter()
                .filter_map(|worker| villagers.get(*worker).ok().map(|(_, id, ..)| *id))
                .collect();
            worker_ids.sort_unstable();
            worker_ids.dedup();
            let mut claims = Vec::new();
            if worker_ids.is_empty() {
                account.defaulted_wages =
                    account.defaulted_wages.saturating_add(account.wage_arrears);
                account.wage_arrears = 0;
            } else {
                let each = account.wage_arrears / worker_ids.len() as u64;
                let remainder = account.wage_arrears % worker_ids.len() as u64;
                claims.extend(worker_ids.iter().enumerate().map(|(index, worker)| {
                    BusinessWageClaim {
                        worker: *worker,
                        pennies: each + u64::from((index as u64) < remainder),
                    }
                }));
            }
            commands
                .entity(business_entity)
                .insert(BusinessLiquidation::insolvency(day, claims));
            for worker in workers_by_business.get(building_id).into_iter().flatten() {
                if let Ok((_, _, _, _, mut occupation, mut status)) = villagers.get_mut(*worker) {
                    occupation.0 = None;
                    *status = WorkStatus::LookingForWork;
                }
                commands
                    .entity(*worker)
                    .remove::<shared::components::EmployedAt>()
                    .remove::<FarmerRoutine>()
                    .remove::<FishingRoutine>()
                    .remove::<LumberjackRoutine>()
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
            opening,
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
            high,
            500,
        );
        assert!(sale.asking_unit_price < high);
    }
}
