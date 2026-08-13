//! Small settlement-level directory of private property offered for takeover.
//!
//! Detailed buildings remain ordinary replicated world entities. The hall also
//! publishes this bounded summary so its market menu remains complete in a
//! large settlement where an outer workplace may be beyond detail interest.

use super::*;

use shared::components::{PropertyListingStage, PropertyMarketListing, SettlementPropertyBoard};

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
}
