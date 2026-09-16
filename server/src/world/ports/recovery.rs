//! Cancelled hull stock returns through an existing access point to the same
//! town market. Consignment transfers title, never pays for the goods twice.

use super::construction::PortConstructionStock;
use super::funding;
use bevy::prelude::*;
use shared::{components::*, economy::*};

#[derive(Clone, Copy)]
pub(super) struct CancelledSupplies {
    pub order: Entity,
    pub hall: Entity,
    pub port: Entity,
    pub company: CompanyId,
    pub source: Entity,
    pub destination: Entity,
}

/// Called only after every hauling claim has completed its physical return.
/// A full market or unavailable warehouse leaves exact stock/basis in place.
/// The project coordinator bounds retries to once per thirty world seconds.
pub(super) fn reclaim(world: &mut World, recovery: CancelledSupplies) -> bool {
    let Some(town) = world.get::<SettlementId>(recovery.hall).copied() else {
        return false;
    };
    let Some(pickup) = funding::hall_pickup(world, recovery.hall) else {
        return false;
    };
    let warehouse = world
        .query::<(
            Entity,
            &BuildingId,
            &BuildingOf,
            &OperatedBy,
            &SettlementBuilding,
            &BusinessAccount,
        )>()
        .iter(world)
        .filter(|(_, _, location, owner, building, _)| {
            location.0 == town
                && owner.0 == recovery.company
                && building.kind == SettlementBuildingKind::StorageHall
        })
        .min_by_key(|(_, id, ..)| id.0)
        .map(|(entity, id, ..)| (entity, *id));
    let Some((warehouse, seller)) = warehouse else {
        return false;
    };
    if world.get::<MootMarket>(recovery.hall).is_none()
        || world.get::<GoodsInventory>(recovery.hall).is_none()
    {
        return false;
    }
    let shore = world
        .get::<SettlementPort>(recovery.port)
        .filter(|port| port.built && port.settlement == town)
        .map(|port| port.geometry.shore);
    let owner = PortCargoOwner::Company(recovery.company);
    for (index, pile) in [recovery.source, recovery.destination]
        .into_iter()
        .enumerate()
    {
        if world
            .get::<GoodsInventory>(pile)
            .is_some_and(GoodsInventory::is_empty)
        {
            continue;
        }
        let access = if index == 0 { Some(pickup) } else { shore };
        if world
            .get::<PortConstructionStock>(pile)
            .is_none_or(|title| title.0 != owner)
            || access.is_none_or(|access| {
                world
                    .get::<PlayerPosition>(pile)
                    .is_none_or(|position| position.0.distance_squared(access) > 1.0)
            })
        {
            continue;
        }
        for good in Good::ALL {
            let stock = world
                .get::<GoodsInventory>(pile)
                .map_or(0, |stock| stock.amount(good));
            if stock == 0
                || !world
                    .get::<MootMarket>(recovery.hall)
                    .unwrap()
                    .can_trade(good)
            {
                continue;
            }
            let price = world
                .get::<MootMarket>(recovery.hall)
                .unwrap()
                .pool(good)
                .ask
                .max(1);
            let mut stores = world.query::<&mut GoodsInventory>();
            let Ok([mut source, mut destination]) =
                stores.get_many_mut(world, [pile, recovery.hall])
            else {
                continue;
            };
            let moved = source.transfer_to(&mut destination, good, stock);
            world.get_mut::<MootMarket>(recovery.hall).unwrap().consign(
                MarketSeller::Business(seller),
                good,
                moved,
                price,
            );
        }
    }
    if [recovery.source, recovery.destination]
        .into_iter()
        .any(|pile| {
            world
                .get::<GoodsInventory>(pile)
                .is_none_or(|stock| !stock.is_empty())
        })
    {
        return false;
    }
    let basis = world
        .get::<BusinessAccount>(recovery.order)
        .map_or(0, |account| account.book_value);
    let Some(value) = world
        .get::<BusinessAccount>(warehouse)
        .unwrap()
        .book_value
        .checked_add(basis)
    else {
        return false;
    };
    world
        .get_mut::<BusinessAccount>(warehouse)
        .unwrap()
        .book_value = value;
    if let Some(mut account) = world.get_mut::<BusinessAccount>(recovery.order) {
        // Historical capital expenditure remains in the original cost centre;
        // only its asset basis moves. Consignment itself creates no revenue.
        account.book_value = 0;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_hull_reclaims_partial_stores_without_cash_or_duplicate_basis() {
        let mut world = World::new();
        let town = SettlementId(8);
        let company = CompanyId(2);
        let mut stock = GoodsInventory::new(8); // only two Wood bundles
        stock.add(Good::Wood, 1);
        let mut market = MootMarket::founding();
        market.unlock_trade_tier(MarketTradeTier::PavedMarketplace);
        market.consign(MarketSeller::Treasury(town), Good::Wood, 1, 70);
        let hall = world
            .spawn((
                town,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.),
                stock,
                market,
            ))
            .id();
        let pickup = funding::hall_pickup(&world, hall).unwrap();
        let shore = Vec3::X * 30.;
        let port = world
            .spawn(SettlementPort {
                settlement: town,
                built: true,
                geometry: PortGeometry {
                    shore,
                    pier_end: shore + Vec3::Z * 20.,
                    berth: shore + Vec3::Z * 26.,
                    departure: shore + Vec3::Z * 26. + Vec3::X * 15.,
                    yaw: 0.,
                    maximum_ship: ShipKind::Coaster,
                },
            })
            .id();
        let mut goods = GoodsInventory::new(20);
        goods.add(Good::Wood, 2);
        let source = world
            .spawn((
                goods.clone(),
                PlayerPosition(pickup),
                PortConstructionStock(PortCargoOwner::Company(company)),
            ))
            .id();
        let destination = world
            .spawn((
                goods,
                PlayerPosition(shore),
                PortConstructionStock(PortCargoOwner::Company(company)),
            ))
            .id();
        let warehouse_id = BuildingId(4);
        let warehouse = world
            .spawn((
                warehouse_id,
                BuildingOf(town),
                OperatedBy(company),
                SettlementBuilding {
                    kind: SettlementBuildingKind::StorageHall,
                    settlement: "Haven".into(),
                    owner: None,
                    quality: 1.,
                    workers: Vec::new(),
                },
                BusinessAccount {
                    book_value: 9,
                    ..default()
                },
            ))
            .id();
        let order = world
            .spawn(BusinessAccount {
                book_value: 100,
                capital_expenditures: 100,
                ..default()
            })
            .id();
        let cash = world
            .spawn(CompanyAccount {
                cash: 77,
                ..default()
            })
            .id();
        let recovery = CancelledSupplies {
            order,
            hall,
            port,
            company,
            source,
            destination,
        };
        assert!(!reclaim(&mut world, recovery));
        assert_eq!(
            world
                .get::<GoodsInventory>(source)
                .unwrap()
                .amount(Good::Wood),
            1
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(destination)
                .unwrap()
                .amount(Good::Wood),
            2
        );
        assert_eq!(
            world
                .get::<MootMarket>(hall)
                .unwrap()
                .seller_listed_units(MarketSeller::Business(warehouse_id), Good::Wood),
            1
        );
        assert!(
            !reclaim(&mut world, recovery),
            "full store must hold the remainder"
        );
        assert_eq!(world.get::<BusinessAccount>(order).unwrap().book_value, 100);
        // Expanding a store is the fixture's explicit event; recovery then moves
        // the remainder through the same real capacity/title transaction.
        let retained = world
            .get::<GoodsInventory>(hall)
            .unwrap()
            .amount(Good::Wood);
        let mut expanded = GoodsInventory::new(24);
        expanded.add(Good::Wood, retained);
        world.entity_mut(hall).insert(expanded);
        world.get_mut::<SettlementPort>(port).unwrap().built = false;
        assert!(
            !reclaim(&mut world, recovery),
            "an unfinished pier is not a shared-market access point"
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(destination)
                .unwrap()
                .amount(Good::Wood),
            2
        );
        world.get_mut::<SettlementPort>(port).unwrap().built = true;
        world.get_mut::<PlayerPosition>(destination).unwrap().0 = shore + Vec3::X * 2.;
        assert!(
            !reclaim(&mut world, recovery),
            "stock away from the port cannot use its market access"
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(destination)
                .unwrap()
                .amount(Good::Wood),
            2
        );
        world.get_mut::<PlayerPosition>(destination).unwrap().0 = shore;
        assert!(reclaim(&mut world, recovery));
        assert_eq!(
            world
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood),
            5
        );
        assert_eq!(
            world
                .get::<MootMarket>(hall)
                .unwrap()
                .seller_listed_units(MarketSeller::Business(warehouse_id), Good::Wood),
            4
        );
        assert_eq!(world.get::<CompanyAccount>(cash).unwrap().cash, 77);
        assert_eq!(
            world.get::<BusinessAccount>(warehouse).unwrap().book_value,
            109
        );
        assert_eq!(world.get::<BusinessAccount>(order).unwrap().book_value, 0);
        assert_eq!(
            world
                .get::<BusinessAccount>(order)
                .unwrap()
                .capital_expenditures,
            100
        );
        assert!(reclaim(&mut world, recovery));
        assert_eq!(
            world.get::<BusinessAccount>(warehouse).unwrap().book_value,
            109
        );
        assert_eq!(
            world
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood),
            5
        );
    }
}
