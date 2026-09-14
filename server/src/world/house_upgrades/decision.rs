//! Daily economic snapshots and one amortized owner decision per fixed tick.
//! Existing homeless groups need their own dwelling; they are never treated as
//! lodgers who could be absorbed by extending somebody else's household.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use shared::components::*;
use shared::economy::{Good, GoodsInventory, HouseholdEconomy, MootMarket, Wallet};

use crate::world::village::{HearthState, UnderConstruction, VillagerIntent};

const FORECAST_DAYS: u32 = 2;
const MAX_BEDS: usize = HouseLevel::UpperStorey.housing_capacity() as usize;

#[derive(Default)]
struct Town {
    beds: u32,
    pending_beds: u32,
    admission_beds: u32,
    expected_arrivals: u32,
    food_price: u64,
    wood_price: u64,
    market: Option<Entity>,
    vacant_houses: [u32; MAX_BEDS + 1],
    waiting_groups: Vec<(HouseholdId, usize)>,
    unplaced_groups: HashSet<HouseholdId>,
}

impl Town {
    fn needs_more_admissions(&self) -> bool {
        self.expected_arrivals > self.admission_beds
    }

    fn assign_waiting_groups(&mut self) {
        // Reserve whole available/pending homes for durable groups, largest
        // first, with stable identity breaking ties. Their spare beds can then
        // admit unrelated future newcomers.
        self.waiting_groups
            .sort_unstable_by_key(|(id, size)| (std::cmp::Reverse(*size), *id));
        for &(id, size) in &self.waiting_groups {
            if let Some(capacity) =
                (size..=MAX_BEDS).find(|&capacity| self.vacant_houses[capacity] > 0)
            {
                self.vacant_houses[capacity] -= 1;
                self.admission_beds = self.admission_beds.saturating_add((capacity - size) as u32);
            } else {
                self.unplaced_groups.insert(id);
            }
        }
        for (capacity, houses) in self.vacant_houses.iter().enumerate() {
            self.admission_beds = self
                .admission_beds
                .saturating_add(houses.saturating_mul(capacity as u32));
        }
    }
}

#[derive(Default)]
struct Observation {
    owner: PersonId,
    day: u32,
    wallet: u64,
    need_days: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerUse {
    NewAdmissions,
    DisplacedGroup,
}

#[derive(Clone, Copy)]
struct Candidate {
    house: BuildingId,
    house_entity: Entity,
    owner: PersonId,
    owner_entity: Entity,
    household: HouseholdId,
    account_entity: Entity,
    settlement: SettlementId,
    purpose: OwnerUse,
}

struct Group {
    entity: Entity,
    members: usize,
    settlement: SettlementId,
    dwelling: Option<BuildingId>,
    account: HouseholdEconomy,
}

#[derive(Resource, Default)]
pub struct HouseUpgradeDecisions {
    clock: Option<QueryState<&'static WorldTime>>,
    day: Option<u32>,
    queue: VecDeque<Candidate>,
    towns: BTreeMap<SettlementId, Town>,
    population: BTreeMap<SettlementId, (u32, u32)>,
    observations: HashMap<BuildingId, Observation>,
    last_reviewed: Option<BuildingId>,
}

fn savings_reserve(town: &Town) -> u64 {
    town.food_price
        .saturating_mul(3)
        .saturating_add(town.wood_price)
}

fn food_price(market: Option<&MootMarket>) -> u64 {
    market
        .and_then(|market| {
            market
                .listings()
                .iter()
                .filter(|listing| {
                    listing.units > 0 && Good::HOUSEHOLD_FOOD_PRIORITY.contains(&listing.good)
                })
                .map(|listing| listing.unit_price.max(1))
                .min()
        })
        .unwrap_or_else(|| {
            Good::HOUSEHOLD_FOOD_PRIORITY
                .into_iter()
                .map(Good::base_price)
                .min()
                .unwrap_or(1)
        })
}

fn observe(
    prior: &mut Observation,
    owner: PersonId,
    day: u32,
    wallet: u64,
    reserve: u64,
    need: bool,
) -> bool {
    let consecutive = prior.owner == owner && prior.day.checked_add(1) == Some(day);
    let protected = super::required_escrow_pennies().saturating_add(reserve);
    let ready_savings = wallet >= protected;
    let need_days = if need && ready_savings {
        if consecutive {
            prior.need_days.saturating_add(1)
        } else {
            1
        }
    } else {
        0
    };
    // Daily spending need not make the owner's balance monotonic. Both dated
    // observations must independently leave necessities and escrow funded.
    let ready = need_days >= 2 && consecutive && prior.wallet >= protected;
    *prior = Observation {
        owner,
        day,
        wallet,
        need_days,
    };
    ready
}

fn funded_reserves(
    occupants: usize,
    pantry: &GoodsInventory,
    hearth: Option<&HearthState>,
    account: &HouseholdEconomy,
    market: Option<&MootMarket>,
    require_current_meals: bool,
) -> bool {
    if occupants == 0
        || (require_current_meals && pantry.edible_amount() < occupants as u32)
        || account.fuel_satisfaction < 100
        || account.fuel_shortage_days > 0
    {
        return false;
    }
    let mut food = (occupants as u32)
        .saturating_mul(u32::from(account.pantry_target_days.max(2)))
        .saturating_sub(pantry.edible_amount());
    let fuel = hearth.map_or_else(
        || HearthState::default().deficit(occupants, account.fuel_target_days, pantry),
        |hearth| hearth.deficit(occupants, account.fuel_target_days, pantry),
    );
    if food == 0 && fuel == 0 {
        return true;
    }
    let Some(market) = market else {
        return false;
    };
    let mut budget = account.pennies;
    let mut room = pantry.free_bulk();
    let mut foods = Good::HOUSEHOLD_FOOD_PRIORITY;
    foods.sort_by_key(|good| market.pool(*good).ask);
    for good in foods {
        let trade = market.preview_purchase(
            good,
            food.min(room / good.bulk_per_unit()),
            budget,
            None,
            None,
        );
        food -= trade.units;
        budget -= trade.pennies;
        room -= trade.units * good.bulk_per_unit();
    }
    food == 0
        && fuel <= room / Good::Wood.bulk_per_unit()
        && market
            .preview_purchase(Good::Wood, fuel, budget, None, None)
            .units
            == fuel
}

/// Idle ticks reuse the clock query and allocate nothing. A new day refreshes
/// one linear snapshot; live validation and any route survey happen for at most
/// one ready household per fixed tick. Time warp never drains an obsolete queue.
pub fn review_house_upgrades(world: &mut World) {
    world.resource_scope(|world, mut decisions: Mut<HouseUpgradeDecisions>| {
        let clock = decisions
            .clock
            .get_or_insert_with(|| world.query::<&WorldTime>());
        let Some(day) = clock.iter(world).next().map(|time| time.day) else {
            return;
        };
        if decisions.day != Some(day) {
            decisions.snapshot(world, day);
        }
        let Some(candidate) = decisions.queue.pop_front() else {
            return;
        };
        decisions.last_reviewed = Some(candidate.house);
        let house = candidate.house_entity;
        let owner = candidate.owner_entity;
        if world.get::<BuildingId>(house) != Some(&candidate.house)
            || world.get::<PersonId>(owner) != Some(&candidate.owner)
            || world.get::<OwnedBy>(house) != Some(&OwnedBy(candidate.owner))
            || world.get::<Hero>(owner).is_some()
            || world.get::<Health>(owner).is_none_or(Health::is_dead)
            || world
                .get::<HouseAppearance>(house)
                .is_none_or(|a| a.level != HouseLevel::Ground)
            || world.get::<HouseholdId>(candidate.account_entity) != Some(&candidate.household)
        {
            return;
        }
        let Some(town) = decisions.towns.get(&candidate.settlement) else {
            return;
        };
        if match candidate.purpose {
            OwnerUse::NewAdmissions => !town.needs_more_admissions(),
            OwnerUse::DisplacedGroup => !town.unplaced_groups.contains(&candidate.household),
        } {
            return;
        }
        let Some(wallet) = world.get::<Wallet>(owner).map(|wallet| wallet.balance()) else {
            return;
        };
        let (Some(roster), Some(pantry), Some(account)) = (
            world.get::<Household>(house),
            world.get::<GoodsInventory>(house),
            world.get::<HouseholdEconomy>(candidate.account_entity),
        ) else {
            return;
        };
        let market = town
            .market
            .and_then(|entity| world.get::<MootMarket>(entity));
        let wood_price = market.map_or(Good::Wood.base_price(), |m| m.pool(Good::Wood).ask.max(1));
        let current_reserve = food_price(market)
            .saturating_mul(3)
            .saturating_add(wood_price);
        let Some(members) = world.get::<HouseholdMembers>(candidate.account_entity) else {
            return;
        };
        if members.settlement != candidate.settlement
            || !members.resident_ids.contains(&candidate.owner)
            || world.get::<HouseholdMember>(owner) != Some(&HouseholdMember(candidate.household))
        {
            return;
        }
        let occupants = match candidate.purpose {
            OwnerUse::NewAdmissions => {
                if world.get::<LivesAt>(owner) != Some(&LivesAt(candidate.house))
                    || world.get::<OccupiedByHousehold>(house)
                        != Some(&OccupiedByHousehold(candidate.household))
                    || members.dwelling != Some(candidate.house)
                    || roster.resident_ids.len()
                        < usize::from(HouseLevel::Ground.housing_capacity())
                    || !roster.resident_ids.contains(&candidate.owner)
                {
                    return;
                }
                roster.resident_ids.len()
            }
            OwnerUse::DisplacedGroup => {
                if members.dwelling.is_some()
                    || !(5..=MAX_BEDS).contains(&members.resident_ids.len())
                    || !roster.resident_ids.is_empty()
                    || world.get::<OccupiedByHousehold>(house).is_some()
                    || world.get::<ResidentOf>(owner) != Some(&ResidentOf(candidate.settlement))
                {
                    return;
                }
                members.resident_ids.len()
            }
        };
        if wallet < super::required_escrow_pennies().saturating_add(current_reserve)
            || wood_price > Good::Wood.base_price()
            || !funded_reserves(
                occupants,
                pantry,
                world.get::<HearthState>(house),
                account,
                market,
                candidate.purpose == OwnerUse::NewAdmissions,
            )
            || world
                .get_resource::<super::HouseUpgradeProjects>()
                .is_some_and(|projects| projects.pending_in_settlement(candidate.settlement) > 0)
        {
            return;
        }
        // The explicit service fee buys four beds without another plot or two
        // extra Wood. It is an accepted owner preference, not fictional new-
        // house labor revenue or a claim that the full refundable escrow costs
        // less than every possible self-build at today's materials price.
        if super::request_upgrade(world, candidate.house, candidate.owner).is_ok() {
            let town = decisions.towns.get_mut(&candidate.settlement).unwrap();
            let added = u32::from(
                HouseLevel::UpperStorey.housing_capacity() - HouseLevel::Ground.housing_capacity(),
            );
            town.pending_beds = town.pending_beds.saturating_add(added);
            match candidate.purpose {
                OwnerUse::NewAdmissions => {
                    town.admission_beds = town.admission_beds.saturating_add(added)
                }
                OwnerUse::DisplacedGroup => {
                    town.unplaced_groups.remove(&candidate.household);
                }
            }
        }
    });
}

impl HouseUpgradeDecisions {
    fn snapshot(&mut self, world: &mut World, day: u32) {
        self.day = Some(day);
        self.queue.clear();
        self.towns.clear();
        let mut halls = HashMap::new();
        for (entity, id, settlement, market) in world
            .query::<(Entity, &SettlementId, &Settlement, Option<&MootMarket>)>()
            .iter(world)
        {
            if settlement.tier < SettlementTier::Village {
                continue;
            }
            halls.insert(entity, *id);
            let growth = self
                .population
                .insert(*id, (day, settlement.residents))
                .filter(|(old_day, _)| old_day.checked_add(1) == Some(day))
                .map_or(0, |(_, old)| settlement.residents.saturating_sub(old));
            let food_price = food_price(market);
            self.towns.insert(
                *id,
                Town {
                    expected_arrivals: growth.saturating_mul(FORECAST_DAYS),
                    food_price,
                    wood_price: market
                        .map_or(Good::Wood.base_price(), |m| m.pool(Good::Wood).ask.max(1)),
                    market: market.map(|_| entity),
                    ..default()
                },
            );
        }
        self.population.retain(|id, _| self.towns.contains_key(id));
        let mut arrivals = HashMap::<SettlementId, u32>::new();
        for intent in world.query::<&VillagerIntent>().iter(world) {
            if let VillagerIntent::ArrivingBySea { settlement } = intent {
                if let Some(id) = halls.get(settlement) {
                    let count = arrivals.entry(*id).or_default();
                    *count = count.saturating_add(1);
                }
            }
        }
        for (id, arriving) in arrivals {
            // Bound arrivals are part of the forecast, not an extra copy of it.
            let town = self.towns.get_mut(&id).unwrap();
            town.expected_arrivals = town.expected_arrivals.max(arriving);
        }
        let groups: HashMap<_, _> = world
            .query::<(Entity, &HouseholdId, &HouseholdMembers, &HouseholdEconomy)>()
            .iter(world)
            .map(|(entity, id, members, account)| {
                (
                    *id,
                    Group {
                        entity,
                        members: members.resident_ids.len(),
                        settlement: members.settlement,
                        dwelling: members.dwelling,
                        account: account.clone(),
                    },
                )
            })
            .collect();
        let owners: HashMap<_, _> = world
            .query::<(
                Entity,
                &PersonId,
                &Wallet,
                Option<&LivesAt>,
                &Health,
                Has<Hero>,
                Option<&HouseholdMember>,
                Option<&ResidentOf>,
            )>()
            .iter(world)
            .filter(|(_, _, _, _, health, hero, _, _)| !*hero && !health.is_dead())
            .map(|(entity, id, wallet, home, _, _, group, settlement)| {
                (
                    *id,
                    (
                        entity,
                        wallet.balance(),
                        home.map(|home| home.0),
                        group.map(|group| group.0),
                        settlement.map(|settlement| settlement.0),
                    ),
                )
            })
            .collect();
        let mut candidates = Vec::new();
        let mut live_houses = HashMap::new();
        let mut vacant_houses = HashSet::new();
        for (entity, id, of, appearance, household, owner, occupied, inventory, hearth) in world
            .query::<(
                Entity,
                &BuildingId,
                &BuildingOf,
                &HouseAppearance,
                &Household,
                Option<&OwnedBy>,
                Option<&OccupiedByHousehold>,
                Option<&GoodsInventory>,
                Option<&HearthState>,
            )>()
            .iter(world)
        {
            let Some(town) = self.towns.get_mut(&of.0) else {
                continue;
            };
            let capacity = u32::from(appearance.level.housing_capacity());
            town.beds = town.beds.saturating_add(capacity);
            live_houses.insert(*id, (of.0, capacity));
            let group = occupied
                .and_then(|id| groups.get(&id.0))
                .filter(|group| group.dwelling == Some(*id) && group.settlement == of.0);
            if let Some(group) = group {
                town.admission_beds = town
                    .admission_beds
                    .saturating_add(capacity.saturating_sub(group.members as u32));
            } else {
                town.vacant_houses[capacity as usize] += 1;
                vacant_houses.insert(*id);
            }
            if appearance.level != HouseLevel::Ground {
                continue;
            }
            let (Some(owner), Some(inventory)) = (owner, inventory) else {
                continue;
            };
            let Some(&(owner_entity, wallet, residence, membership, owner_settlement)) =
                owners.get(&owner.0)
            else {
                continue;
            };
            let (group_id, group, purpose, occupants) =
                if let (Some(group), Some(occupied)) = (group, occupied) {
                    if residence != Some(*id)
                        || membership != Some(occupied.0)
                        || !household.resident_ids.contains(&owner.0)
                    {
                        continue;
                    }
                    (
                        occupied.0,
                        group,
                        OwnerUse::NewAdmissions,
                        household.resident_ids.len(),
                    )
                } else {
                    if !household.resident_ids.is_empty()
                        || occupied.is_some()
                        || owner_settlement != Some(of.0)
                    {
                        continue;
                    }
                    let Some(group_id) = membership else {
                        continue;
                    };
                    let Some(group) = groups.get(&group_id) else {
                        continue;
                    };
                    if group.dwelling.is_some()
                        || group.settlement != of.0
                        || !(5..=MAX_BEDS).contains(&group.members)
                        || world
                            .get::<HouseholdMembers>(group.entity)
                            .is_none_or(|members| !members.resident_ids.contains(&owner.0))
                    {
                        continue;
                    }
                    (group_id, group, OwnerUse::DisplacedGroup, group.members)
                };
            let full = occupants >= capacity as usize;
            let provisioned = funded_reserves(
                occupants,
                inventory,
                hearth,
                &group.account,
                town.market
                    .and_then(|entity| world.get::<MootMarket>(entity)),
                purpose == OwnerUse::NewAdmissions,
            );
            candidates.push((
                Candidate {
                    house: *id,
                    house_entity: entity,
                    owner: owner.0,
                    owner_entity,
                    household: group_id,
                    account_entity: group.entity,
                    settlement: of.0,
                    purpose,
                },
                wallet,
                full && provisioned,
            ));
        }
        for (id, group) in &groups {
            if group.members > 0
                && group.members <= MAX_BEDS
                && group
                    .dwelling
                    .is_none_or(|house| !live_houses.contains_key(&house))
            {
                if let Some(town) = self.towns.get_mut(&group.settlement) {
                    town.waiting_groups.push((*id, group.members));
                }
            }
        }
        // Organic worksites carry their settlement on UnderConstruction; they
        // need not already have the completed building's BuildingOf component.
        for (site, appearance) in world
            .query::<(&UnderConstruction, Option<&HouseAppearance>)>()
            .iter(world)
        {
            if let Some(town) = self.towns.get_mut(&site.settlement_id) {
                let capacity = u32::from(site.kind.housing_capacity_with_house(appearance));
                town.pending_beds = town.pending_beds.saturating_add(capacity);
                if capacity > 0 {
                    town.vacant_houses[capacity as usize] += 1;
                }
            }
        }
        let mut pending = HashSet::new();
        for (site, of) in world
            .query::<(&HouseUpgradeWorksite, &BuildingOf)>()
            .iter(world)
        {
            if !pending.insert(site.house) {
                continue;
            }
            if let (Some(town), Some((settlement, current))) =
                (self.towns.get_mut(&of.0), live_houses.get(&site.house))
            {
                if *settlement != of.0 {
                    continue;
                }
                let added =
                    u32::from(site.target.level.housing_capacity()).saturating_sub(*current);
                town.pending_beds = town.pending_beds.saturating_add(added);
                if vacant_houses.contains(&site.house) {
                    town.vacant_houses[*current as usize] =
                        town.vacant_houses[*current as usize].saturating_sub(1);
                    town.vacant_houses[site.target.level.housing_capacity() as usize] += 1;
                } else {
                    town.admission_beds = town.admission_beds.saturating_add(added);
                }
            }
        }
        for town in self.towns.values_mut() {
            town.assign_waiting_groups();
        }
        self.observations
            .retain(|id, _| live_houses.contains_key(id));
        candidates.sort_unstable_by_key(|(candidate, ..)| candidate.house);
        for (candidate, wallet, provisioned) in candidates {
            let town = &self.towns[&candidate.settlement];
            let has_need = match candidate.purpose {
                OwnerUse::NewAdmissions => town.needs_more_admissions(),
                OwnerUse::DisplacedGroup => town.unplaced_groups.contains(&candidate.household),
            };
            let need = provisioned && has_need && town.wood_price <= Good::Wood.base_price();
            if observe(
                self.observations.entry(candidate.house).or_default(),
                candidate.owner,
                day,
                wallet,
                savings_reserve(town),
                need,
            ) {
                self.queue.push_back(candidate);
            }
        }
        if let Some(last) = self.last_reviewed {
            let offset = self
                .queue
                .partition_point(|candidate| candidate.house <= last);
            self.queue.rotate_left(offset);
        }
    }
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod tests;
