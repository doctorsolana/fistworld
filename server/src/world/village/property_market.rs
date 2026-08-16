//! Small settlement-level directory of private property offered for takeover.
//!
//! Detailed buildings remain ordinary replicated world entities. The hall also
//! publishes this bounded summary so its market menu remains complete in a
//! large settlement where an outer workplace may be beyond detail interest.

use super::*;

use shared::components::{PropertyListingStage, PropertyMarketListing, SettlementPropertyBoard};

/// An empty completed business gets several full market days to find a buyer
/// before its unused structure decays out of the embodied world. Liquidation,
/// creditor claims, physical stock and in-flight cargo all finish first, so
/// decay is never a shortcut that destroys somebody's property.
pub(crate) const ABANDONED_BUSINESS_REMOVAL_DAYS: u32 = 3;

/// Rebuild a hall's property board from authoritative sale components.
///
/// The assignment is diff-gated: an empty/stable market causes no replication
/// churn even though this cheap fold runs after the daily economy pipeline.
pub fn publish_property_boards(
    mut commands: Commands,
    settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        Option<&SettlementPropertyBoard>,
    )>,
    completed: Query<(
        &shared::components::BuildingOf,
        &SettlementBuilding,
        &PlayerPosition,
        &BusinessForSale,
    )>,
    worksites: Query<(&UnderConstruction, &BusinessForSale)>,
) {
    for (hall, settlement_id, current) in settlements.iter() {
        let mut listings = completed
            .iter()
            .filter(|(building_of, ..)| building_of.0 == *settlement_id)
            .map(|(_, building, position, listing)| PropertyMarketListing {
                kind: building.kind,
                stage: PropertyListingStage::CompletedBusiness,
                asking_price: listing.asking_price,
                listed_day: listing.listed_day,
                reason: listing.reason,
                position: position.0,
            })
            .chain(
                worksites
                    .iter()
                    .filter(|(site, _)| site.settlement_id == *settlement_id)
                    .map(|(site, listing)| PropertyMarketListing {
                        kind: site.kind,
                        stage: PropertyListingStage::UnfinishedWorksite,
                        asking_price: listing.asking_price,
                        listed_day: listing.listed_day,
                        reason: listing.reason,
                        position: site.position,
                    }),
            )
            .collect::<Vec<_>>();
        listings.sort_by(|a, b| {
            a.listed_day
                .cmp(&b.listed_day)
                .then_with(|| a.kind.label().cmp(b.kind.label()))
                .then_with(|| a.position.x.total_cmp(&b.position.x))
                .then_with(|| a.position.z.total_cmp(&b.position.z))
        });
        let next = SettlementPropertyBoard { listings };
        if current != Some(&next) {
            commands.entity(hall).insert(next);
        }
    }
}

/// Remove completed business shells which remained abandoned after their
/// takeover window.
///
/// Roads are durable settlement infrastructure and deliberately remain as an
/// old spur. Farm fields and fishing piers belong to the removed workplace,
/// however, and disappear with it. A non-empty store, market listing, worker,
/// liability or physical delivery keeps the property alive until the ordinary
/// liquidation systems have resolved it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn remove_abandoned_businesses(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut reviewed_day: Local<Option<u32>>,
    businesses: Query<
        (
            Entity,
            &shared::components::BuildingId,
            &shared::components::BuildingOf,
            &SettlementBuilding,
            &GoodsInventory,
            &BusinessAccount,
            &BusinessCondition,
            &BusinessForSale,
            Option<&RoadRequest>,
        ),
        Without<BusinessLiquidation>,
    >,
    markets: Query<(&shared::components::SettlementId, &MootMarket), With<Settlement>>,
    adjuncts: Query<
        (Entity, &shared::components::AttachedTo),
        Or<(With<FarmField>, With<FishingPier>)>,
    >,
    workers: Query<&shared::components::EmployedAt>,
    collections: Query<&MarketCollectionRoutine>,
    deliveries: Query<&InternalDeliveryRoutine>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *reviewed_day == Some(day) {
        return;
    }
    *reviewed_day = Some(day);

    let markets: HashMap<_, _> = markets.iter().map(|(id, market)| (*id, market)).collect();
    for (
        entity,
        building_id,
        building_of,
        building,
        inventory,
        account,
        condition,
        listing,
        road_request,
    ) in businesses.iter()
    {
        if condition.state != BusinessState::ForSale
            || day.saturating_sub(listing.listed_day) < ABANDONED_BUSINESS_REMOVAL_DAYS
            || inventory.used_bulk() > 0
            || account.wage_arrears > 0
            || account.tax_arrears > 0
            || road_request.is_some()
            || workers
                .iter()
                .any(|employment| employment.0 == *building_id)
            || collections
                .iter()
                .any(|routine| routine.business == entity || routine.seller == *building_id)
            || deliveries.iter().any(|routine| {
                routine.supplier == entity
                    || routine.receiver == entity
                    || routine.supplier_id == *building_id
                    || routine.receiver_id == *building_id
            })
            || markets.get(&building_of.0).is_some_and(|market| {
                market.seller_total_listed_units(MarketSeller::Business(*building_id)) > 0
            })
        {
            continue;
        }

        for (adjunct, attached_to) in adjuncts.iter() {
            if attached_to.0 == *building_id {
                commands.entity(adjunct).despawn();
            }
        }
        commands.entity(entity).despawn();
        info!(
            "Village '{}': abandoned {} decayed after {} days without a buyer",
            building.settlement,
            building.kind.label(),
            day.saturating_sub(listing.listed_day),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{SettlementBuildingKind, SettlementId, SettlementTier};
    use shared::economy::{BusinessSaleReason, PENNIES_PER_COIN};

    fn sale(day: u32, reason: BusinessSaleReason) -> BusinessForSale {
        BusinessForSale {
            previous_owner: shared::components::PersonId(99),
            asking_price: 3 * PENNIES_PER_COIN,
            listed_day: day,
            reason,
        }
    }

    #[test]
    fn hall_board_collects_completed_and_unfinished_listings_then_clears() {
        let mut app = App::new();
        app.add_systems(Update, publish_property_boards);
        let settlement_id = SettlementId(7);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Market Cross".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 0,
                },
            ))
            .id();
        let business = app
            .world_mut()
            .spawn((
                shared::components::BuildingOf(settlement_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Bakery,
                    settlement: "Market Cross".into(),
                    owner: None,
                    quality: 0.8,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(10.0, 0.0, 4.0)),
                sale(4, BusinessSaleReason::Insolvent),
            ))
            .id();
        let worksite = app
            .world_mut()
            .spawn((
                UnderConstruction {
                    kind: SettlementBuildingKind::Windmill,
                    position: Vec3::new(20.0, 0.0, 8.0),
                    rotation: 0.0,
                    owner: None,
                    owner_id: None,
                    builder: None,
                    settlement: hall,
                    settlement_id,
                    stand: Vec3::new(20.0, 0.0, 3.0),
                    failed_stand_routes: 0,
                    stage: BuildStage::Supplying,
                    quality: 1.0,
                },
                sale(3, BusinessSaleReason::OwnerDied),
            ))
            .id();

        app.update();
        let board = app.world().get::<SettlementPropertyBoard>(hall).unwrap();
        assert_eq!(board.listings.len(), 2);
        assert_eq!(
            board.listings[0].stage,
            PropertyListingStage::UnfinishedWorksite
        );
        assert_eq!(
            board.listings[1].stage,
            PropertyListingStage::CompletedBusiness
        );

        app.world_mut()
            .entity_mut(business)
            .remove::<BusinessForSale>();
        app.world_mut()
            .entity_mut(worksite)
            .remove::<BusinessForSale>();
        app.update();
        assert!(app
            .world()
            .get::<SettlementPropertyBoard>(hall)
            .unwrap()
            .listings
            .is_empty());
    }

    #[test]
    fn empty_abandoned_business_and_its_adjunct_decay_but_stocked_property_remains() {
        let mut app = App::new();
        app.add_systems(Update, remove_abandoned_businesses);
        let mut clock = WorldTime::new_default();
        clock.day = ABANDONED_BUSINESS_REMOVAL_DAYS;
        app.world_mut().spawn(clock);

        let settlement_id = SettlementId(8);
        app.world_mut().spawn((
            settlement_id,
            Settlement {
                name: "Fallows".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            GoodsInventory::new(SettlementBuildingKind::Hall.storage_bulk_capacity()),
            MootMarket::founding(),
        ));

        let spawn_business = |app: &mut App, id: shared::components::BuildingId, stock: u32| {
            let mut inventory =
                GoodsInventory::new(SettlementBuildingKind::Farmstead.storage_bulk_capacity());
            inventory.add(Good::Wheat, stock);
            let mut condition = BusinessCondition::default();
            condition.state = BusinessState::ForSale;
            app.world_mut()
                .spawn((
                    id,
                    shared::components::BuildingOf(settlement_id),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::Farmstead,
                        settlement: "Fallows".into(),
                        owner: None,
                        quality: 0.8,
                        workers: Vec::new(),
                    },
                    inventory,
                    BusinessAccount::default(),
                    condition,
                    sale(0, BusinessSaleReason::Insolvent),
                ))
                .id()
        };

        let empty_id = shared::components::BuildingId(801);
        let empty = spawn_business(&mut app, empty_id, 0);
        let field = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Fallows".into(),
                    farmstead: Vec3::ZERO,
                    plot_index: 0,
                    quality: 0.8,
                },
                shared::components::AttachedTo(empty_id),
            ))
            .id();
        let stocked = spawn_business(&mut app, shared::components::BuildingId(802), 1);

        app.update();

        assert!(app.world().get_entity(empty).is_err());
        assert!(app.world().get_entity(field).is_err());
        assert!(app.world().get_entity(stocked).is_ok());
    }
}
