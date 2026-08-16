//! Server-authoritative hero trading at a nearby settlement Hall.
//!
//! This is deliberately a physical interaction, not a remote spreadsheet:
//! the live hero must be close, have cargo room/cargo to sell, and own the
//! money being spent. The Hall remains a consignment exchange and never acts
//! as a synthetic buyer.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{
    BuildingOf, Hero, PersonId, PlayerPosition, PlayerRotation, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementId, WorldTime,
};
use shared::economy::{
    format_money, Good, GoodsInventory, MarketFill, MarketSeller, MootMarket, Wallet,
};
use shared::protocol::{
    HeroMarketAction, HeroMarketOrder, HeroMarketResult, ReliableChannel,
    MAX_HERO_MARKET_ORDER_UNITS,
};

use super::hero::OfflineHero;
use crate::world::village::BusinessEventQueue;

/// Includes the authored 5.2m door/queue apron while still requiring the hero
/// to be visibly at the civic building rather than anywhere in town.
pub const HERO_MARKET_INTERACTION_RANGE: f32 = 12.0;

struct CompletedHeroTrade {
    message: String,
    fills: Vec<MarketFill>,
}

/// Atomic physical/financial half of a nearby trade. Sale proceeds are not in
/// this function because posting an offer creates no proceeds; a later real
/// purchase returns fills for the ordinary business-event settlement pass.
#[allow(clippy::too_many_arguments)]
fn execute_hero_market_order(
    person_id: PersonId,
    good: Good,
    action: HeroMarketAction,
    units: u32,
    hero_store: &mut GoodsInventory,
    hero_wallet: &mut Wallet,
    hall_store: &mut GoodsInventory,
    market: &mut MootMarket,
) -> Result<CompletedHeroTrade, String> {
    if !market.can_trade(good) {
        return Err(format!(
            "{} trade requires {}.",
            good.label(),
            good.minimum_market_tier().requirement_label(),
        ));
    }
    match action {
        HeroMarketAction::Buy => {
            let cargo_units = hero_store.free_bulk() / good.bulk_per_unit();
            let physical_units = hall_store.amount(good);
            let requested = units.min(cargo_units).min(physical_units);
            let self_seller = MarketSeller::Person(person_id);
            let preview = market.preview_purchase(
                good,
                requested,
                hero_wallet.balance(),
                None,
                Some(self_seller),
            );
            if preview.units == 0 {
                let reason = if cargo_units == 0 {
                    "Your hero has no cargo space."
                } else if physical_units == 0 || market.listed_units(good) == 0 {
                    "No listed stock is available."
                } else {
                    "Your hero cannot afford the cheapest offer."
                };
                return Err(reason.to_string());
            }
            let purchase = market.purchase_recording_demand(
                good,
                preview.units,
                preview.pennies,
                None,
                Some(self_seller),
            );
            debug_assert_eq!(purchase.trade, preview);
            if !hero_wallet.debit(purchase.trade.pennies) {
                return Err("Your wallet changed before the trade cleared.".to_string());
            }
            let removed = hall_store.remove(good, purchase.trade.units);
            let accepted = hero_store.add(good, removed);
            debug_assert_eq!(removed, purchase.trade.units);
            debug_assert_eq!(accepted, purchase.trade.units);
            Ok(CompletedHeroTrade {
                message: format!(
                    "Bought {} {} for {} coin.",
                    purchase.trade.units,
                    good.label(),
                    format_money(purchase.trade.pennies)
                ),
                fills: purchase.fills,
            })
        }
        HeroMarketAction::PostSellOrder { unit_price } => {
            let price = unit_price.max(1);
            let available = hero_store.amount(good);
            let cargo_units = hall_store.free_units(good);
            let moved = units.min(available).min(cargo_units);
            if moved == 0 {
                let reason = if available == 0 {
                    format!("Your hero carries no {}.", good.label())
                } else {
                    "The Hall store has no room for that good.".to_string()
                };
                return Err(reason);
            }
            let removed = hero_store.remove(good, moved);
            let accepted = hall_store.add(good, removed);
            debug_assert_eq!(accepted, moved);
            market.consign(MarketSeller::Person(person_id), good, moved, price);
            Ok(CompletedHeroTrade {
                message: format!(
                    "Posted {} {} at {} coin each; payment arrives when sold.",
                    moved,
                    good.label(),
                    format_money(price)
                ),
                fills: Vec::new(),
            })
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn handle_hero_market_orders(
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroMarketOrder>,
            &mut MessageSender<HeroMarketResult>,
        ),
        With<ClientOf>,
    >,
    heroes: Query<(Entity, &Hero, &PersonId, &PlayerPosition), Without<OfflineHero>>,
    halls: Query<
        (
            &SettlementId,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        With<MootMarket>,
    >,
    marketplaces: Query<(
        &SettlementBuilding,
        &BuildingOf,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    mut stores: Query<(
        &mut GoodsInventory,
        Option<&mut Wallet>,
        Option<&mut MootMarket>,
    )>,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
) {
    let day = world_time.iter().next().map_or(0, |time| time.day);
    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let mut reply = |success: bool, message: String| {
                sender.send::<ReliableChannel>(HeroMarketResult { success, message });
            };
            let Some((hero_entity, _, person_id, hero_position)) =
                heroes.iter().find(|(_, hero, ..)| hero.owner == remote.0)
            else {
                reply(false, "Create your hero before trading.".to_string());
                continue;
            };
            let Ok((settlement_id, settlement, hall_position, hall_rotation)) =
                halls.get(order.market)
            else {
                reply(false, "That building is not a public exchange.".to_string());
                continue;
            };
            let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
                hall_position.0,
                hall_rotation.map_or(0.0, |rotation| rotation.0),
            );
            let counter = crate::world::village::nearest_public_market_entrance(
                hero_position.0,
                hall_entrance,
                marketplaces
                    .iter()
                    .filter(|(building, building_of, ..)| {
                        building.kind == SettlementBuildingKind::Market
                            && building_of.0 == *settlement_id
                    })
                    .map(|(building, _, position, rotation)| {
                        building.kind.entrance_position(position.0, rotation.0)
                    }),
            );
            let distance = Vec2::new(hero_position.0.x, hero_position.0.z)
                .distance(Vec2::new(counter.x, counter.z));
            if distance > HERO_MARKET_INTERACTION_RANGE {
                reply(
                    false,
                    format!(
                        "Move your hero closer to a {} public market counter ({distance:.1}m / {:.0}m).",
                        settlement.name, HERO_MARKET_INTERACTION_RANGE
                    ),
                );
                continue;
            }
            let units = order.units.min(MAX_HERO_MARKET_ORDER_UNITS);
            if units == 0 {
                reply(false, "Choose at least one unit.".to_string());
                continue;
            }

            let Ok(
                [(mut hero_store, Some(mut hero_wallet), _), (mut hall_store, _, Some(mut market))],
            ) = stores.get_many_mut([hero_entity, order.market])
            else {
                reply(
                    false,
                    "The hero or exchange inventory is unavailable.".to_string(),
                );
                continue;
            };

            match execute_hero_market_order(
                *person_id,
                order.good,
                order.action,
                units,
                &mut hero_store,
                &mut hero_wallet,
                &mut hall_store,
                &mut market,
            ) {
                Ok(completed) => {
                    business_events.record_market_purchase(day, *settlement_id, completed.fills);
                    reply(true, completed.message);
                }
                Err(reason) => reply(false, reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::economy::{capacity, PENNIES_PER_COIN};

    #[test]
    fn posting_an_offer_moves_physical_stock_but_creates_no_liquidity() {
        let seller = PersonId(41);
        let mut hero_store = GoodsInventory::new(capacity::VILLAGER);
        hero_store.add(Good::Wood, 2);
        let mut wallet = Wallet::new(0);
        let mut hall_store = GoodsInventory::new(capacity::HALL);
        let mut market = MootMarket::founding();

        let result = execute_hero_market_order(
            seller,
            Good::Wood,
            HeroMarketAction::PostSellOrder {
                unit_price: 3 * PENNIES_PER_COIN,
            },
            2,
            &mut hero_store,
            &mut wallet,
            &mut hall_store,
            &mut market,
        )
        .unwrap();

        assert!(result.fills.is_empty());
        assert_eq!(wallet.balance(), 0, "the Hall must not buy the offer");
        assert_eq!(hero_store.amount(Good::Wood), 0);
        assert_eq!(hall_store.amount(Good::Wood), 2);
        assert_eq!(
            market.seller_listed_units(MarketSeller::Person(seller), Good::Wood),
            2
        );
    }

    #[test]
    fn a_locked_good_cannot_leave_the_hero_inventory() {
        let seller = PersonId(42);
        let mut hero_store = GoodsInventory::new(capacity::VILLAGER);
        hero_store.add(Good::Iron, 2);
        let mut wallet = Wallet::new(0);
        let mut hall_store = GoodsInventory::new(capacity::HALL);
        let mut market = MootMarket::founding();

        let result = execute_hero_market_order(
            seller,
            Good::Iron,
            HeroMarketAction::PostSellOrder { unit_price: 400 },
            2,
            &mut hero_store,
            &mut wallet,
            &mut hall_store,
            &mut market,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("a Moot must reject level-two Iron trade"),
        };

        assert!(error.contains("level 2 paved Marketplace"));
        assert_eq!(hero_store.amount(Good::Iron), 2);
        assert_eq!(hall_store.amount(Good::Iron), 0);
        assert_eq!(market.listed_units(Good::Iron), 0);
    }

    #[test]
    fn a_real_buyer_clears_the_offer_and_pays_exactly_once() {
        let seller = PersonId(51);
        let buyer = PersonId(52);
        let mut seller_store = GoodsInventory::new(capacity::VILLAGER);
        seller_store.add(Good::Bread, 1);
        let mut seller_wallet = Wallet::new(0);
        let mut hall_store = GoodsInventory::new(capacity::HALL);
        let mut market = MootMarket::founding();
        execute_hero_market_order(
            seller,
            Good::Bread,
            HeroMarketAction::PostSellOrder { unit_price: 250 },
            1,
            &mut seller_store,
            &mut seller_wallet,
            &mut hall_store,
            &mut market,
        )
        .unwrap();

        let mut buyer_store = GoodsInventory::new(capacity::VILLAGER);
        let mut buyer_wallet = Wallet::new(500);
        let completed = execute_hero_market_order(
            buyer,
            Good::Bread,
            HeroMarketAction::Buy,
            1,
            &mut buyer_store,
            &mut buyer_wallet,
            &mut hall_store,
            &mut market,
        )
        .unwrap();

        assert_eq!(buyer_wallet.balance(), 250);
        assert_eq!(buyer_store.amount(Good::Bread), 1);
        assert_eq!(hall_store.amount(Good::Bread), 0);
        assert_eq!(completed.fills.len(), 1);
        assert_eq!(completed.fills[0].seller, MarketSeller::Person(seller));
        assert_eq!(completed.fills[0].gross, 250);
    }
}
