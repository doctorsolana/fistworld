use super::super::*;

use shared::economy::{MarketFill, MarketSeller};

#[derive(Debug, Clone, Copy)]
enum BusinessEvent {
    Production {
        day: u32,
        business: shared::components::BuildingId,
        units: u32,
    },
    Sale {
        day: u32,
        market: shared::components::SettlementId,
        fill: MarketFill,
    },
}

/// Short-lived transaction buffer shared by tactical and strategic systems.
/// Events are aggregated by stable identity before ledgers are touched, so a
/// thousand household purchases do not become a thousand business scans.
#[derive(Resource, Default)]
pub struct BusinessEventQueue {
    events: Vec<BusinessEvent>,
}

impl BusinessEventQueue {
    pub fn record_production(
        &mut self,
        day: u32,
        business: shared::components::BuildingId,
        units: u32,
    ) {
        if units > 0 {
            self.events.push(BusinessEvent::Production {
                day,
                business,
                units,
            });
        }
    }

    pub fn record_market_purchase(
        &mut self,
        day: u32,
        market: shared::components::SettlementId,
        fills: impl IntoIterator<Item = MarketFill>,
    ) {
        self.events.extend(
            fills
                .into_iter()
                .map(|fill| BusinessEvent::Sale { day, market, fill }),
        );
    }

    /// A purchase can be queued after the daily settlement pass and its
    /// person-seller can die before the next pass. Route that unsettled claim
    /// to the local treasury as an unclaimed estate instead of retaining an
    /// impossible recipient forever (which would remove the buyer's coin from
    /// circulation). Business sellers remain durable entities and are not
    /// handled here.
    pub fn reroute_deceased_person_sales(
        &mut self,
        person: shared::components::PersonId,
        fallback_settlement: Option<shared::components::SettlementId>,
    ) {
        for event in &mut self.events {
            let BusinessEvent::Sale { market, fill, .. } = event else {
                continue;
            };
            if fill.seller == MarketSeller::Person(person) {
                fill.seller = MarketSeller::Treasury(fallback_settlement.unwrap_or(*market));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_sale_gross(&self) -> u64 {
        self.events
            .iter()
            .filter_map(|event| match event {
                BusinessEvent::Sale { fill, .. } => Some(fill.gross),
                BusinessEvent::Production { .. } => None,
            })
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn pending_sale_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event, BusinessEvent::Sale { .. }))
            .count()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SaleAggregate {
    gross: u64,
    fee: u64,
    units: u32,
}

/// Settle every queued transfer against one indexed pass over businesses,
/// people and settlements. A missing market remains queued because there is no
/// authoritative local account to receive the fill. A missing seller is an
/// unclaimed local estate: their already-delivered goods and the buyer's coin
/// are settled into the market treasury rather than retained forever.
#[allow(clippy::type_complexity)]
pub fn apply_business_events(
    world_time: Query<&shared::components::WorldTime>,
    mut queue: ResMut<BusinessEventQueue>,
    mut businesses: Query<(
        Entity,
        &shared::components::BuildingId,
        &shared::components::OperatedBy,
        &mut BusinessAccount,
    )>,
    company_entities: Query<(Entity, &shared::components::CompanyId)>,
    mut company_accounts: Query<&mut shared::economy::CompanyAccount>,
    mut people: Query<(Entity, &shared::components::PersonId, &mut Wallet)>,
    mut settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &mut Settlement,
        Option<&mut shared::economy::CivicAccount>,
    )>,
) {
    if queue.events.is_empty() {
        return;
    }
    let civic_day = world_time
        .iter()
        .next()
        .map_or(1, |clock| clock.day.saturating_add(1));

    let business_entities: HashMap<
        shared::components::BuildingId,
        (Entity, shared::components::CompanyId),
    > = businesses
        .iter()
        .map(|(entity, id, operated_by, ..)| (*id, (entity, operated_by.0)))
        .collect();
    let companies: HashMap<shared::components::CompanyId, Entity> = company_entities
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let people_entities: HashMap<shared::components::PersonId, Entity> =
        people.iter().map(|(entity, id, _)| (*id, entity)).collect();
    let settlement_entities: HashMap<shared::components::SettlementId, Entity> = settlements
        .iter()
        .map(|(entity, id, ..)| (*id, entity))
        .collect();

    let mut production: HashMap<(u32, shared::components::BuildingId), u32> = HashMap::new();
    let mut sales: HashMap<
        (u32, shared::components::SettlementId, MarketSeller, Good),
        SaleAggregate,
    > = HashMap::new();
    for event in std::mem::take(&mut queue.events) {
        match event {
            BusinessEvent::Production {
                day,
                business,
                units,
            } => {
                let total = production.entry((day, business)).or_default();
                *total = total.saturating_add(units);
            }
            BusinessEvent::Sale { day, market, fill } => {
                let total = sales
                    .entry((day, market, fill.seller, fill.good))
                    .or_default();
                total.gross = total.gross.saturating_add(fill.gross);
                total.fee = total.fee.saturating_add(fill.market_fee);
                total.units = total.units.saturating_add(fill.units);
            }
        }
    }

    for ((day, business), units) in production {
        let Some((entity, _)) = business_entities.get(&business).copied() else {
            queue.events.push(BusinessEvent::Production {
                day,
                business,
                units,
            });
            continue;
        };
        if let Ok((_, _, _, mut account)) = businesses.get_mut(entity) {
            account.record_production(day, units);
        }
    }

    for ((day, market_id, seller, good), sale) in sales {
        let seller_net = sale.gross.saturating_sub(sale.fee);
        let seller_entity = match seller {
            MarketSeller::Business(id) => business_entities.get(&id).map(|(entity, _)| *entity),
            MarketSeller::Person(id) => people_entities.get(&id).copied(),
            MarketSeller::Treasury(id) => settlement_entities.get(&id).copied(),
        };
        let fee_entity = settlement_entities.get(&market_id).copied();
        if fee_entity.is_none() {
            queue.events.push(BusinessEvent::Sale {
                day,
                market: market_id,
                fill: MarketFill {
                    seller,
                    good,
                    units: sale.units,
                    unit_price: sale.gross.checked_div(u64::from(sale.units)).unwrap_or(0),
                    gross: sale.gross,
                    market_fee: sale.fee,
                },
            });
            continue;
        }

        match (seller, seller_entity) {
            (MarketSeller::Business(_), Some(entity)) => {
                let company = businesses
                    .get(entity)
                    .ok()
                    .map(|(_, _, operated_by, _)| operated_by.0);
                let Some(company_entity) = company.and_then(|id| companies.get(&id)).copied()
                else {
                    queue.events.push(BusinessEvent::Sale {
                        day,
                        market: market_id,
                        fill: MarketFill {
                            seller,
                            good,
                            units: sale.units,
                            unit_price: sale.gross.checked_div(u64::from(sale.units)).unwrap_or(0),
                            gross: sale.gross,
                            market_fee: sale.fee,
                        },
                    });
                    continue;
                };
                if let Ok((_, _, _, mut account)) = businesses.get_mut(entity) {
                    account.record_sale(day, sale.gross, sale.fee, sale.units);
                }
                if let Ok(mut account) = company_accounts.get_mut(company_entity) {
                    account.credit(seller_net);
                }
            }
            (MarketSeller::Person(_), Some(entity)) => {
                if let Ok((_, _, mut wallet)) = people.get_mut(entity) {
                    wallet.credit(seller_net);
                }
            }
            (MarketSeller::Treasury(_), Some(entity)) => {
                if let Ok((_, _, mut settlement, civic)) = settlements.get_mut(entity) {
                    settlement.treasury = settlement.treasury.saturating_add(seller_net);
                    if let Some(mut civic) = civic {
                        civic.record_public_sale_income(civic_day, seller_net);
                    }
                }
            }
            // The seller died, its business record vanished, or its source
            // settlement was retired after the buyer had already paid. The
            // physical consignment is gone, so the current market is the only
            // durable authority that can settle the orphaned proceeds.
            (_, None) => {
                let entity = fee_entity.expect("fee settlement presence checked");
                if let Ok((_, _, mut settlement, civic)) = settlements.get_mut(entity) {
                    settlement.treasury = settlement.treasury.saturating_add(seller_net);
                    if let Some(mut civic) = civic {
                        civic.record_public_sale_income(civic_day, seller_net);
                    }
                }
            }
        }
        let fee_entity = fee_entity.expect("fee settlement presence checked");
        if let Ok((_, _, mut settlement, civic)) = settlements.get_mut(fee_entity) {
            settlement.treasury = settlement.treasury.saturating_add(sale.fee);
            if let Some(mut civic) = civic {
                civic.record_market_fee_income(civic_day, sale.fee);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offline_heroes_sale_pays_once_and_survives_readoption() {
        let person = shared::components::PersonId(81);
        let settlement_id = shared::components::SettlementId(82);
        let mut app = App::new();
        app.init_resource::<BusinessEventQueue>()
            .add_systems(Update, apply_business_events);
        app.world_mut().spawn(WorldTime::new_default());
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Elderham".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
            ))
            .id();
        let hero = app
            .world_mut()
            .spawn((person, Wallet::new(1856), crate::player::hero::OfflineHero))
            .id();
        let mut market = shared::economy::MootMarket::founding();
        market.consign(MarketSeller::Person(person), Good::Wheat, 1, 72);
        let purchase = market.purchase(Good::Wheat, 1, 72, None, None);
        let fee = purchase
            .fills
            .iter()
            .map(|fill| fill.market_fee)
            .sum::<u64>();
        assert_eq!(purchase.trade.pennies, 72);
        assert_eq!(
            market.seller_listed_units(MarketSeller::Person(person), Good::Wheat),
            0
        );
        app.world_mut()
            .resource_mut::<BusinessEventQueue>()
            .record_market_purchase(0, settlement_id, purchase.fills);
        app.update();
        assert_eq!(
            app.world().get::<Wallet>(hero).unwrap().balance(),
            1856 + 72 - fee
        );
        assert_eq!(app.world().get::<Settlement>(hall).unwrap().treasury, fee);
        app.world_mut()
            .entity_mut(hero)
            .remove::<crate::player::hero::OfflineHero>();
        app.update();
        assert_eq!(
            app.world().get::<Wallet>(hero).unwrap().balance(),
            1856 + 72 - fee
        );
        assert_eq!(
            app.world()
                .resource::<BusinessEventQueue>()
                .pending_sale_count(),
            0
        );
    }

    #[test]
    fn a_dead_persons_unsettled_sale_becomes_a_local_treasury_claim() {
        let person = shared::components::PersonId(91);
        let settlement = shared::components::SettlementId(92);
        let mut queue = BusinessEventQueue::default();
        queue.record_market_purchase(
            3,
            settlement,
            [MarketFill {
                seller: MarketSeller::Person(person),
                good: Good::Wood,
                units: 2,
                unit_price: 24,
                gross: 48,
                market_fee: 3,
            }],
        );

        queue.reroute_deceased_person_sales(person, Some(settlement));

        assert!(matches!(
            queue.events.as_slice(),
            [BusinessEvent::Sale {
                fill: MarketFill {
                    seller: MarketSeller::Treasury(id),
                    gross: 48,
                    market_fee: 3,
                    ..
                },
                ..
            }] if *id == settlement
        ));
    }

    #[test]
    fn settlement_settles_a_missing_person_seller_as_unclaimed_estate() {
        let person = shared::components::PersonId(95);
        let settlement_id = shared::components::SettlementId(96);
        let mut app = App::new();
        app.init_resource::<BusinessEventQueue>()
            .add_systems(Update, apply_business_events);
        app.world_mut().spawn(WorldTime::new_default());
        let settlement = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Claimsford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<BusinessEventQueue>()
            .record_market_purchase(
                0,
                settlement_id,
                [MarketFill {
                    seller: MarketSeller::Person(person),
                    good: Good::Wood,
                    units: 2,
                    unit_price: 29,
                    gross: 58,
                    market_fee: 3,
                }],
            );

        app.update();

        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().treasury,
            58,
            "the seller net and market fee must both remain in circulation"
        );
        assert!(app
            .world()
            .resource::<BusinessEventQueue>()
            .events
            .is_empty());
    }

    #[test]
    fn settlement_settles_a_missing_business_seller_as_unclaimed_estate() {
        let business = shared::components::BuildingId(97);
        let settlement_id = shared::components::SettlementId(98);
        let mut app = App::new();
        app.init_resource::<BusinessEventQueue>()
            .add_systems(Update, apply_business_events);
        app.world_mut().spawn(WorldTime::new_default());
        let settlement = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Lastmarket".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<BusinessEventQueue>()
            .record_market_purchase(
                0,
                settlement_id,
                [MarketFill {
                    seller: MarketSeller::Business(business),
                    good: Good::Flour,
                    units: 2,
                    unit_price: 57,
                    gross: 114,
                    market_fee: 6,
                }],
            );

        app.update();

        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().treasury,
            114,
            "a vanished firm's seller net and fee must remain in circulation"
        );
        assert_eq!(
            app.world()
                .resource::<BusinessEventQueue>()
                .pending_sale_count(),
            0
        );
    }
}
