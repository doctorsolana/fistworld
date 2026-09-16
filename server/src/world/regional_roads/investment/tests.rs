use super::*;

fn hall(world: &mut World, cash: u64, listed: bool) -> Funding {
    let mut goods = GoodsInventory::new(100);
    goods.add(Good::Wood, 8);
    goods.add(Good::Stone, 4);
    let mut market = MootMarket::founding();
    if listed {
        market.consign(MarketSeller::Person(PersonId(9)), Good::Wood, 8, 7);
        market.consign(MarketSeller::Person(PersonId(9)), Good::Stone, 4, 11);
    }
    let entity = world
        .spawn((
            Settlement {
                name: "Stoneford".into(),
                tier: SettlementTier::Village,
                residents: 8,
                treasury: cash,
            },
            market,
            goods,
        ))
        .id();
    Funding {
        hall: entity,
        id: SettlementId(1),
        name: "Stoneford".into(),
        pickup: Vec3::ZERO,
        daily_wage: 100,
        budget: cash,
    }
}

#[test]
fn material_quote_preserves_private_title_and_cash_until_complete_approval() {
    let mut world = World::new();
    let funding = hall(&mut world, 200, true);
    let before = world.get::<MootMarket>(funding.hall).unwrap().clone();
    assert!(quote_materials(&world, &funding, [8, 4], 99).is_none());
    assert_eq!(world.get::<MootMarket>(funding.hall), Some(&before));
    assert_eq!(
        world
            .get::<GoodsInventory>(funding.hall)
            .unwrap()
            .amount(Good::Wood),
        8
    );
    assert_eq!(world.get::<Settlement>(funding.hall).unwrap().treasury, 200);
    let (_, inventory, stock, fills, cost) =
        quote_materials(&world, &funding, [8, 4], 100).unwrap();
    assert_eq!(cost, 100);
    assert_eq!(inventory.amount(Good::Wood), 0);
    assert_eq!(inventory.amount(Good::Stone), 0);
    assert_eq!(stock.amount(Good::Wood), 8);
    assert_eq!(stock.amount(Good::Stone), 4);
    assert_eq!(fills.iter().map(|fill| fill.units).sum::<u32>(), 12);
    assert!(
        fills
            .iter()
            .all(|fill| fill.seller == MarketSeller::Person(PersonId(9)))
    );
    assert_eq!(
        world.get::<MootMarket>(funding.hall),
        Some(&before),
        "a quote is not yet a purchase"
    );
}

#[test]
fn untitled_hall_stock_is_not_free_bridge_material() {
    let mut world = World::new();
    let funding = hall(&mut world, 200, false);
    assert!(quote_materials(&world, &funding, [8, 4], 200).is_none());
    assert_eq!(
        world
            .get::<GoodsInventory>(funding.hall)
            .unwrap()
            .amount(Good::Stone),
        4
    );
}

#[test]
fn road_worker_gap_remains_owned_and_does_not_get_another_job() {
    let mut world = World::new();
    let project = world.spawn_empty().id();
    let actor = world.spawn(RegionalRoadWorker { project }).id();
    let mut blocked = world.query_filtered::<Entity, worker_activity::JobChangeBlocked>();
    assert!(blocked.get(&world, actor).is_ok());
    world.entity_mut(actor).remove::<RegionalRoadWorker>();
    assert!(!blocked.get(&world, actor).is_ok());
}
