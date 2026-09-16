//! Test-only evidence bracketing the real abandoned-property removal system.
//! Retirement is never inferred merely from a building vanishing in a snapshot.

use super::*;
use shared::economy::{BusinessForSale, BusinessLiquidation, BusinessState};

struct Removal {
    building: BuildingId,
    kind: SettlementBuildingKind,
    day: u32,
    listed_day: u32,
}

#[derive(Resource, Default)]
pub(super) struct Audit {
    day: Option<u32>,
    candidates: Vec<(Entity, Removal)>,
    confirmed: HashSet<BuildingId>,
    history: Vec<Removal>,
}

pub(super) fn configure(app: &mut App) {
    use village::schedule::VillageEconomySet;
    app.init_resource::<Audit>();
    app.add_systems(
        Update,
        (
            observe_candidates
                .after(village::acquire_businesses_for_sale)
                .before(village::remove_abandoned_businesses),
            confirm_removals.after(village::remove_abandoned_businesses),
        )
            .in_set(VillageEconomySet::MarketsBusinesses),
    );
}

/// The production remover owns the elapsed sale window and private delivery
/// reservations. This observer records public property/finance eligibility
/// immediately before it, then confirms removal by that actual system. It does
/// not inspect private cargo fields or duplicate the production delivery policy.
/// Both observers are bounded to one pass per world day, like the remover.
#[allow(clippy::type_complexity)]
fn observe_candidates(
    clock: Query<&WorldTime>,
    mut audit: ResMut<Audit>,
    businesses: Query<
        (
            Entity,
            &BuildingId,
            &BuildingOf,
            &SettlementBuilding,
            &GoodsInventory,
            &BusinessAccount,
            &BusinessCondition,
            &BusinessForSale,
            Option<&village_roads::RoadRequest>,
        ),
        Without<BusinessLiquidation>,
    >,
    markets: Query<(&SettlementId, &MootMarket), With<Settlement>>,
    workers: Query<&EmployedAt>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };
    if audit.day == Some(day) {
        return;
    }
    audit.day = Some(day);
    audit.candidates.clear();
    let assigned: HashSet<_> = workers.iter().map(|job| job.0).collect();
    let markets: HashMap<_, _> = markets.iter().map(|(id, market)| (*id, market)).collect();
    for (entity, id, town, building, stock, account, condition, sale, road) in &businesses {
        if condition.state != BusinessState::ForSale
            || day < sale.listed_day
            || !stock.is_empty()
            || account.wage_arrears > 0
            || account.tax_arrears > 0
            || road.is_some()
            || assigned.contains(id)
            || markets.get(&town.0).is_some_and(|market| {
                market.seller_total_listed_units(MarketSeller::Business(*id)) > 0
            })
        {
            continue;
        }
        audit.candidates.push((
            entity,
            Removal {
                building: *id,
                kind: building.kind,
                day,
                listed_day: sale.listed_day,
            },
        ));
    }
}

fn confirm_removals(mut audit: ResMut<Audit>, entities: Query<Entity>) {
    for (entity, evidence) in std::mem::take(&mut audit.candidates) {
        if entities.contains(entity) {
            continue;
        }
        audit.confirmed.insert(evidence.building);
        audit.history.push(evidence);
    }
}

impl Audit {
    pub(super) fn retire_missing(
        &mut self,
        present: &HashSet<BuildingId>,
        accepted: &mut HashMap<BuildingId, (SettlementBuildingKind, Vec3, f32)>,
    ) {
        accepted.retain(|id, _| {
            if present.contains(id) {
                return true;
            }
            assert!(
                self.confirmed.remove(id),
                "growth removed accepted building {id:?} without audited abandonment"
            );
            false
        });
    }

    pub(super) fn diagnostics(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.history
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "building": entry.building,
                        "kind": entry.kind,
                        "day": entry.day,
                        "listed_day": entry.listed_day,
                    })
                })
                .collect(),
        )
    }
}

#[test]
fn only_the_real_completed_abandonment_is_authorized() {
    let mut app = App::new();
    app.init_resource::<Audit>();
    app.add_systems(
        Update,
        (
            observe_candidates,
            village::remove_abandoned_businesses,
            confirm_removals,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let id = BuildingId(401);
    let entity = app
        .world_mut()
        .spawn((
            id,
            BuildingOf(SettlementId(1)),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Audit".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            GoodsInventory::new(100),
            BusinessAccount::default(),
            BusinessCondition {
                state: BusinessState::ForSale,
                ..default()
            },
            BusinessForSale {
                previous_owner: PersonId(1),
                asking_price: 100,
                listed_day: 0,
                reason: shared::economy::BusinessSaleReason::Insolvent,
            },
        ))
        .id();
    app.update();
    assert!(app.world().get_entity(entity).is_ok());
    assert!(app.world().resource::<Audit>().confirmed.is_empty());
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 100;
    app.world_mut()
        .get_mut::<GoodsInventory>(entity)
        .unwrap()
        .add(Good::Wood, 1);
    app.update();
    assert!(
        app.world().get_entity(entity).is_ok(),
        "stock still protects a listed site"
    );
    assert!(app.world().resource::<Audit>().confirmed.is_empty());
    app.world_mut()
        .get_mut::<GoodsInventory>(entity)
        .unwrap()
        .remove(Good::Wood, 1);
    app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 101;
    app.update();
    assert!(app.world().get_entity(entity).is_err());
    let mut accepted = HashMap::from([(id, (SettlementBuildingKind::Farmstead, Vec3::ZERO, 0.0))]);
    app.world_mut()
        .resource_mut::<Audit>()
        .retire_missing(&HashSet::new(), &mut accepted);
    assert!(accepted.is_empty());
    assert_eq!(
        app.world().resource::<Audit>().diagnostics()[0]["building"],
        serde_json::json!(id)
    );
}

#[test]
#[should_panic(expected = "without audited abandonment")]
fn an_unexplained_missing_building_still_fails() {
    let mut accepted = HashMap::from([(
        BuildingId(402),
        (SettlementBuildingKind::House, Vec3::ZERO, 0.0),
    )]);
    Audit::default().retire_missing(&HashSet::new(), &mut accepted);
}
