//! Cheap off-screen village simulation.
//!
//! People remain durable ECS records for identity, money and households, but
//! strategic residents carry no routes, door choreography or work phases.
//! Productive labour is integrated once per strategic tick by workplace.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use shared::components::{
    AttachedTo, BuildingDoorUse, BuildingId, BuildingOf, CharacterActivity, CharacterKind,
    CivicEmployment, CivicRole, EmployedAt, FarmField, MootAdministration, PlayerPosition,
    Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId, WorldTime,
};
use shared::economy::{BusinessAccount, BusinessSalePolicy, Good, GoodsInventory, MootMarket};
use shared::region::{RegionCoord, SimLevel};

use crate::player::hero::MoveTarget;
use crate::world::regions::{RegionRegistry, StrategicStep};
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, TravelRoute,
};

use super::{
    ambient, business_output, farmer_seconds_per_wheat, fisher_seconds_per_food, lumber_tree_yield,
    ordinary_workday, ConstructionMaterialRoutine, FarmerHarvestProgress, FarmerRoutine,
    FishingRoutine, FishingWorkProgress, HomeRoutine, HouseholdShoppingRoutine, LumberjackRoutine,
    LumberjackWorkProgress, MarketCollectionRoutine, PierTraversal, SettlementEconomyRuntime,
    WorkerOffDuty, WorkplaceDoorTransit, CHOP_SECONDS,
};

#[derive(Component, Debug, Clone, Copy)]
pub struct StrategicPerson;

/// A transaction or public work already in motion is allowed to reach a safe
/// boundary before the person's expensive tactical state is stripped.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PendingStrategicDemotion;

#[derive(Resource, Default)]
pub struct StrategicProductionProgress {
    seconds: HashMap<BuildingId, f64>,
}

#[allow(clippy::type_complexity)]
pub fn update_person_simulation_lod(
    mut commands: Commands,
    registry: Res<RegionRegistry>,
    people: Query<
        (
            Entity,
            &RegionCoord,
            Option<&StrategicPerson>,
            Has<ConstructionMaterialRoutine>,
            Has<RoadBuilderRoutine>,
            Has<MarketCollectionRoutine>,
            &super::VillagerIntent,
        ),
        (With<CharacterKind>, Without<shared::components::Hero>),
    >,
    changed_people: Query<
        (
            Entity,
            &RegionCoord,
            Option<&StrategicPerson>,
            Has<ConstructionMaterialRoutine>,
            Has<RoadBuilderRoutine>,
            Has<MarketCollectionRoutine>,
            &super::VillagerIntent,
        ),
        (
            With<CharacterKind>,
            Without<shared::components::Hero>,
            Changed<RegionCoord>,
        ),
    >,
    pending: Query<
        (
            Entity,
            &RegionCoord,
            Has<ConstructionMaterialRoutine>,
            Has<RoadBuilderRoutine>,
            Has<MarketCollectionRoutine>,
            &super::VillagerIntent,
        ),
        (
            With<CharacterKind>,
            Without<shared::components::Hero>,
            With<PendingStrategicDemotion>,
        ),
    >,
    mut last_revision: Local<Option<u64>>,
) {
    let revision_changed = *last_revision != Some(registry.revision());
    if revision_changed {
        *last_revision = Some(registry.revision());
        for (entity, region, strategic, construction, road, market, intent) in people.iter() {
            apply_person_lod(
                &mut commands,
                &registry,
                entity,
                *region,
                strategic.is_some(),
                construction
                    || road
                    || market
                    || matches!(intent, super::VillagerIntent::Travelling { .. }),
            );
        }
    } else {
        for (entity, region, strategic, construction, road, market, intent) in changed_people.iter()
        {
            apply_person_lod(
                &mut commands,
                &registry,
                entity,
                *region,
                strategic.is_some(),
                construction
                    || road
                    || market
                    || matches!(intent, super::VillagerIntent::Travelling { .. }),
            );
        }
    }

    for (entity, region, construction, road, market, intent) in pending.iter() {
        let critical = construction
            || road
            || market
            || matches!(intent, super::VillagerIntent::Travelling { .. });
        if !critical {
            apply_person_lod(&mut commands, &registry, entity, *region, false, false);
        }
    }
}

fn apply_person_lod(
    commands: &mut Commands,
    registry: &RegionRegistry,
    entity: Entity,
    region: RegionCoord,
    is_strategic: bool,
    critical_work: bool,
) {
    let level = registry
        .get(region)
        .map_or(SimLevel::Strategic, |state| state.sim_level);
    if level == SimLevel::Tactical {
        if is_strategic {
            commands.entity(entity).remove::<StrategicPerson>();
        }
        commands.entity(entity).remove::<PendingStrategicDemotion>();
        return;
    }
    if critical_work {
        commands.entity(entity).insert(PendingStrategicDemotion);
        return;
    }
    commands
        .entity(entity)
        .insert((StrategicPerson, CharacterActivity::Idle))
        .remove::<PendingStrategicDemotion>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .remove::<BuildingDoorUse>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<PierTraversal>()
        .remove::<HomeRoutine>()
        .remove::<HouseholdShoppingRoutine>()
        .remove::<FarmerRoutine>()
        .remove::<FishingRoutine>()
        .remove::<LumberjackRoutine>()
        .remove::<FarmerHarvestProgress>()
        .remove::<FishingWorkProgress>()
        .remove::<LumberjackWorkProgress>()
        .remove::<WorkerOffDuty>()
        .remove::<ambient::AmbientRoutine>();
}

/// Integrate physical output and porter collection for unobserved workers.
/// Money, storage limits, sale policy and Moot prices remain the exact live
/// systems; only walking and animation are collapsed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn advance_strategic_villages(
    step: Res<StrategicStep>,
    world_time: Query<&WorldTime>,
    mut last_serial: Local<u64>,
    mut progress: ResMut<StrategicProductionProgress>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    workers: Query<(&EmployedAt, Option<&StrategicPerson>)>,
    civic_workers: Query<(&CivicEmployment, Option<&StrategicPerson>)>,
    fields: Query<(&FarmField, Option<&AttachedTo>)>,
    hall_index: Query<(Entity, &SettlementId), With<Settlement>>,
    mut halls: Query<
        (&MootAdministration, &mut GoodsInventory, &mut MootMarket),
        (With<Settlement>, Without<SettlementBuilding>),
    >,
    mut businesses: Query<
        (
            &BuildingId,
            &BuildingOf,
            &SettlementBuilding,
            &PlayerPosition,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            &mut BusinessAccount,
        ),
        (With<SettlementBuilding>, Without<Settlement>),
    >,
) {
    if step.serial == 0 || *last_serial == step.serial {
        return;
    }
    *last_serial = step.serial;
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if !ordinary_workday(clock) {
        return;
    }

    let mut worker_counts: HashMap<BuildingId, u32> = HashMap::new();
    for (employment, strategic) in workers.iter() {
        if strategic.is_some() {
            *worker_counts.entry(employment.0).or_default() += 1;
        }
    }
    let halls_by_id: HashMap<SettlementId, Entity> = hall_index
        .iter()
        .map(|(entity, id)| (*id, entity))
        .collect();
    let strategic_porters: HashSet<SettlementId> = civic_workers
        .iter()
        .filter_map(|(employment, strategic)| {
            (employment.role == CivicRole::MarketPorter && strategic.is_some())
                .then_some(employment.settlement)
        })
        .collect();
    let mut fields_by_building: HashMap<BuildingId, u32> = HashMap::new();
    let field_counts: HashMap<(u32, u32, u32), u32> =
        fields
            .iter()
            .fold(HashMap::new(), |mut counts, (field, attached)| {
                if let Some(attached) = attached {
                    *fields_by_building.entry(attached.0).or_default() += 1;
                }
                *counts
                    .entry(super::farmstead_position_key(field.farmstead))
                    .or_default() += 1;
                counts
            });
    let mut live_buildings = HashSet::new();

    for (id, building_of, building, position, mut store, policy, mut account) in
        businesses.iter_mut()
    {
        live_buildings.insert(*id);
        let worker_count = worker_counts.get(id).copied().unwrap_or(0);
        if worker_count == 0 {
            continue;
        }
        let Some(good) = business_output(building.kind) else {
            continue;
        };
        let field_factor = if building.kind == SettlementBuildingKind::Farmstead {
            fields_by_building
                .get(id)
                .copied()
                .or_else(|| {
                    field_counts
                        .get(&super::farmstead_position_key(position.0))
                        .copied()
                })
                .unwrap_or(0)
                .min(2) as f64
                / 2.0
        } else {
            1.0
        };
        let seconds_per_unit = match building.kind {
            SettlementBuildingKind::Farmstead => farmer_seconds_per_wheat(building.quality),
            SettlementBuildingKind::FishermansHut => fisher_seconds_per_food(building.quality),
            SettlementBuildingKind::LumberjackHut => CHOP_SECONDS,
            _ => continue,
        } as f64;
        let accumulated = progress.seconds.entry(*id).or_default();
        *accumulated +=
            step.elapsed_world_seconds * f64::from(worker_count) * field_factor.max(0.0);
        let cycles = (*accumulated / seconds_per_unit).floor() as u32;
        if cycles > 0 {
            *accumulated -= f64::from(cycles) * seconds_per_unit;
            let units = if good == Good::Wood {
                cycles.saturating_mul(lumber_tree_yield(building.quality))
            } else {
                cycles
            };
            let produced = store.add(good, units);
            if good.is_edible() {
                if let Some(hall) = halls_by_id.get(&building_of.0) {
                    economy_runtime.record_food_production(*hall, produced);
                }
            }
        }

        let Some(hall_entity) = halls_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let Ok((administration, mut hall_store, mut market)) = halls.get_mut(hall_entity) else {
            continue;
        };
        // A porter finishing a real delivery is transition-critical and has
        // not demoted yet. Do not simultaneously execute its abstract haul.
        if administration.market_porter.is_none()
            || !strategic_porters.contains(&building_of.0)
            || !policy.collection_enabled
            || market.pool(good).bid < policy.minimum_unit_price
        {
            continue;
        }
        let offered = store
            .amount(good)
            .saturating_sub(policy.keep_units)
            .min(policy.max_units_per_collection)
            .min(hall_store.free_bulk() / good.bulk_per_unit());
        let trade = market.buy_from_producer(good, hall_store.amount(good), offered);
        if trade.units == 0 {
            continue;
        }
        let moved = store.transfer_to(&mut hall_store, good, trade.units);
        if moved == trade.units {
            account.cash = account.cash.saturating_add(trade.pennies);
        } else {
            market.cancel_producer_purchase(good, hall_store.amount(good), trade);
        }
    }
    progress
        .seconds
        .retain(|building, _| live_buildings.contains(building));
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{CharacterName, SettlementTier};

    #[test]
    fn offscreen_people_drop_routes_but_migrants_finish_their_journey() {
        let mut app = App::new();
        app.init_resource::<RegionRegistry>();
        app.init_resource::<shared::terrain::WorldTerrain>();
        app.add_systems(
            Update,
            (
                crate::world::regions::build_region_registry,
                update_person_simulation_lod,
            )
                .chain(),
        );
        let resident = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Resident".into()),
                RegionCoord::new(0, 0),
                super::super::VillagerIntent::Resident {
                    settlement: Entity::PLACEHOLDER,
                },
                MoveTarget(Vec3::X),
            ))
            .id();
        let migrant = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Migrant".into()),
                RegionCoord::new(0, 0),
                super::super::VillagerIntent::Travelling {
                    settlement: Entity::PLACEHOLDER,
                },
                MoveTarget(Vec3::X),
            ))
            .id();

        app.update();

        assert!(app.world().get::<StrategicPerson>(resident).is_some());
        assert!(app.world().get::<MoveTarget>(resident).is_none());
        assert!(app.world().get::<StrategicPerson>(migrant).is_none());
        assert!(app.world().get::<MoveTarget>(migrant).is_some());
    }

    #[test]
    fn two_fields_give_a_strategic_farm_full_output_and_one_field_gives_half() {
        let mut app = App::new();
        app.init_resource::<StrategicStep>();
        app.init_resource::<StrategicProductionProgress>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(Update, advance_strategic_villages);
        app.world_mut().spawn(WorldTime::new_default());

        let settlement_id = SettlementId(1);
        app.world_mut().spawn((
            settlement_id,
            Settlement {
                name: "Test".into(),
                tier: SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            },
            MootAdministration {
                market_porter: Some("Porter".into()),
                ..default()
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
            MootMarket::founding(),
        ));
        let building_id = BuildingId(7);
        let farm_position = Vec3::new(10.0, 0.0, 10.0);
        let farm = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(settlement_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Test".into(),
                    owner: Some("Farmer".into()),
                    quality: 1.0,
                    workers: vec!["Farmer".into()],
                },
                PlayerPosition(farm_position),
                GoodsInventory::new(100),
                BusinessSalePolicy {
                    collection_enabled: false,
                    ..default()
                },
                BusinessAccount::default(),
            ))
            .id();
        app.world_mut().spawn((
            FarmField {
                settlement: "Test".into(),
                farmstead: farm_position,
                plot_index: 0,
                quality: 1.0,
            },
            AttachedTo(building_id),
        ));
        let second_field = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Test".into(),
                    farmstead: farm_position,
                    plot_index: 1,
                    quality: 1.0,
                },
                AttachedTo(building_id),
            ))
            .id();
        app.world_mut()
            .spawn((EmployedAt(building_id), StrategicPerson));

        app.world_mut().resource_mut::<StrategicStep>().serial = 1;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 340.0;
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Wheat),
            2
        );

        app.world_mut().despawn(second_field);
        app.world_mut().resource_mut::<StrategicStep>().serial = 2;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 340.0;
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(farm)
                .unwrap()
                .amount(Good::Wheat),
            3
        );
    }

    #[test]
    fn abstract_collection_waits_until_the_market_porter_is_strategic() {
        let mut app = App::new();
        app.init_resource::<StrategicStep>();
        app.init_resource::<StrategicProductionProgress>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(Update, advance_strategic_villages);
        app.world_mut().spawn(WorldTime::new_default());

        let settlement_id = SettlementId(3);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Porter Test".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 0,
                },
                MootAdministration {
                    market_porter: Some("Porter".into()),
                    ..default()
                },
                GoodsInventory::new(shared::economy::capacity::HALL),
                MootMarket::founding(),
            ))
            .id();
        let building_id = BuildingId(11);
        let mut stock = GoodsInventory::new(100);
        stock.add(Good::Wood, 10);
        let business = app
            .world_mut()
            .spawn((
                building_id,
                BuildingOf(settlement_id),
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Porter Test".into(),
                    owner: Some("Worker".into()),
                    quality: 1.0,
                    workers: vec!["Worker".into()],
                },
                PlayerPosition(Vec3::ZERO),
                stock,
                BusinessSalePolicy::default(),
                BusinessAccount::default(),
            ))
            .id();
        app.world_mut()
            .spawn((EmployedAt(building_id), StrategicPerson));
        let porter = app
            .world_mut()
            .spawn(CivicEmployment {
                settlement: settlement_id,
                role: CivicRole::MarketPorter,
            })
            .id();

        app.world_mut().resource_mut::<StrategicStep>().serial = 1;
        app.world_mut()
            .resource_mut::<StrategicStep>()
            .elapsed_world_seconds = 1.0;
        app.update();
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood),
            0
        );

        app.world_mut().entity_mut(porter).insert(StrategicPerson);
        app.world_mut().resource_mut::<StrategicStep>().serial = 2;
        app.update();
        assert!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Wood)
                > 0
        );
        assert!(app.world().get::<BusinessAccount>(business).unwrap().cash > 0);
    }
}
