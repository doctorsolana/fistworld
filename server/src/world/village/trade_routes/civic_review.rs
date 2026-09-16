//! Daily, buyer-funded repricing of civic tenders before any carrier commits.

use super::*;

pub(super) const OPEN_TENDER_LIFETIME_DAYS: u32 = 7;
const CANCELLED_TENDER_COOLDOWN_DAYS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CivicQuote {
    pub units: u32,
    pub maximum_unit_price: u64,
    pub delivery_fee_per_bulk: u64,
    pub escrow_cash: u64,
}

pub(super) fn blocks_new_tender(contract: &CivicTradeContract, day: u32) -> bool {
    contract.status.is_active()
        || (contract.status == TradeContractStatus::Cancelled
            && contract.last_attempt_day != u32::MAX
            && day.saturating_sub(contract.last_attempt_day) < CANCELLED_TENDER_COOLDOWN_DAYS)
}

/// Hall positions are snapshotted by the caller. Cache one distance allowance
/// per source while comparing its sellers, rather than surveying a route for
/// each bid. Physical travel still uses the ordinary navigation system.
pub(super) fn journey_allowances(
    destination: HallSnapshot,
    halls: &HashMap<SettlementId, HallSnapshot>,
) -> HashMap<SettlementId, u64> {
    halls
        .iter()
        .map(|(id, origin)| {
            let distance = Vec2::new(
                destination.position.x - origin.position.x,
                destination.position.z - origin.position.z,
            )
            .length();
            (
                *id,
                ((distance / 100.0).ceil() as u64).saturating_mul(JOURNEY_PENNIES_PER_100_METRES),
            )
        })
        .collect()
}

/// A tender can buy at most one ordinary porter load. Checked arithmetic
/// rejects an unrepresentable offer instead of backing it with saturated cash.
pub(super) fn affordable_quote(
    good: Good,
    wanted_units: u32,
    unit_price: u64,
    budget: u64,
    carrier_cost: u64,
) -> Option<CivicQuote> {
    let bulk_per_unit = u64::from(good.bulk_per_unit().max(1));
    let carry_units = shared::economy::capacity::PORTER / good.bulk_per_unit().max(1);
    (1..=wanted_units.min(carry_units)).rev().find_map(|units| {
        let total_bulk = u64::from(units).checked_mul(bulk_per_unit)?;
        let delivery_fee_per_bulk = CONTRACT_DELIVERY_PENNIES_PER_BULK.max(
            carrier_cost
                .max(CONTRACT_MINIMUM_DELIVERY_PENNIES)
                .div_ceil(total_bulk),
        );
        let escrow_cash = unit_price
            .checked_mul(u64::from(units))?
            .checked_add(delivery_fee_per_bulk.checked_mul(total_bulk)?)?;
        (escrow_cash <= budget).then_some(CivicQuote {
            units,
            maximum_unit_price: unit_price,
            delivery_fee_per_bulk,
            escrow_cash,
        })
    })
}

/// Compare complete landed prices, including freight, without truncating a
/// per-unit division. Stable seller and settlement ids break exact ties.
pub(super) fn quote_order(
    a: &(SettlementId, MarketSeller, CivicQuote),
    b: &(SettlementId, MarketSeller, CivicQuote),
) -> std::cmp::Ordering {
    (u128::from(a.2.escrow_cash) * u128::from(b.2.units))
        .cmp(&(u128::from(b.2.escrow_cash) * u128::from(a.2.units)))
        .then_with(|| b.2.units.cmp(&a.2.units))
        .then_with(|| (a.0, a.1).cmp(&(b.0, b.1)))
}

/// Only the unused reservation moves. Terms already attached to a route or
/// cargo are excluded by the caller and again by the contract status guard.
pub(super) fn apply_quote(
    contract: &mut CivicTradeContract,
    treasury: &mut u64,
    quote: CivicQuote,
) -> bool {
    if contract.status != TradeContractStatus::Open {
        return false;
    }
    let Some(total_available) = treasury.checked_add(contract.escrow_cash) else {
        return false;
    };
    let Some(remaining_treasury) = total_available.checked_sub(quote.escrow_cash) else {
        return false;
    };
    let Some(reserved_cash) = quote
        .escrow_cash
        .checked_add(contract.spent_on_goods)
        .and_then(|cash| cash.checked_add(contract.spent_on_freight))
    else {
        return false;
    };
    *treasury = remaining_treasury;
    contract.requested_units = contract.delivered_units.saturating_add(quote.units);
    contract.maximum_unit_price = quote.maximum_unit_price;
    contract.delivery_fee_per_bulk = quote.delivery_fee_per_bulk;
    contract.escrow_cash = quote.escrow_cash;
    contract.reserved_cash = reserved_cash;
    true
}

pub(super) fn expire_open_tender(
    contract: &mut CivicTradeContract,
    treasury: &mut u64,
    day: u32,
) -> bool {
    if contract.status != TradeContractStatus::Open
        || day.saturating_sub(contract.created_day) < OPEN_TENDER_LIFETIME_DAYS
    {
        return false;
    }
    let Some(refunded_treasury) = treasury.checked_add(contract.escrow_cash) else {
        return false;
    };
    *treasury = refunded_treasury;
    contract.escrow_cash = 0;
    contract.status = TradeContractStatus::Cancelled;
    contract.last_attempt_day = day;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{
        BuildingId, BuildingOf, CivicHallLevel, CompanyId, OperatedBy, PersonId, SettlementTier,
    };

    fn spawn_hall(
        app: &mut App,
        id: SettlementId,
        position: Vec3,
        treasury: u64,
        stone: u32,
        price: u64,
    ) -> Entity {
        let mut market = MootMarket::founding();
        market.unlock_trade_tier(shared::economy::MarketTradeTier::Marketplace);
        let mut inventory = GoodsInventory::new(shared::economy::capacity::HALL);
        inventory.add(Good::Stone, stone);
        if stone > 0 {
            market.consign(
                MarketSeller::Business(BuildingId(id.0 + 10)),
                Good::Stone,
                stone,
                price,
            );
        }
        app.world_mut()
            .spawn((
                id,
                Settlement {
                    name: format!("Town {}", id.0),
                    tier: SettlementTier::Village,
                    residents: 30,
                    treasury,
                },
                PlayerPosition(position),
                PlayerRotation(0.0),
                inventory,
                market,
            ))
            .id()
    }

    fn spawn_project(app: &mut App) {
        app.world_mut().spawn((
            CivicHallUpgradeWorksite {
                target: CivicHallLevel::Town,
                material: Good::Stone,
                material_required: 3,
            },
            BuildingOf(SettlementId(2)),
            GoodsInventory::new(18),
            ConstructionSite {
                kind: SettlementBuildingKind::Hall,
                settlement: "Town 2".into(),
                raising: false,
                stand: Vec3::ZERO,
                rotation: 0.0,
            },
        ));
    }

    fn tender() -> CivicTradeContract {
        CivicTradeContract {
            origin: None,
            destination: SettlementId(2),
            good: Good::Stone,
            source_seller: None,
            requested_units: 3,
            delivered_units: 0,
            maximum_unit_price: 250,
            delivery_fee_per_bulk: 12,
            reserved_cash: 966,
            escrow_cash: 966,
            spent_on_goods: 0,
            spent_on_freight: 0,
            created_day: 0,
            last_attempt_day: u32::MAX,
            status: TradeContractStatus::Open,
        }
    }

    #[test]
    fn small_remote_load_covers_distance_and_the_actual_wage() {
        let destination = HallSnapshot {
            position: Vec3::X * 3_000.0,
            rotation: 0.0,
        };
        let halls = HashMap::from([(
            SettlementId(1),
            HallSnapshot {
                position: Vec3::ZERO,
                rotation: 0.0,
            },
        )]);
        let allowance = journey_allowances(destination, &halls)[&SettlementId(1)];
        let quote = affordable_quote(Good::Stone, 3, 250, 2_000, 180 + allowance).unwrap();
        assert_eq!(allowance, 150);
        assert_eq!(quote.delivery_fee_per_bulk, 19);
        assert_eq!(quote.escrow_cash, 1_092);
        assert!(quote.delivery_fee_per_bulk * 18 >= 330);
    }

    #[test]
    fn unaffordable_or_overflowing_offer_never_gets_a_quote() {
        assert!(affordable_quote(Good::Stone, 0, 250, 2_000, 330).is_none());
        assert!(affordable_quote(Good::Stone, 3, 250, 400, 180 + 150).is_none());
        assert!(affordable_quote(Good::Stone, 3, u64::MAX, u64::MAX, 330).is_none());
    }

    #[test]
    fn supplier_selection_includes_freight_in_landed_price() {
        let distant = (
            SettlementId(1),
            MarketSeller::Business(shared::components::BuildingId(1)),
            affordable_quote(Good::Stone, 3, 200, 5_000, 600).unwrap(),
        );
        let nearby = (
            SettlementId(3),
            MarketSeller::Business(shared::components::BuildingId(2)),
            affordable_quote(Good::Stone, 3, 250, 5_000, 105).unwrap(),
        );
        assert_eq!(quote_order(&nearby, &distant), std::cmp::Ordering::Less);
    }

    #[test]
    fn rebid_and_expiry_preserve_cash_and_enforce_reposting_cooldown() {
        let mut contract = tender();
        let mut treasury = 2_000;
        let total_cash = treasury + contract.escrow_cash;
        let quote = affordable_quote(Good::Stone, 3, 300, total_cash, 330).unwrap();
        assert!(apply_quote(&mut contract, &mut treasury, quote));
        assert_eq!(treasury + contract.escrow_cash, total_cash);
        assert_eq!(contract.reserved_cash, contract.escrow_cash);
        let before_refund = treasury;
        let cheaper_quote = affordable_quote(Good::Stone, 3, 100, total_cash, 105).unwrap();
        assert!(apply_quote(&mut contract, &mut treasury, cheaper_quote));
        assert!(treasury > before_refund);
        assert_eq!(treasury + contract.escrow_cash, total_cash);
        assert!(!expire_open_tender(&mut contract, &mut treasury, 6));
        assert!(expire_open_tender(&mut contract, &mut treasury, 7));
        assert_eq!(treasury, total_cash);
        assert_eq!(contract.escrow_cash, 0);
        assert!(!expire_open_tender(&mut contract, &mut treasury, 8));
        assert_eq!(treasury, total_cash);
        assert!(blocks_new_tender(&contract, 7));
        assert!(blocks_new_tender(&contract, 8));
        assert!(!blocks_new_tender(&contract, 9));
    }

    #[test]
    fn committed_cargo_terms_and_cash_are_never_repriced_or_refunded() {
        for status in [
            TradeContractStatus::Assigned,
            TradeContractStatus::InTransit,
        ] {
            let mut contract = tender();
            contract.status = status;
            let before = contract;
            let mut treasury = 2_000;
            let quote = affordable_quote(Good::Stone, 3, 300, 2_966, 330).unwrap();
            assert!(!apply_quote(&mut contract, &mut treasury, quote));
            assert!(!expire_open_tender(&mut contract, &mut treasury, 100));
            assert_eq!(contract, before);
            assert_eq!(treasury, 2_000);
        }
    }

    #[test]
    fn daily_review_funds_a_real_wage_and_assigns_a_formerly_underpaid_load() {
        let mut app = App::new();
        app.add_systems(Update, manage_company_trade_routes);
        app.world_mut().spawn(WorldTime::new_default());
        let source = spawn_hall(&mut app, SettlementId(1), Vec3::ZERO, 0, 3, 300);
        // This cheaper offer has no warehouse or employee to carry it.
        spawn_hall(&mut app, SettlementId(3), Vec3::X * 2_990.0, 0, 3, 100);
        let destination = spawn_hall(&mut app, SettlementId(2), Vec3::X * 3_000.0, 2_000, 0, 0);
        app.world_mut().spawn((
            BuildingId(21),
            BuildingOf(SettlementId(1)),
            OperatedBy(CompanyId(11)),
            SettlementBuilding {
                kind: SettlementBuildingKind::StorageHall,
                settlement: "Town 1".into(),
                owner: Some("Carrier".into()),
                quality: 1.0,
                workers: vec!["Porter".into()],
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            BusinessWagePolicy {
                daily_wage: 180,
                ..default()
            },
        ));
        app.world_mut().spawn((
            CharacterKind::Villager,
            PersonId(40),
            CompanyPorter {
                settlement: source,
                settlement_id: SettlementId(1),
                company: CompanyId(11),
                storage_hall: BuildingId(21),
            },
            GoodsInventory::new(shared::economy::capacity::PORTER),
        ));
        let contract_entity = app.world_mut().spawn((TradeContractId(30), tender())).id();
        app.update();
        let contract = app
            .world()
            .get::<CivicTradeContract>(contract_entity)
            .unwrap();
        assert_eq!(contract.status, TradeContractStatus::Assigned);
        assert_eq!(contract.origin, Some(SettlementId(1)));
        assert_eq!(contract.maximum_unit_price, 300);
        assert_eq!(contract.delivery_fee_per_bulk, 19);
        assert_eq!(contract.escrow_cash, 1_242);
        assert_eq!(
            contract.escrow_cash + app.world().get::<Settlement>(destination).unwrap().treasury,
            2_966
        );
        let route = app
            .world_mut()
            .query::<&CompanyTradeRoute>()
            .single(app.world())
            .unwrap();
        assert_eq!(route.cargo_target, 3);
        assert_eq!(route.maximum_purchase_price, 300);
    }

    #[test]
    fn expired_tender_refunds_through_the_system_and_cannot_repost_until_cooldown() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (manage_company_trade_routes, post_civic_import_contracts).chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = OPEN_TENDER_LIFETIME_DAYS;
        spawn_hall(&mut app, SettlementId(1), Vec3::ZERO, 0, 0, 0);
        let destination = spawn_hall(&mut app, SettlementId(2), Vec3::X * 3_000.0, 2_000, 0, 0);
        spawn_project(&mut app);
        let contract_entity = app.world_mut().spawn((TradeContractId(30), tender())).id();
        for day in [7, 8] {
            app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
            app.update();
            assert_eq!(
                app.world().get::<Settlement>(destination).unwrap().treasury,
                2_966
            );
            assert_eq!(
                app.world()
                    .get::<CivicTradeContract>(contract_entity)
                    .unwrap()
                    .status,
                TradeContractStatus::Cancelled
            );
            assert_eq!(
                app.world_mut()
                    .query::<&CivicTradeContract>()
                    .iter(app.world())
                    .count(),
                1
            );
        }
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 9;
        app.update();
        let escrow: u64 = app
            .world_mut()
            .query::<&CivicTradeContract>()
            .iter(app.world())
            .map(|contract| contract.escrow_cash)
            .sum();
        assert!(escrow > 0);
        assert_eq!(
            escrow + app.world().get::<Settlement>(destination).unwrap().treasury,
            2_966
        );
        assert_eq!(
            app.world_mut()
                .query::<&CivicTradeContract>()
                .iter(app.world())
                .count(),
            2
        );
    }

    #[test]
    fn missing_refund_destination_keeps_the_cash_liability_intact() {
        let mut app = App::new();
        app.add_systems(Update, manage_company_trade_routes);
        let mut clock = WorldTime::new_default();
        clock.day = 7;
        app.world_mut().spawn(clock);
        let contract_entity = app.world_mut().spawn((TradeContractId(30), tender())).id();
        app.update();
        let contract = app
            .world()
            .get::<CivicTradeContract>(contract_entity)
            .unwrap();
        assert_eq!(contract.escrow_cash, 966);
        assert_eq!(contract.status, TradeContractStatus::Open);
    }

    #[test]
    fn initial_order_chooses_a_nearer_supplier_with_the_cheaper_landed_cost() {
        let mut app = App::new();
        app.add_systems(Update, post_civic_import_contracts);
        app.world_mut().spawn(WorldTime::new_default());
        spawn_hall(&mut app, SettlementId(1), Vec3::ZERO, 0, 3, 200);
        spawn_hall(&mut app, SettlementId(2), Vec3::X * 10_000.0, 5_000, 0, 0);
        spawn_hall(&mut app, SettlementId(3), Vec3::X * 9_960.0, 0, 3, 250);
        spawn_project(&mut app);
        app.update();
        let contract = app
            .world_mut()
            .query::<&CivicTradeContract>()
            .single(app.world())
            .unwrap();
        assert_eq!(contract.origin, Some(SettlementId(3)));
        assert_eq!(contract.maximum_unit_price, 250);
        assert_eq!(contract.escrow_cash, 966);
    }
}
