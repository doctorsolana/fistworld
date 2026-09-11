//! Read-only market presentation, derived from authoritative stock and offers.

use super::*;
use shared::{components::PersonId, economy::MarketSeller};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MarketRowModel {
    pub(super) good: Good,
    pub(super) condition: String,
    pub(super) store: String,
    pub(super) listed: String,
    pub(super) last_sale: String,
    pub(super) best_offer: String,
    pub(super) today: String,
    pub(super) hero_cargo: String,
    pub(super) buy_label: String,
    pub(super) offer_label: String,
    pub(super) buy_enabled: bool,
    pub(super) buy_price: u64,
    pub(super) offer_enabled: bool,
    pub(super) offer_price: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MarketPageModel {
    pub(super) settlement: Entity,
    pub(super) place: String,
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) access: String,
    pub(super) access_enabled: bool,
    pub(super) summaries: [String; 5],
    pub(super) rows: Vec<MarketRowModel>,
    pub(super) feedback: String,
    pub(super) feedback_success: Option<bool>,
}

impl MarketPageModel {
    pub(super) fn summary(&self, field: MarketSummaryField) -> &str {
        &self.summaries[match field {
            MarketSummaryField::OnOffer => 0,
            MarketSummaryField::CommonStore => 1,
            MarketSummaryField::Today => 2,
            MarketSummaryField::Lifetime => 3,
            MarketSummaryField::HeroFunds => 4,
        }]
    }

    pub(super) fn row(&self, good: Good) -> &MarketRowModel {
        &self.rows[good.index()]
    }

    pub(super) fn text(&self, field: MarketBoundText) -> &str {
        match field {
            MarketBoundText::Title => &self.title,
            MarketBoundText::Subtitle => &self.subtitle,
            MarketBoundText::Access => &self.access,
            MarketBoundText::Summary(field) => self.summary(field),
            MarketBoundText::Row(good, field) => {
                let row = self.row(good);
                match field {
                    MarketRowField::Condition => &row.condition,
                    MarketRowField::Store => &row.store,
                    MarketRowField::Listed => &row.listed,
                    MarketRowField::LastSale => &row.last_sale,
                    MarketRowField::BestOffer => &row.best_offer,
                    MarketRowField::Today => &row.today,
                    MarketRowField::HeroCargo => &row.hero_cargo,
                }
            }
        }
    }
}

pub(super) fn market_page_model(
    target: &MarketPage,
    world: &MarketWorld<'_, '_>,
    feedback: &MarketFeedback,
) -> Option<MarketPageModel> {
    let market = world.markets.get(target.settlement).ok()?;
    let (settlement, settlement_id, hall_level, hall_position, hall_rotation) =
        world.settlements.get(target.settlement).ok()?;
    let inventory = world.inventories.get(target.settlement).ok();
    let hall_level = hall_level
        .copied()
        .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
    let local_hero = world.local.as_ref().and_then(|local| {
        world
            .heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let hero_inventory = local_hero.and_then(|(_, _, inventory, _, _)| inventory);
    let hero_wallet = local_hero.and_then(|(_, _, _, wallet, _)| wallet);
    let hero_person = local_hero.and_then(|(_, _, _, _, person)| person.copied());
    let counter = local_hero.map(|(_, hero_position, ..)| {
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        nearest_public_market_entrance(
            hero_position.0,
            hall_entrance,
            world
                .buildings
                .iter()
                .filter(|(building, owner, ..)| {
                    building.kind == SettlementBuildingKind::Market
                        && owner.is_some_and(|owner| owner.0 == *settlement_id)
                })
                .filter_map(|(building, _, position, rotation)| {
                    Some(building.kind.entrance_position(position?.0, rotation?.0))
                }),
        )
    });
    let hero_distance = local_hero.zip(counter).map(|((_, position, ..), counter)| {
        Vec2::new(position.0.x, position.0.z).distance(Vec2::new(counter.x, counter.z))
    });
    let can_trade = hero_person.is_some()
        && hero_distance.is_some_and(|distance| distance <= HERO_MARKET_INTERACTION_RANGE);
    let access = if local_hero.is_none() {
        "VIEW ONLY / CREATE A HERO TO TRADE".to_string()
    } else if can_trade {
        "AT THE COUNTER / TRADING ENABLED".to_string()
    } else {
        hero_distance.map_or_else(
            || "VIEW ONLY / MARKET LOCATION UNKNOWN".to_string(),
            |distance| format!("VIEW ONLY / {distance:.0}M FROM THE COUNTER"),
        )
    };
    let rows: Vec<_> = Good::ALL
        .into_iter()
        .map(|good| {
            market_row_model(
                good,
                inventory,
                market,
                hero_inventory,
                hero_wallet,
                hero_person,
                can_trade,
            )
        })
        .collect();
    let listed_units = Good::ALL
        .into_iter()
        .map(|good| market.listed_units(good))
        .fold(0u32, u32::saturating_add);
    let today_coin = Good::ALL
        .into_iter()
        .map(|good| market.pool(good).day.consumer_coin)
        .fold(0u64, u64::saturating_add);
    let today_units = Good::ALL
        .into_iter()
        .map(|good| market.pool(good).day.consumer_units)
        .fold(0u64, u64::saturating_add);
    let common_store = inventory.map_or_else(
        || "No common store".to_string(),
        |stock| format!("{} / {} bulk", stock.used_bulk(), stock.bulk_capacity()),
    );
    let hero_funds = hero_wallet.map_or_else(
        || "No wallet".to_string(),
        |wallet| format!("{} coin", format_money(wallet.balance())),
    );
    let feedback_present =
        feedback.market == Some(target.settlement) && !feedback.message.is_empty();
    let feedback_text = if feedback_present {
        feedback.message.clone()
    } else if can_trade {
        "BUY takes one unit from another seller. POST lists one carried unit; your listed goods remain yours until bought, then proceeds arrive minus the market fee.".to_string()
    } else {
        "Market records are readable from anywhere. To transact, take your hero within 12m of this town's Hall or Marketplace counter.".to_string()
    };
    Some(MarketPageModel {
        settlement: target.settlement,
        place: settlement.name.clone(),
        title: format!("{} MARKET", settlement.name.to_uppercase()),
        subtitle: format!(
            "{} / {} / {} / {:.2}% FEE",
            settlement.tier.label().to_uppercase(),
            hall_level.label(),
            market.trade_tier().label().to_uppercase(),
            market.market_fee_bps() as f32 / 100.0,
        ),
        access,
        access_enabled: can_trade,
        summaries: [
            format!("{listed_units} units"),
            common_store,
            format!("{} coin / {today_units} units", format_money(today_coin)),
            format!("{} coin", format_money(market.total_volume())),
            hero_funds,
        ],
        rows,
        feedback: feedback_text,
        feedback_success: feedback_present.then_some(feedback.success),
    })
}

fn market_row_model(
    good: Good,
    inventory: Option<&GoodsInventory>,
    market: &MootMarket,
    hero_inventory: Option<&GoodsInventory>,
    hero_wallet: Option<&Wallet>,
    hero_person: Option<PersonId>,
    can_trade: bool,
) -> MarketRowModel {
    let pool = market.pool(good);
    let stock = inventory.map_or(0, |inventory| inventory.amount(good));
    let listed = market.listed_units(good);
    let unlocked = market.can_trade(good);
    let unmet = pool.day.unmet_units();
    let condition = if !unlocked {
        format!(
            "UNLOCKS AT {}",
            good.minimum_market_tier().label().to_uppercase()
        )
    } else if unmet > 0 {
        format!("{unmet} REQUESTED UNITS UNFILLED TODAY")
    } else if listed == 0 {
        "NO LIVE OFFERS".to_string()
    } else if stock < pool.target_stock {
        "SHORT SUPPLY".to_string()
    } else if pool.target_stock > 0 && stock > pool.target_stock.saturating_mul(2) {
        "SURPLUS".to_string()
    } else {
        "BALANCED".to_string()
    };
    let hero_units = hero_inventory.map_or(0, |stock| stock.amount(good));
    let hero_balance = hero_wallet.map_or(0, |wallet| wallet.balance());
    let seller = hero_person.map(MarketSeller::Person);
    let own_listed = seller.map_or(0, |seller| market.seller_listed_units(seller, good));
    // Quote the same eligible order book as the authoritative buyer. The
    // settlement-wide ask may be our own cheaper offer, which we cannot buy.
    let quote = market.preview_purchase(good, 1, u64::MAX, None, seller);
    let cargo_space = hero_inventory.is_some_and(|cargo| cargo.free_units(good) > 0);
    let hall_space = inventory.is_some_and(|store| store.free_units(good) > 0);
    let buy_enabled = can_trade
        && unlocked
        && stock > 0
        && quote.units > 0
        && cargo_space
        && hero_balance >= quote.pennies;
    let offer_enabled = can_trade && unlocked && hero_units > 0 && hall_space;
    let offer_price = market.suggested_price(good);
    let buy_label = if !unlocked {
        "BUY / LOCKED".to_string()
    } else if !can_trade {
        "BUY / VISIT".to_string()
    } else if listed == 0 {
        "BUY / NO OFFER".to_string()
    } else if stock == 0 {
        "BUY / NO STOCK".to_string()
    } else if quote.units == 0 {
        "BUY / OWN OFFER".to_string()
    } else if !cargo_space {
        "BUY / CARGO FULL".to_string()
    } else if hero_balance < quote.pennies {
        "BUY / NEED COIN".to_string()
    } else {
        format!("BUY 1 / {}", format_money(quote.pennies))
    };
    let offer_label = if !unlocked {
        "POST / LOCKED".to_string()
    } else if !can_trade {
        "POST / VISIT".to_string()
    } else if hero_units == 0 {
        "POST / EMPTY".to_string()
    } else if !hall_space {
        "POST / STORE FULL".to_string()
    } else {
        format!("POST 1 / {}", format_money(offer_price))
    };
    MarketRowModel {
        good,
        condition,
        store: format!("{stock} / {}", pool.target_stock),
        listed: format!("{listed}"),
        last_sale: if pool.bid == 0 {
            "No sale".to_string()
        } else {
            format!("{} coin", format_money(pool.bid))
        },
        best_offer: if quote.units == 0 {
            "None".to_string()
        } else {
            format!("{} coin", format_money(quote.pennies))
        },
        today: format!("{} / {unmet}", pool.day.consumer_units),
        hero_cargo: format!("{hero_units} carried\n{own_listed} listed"),
        buy_label,
        offer_label,
        buy_enabled,
        buy_price: quote.pennies,
        offer_enabled,
        offer_price,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_good_has_one_stable_row_slot() {
        let market = MootMarket::founding();
        let rows: Vec<_> = Good::ALL
            .into_iter()
            .map(|good| market_row_model(good, None, &market, None, None, None, false))
            .collect();
        assert_eq!(rows.len(), Good::COUNT);
        for good in Good::ALL {
            assert_eq!(rows[good.index()].good, good);
        }
    }

    #[test]
    fn remote_market_is_readable_but_not_actionable() {
        let market = MootMarket::founding();
        let row = market_row_model(
            Good::Wood,
            None,
            &market,
            None,
            Some(&Wallet::new(10_000)),
            None,
            false,
        );
        assert!(!row.buy_enabled);
        assert!(!row.offer_enabled);
        assert_eq!(row.buy_label, "BUY / VISIT");
        assert_eq!(row.offer_label, "POST / VISIT");
    }

    fn row_for(
        market: &MootMarket,
        hall: &GoodsInventory,
        hero: &GoodsInventory,
        pennies: u64,
    ) -> MarketRowModel {
        market_row_model(
            Good::Wheat,
            Some(hall),
            market,
            Some(hero),
            Some(&Wallet::new(pennies)),
            Some(PersonId(41)),
            true,
        )
    }

    #[test]
    fn buying_quotes_another_seller_even_when_my_offer_is_cheapest() {
        let mut market = MootMarket::founding();
        market.consign(MarketSeller::Person(PersonId(41)), Good::Wheat, 1, 20);
        market.consign(MarketSeller::Person(PersonId(42)), Good::Wheat, 1, 120);
        let mut hall = GoodsInventory::new(100);
        hall.add(Good::Wheat, 2);
        let row = row_for(&market, &hall, &GoodsInventory::new(18), 200);
        assert_eq!(row.buy_label, "BUY 1 / 1.20");
        assert!(row.buy_enabled);
        let poor = row_for(&market, &hall, &GoodsInventory::new(18), 50);
        assert!(!poor.buy_enabled, "my own cheap listing is not purchasable");
        assert_eq!(poor.buy_label, "BUY / NEED COIN");
    }

    #[test]
    fn only_my_unsold_offer_is_visible_but_cannot_be_bought() {
        let mut market = MootMarket::founding();
        market.consign(MarketSeller::Person(PersonId(41)), Good::Wheat, 1, 72);
        let mut hall = GoodsInventory::new(100);
        hall.add(Good::Wheat, 1);
        let row = row_for(&market, &hall, &GoodsInventory::new(18), 2000);
        assert!(!row.buy_enabled);
        assert_eq!(row.buy_label, "BUY / OWN OFFER");
        assert_eq!(row.hero_cargo, "0 carried\n1 listed");
    }

    #[test]
    fn full_cargo_and_missing_physical_stock_disable_buying() {
        let mut market = MootMarket::founding();
        market.consign(MarketSeller::Person(PersonId(42)), Good::Wheat, 1, 72);
        let mut hall = GoodsInventory::new(100);
        hall.add(Good::Wheat, 1);
        let full = row_for(&market, &hall, &GoodsInventory::new(0), 2000);
        assert!(!full.buy_enabled);
        assert_eq!(full.buy_label, "BUY / CARGO FULL");
        let missing = row_for(
            &market,
            &GoodsInventory::new(100),
            &GoodsInventory::new(18),
            2000,
        );
        assert!(!missing.buy_enabled);
        assert_eq!(missing.buy_label, "BUY / NO STOCK");
    }

    #[test]
    fn a_full_hall_cannot_accept_another_consignment() {
        let mut hero = GoodsInventory::new(18);
        hero.add(Good::Wheat, 1);
        let row = row_for(
            &MootMarket::founding(),
            &GoodsInventory::new(0),
            &hero,
            2000,
        );
        assert!(!row.offer_enabled);
        assert_eq!(row.offer_label, "POST / STORE FULL");
    }
}
