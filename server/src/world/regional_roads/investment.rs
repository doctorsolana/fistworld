//! Bounded survey and purchase approval. Promised commerce cannot spend money.

use super::*;
use crate::world::village::{BusinessEventQueue, VillagerIntent, worker_activity};
use shared::components::*;
use shared::economy::{CivicAccount, Good, GoodsInventory, MarketSeller, MootMarket, Wallet};
use std::collections::BTreeSet;

const MAX_ACTIVE_PROJECTS: usize = 2;
const PAYBACK_DAYS: f64 = 30.0;

struct PendingSurvey {
    pair: SettlementPair,
    corridor: Vec<Vec2>,
    steps: Option<Vec<RegionalStep>>,
    checked: usize,
    trips_per_day: f64,
    selected_day: u32,
}

#[derive(Resource, Default)]
pub(crate) struct RegionalInfrastructure {
    last_review_day: Option<u32>,
    next_pair: usize,
    pending: Option<PendingSurvey>,
    pub(super) active: Vec<Entity>,
    /// Durable successful pair ownership prevents funding the same corridor
    /// again just because the old trade observations expired.
    pub(super) completed: BTreeSet<SettlementPair>,
}

/// One pair is nominated per game day. Its retained survey warms at most one
/// procedural chunk and validates at most one <=128 m section per update.
/// Approval reserves the complete finite contract, never wages from the future.
pub(crate) fn review_regional_investment(
    world: &mut World,
    mut clock_query: Local<Option<bevy::ecs::query::QueryState<&'static WorldTime>>>,
) {
    let clocks = clock_query.get_or_insert_with(|| world.query::<&WorldTime>());
    let Some(clock) = clocks.iter(world).next().cloned() else {
        return;
    };
    let Some(mut state) = world.remove_resource::<RegionalInfrastructure>() else {
        return;
    };
    if let Some(mut survey) = state.pending.take() {
        // A proposal cannot reserve scarce work indefinitely after its evidence
        // has gone stale or another world clock has replaced this one.
        if clock.day.saturating_sub(survey.selected_day) >= super::evidence::EVIDENCE_DAYS {
            world.insert_resource(state);
            return;
        }
        if survey.steps.is_none() {
            survey.steps = bridge::plan_shortcuts(world, &survey.corridor);
            if survey.steps.is_none() {
                state.pending = Some(survey);
                world.insert_resource(state);
                return;
            }
        }
        let steps = survey.steps.as_ref().unwrap();
        if steps.is_empty() {
            world.insert_resource(state);
            return;
        }
        if let Some(step) = steps.get(survey.checked) {
            let accepted = match step {
                RegionalStep::Dirt(points) => {
                    crate::world::village_roads::regional_section_clear(world, points, 2.6)
                }
                RegionalStep::Bridge(deck) => Some(deck.valid()),
            };
            match accepted {
                None => {
                    state.pending = Some(survey);
                }
                Some(false) => {}
                Some(true) => {
                    survey.checked += 1;
                    state.pending = Some(survey);
                }
            }
        } else if let Some(project) = approve(world, &clock, survey) {
            state.active.push(project);
        }
        world.insert_resource(state);
        return;
    }
    if state.last_review_day == Some(clock.day)
        || state.active.len() >= MAX_ACTIVE_PROJECTS
        || !clock.is_ordinary_work_time()
    {
        world.insert_resource(state);
        return;
    }
    state.last_review_day = Some(clock.day);
    if let Some(mut traffic) = world.get_resource_mut::<RegionalRoadTraffic>() {
        traffic.expire(clock.day);
    }
    let busy: BTreeSet<_> = state
        .active
        .iter()
        .filter_map(|entity| {
            world
                .get::<RegionalProject>(*entity)
                .map(|project| project.pair)
        })
        .collect();
    if let Some(traffic) = world.get_resource::<RegionalRoadTraffic>() {
        let eligible: Vec<_> = traffic
            .pairs
            .iter()
            .filter(|(pair, flow)| {
                flow.trips() >= 2
                    && flow.units() >= 4
                    && flow.corridor.len() >= 2
                    && !busy.contains(pair)
                    && !state.completed.contains(pair)
            })
            .collect();
        if !eligible.is_empty() {
            let (pair, flow) = eligible[state.next_pair % eligible.len()];
            state.next_pair = state.next_pair.wrapping_add(1);
            state.pending = Some(PendingSurvey {
                pair: *pair,
                corridor: flow.corridor.clone(),
                steps: None,
                checked: 0,
                trips_per_day: flow.trips() as f64 / f64::from(flow.observed_days(clock.day)),
                selected_day: clock.day,
            });
        }
    }
    world.insert_resource(state);
}

struct Funding {
    hall: Entity,
    id: SettlementId,
    name: String,
    pickup: Vec3,
    daily_wage: u64,
    budget: u64,
}

fn approve(world: &mut World, clock: &WorldTime, survey: PendingSurvey) -> Option<Entity> {
    let steps = survey.steps?;
    let old_length: f32 = survey
        .corridor
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum();
    let new_length: f32 = steps.iter().map(RegionalStep::length).sum();
    if !old_length.is_finite() || new_length < 8.0 {
        return None;
    }
    let mut halls = world.query::<(
        Entity,
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&MootAdministration>,
        Option<&SettlementPolicies>,
    )>();
    let mut funders: Vec<_> = halls
        .iter(world)
        .filter(|(_, id, ..)| **id == survey.pair.0 || **id == survey.pair.1)
        .map(
            |(hall, id, settlement, position, rotation, office, policy)| Funding {
                hall,
                id: *id,
                name: settlement.name.clone(),
                pickup: SettlementBuildingKind::Hall
                    .entrance_position(position.0, rotation.map_or(0.0, |r| r.0)),
                daily_wage: office
                    .map_or(shared::economy::MOOT_STEWARD_DAILY_SALARY, |office| {
                        office.steward_daily_salary
                    })
                    .max(1),
                budget: crate::world::village::civic::civic_discretionary_budget(
                    settlement, office, policy,
                ),
            },
        )
        .collect();
    funders.sort_by_key(|funding| (std::cmp::Reverse(funding.budget), funding.id));
    let material_amounts = steps.iter().fold([0u32; 2], |mut amounts, step| {
        if let RegionalStep::Bridge(deck) = step {
            amounts[0] = amounts[0].saturating_add(deck.wood_required());
            amounts[1] = amounts[1].saturating_add(deck.stone_required());
        }
        amounts
    });
    for funding in funders {
        let work = steps.iter().map(RegionalStep::work_units).sum::<u64>();
        // Piecework quote includes ordinary foot travel and two passes of
        // laying/clearing effort. Payback values only observed journeys, using
        // the public local wage as the opportunity cost of time saved.
        let work_seconds =
            new_length as f64 / f64::from(shared::player::HERO_MOVE_SPEED) + work as f64 * 2.0;
        let wages = (work_seconds / f64::from(clock.ordinary_shift_seconds().max(1.0))
            * funding.daily_wage as f64)
            .ceil()
            .max(1.0) as u64;
        let saved_seconds = (f64::from(old_length)
            - f64::from(new_length)
                / f64::from(crate::world::village_roads::ROAD_SPEED_MULTIPLIER))
        .max(0.0)
            / f64::from(shared::player::HERO_MOVE_SPEED);
        let benefit = saved_seconds * survey.trips_per_day * PAYBACK_DAYS
            / f64::from(clock.ordinary_shift_seconds().max(1.0))
            * funding.daily_wage as f64;
        if funding.budget < wages {
            continue;
        }
        let Some((market, inventory, supplies, fills, materials_cost)) =
            quote_materials(world, &funding, material_amounts, funding.budget - wages)
        else {
            continue;
        };
        if benefit < wages.saturating_add(materials_cost) as f64 {
            continue;
        }
        let Some((worker, person, name)) =
            select_worker(world, funding.hall, funding.pickup, wages)
        else {
            continue;
        };
        // Everything was priced against cloned title/physical books. Commit
        // the matching books together only after full funding and a worker.
        let total = wages.checked_add(materials_cost)?;
        let Some(mut settlement) = world.get_mut::<Settlement>(funding.hall) else {
            continue;
        };
        if settlement.treasury < total {
            continue;
        }
        settlement.treasury -= total;
        drop(settlement);
        world.entity_mut(funding.hall).insert((market, inventory));
        if let Some(mut account) = world.get_mut::<CivicAccount>(funding.hall) {
            account.record_material_expense(clock.day.saturating_add(1), materials_cost);
        }
        world
            .resource_mut::<BusinessEventQueue>()
            .record_market_purchase(clock.day, funding.id, fills);
        let project = super::projects::start_project(
            world,
            survey.pair,
            funding.hall,
            funding.id,
            funding.name,
            funding.pickup,
            worker,
            person,
            name,
            steps,
            supplies,
            wages,
            clock.day,
        );
        return Some(project);
    }
    None
}

/// A quote mutates cloned order and physical books. A partial material basket
/// cannot debit cash, erase somebody's consignment, or start an unfunded bridge.
fn quote_materials(
    world: &World,
    funding: &Funding,
    amounts: [u32; 2],
    budget: u64,
) -> Option<(
    MootMarket,
    GoodsInventory,
    GoodsInventory,
    Vec<shared::economy::MarketFill>,
    u64,
)> {
    let mut market = world.get::<MootMarket>(funding.hall)?.clone();
    let mut inventory = world.get::<GoodsInventory>(funding.hall)?.clone();
    let capacity = amounts[0]
        .checked_mul(Good::Wood.bulk_per_unit())?
        .checked_add(amounts[1].checked_mul(Good::Stone.bulk_per_unit())?)?;
    let mut supplies = GoodsInventory::new(capacity.max(1));
    let mut cost = 0u64;
    let mut fills = Vec::new();
    for (good, requested) in [Good::Wood, Good::Stone].into_iter().zip(amounts) {
        if requested == 0 {
            continue;
        }
        let purchase = market.purchase(
            good,
            requested,
            budget.saturating_sub(cost),
            None,
            Some(MarketSeller::Treasury(funding.id)),
        );
        if purchase.trade.units != requested || inventory.amount(good) < requested {
            return None;
        }
        assert_eq!(inventory.remove(good, requested), requested);
        assert_eq!(supplies.add(good, requested), requested);
        cost = cost.checked_add(purchase.trade.pennies)?;
        fills.extend(purchase.fills);
    }
    Some((market, inventory, supplies, fills, cost))
}

fn select_worker(
    world: &mut World,
    hall: Entity,
    pickup: Vec3,
    wages: u64,
) -> Option<(Entity, PersonId, String)> {
    let mut blocked = world.query_filtered::<Entity, worker_activity::JobChangeBlocked>();
    let blocked: BTreeSet<_> = blocked.iter(world).collect();
    let mut people = world.query_filtered::<(
        Entity,
        &PersonId,
        &CharacterName,
        &VillagerIntent,
        &Occupation,
        &GoodsInventory,
        &Wallet,
        &PlayerPosition,
    ), (
        With<CharacterKind>,
        Without<Hero>,
        Without<crate::player::hero::MoveTarget>,
        Without<crate::world::village_roads::TravelRoute>,
        Without<crate::world::village_roads::NavigationRoutePending>,
        Without<CivicEmployment>,
        Without<EmployedAt>,
    )>();
    people.iter(world).filter(|(entity, person, _, intent, occupation, goods, wallet, _)| {
        person.is_assigned() && matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
            && occupation.0.is_none() && goods.is_empty() && wallet.balance().checked_add(wages).is_some() && !blocked.contains(entity)
    }).min_by(|a,b| a.7.0.distance_squared(pickup).total_cmp(&b.7.0.distance_squared(pickup)).then_with(|| a.1.cmp(b.1)))
        .map(|(entity, person, name, ..)| (entity, *person, name.0.clone()))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_flow;
