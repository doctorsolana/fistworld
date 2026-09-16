//! Autonomous property investment, reviewed once per completed clock day.

use super::*;
use crate::world::village::commerce::payroll_claims::PrivatePayrollClaims;
use crate::world::village::development_market::investment::{InvestmentMarket, RestartPlan};

const TAKEOVER_MARKDOWN_DAYS: u32 = 10;
const TAKEOVER_PAYBACK_DAYS: u64 = 14;

fn marked_down_price(kind: SettlementBuildingKind, listing: &BusinessForSale, day: u32) -> u64 {
    let age = day
        .saturating_sub(listing.listed_day)
        .saturating_sub(PROPERTY_MARKET_EXPOSURE_DAYS);
    let remaining = TAKEOVER_MARKDOWN_DAYS.saturating_sub(age.min(TAKEOVER_MARKDOWN_DAYS));
    listing.asking_price.min(
        takeover_price(kind).saturating_mul(u64::from(remaining))
            / u64::from(TAKEOVER_MARKDOWN_DAYS),
    )
}

#[derive(Clone, Copy)]
struct Buyer {
    entity: Entity,
    id: PersonId,
    cash: u64,
    can_build: bool,
}

fn choose_buyer(
    buyers: &[Buyer],
    previous_owner: PersonId,
    contribution: u64,
    personal_reserve: u64,
    must_build: bool,
    holdings: &HashMap<PersonId, usize>,
    blocked: &HashSet<PersonId>,
) -> Option<Buyer> {
    buyers
        .iter()
        .copied()
        .filter(|buyer| {
            buyer.id != previous_owner
                && !blocked.contains(&buyer.id)
                && (!must_build || buyer.can_build)
                && buyer.cash >= contribution.saturating_add(personal_reserve)
        })
        .min_by_key(|buyer| {
            (
                holdings.get(&buyer.id).copied().unwrap_or(0),
                std::cmp::Reverse(buyer.cash),
                buyer.id,
            )
        })
}

/// Price is a purchase contribution into the reopened firm, not a second
/// payment to a dead owner. Any additional startup cash is posted by the same
/// transaction, and liabilities are never mistaken for usable inherited cash.
fn required_contribution(
    price: u64,
    inherited: u64,
    liabilities: u64,
    plan: RestartPlan,
) -> Option<u64> {
    let contribution = price.max(
        plan.working_cash
            .saturating_add(liabilities)
            .saturating_sub(inherited),
    );
    (contribution <= plan.daily_profit.saturating_mul(TAKEOVER_PAYBACK_DAYS))
        .then_some(contribution)
}

fn service_restart_plan(
    kind: SettlementBuildingKind,
    ledger: shared::economy::BusinessDayLedger,
    previous_wage: u64,
    hiring_wage: u64,
) -> Option<RestartPlan> {
    if !matches!(
        kind,
        SettlementBuildingKind::Tavern | SettlementBuildingKind::StorageHall
    ) || ledger.day == u32::MAX
        || ledger.gross_revenue == 0
        || ledger.profit() <= 0
    {
        return None;
    }
    let staff = ledger
        .wage_expense
        .div_ceil(previous_wage.max(MINIMUM_BUSINESS_DAILY_WAGE))
        .clamp(1, u64::from(kind.positions().max(1)));
    let payroll = hiring_wage.saturating_mul(staff);
    let non_wage_costs = ledger
        .operating_expenses()
        .saturating_sub(ledger.wage_expense);
    let profit = ledger
        .gross_revenue
        .saturating_add(ledger.internal_revenue)
        .saturating_sub(non_wage_costs.saturating_add(payroll));
    (profit > 0).then_some(RestartPlan {
        daily_profit: profit,
        working_cash: payroll
            .saturating_add(ledger.input_expense)
            .saturating_mul(2),
        ..default()
    })
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn acquire_businesses_for_sale(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut last_day: Local<Option<u32>>,
    busy: Query<(), crate::world::village::worker_activity::PermitStartBlocked>,
    employees: Query<&shared::components::EmployedAt>,
    collections: Query<&super::super::MarketCollectionRoutine>,
    deliveries: Query<&super::super::InternalDeliveryRoutine>,
    markets: Query<(&shared::components::SettlementId, &MootMarket)>,
    local_workplaces: Query<
        (
            &shared::components::BuildingOf,
            &SettlementBuilding,
            &BusinessWagePolicy,
            Option<&BusinessCondition>,
        ),
        Without<BusinessForSale>,
    >,
    mut completed: Query<
        (
            Entity,
            &mut BusinessForSale,
            &mut SettlementBuilding,
            &shared::components::BuildingOf,
            &mut BusinessAccount,
            &mut BusinessCondition,
            &mut BusinessSalePolicy,
            Option<&OwnedBy>,
            Option<&GoodsInventory>,
            Option<&mut BusinessWagePolicy>,
            Option<&shared::components::BuildingId>,
            Option<&mut BusinessManagementPolicy>,
            (
                Option<&BusinessLiquidation>,
                Option<&mut PrivatePayrollClaims>,
            ),
        ),
        (With<SettlementBuilding>, Without<UnderConstruction>),
    >,
    mut sites: Query<
        (
            Entity,
            &mut BusinessForSale,
            &mut UnderConstruction,
            Option<&InheritedBusinessCapital>,
            Option<&GoodsInventory>,
            Option<&shared::components::BuildingId>,
        ),
        (With<UnderConstruction>, Without<SettlementBuilding>),
    >,
    active_business_owners: Query<
        (&OwnedBy, &BusinessCondition),
        (
            With<BusinessAccount>,
            With<SettlementBuilding>,
            Without<BusinessForSale>,
        ),
    >,
    mut residents: Query<(
        Entity,
        &PersonId,
        &CharacterName,
        &ResidentOf,
        &mut VillagerIntent,
        &mut Wallet,
        Option<&WorkStatus>,
        Option<&Occupation>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        Option<&Health>,
    )>,
) {
    if completed.is_empty() && sites.is_empty() {
        return;
    }
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *last_day == Some(day) {
        return;
    }
    *last_day = Some(day);

    // A buyer may assume an estate's stock and debts, but cannot change its
    // employer/company while an old employee or shipment still owns the
    // physical handoff. Wait for those existing routines to settle naturally.
    let mut committed_sites: HashSet<_> = employees.iter().map(|job| job.0).collect();
    for trip in &collections {
        committed_sites.insert(trip.seller);
    }
    for trip in &deliveries {
        committed_sites.insert(trip.supplier_id);
        committed_sites.insert(trip.receiver_id);
    }

    let mut markets: HashMap<_, _> = markets
        .iter()
        .map(|(id, market)| (*id, InvestmentMarket::new(market)))
        .collect();
    // Use the same local hiring forecast as new permit investment. A failed
    // property's old offer is not evidence that its next staff can be hired
    // at that price; neither is the founding wage a permanent labour quote.
    let mut local_wages = HashMap::<shared::components::SettlementId, Vec<u64>>::new();
    for (settlement, building, wage, condition) in &local_workplaces {
        if is_private_business(building.kind)
            && condition.is_none_or(|condition| condition.state.accepts_new_workers())
        {
            local_wages
                .entry(settlement.0)
                .or_default()
                .push(wage.daily_wage);
        }
    }
    for (settlement, wages) in &mut local_wages {
        wages.sort_unstable();
        if let Some(market) = markets.get_mut(settlement) {
            market.set_hiring_wage(wages[wages.len() / 2]);
        }
    }
    let mut holdings = HashMap::<PersonId, usize>::new();
    let mut blocked = HashSet::<PersonId>::new();
    for (owner, condition) in active_business_owners.iter() {
        *holdings.entry(owner.0).or_default() += 1;
        if condition.state.blocks_owner_expansion() {
            blocked.insert(owner.0);
        }
    }
    for (_, _, _, _, _, _, _, owner, _, _, _, _, _) in completed.iter() {
        if let Some(owner) = owner {
            *holdings.entry(owner.0).or_default() += 1;
            blocked.insert(owner.0);
        }
    }
    let mut buyers = HashMap::<shared::components::SettlementId, Vec<Buyer>>::new();
    for (entity, id, _, settlement, intent, wallet, status, occupation, employed, civic, health) in
        residents.iter()
    {
        if !intent.counts_as_resident()
            || civic.is_some()
            || health.is_some_and(|health| health.is_dead())
        {
            continue;
        }
        buyers.entry(settlement.0).or_default().push(Buyer {
            entity,
            id: *id,
            cash: wallet.balance(),
            can_build: intent.is_settled()
                && busy.get(entity).is_err()
                && employed.is_none()
                && occupation.is_none_or(|occupation| occupation.0.is_none())
                && status.is_none_or(|status| *status == WorkStatus::LookingForWork),
        });
    }

    // Entity order is not an investment priority. A stable ordering also makes
    // one buyer's single purchase independent of ECS archetype iteration.
    let mut properties: Vec<_> = completed
        .iter()
        .map(|(entity, listing, _, _, _, _, _, _, _, _, id, _, _)| {
            (
                (listing.listed_day, id.map_or(entity.to_bits(), |id| id.0)),
                entity,
            )
        })
        .collect();
    properties.sort_unstable();
    for (_, entity) in properties {
        let Ok((
            business,
            mut listing,
            mut building,
            building_of,
            mut account,
            mut condition,
            mut sale,
            _,
            inventory,
            wage,
            building_id,
            management,
            (liquidation, payroll),
        )) = completed.get_mut(entity)
        else {
            continue;
        };
        if building_id.is_some_and(|id| committed_sites.contains(id)) {
            continue;
        }
        if day.saturating_sub(listing.listed_day) < PROPERTY_MARKET_EXPOSURE_DAYS {
            continue;
        }
        let markdown = marked_down_price(building.kind, &listing, day);
        if listing.asking_price != markdown {
            listing.asking_price = markdown;
        }
        let Some(market) = markets.get(&building_of.0) else {
            continue;
        };
        let previous_wage = wage
            .as_ref()
            .map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage);
        let daily_wage = previous_wage.max(market.hiring_wage());
        let own_sales = account
            .current_day
            .sold_units
            .max(account.previous_day.sold_units);
        let plan = market
            .restart_plan(
                building.kind,
                building.quality,
                daily_wage,
                own_sales,
                inventory,
                None,
            )
            .or_else(|| {
                // Services have no manufactured output recipe. An established
                // inn/depot can use its own paid completed-day business, but a
                // mere building label is not evidence for a service restart.
                service_restart_plan(
                    building.kind,
                    account.previous_day,
                    previous_wage,
                    daily_wage,
                )
            });
        let Some(plan) = plan else {
            continue;
        };
        let Some(contribution) = required_contribution(
            listing.asking_price,
            account.unposted_company_capital,
            account.wage_arrears.saturating_add(account.tax_arrears),
            plan,
        ) else {
            continue;
        };
        let Some(buyer) = choose_buyer(
            buyers.get(&building_of.0).map_or(&[], Vec::as_slice),
            listing.previous_owner,
            contribution,
            market.personal_reserve(),
            false,
            &holdings,
            &blocked,
        ) else {
            continue;
        };
        let Ok((_, _, name, _, _, mut wallet, ..)) = residents.get_mut(buyer.entity) else {
            continue;
        };
        if !wallet.debit(contribution) {
            continue;
        }
        account.contribute_capital(contribution);
        condition.state = reopened_state(&account);
        condition.insolvent_days = 0;
        condition.cash_tight_days = 0;
        condition.opened_day = u32::MAX;
        condition.operating_days = 0;
        condition.liquidation_days = 0;
        sale.collection_enabled = true;
        if plan.asking_price > 0 {
            sale.asking_unit_price = plan.asking_price;
            sale.minimum_unit_price = 1;
            sale.automatic_pricing = true;
        }
        if let Some(mut management) = management {
            management.autopilot = true;
        }
        let reopened_wage = BusinessWagePolicy {
            daily_wage,
            ..default()
        };
        if let Some(mut wage) = wage {
            *wage = reopened_wage;
        } else {
            commands.entity(business).insert(reopened_wage);
        }
        building.owner = Some(name.0.clone());
        // Financing a restart does not erase the employees who earned its
        // inherited arrears. Return the liquidation claims to normal payroll
        // before retiring the liquidation lifecycle marker.
        if let Some(liquidation) =
            liquidation.filter(|liquidation| !liquidation.wage_claims.is_empty())
        {
            if let Some(mut payroll) = payroll {
                for claim in &liquidation.wage_claims {
                    payroll.accrue(claim.worker, claim.pennies);
                }
            } else {
                let mut payroll = PrivatePayrollClaims::default();
                for claim in &liquidation.wage_claims {
                    payroll.accrue(claim.worker, claim.pennies);
                }
                commands.entity(business).insert(payroll);
            }
        }
        commands
            .entity(business)
            .insert(OwnedBy(buyer.id))
            .remove::<BusinessForSale>()
            .remove::<BusinessLiquidation>();
        *holdings.entry(buyer.id).or_default() += 1;
        blocked.insert(buyer.id);
        if let Some(market) = markets.get_mut(&building_of.0) {
            market.reserve_restart(building.kind, plan);
        }
        info!(
            "{} invested {} coin to acquire and restart the {} in '{}'",
            name.0,
            shared::economy::format_money(contribution),
            building.kind.label(),
            building.settlement
        );
    }

    let mut properties: Vec<_> = sites
        .iter()
        .map(|(entity, listing, _, _, _, id)| {
            (
                (listing.listed_day, id.map_or(entity.to_bits(), |id| id.0)),
                entity,
            )
        })
        .collect();
    properties.sort_unstable();
    for (_, entity) in properties {
        let Ok((site_entity, mut listing, mut site, inherited_capital, _inventory, _)) =
            sites.get_mut(entity)
        else {
            continue;
        };
        if day.saturating_sub(listing.listed_day) < PROPERTY_MARKET_EXPOSURE_DAYS {
            continue;
        }
        let markdown = marked_down_price(site.kind, &listing, day);
        if listing.asking_price != markdown {
            listing.asking_price = markdown;
        }
        let Some(market) = markets.get(&site.settlement_id) else {
            continue;
        };
        let Some(plan) =
            market.restart_plan(site.kind, site.quality, market.hiring_wage(), 0, None, None)
        else {
            continue;
        };
        let inherited = inherited_capital.map_or(0, |capital| capital.0);
        let Some(contribution) = required_contribution(listing.asking_price, inherited, 0, plan)
        else {
            continue;
        };
        let Some(buyer) = choose_buyer(
            buyers.get(&site.settlement_id).map_or(&[], Vec::as_slice),
            listing.previous_owner,
            contribution,
            market.personal_reserve(),
            true,
            &holdings,
            &blocked,
        ) else {
            continue;
        };
        let Ok((_, _, name, _, mut intent, mut wallet, ..)) = residents.get_mut(buyer.entity)
        else {
            continue;
        };
        if !wallet.debit(contribution) {
            continue;
        }
        site.owner = Some(name.0.clone());
        site.owner_id = Some(buyer.id);
        site.builder = Some(buyer.entity);
        *intent = VillagerIntent::Building {
            settlement: site.settlement,
            site: site_entity,
        };
        commands
            .entity(site_entity)
            .insert(InheritedBusinessCapital(
                inherited.saturating_add(contribution),
            ))
            .remove::<BusinessForSale>();
        commands.entity(buyer.entity).insert((
            ConstructionMaterialRoutine::new(site_entity),
            CharacterActivity::Idle,
        ));
        *holdings.entry(buyer.id).or_default() += 1;
        blocked.insert(buyer.id);
        if let Some(market) = markets.get_mut(&site.settlement_id) {
            market.reserve_restart(site.kind, plan);
        }
    }
}

#[cfg(test)]
#[path = "takeover_tests.rs"]
mod tests;
