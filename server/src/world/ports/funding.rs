//! Atomic material baskets and finite labour quotes. Quoting never spends.

use bevy::prelude::*;
use shared::components::*;
use shared::economy::*;
use shared::terrain::WorldTerrain;

pub(super) struct MaterialQuote {
    pub(super) market: MootMarket,
    pub(super) stock: GoodsInventory,
    pub(super) supplies: GoodsInventory,
    pub(super) fills: Vec<MarketFill>,
    pub(super) cost: u64,
}

pub(super) fn material_bulk(materials: &[(Good, u32)]) -> Option<u32> {
    materials.iter().try_fold(0u32, |sum, (good, amount)| {
        sum.checked_add(amount.checked_mul(good.bulk_per_unit())?)
    })
}

pub(super) fn quote_materials(
    world: &World,
    hall: Entity,
    materials: &[(Good, u32)],
    budget: u64,
    exclude: Option<MarketSeller>,
) -> Option<MaterialQuote> {
    let mut market = world.get::<MootMarket>(hall)?.clone();
    let mut stock = world.get::<GoodsInventory>(hall)?.clone();
    let mut supplies = GoodsInventory::new(material_bulk(materials)?.max(1));
    let mut fills = Vec::new();
    let mut cost = 0u64;
    for &(good, requested) in materials {
        if requested == 0 {
            continue;
        }
        if !market.can_trade(good) || stock.amount(good) < requested {
            return None;
        }
        let purchase = market.purchase(good, requested, budget.checked_sub(cost)?, None, exclude);
        if purchase.trade.units != requested {
            return None;
        }
        cost = cost.checked_add(purchase.trade.pennies)?;
        if stock.remove(good, requested) != requested || supplies.add(good, requested) != requested
        {
            return None;
        }
        fills.extend(purchase.fills);
    }
    Some(MaterialQuote {
        market,
        stock,
        supplies,
        fills,
        cost,
    })
}

pub(super) fn clock(world: &mut World) -> Option<WorldTime> {
    world.query::<&WorldTime>().iter(world).next().cloned()
}

pub(super) fn hall_for(world: &mut World, id: SettlementId) -> Option<Entity> {
    world
        .query_filtered::<(Entity, &SettlementId), With<Settlement>>()
        .iter(world)
        .find(|(_, candidate)| **candidate == id)
        .map(|(entity, _)| entity)
}

pub(super) fn company_for(world: &mut World, id: CompanyId) -> Option<Entity> {
    world
        .query_filtered::<(Entity, &CompanyId), With<CompanyAccount>>()
        .iter(world)
        .find(|(_, candidate)| **candidate == id)
        .map(|(entity, _)| entity)
}

pub(super) fn hall_pickup(world: &World, hall: Entity) -> Option<Vec3> {
    let position = world.get::<PlayerPosition>(hall)?.0;
    let rotation = world
        .get::<PlayerRotation>(hall)
        .map_or(0.0, |rotation| rotation.0);
    let mut point = SettlementBuildingKind::Hall.entrance_position(position, rotation);
    if let Some(terrain) = world.get_resource::<WorldTerrain>() {
        point.y = terrain.get_height(point.x, point.z);
    }
    Some(point)
}

pub(super) fn daily_wage(world: &World, hall: Entity) -> u64 {
    world
        .get::<MootAdministration>(hall)
        .map_or(MOOT_STEWARD_DAILY_SALARY, |office| {
            office.steward_daily_salary
        })
        .max(1)
}

pub(super) fn labour_quote(clock: &WorldTime, daily_wage: u64, seconds: f64) -> u64 {
    let shift = f64::from(clock.ordinary_shift_seconds());
    ((seconds.max(0.0) / shift.max(1.0)) * daily_wage as f64)
        .ceil()
        .max(1.0) as u64
}

pub(super) fn company_spendable(world: &mut World, company: CompanyId) -> Option<(Entity, u64)> {
    let entity = company_for(world, company)?;
    let account = *world.get::<CompanyAccount>(entity)?;
    let reserve_days = world
        .get::<CompanyManagementPolicy>(entity)
        .map_or(7, |policy| u64::from(policy.payroll_reserve_days));
    // Company finance already consolidates actual liabilities. Add the live
    // staffed payroll runway rather than trusting a just-lowered job target.
    let mut payroll = 0u64;
    let mut sites = world.query::<(&OperatedBy, &BuildingId, &BusinessWagePolicy)>();
    let wages: std::collections::HashMap<_, _> = sites
        .iter(world)
        .filter(|(owner, _, _)| owner.0 == company)
        .map(|(_, id, wage)| (*id, wage.daily_wage))
        .collect();
    for employed in world.query::<&EmployedAt>().iter(world) {
        payroll = payroll.saturating_add(wages.get(&employed.0).copied().unwrap_or(0));
    }
    let protected = account
        .wage_arrears
        .saturating_add(account.tax_arrears)
        .saturating_add(payroll.saturating_mul(reserve_days))
        .saturating_add(2 * PENNIES_PER_COIN);
    Some((entity, account.cash.saturating_sub(protected)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ship_material_basket_requires_real_iron_and_never_partially_spends() {
        let mut world = World::new();
        let materials = ShipKind::Coaster.materials();
        let seller = MarketSeller::Person(PersonId(42));
        let mut market = MootMarket::founding();
        market.unlock_trade_tier(MarketTradeTier::PavedMarketplace);
        let mut stock = GoodsInventory::new(2000);
        for (good, units) in materials {
            if good == Good::Iron {
                continue;
            }
            stock.add(good, units);
            market.consign(seller, good, units, good.base_price());
        }
        let hall = world.spawn((market, stock)).id();
        assert!(quote_materials(&world, hall, &materials, 1_000_000, None).is_none());
        assert_eq!(
            world
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood),
            48
        );
        assert_eq!(
            world
                .get::<MootMarket>(hall)
                .unwrap()
                .listed_units(Good::Wood),
            48
        );
        world
            .get_mut::<GoodsInventory>(hall)
            .unwrap()
            .add(Good::Iron, 8);
        // Physical but unowned/unlisted iron is not permission to seize it.
        assert!(quote_materials(&world, hall, &materials, 1_000_000, None).is_none());
        world.get_mut::<MootMarket>(hall).unwrap().consign(
            seller,
            Good::Iron,
            8,
            Good::Iron.base_price(),
        );
        let quote = quote_materials(&world, hall, &materials, 1_000_000, None).unwrap();
        assert!(quote.cost > 0);
        assert!(quote_materials(&world, hall, &materials, quote.cost - 1, None).is_none());
        for (good, units) in materials {
            assert_eq!(quote.supplies.amount(good), units);
            assert_eq!(quote.stock.amount(good), 0);
            assert_eq!(
                world.get::<GoodsInventory>(hall).unwrap().amount(good),
                units,
                "quote must leave the authoritative stock untouched"
            );
        }
        assert_eq!(
            quote.fills.iter().map(|fill| fill.gross).sum::<u64>(),
            quote.cost
        );
    }

    #[test]
    fn hull_investment_keeps_actual_staff_wages_and_liabilities_reserved() {
        let mut world = World::new();
        let company = CompanyId(7);
        world.spawn((
            company,
            CompanyAccount {
                cash: 10_000,
                wage_arrears: 300,
                tax_arrears: 200,
                ..default()
            },
            CompanyManagementPolicy {
                payroll_reserve_days: 3,
                ..default()
            },
        ));
        let site = BuildingId(9);
        world.spawn((
            OperatedBy(company),
            site,
            BusinessWagePolicy {
                daily_wage: 200,
                ..default()
            },
        ));
        world.spawn(EmployedAt(site));
        world.spawn(EmployedAt(site));
        let (_, available) = company_spendable(&mut world, company).unwrap();
        assert_eq!(
            available,
            10_000 - 300 - 200 - 2 * 200 * 3 - 2 * PENNIES_PER_COIN
        );
    }
    #[test]
    fn finite_labour_quote_uses_the_same_shift_as_the_world_clock() {
        for clock in [WorldTime::new_default(), WorldTime::new(180., 60., 0.)] {
            assert_eq!(
                labour_quote(&clock, 180, f64::from(clock.ordinary_shift_seconds())),
                180
            );
            assert_eq!(
                labour_quote(&clock, 180, f64::from(clock.ordinary_shift_seconds()) * 0.5),
                90
            );
        }
    }
}
