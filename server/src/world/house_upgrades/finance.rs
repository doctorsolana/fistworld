//! Project escrow, funded market claims and lossless refunds.
use super::labor::release_worker;
use super::lifecycle::alive;
use super::project::*;
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{Good, GoodsInventory, HouseholdEconomy, MarketSeller, MootMarket, Wallet};
const RETRY_SECONDS: f64 = 15.0;

pub(super) fn material_budget(project: &UpgradeProject) -> u64 {
    project
        .escrow
        .saturating_sub(HOUSE_UPGRADE_BUILDER_FEE_PENNIES.saturating_sub(project.paid_labor))
}

pub(super) fn affordable_batch(world: &World, project: &UpgradeProject) -> u32 {
    let (Some(market), Some(stock)) = (
        world.get::<MootMarket>(project.hall),
        world.get::<GoodsInventory>(project.hall),
    ) else {
        return 0;
    };
    let amount = HOUSE_UPGRADE_WOOD_REQUIRED
        .saturating_sub(project.delivered + project.cargo.amount(Good::Wood))
        .min(shared::economy::capacity::VILLAGER / Good::Wood.bulk_per_unit())
        .min(stock.amount(Good::Wood));
    market
        .preview_purchase(
            Good::Wood,
            amount,
            material_budget(project),
            Some(Good::Wood.base_price()),
            None,
        )
        .units
}

pub(super) fn withdraw_claim(world: &mut World, project: &mut UpgradeProject) {
    if let Some(mut market) = world.get_mut::<MootMarket>(project.hall) {
        if project.claim.epoch == Some(market.demand_epoch()) {
            market.withdraw_unmet_demand(
                Good::Wood,
                project.claim.unavailable,
                project.claim.unaffordable,
                project.claim.funded,
                Good::Wood.base_price(),
            );
        }
    }
    project.claim = DemandClaim::default();
}

pub(super) fn refresh_claim(world: &mut World, project: &mut UpgradeProject) {
    withdraw_claim(world, project);
    let remaining = HOUSE_UPGRADE_WOOD_REQUIRED
        .saturating_sub(project.delivered + project.cargo.amount(Good::Wood));
    let physical = world
        .get::<GoodsInventory>(project.hall)
        .map_or(0, |stock| stock.amount(Good::Wood));
    let mut market = world
        .get_mut::<MootMarket>(project.hall)
        .expect("live market");
    if !market.can_trade(Good::Wood) {
        return;
    }
    let available = physical.min(market.listed_units(Good::Wood));
    let unavailable = remaining.saturating_sub(available);
    let preview = market.preview_purchase(
        Good::Wood,
        remaining.min(available),
        material_budget(project),
        Some(Good::Wood.base_price()),
        None,
    );
    // Carrying six of eight affordable bundles is a second trip, never an
    // affordability failure. Claims describe the entire remaining contract.
    let unaffordable = remaining
        .saturating_sub(unavailable)
        .saturating_sub(preview.units);
    let funded = unavailable.min(
        material_budget(project)
            .saturating_sub(preview.pennies)
            .div_euclid(Good::Wood.base_price()) as u32,
    );
    market.record_unmet_demand(
        Good::Wood,
        unavailable,
        unaffordable,
        funded,
        Good::Wood.base_price(),
    );
    project.claim = DemandClaim {
        epoch: Some(market.demand_epoch()),
        unavailable,
        unaffordable,
        funded,
    };
}

pub(super) fn begin_refund(world: &mut World, project: &mut UpgradeProject) {
    if project.phase == Phase::Refunding {
        return;
    }
    info!(
        "HOUSE_UPGRADE refund house={} cash={} delivered={} transit={}",
        project.house.0,
        project.escrow,
        project.delivered,
        project.cargo.amount(Good::Wood)
    );
    withdraw_claim(world, project);
    release_worker(world, project);
    let recovered = if let Some(mut pile) = world.get_mut::<GoodsInventory>(project.worksite) {
        pile.remove(Good::Wood, project.delivered)
    } else {
        project.delivered
    };
    let stored = project.cargo.add(Good::Wood, recovered);
    debug_assert_eq!(stored, recovered);
    project.delivered = 0;
    if let Ok(entity) = world.get_entity_mut(project.worksite) {
        entity.despawn();
    }
    project.phase = Phase::Refunding;
    project.next_attempt = project.last_time + RETRY_SECONDS;
}

/// Same estate order as ordinary mortality: the surviving stable household,
/// then the settlement. The remembered ID survives destruction of the body.
fn surviving_household(
    world: &mut World,
    project: &UpgradeProject,
) -> Option<(Entity, HouseholdId, Option<BuildingId>)> {
    let id = project.estate_household?;
    world
        .query::<(Entity, &HouseholdId, &HouseholdMembers, &HouseholdEconomy)>()
        .iter(world)
        .find(|(_, candidate, members, _)| {
            **candidate == id
                && members
                    .resident_ids
                    .iter()
                    .any(|member| *member != project.owner)
        })
        .map(|(entity, _, members, _)| (entity, id, members.dwelling))
}

pub(super) fn settle_refund(world: &mut World, project: &mut UpgradeProject) -> bool {
    let owner_alive = alive(world, project.owner_entity)
        && world.get::<PersonId>(project.owner_entity) == Some(&project.owner);
    let household = (!owner_alive)
        .then(|| surviving_household(world, project))
        .flatten();
    if project.escrow > 0 {
        if owner_alive {
            if let Some(mut wallet) = world.get_mut::<Wallet>(project.owner_entity) {
                wallet.credit(project.escrow);
                project.escrow = 0;
            }
        } else if let Some((entity, _, _)) = household {
            let mut estate = world
                .get_mut::<HouseholdEconomy>(entity)
                .expect("surviving estate");
            estate.pennies = estate.pennies.saturating_add(project.escrow);
            project.escrow = 0;
        } else if let Some(mut hall) = world.get_mut::<Settlement>(project.hall) {
            hall.treasury = hall.treasury.saturating_add(project.escrow);
            project.escrow = 0;
        }
    }
    let mut remaining = project.cargo.amount(Good::Wood);
    let pantry = if owner_alive
        && world.get::<OwnedBy>(project.home).map(|owner| owner.0) == Some(project.owner)
    {
        Some(project.home)
    } else if let Some((_, id, Some(dwelling))) = household {
        world
            .query::<(Entity, &BuildingId, &OccupiedByHousehold)>()
            .iter(world)
            .find_map(|(entity, building, occupant)| {
                (*building == dwelling && occupant.0 == id).then_some(entity)
            })
    } else {
        None
    };
    if let Some(home) = pantry {
        if let Some(mut pantry) = world.get_mut::<GoodsInventory>(home) {
            let restored = pantry.add(Good::Wood, remaining);
            project.cargo.remove(Good::Wood, restored);
            remaining -= restored;
        }
    }
    if remaining > 0
        && world
            .get::<MootMarket>(project.hall)
            .is_some_and(|market| market.can_trade(Good::Wood))
    {
        if let Some(mut hall) = world.get_mut::<GoodsInventory>(project.hall) {
            let restored = hall.add(Good::Wood, remaining);
            project.cargo.remove(Good::Wood, restored);
            let seller = if owner_alive {
                MarketSeller::Person(project.owner)
            } else if let Some((_, id, _)) = household {
                MarketSeller::Household(id)
            } else {
                MarketSeller::Treasury(project.settlement)
            };
            world
                .get_mut::<MootMarket>(project.hall)
                .expect("market")
                .consign(seller, Good::Wood, restored, Good::Wood.base_price());
        }
    }
    // Full/missing stores retain an explicit refund liability, never destroy
    // cargo or force it into an over-capacity inventory. No worker stays bound.
    project.escrow == 0 && project.cargo.amount(Good::Wood) == 0
}
