//! Append-only settlement plans, tier progression and material road upgrades.
//!
//! A charter biases future choices; it never owns a mutable list of final plots.
//! Demand can therefore add an unexpected farm without moving any structure
//! that already exists.

use bevy::prelude::*;
use shared::building::PlacedBuilding;
#[cfg(test)]
use shared::components::MootAdministration;
use shared::components::{
    BuildingOf, CivicEmployment, CivicHallLevel, CivicRole, MarketLevel, PlayerPosition,
    PlayerRotation, RoadClass, RoadSurface, Settlement, SettlementBuilding, SettlementBuildingKind,
    SettlementDevelopment, SettlementId, SettlementProgressGate, SettlementTier, VillageRoad,
    WorldTime,
};
use shared::economy::{
    Good, GoodsInventory, MootMarket, SettlementEconomy, CITY_MIN_PROSPERITY, CITY_MIN_RESIDENTS,
    CITY_REQUIRED_DAYS, TOWN_MIN_MARKET_VOLUME, TOWN_MIN_PROSPERITY, TOWN_MIN_RESIDENTS,
    TOWN_REQUIRED_DAYS,
};

pub fn ensure_settlement_developments(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    settlements: Query<(Entity, &Settlement, &PlayerPosition), Without<SettlementDevelopment>>,
) {
    let day = clock.iter().next().map_or(0, |clock| clock.day);
    for (entity, settlement, position) in settlements.iter() {
        let development = SettlementDevelopment::from_foundation(&settlement.name, position.0, day);
        info!(
            "Settlement '{}': charter {} / {} centre / seed {}",
            settlement.name,
            development.layout.label(),
            development.center.label(),
            development.plan_seed,
        );
        commands.entity(entity).insert(development);
    }
}

/// Keep the physical Hall rung explicit while promotion still completes the
/// upgrade immediately. The component is the future construction seam: a
/// treasury-funded project can later leave it below the unlocked settlement
/// tier without replacing the authoritative settlement entity.
pub fn sync_civic_hall_levels(
    mut commands: Commands,
    settlements: Query<(Entity, &Settlement, Option<&CivicHallLevel>)>,
) {
    for (entity, settlement, current) in settlements.iter() {
        let desired = CivicHallLevel::for_tier(settlement.tier);
        if current.is_some_and(|current| *current == desired) {
            continue;
        }
        if let Some(previous) = current {
            info!(
                "Settlement '{}': civic building upgraded from {} to {}",
                settlement.name,
                previous.label(),
                desired.label()
            );
        }
        commands.entity(entity).insert(desired);
    }
}

/// Upgrade a completed marketplace's physical finish without replacing its
/// authoritative building entity. Inventory, ownership, jobs and road links
/// all remain attached while the local art/navigation record changes from the
/// earthen square to its identically sized paved counterpart.
pub fn sync_market_levels(
    mut commands: Commands,
    settlements: Query<(&SettlementId, &Settlement)>,
    markets: Query<(
        Entity,
        &SettlementBuilding,
        &BuildingOf,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&PlacedBuilding>,
    )>,
) {
    for (entity, building, building_of, rotation, current, placed) in markets.iter() {
        if building.kind != SettlementBuildingKind::Market {
            continue;
        }
        let Some((_, settlement)) = settlements
            .iter()
            .find(|(settlement_id, _)| **settlement_id == building_of.0)
        else {
            continue;
        };
        let desired = MarketLevel::for_tier(settlement.tier);
        let desired_art = desired.building_type();
        if !current.is_some_and(|current| *current == desired) {
            if let Some(previous) = current {
                info!(
                    "Settlement '{}': marketplace upgraded from {} to {}",
                    settlement.name,
                    previous.label(),
                    desired.label()
                );
            }
            commands.entity(entity).insert(desired);
        }
        if !placed.is_some_and(|placed| {
            placed.building_type == desired_art && placed.rotation.to_bits() == rotation.0.to_bits()
        }) {
            commands.entity(entity).insert(PlacedBuilding {
                building_type: desired_art,
                rotation: rotation.0,
            });
        }
    }
}

fn has_building(
    buildings: &Query<(&SettlementBuilding, &shared::components::BuildingOf)>,
    settlement: shared::components::SettlementId,
    kind: SettlementBuildingKind,
) -> bool {
    buildings
        .iter()
        .any(|(building, building_of)| building_of.0 == settlement && building.kind == kind)
}

/// Keep the promotion ledger current and promote only after all visible
/// requirements remain true for the advertised number of whole days.
pub fn update_settlement_developments(
    clock: Query<&WorldTime>,
    mut settlements: Query<(
        &shared::components::SettlementId,
        &mut Settlement,
        &SettlementEconomy,
        &MootMarket,
        &mut SettlementDevelopment,
    )>,
    buildings: Query<(&SettlementBuilding, &shared::components::BuildingOf)>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };

    for (settlement_id, mut settlement, economy, market, mut development) in settlements.iter_mut()
    {
        let mut dirt = 0u16;
        let mut stone = 0u16;
        let mut committed = 0u32;
        let mut stone_needed = 0u32;
        for (road, _) in roads
            .iter()
            .filter(|(road, road_of)| road_of.0 == *settlement_id && road.is_complete())
        {
            committed = committed.saturating_add(road.stone_committed);
            match road.surface {
                RoadSurface::Dirt => {
                    dirt = dirt.saturating_add(1);
                    if settlement.tier >= SettlementTier::Town
                        && road.class == RoadClass::Main
                        && stone_needed == 0
                    {
                        stone_needed = road.stone_required().saturating_sub(road.stone_committed);
                    }
                }
                RoadSurface::Stone => stone = stone.saturating_add(1),
            }
        }
        if development.dirt_roads != dirt {
            development.dirt_roads = dirt;
        }
        if development.stone_roads != stone {
            development.stone_roads = stone;
        }
        if development.stone_committed != committed {
            development.stone_committed = committed;
        }
        if development.stone_needed != stone_needed {
            development.stone_needed = stone_needed;
        }

        let elapsed_days = day.saturating_sub(development.last_progress_day);
        if development.last_progress_day != day {
            development.last_progress_day = day;
        }

        let (gate, all_met, required_days) = match settlement.tier {
            SettlementTier::Ruins => (SettlementProgressGate::FoodSecurity, false, 0),
            SettlementTier::Hamlet => {
                let progress_days = economy.food_secure_days;
                let required_days = shared::economy::VILLAGE_REQUIRED_SECURE_DAYS;
                let next_gate = if settlement.residents < shared::economy::VILLAGE_MIN_RESIDENTS {
                    SettlementProgressGate::Population
                } else if economy.prosperity < shared::economy::VILLAGE_MIN_PROSPERITY {
                    SettlementProgressGate::Prosperity
                } else {
                    SettlementProgressGate::FoodSecurity
                };
                if development.progress_days != progress_days {
                    development.progress_days = progress_days;
                }
                if development.required_days != required_days {
                    development.required_days = required_days;
                }
                if development.next_gate != next_gate {
                    development.next_gate = next_gate;
                }
                continue;
            }
            SettlementTier::Village => {
                let market_built =
                    has_building(&buildings, *settlement_id, SettlementBuildingKind::Market);
                let tavern_built =
                    has_building(&buildings, *settlement_id, SettlementBuildingKind::Tavern);
                let gate = if settlement.residents < TOWN_MIN_RESIDENTS {
                    SettlementProgressGate::Population
                } else if !market_built {
                    SettlementProgressGate::Marketplace
                } else if !tavern_built {
                    SettlementProgressGate::Tavern
                } else if market.total_volume() < TOWN_MIN_MARKET_VOLUME {
                    SettlementProgressGate::Trade
                } else if economy.prosperity < TOWN_MIN_PROSPERITY {
                    SettlementProgressGate::Prosperity
                } else {
                    SettlementProgressGate::Sustaining
                };
                (
                    gate,
                    gate == SettlementProgressGate::Sustaining,
                    TOWN_REQUIRED_DAYS,
                )
            }
            SettlementTier::Town => {
                let church_built =
                    has_building(&buildings, *settlement_id, SettlementBuildingKind::Church);
                let gate = if settlement.residents < CITY_MIN_RESIDENTS {
                    SettlementProgressGate::Population
                } else if !church_built {
                    SettlementProgressGate::Church
                } else if economy.prosperity < CITY_MIN_PROSPERITY {
                    SettlementProgressGate::Prosperity
                } else {
                    SettlementProgressGate::Sustaining
                };
                (
                    gate,
                    gate == SettlementProgressGate::Sustaining,
                    CITY_REQUIRED_DAYS,
                )
            }
            SettlementTier::City => (SettlementProgressGate::Complete, false, 0),
        };

        if development.next_gate != gate {
            development.next_gate = gate;
        }
        if development.required_days != required_days {
            development.required_days = required_days;
        }
        let progress_days = if all_met {
            development
                .progress_days
                .saturating_add(elapsed_days.min(u32::from(u16::MAX)) as u16)
        } else {
            0
        };
        if development.progress_days != progress_days {
            development.progress_days = progress_days;
        }

        if required_days > 0 && development.progress_days >= required_days {
            let previous = settlement.tier;
            settlement.tier = match settlement.tier {
                SettlementTier::Village => SettlementTier::Town,
                SettlementTier::Town => SettlementTier::City,
                other => other,
            };
            if settlement.tier != previous {
                development.progress_days = 0;
                development.next_gate = match settlement.tier {
                    SettlementTier::Town => SettlementProgressGate::Church,
                    SettlementTier::City => SettlementProgressGate::Complete,
                    _ => development.next_gate,
                };
                info!(
                    "Settlement '{}' advanced from {} to {}",
                    settlement.name,
                    previous.label(),
                    settlement.tier.label()
                );
            }
        }
    }
}

/// Upgrade one unit of a principal road per elapsed day. Stone is removed from
/// the bounded hall inventory first, and the surface flips only when the whole
/// road has been paid for.
pub fn upgrade_town_roads(
    clock: Query<&WorldTime>,
    mut halls: Query<(
        &Settlement,
        &shared::components::SettlementId,
        &PlayerPosition,
        Option<&PlayerRotation>,
        &mut SettlementDevelopment,
        &mut GoodsInventory,
    )>,
    mut roads: Query<(Entity, &mut VillageRoad, &shared::components::RoadOf)>,
    civic_workers: Query<&CivicEmployment>,
) {
    let Some(day) = clock.iter().next().map(|clock| clock.day) else {
        return;
    };

    for (settlement, settlement_id, hall, rotation, mut development, mut inventory) in
        halls.iter_mut()
    {
        let has_city_worker = civic_workers.iter().any(|employment| {
            employment.settlement == *settlement_id
                && matches!(
                    employment.role,
                    CivicRole::MootSteward | CivicRole::RoadSteward | CivicRole::CityWorker
                )
        });
        if settlement.tier < SettlementTier::Town || !has_city_worker {
            if development.last_road_work_day != day {
                development.last_road_work_day = day;
            }
            continue;
        }
        let elapsed = day.saturating_sub(development.last_road_work_day);
        if elapsed == 0 {
            continue;
        }
        development.last_road_work_day = day;

        // Old roads predate hierarchy metadata. Promote the hall connector
        // rather than leaving an upgraded save with no eligible main street.
        let has_main = roads.iter().any(|(_, road, road_of)| {
            road_of.0 == *settlement_id && road.is_complete() && road.class == RoadClass::Main
        });
        if !has_main {
            let door = SettlementBuildingKind::Hall
                .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
            let door = Vec2::new(door.x, door.z);
            let candidate = roads
                .iter()
                .filter(|(_, road, road_of)| road_of.0 == *settlement_id && road.is_complete())
                .min_by(|(_, a, _), (_, b, _)| {
                    let distance = |road: &VillageRoad| {
                        road.built_points()
                            .iter()
                            .map(|point| point.distance_squared(door))
                            .fold(f32::INFINITY, f32::min)
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .map(|(entity, _, _)| entity);
            if let Some(candidate) = candidate {
                if let Ok((_, mut road, _)) = roads.get_mut(candidate) {
                    road.class = RoadClass::Main;
                    road.widen_within_reservation(4.0);
                }
            }
        }

        let candidate = roads
            .iter()
            .filter(|(_, road, road_of)| {
                road_of.0 == *settlement_id
                    && road.is_complete()
                    && road.class == RoadClass::Main
                    && road.surface == RoadSurface::Dirt
            })
            .min_by_key(|(entity, _, _)| entity.to_bits())
            .map(|(entity, _, _)| entity);
        let Some(candidate) = candidate else { continue };
        let Ok((_, mut road, _)) = roads.get_mut(candidate) else {
            continue;
        };
        let required = road.stone_required();
        let remaining = required.saturating_sub(road.stone_committed);
        let requested = remaining.min(elapsed);
        let moved = inventory.remove(Good::Stone, requested);
        if moved > 0 {
            road.stone_committed = road.stone_committed.saturating_add(moved);
        }
        if road.stone_committed >= required {
            road.surface = RoadSurface::Stone;
            road.widen_within_reservation(4.0);
            info!(
                "Settlement '{}': completed a stone main road using {} Stone",
                settlement.name, required
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn development_test_app() -> App {
        let mut app = App::new();
        app.init_resource::<crate::world::identity::WorldIdAllocator>()
            .init_resource::<crate::world::identity::WorldIdentityIndex>()
            .add_systems(
                PreUpdate,
                (
                    crate::world::identity::assign_stable_world_ids,
                    crate::world::identity::rebuild_world_identity_index,
                    crate::world::identity::reconcile_stable_world_relationships,
                    crate::world::identity::reconcile_stable_road_relationships,
                )
                    .chain(),
            );
        app
    }

    #[test]
    fn charter_is_deterministic_and_uses_independent_wall_choices() {
        let a = SettlementDevelopment::from_foundation("Oakmead", Vec3::new(2.0, 0.0, 7.0), 0);
        let b = SettlementDevelopment::from_foundation("Oakmead", Vec3::new(2.0, 0.0, 7.0), 9);
        assert_eq!(a.plan_seed, b.plan_seed);
        assert_eq!(a.layout, b.layout);
        assert_ne!(a.inner_wall, a.outer_wall);
    }

    #[test]
    fn promotion_changes_the_hall_level_without_replacing_the_settlement() {
        let mut app = App::new();
        app.add_systems(Update, sync_civic_hall_levels);
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Doorstead".into(),
                tier: SettlementTier::Hamlet,
                residents: 0,
                treasury: 73,
            })
            .id();

        app.update();
        assert_eq!(
            app.world().get::<CivicHallLevel>(settlement),
            Some(&CivicHallLevel::Moot)
        );
        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .tier = SettlementTier::Village;
        app.update();
        assert_eq!(
            app.world().get::<CivicHallLevel>(settlement),
            Some(&CivicHallLevel::Village)
        );
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().treasury,
            73,
            "the authoritative Hall entity and its state must survive the art upgrade"
        );

        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .tier = SettlementTier::City;
        app.update();
        assert_eq!(
            app.world().get::<CivicHallLevel>(settlement),
            Some(&CivicHallLevel::Town)
        );
    }

    #[test]
    fn town_paves_the_existing_market_without_replacing_its_state() {
        let mut app = App::new();
        app.add_systems(Update, sync_market_levels);
        let settlement_id = SettlementId(17);
        let settlement = app
            .world_mut()
            .spawn((
                settlement_id,
                Settlement {
                    name: "Marketford".into(),
                    tier: SettlementTier::Village,
                    residents: 11,
                    treasury: 83,
                },
            ))
            .id();
        let market = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Market,
                    settlement: "Marketford".into(),
                    owner: None,
                    quality: 0.5,
                    workers: vec!["Alda".into()],
                },
                BuildingOf(settlement_id),
                PlayerRotation(0.37),
                PlacedBuilding {
                    building_type: shared::building::BuildingType::Market,
                    rotation: 0.37,
                },
                GoodsInventory::new(250),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<MarketLevel>(market),
            Some(&MarketLevel::Earthen)
        );
        assert_eq!(
            app.world()
                .get::<PlacedBuilding>(market)
                .unwrap()
                .building_type,
            shared::building::BuildingType::Market
        );

        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .tier = SettlementTier::Town;
        app.update();
        assert_eq!(
            app.world().get::<MarketLevel>(market),
            Some(&MarketLevel::Paved)
        );
        let placed = app.world().get::<PlacedBuilding>(market).unwrap();
        assert_eq!(
            placed.building_type,
            shared::building::BuildingType::MarketPaved
        );
        assert_eq!(placed.rotation, 0.37);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(market)
                .unwrap()
                .bulk_capacity(),
            250,
            "art promotion must not replace the market entity or inventory"
        );
    }

    #[test]
    fn stone_main_road_waits_for_physical_stone() {
        let mut app = development_test_app();
        app.add_systems(Update, upgrade_town_roads);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        app.world_mut()
            .entity_mut(clock)
            .get_mut::<WorldTime>()
            .unwrap()
            .day = 1;

        let mut development = SettlementDevelopment::from_foundation("Stoneford", Vec3::ZERO, 0);
        development.last_road_work_day = 0;
        let hall = app
            .world_mut()
            .spawn((
                shared::components::SettlementId(1),
                Settlement {
                    name: "Stoneford".into(),
                    tier: SettlementTier::Town,
                    residents: 12,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                MootAdministration {
                    city_workers: vec!["Mara".into()],
                    ..default()
                },
                development,
                GoodsInventory::new(100),
            ))
            .id();
        let road = app
            .world_mut()
            .spawn((
                VillageRoad {
                    settlement: "Stoneford".into(),
                    builder: "Mara".into(),
                    points: vec![Vec2::ZERO, Vec2::new(2.0, 0.0)],
                    built_through: 2,
                    width: 2.6,
                    reserved_width: RoadClass::Main.initial_reserved_width(),
                    surface: RoadSurface::Dirt,
                    class: RoadClass::Main,
                    stone_committed: 0,
                },
                shared::components::RoadOf(shared::components::SettlementId(1)),
            ))
            .id();
        app.world_mut().spawn(CivicEmployment {
            settlement: shared::components::SettlementId(1),
            role: CivicRole::CityWorker,
        });

        app.update();
        assert_eq!(app.world().get::<VillageRoad>(road).unwrap().width, 2.6);
        assert_eq!(
            app.world().get::<VillageRoad>(road).unwrap().surface,
            RoadSurface::Dirt
        );
        assert_eq!(
            app.world()
                .get::<VillageRoad>(road)
                .unwrap()
                .stone_committed,
            0
        );

        app.world_mut()
            .get_mut::<GoodsInventory>(hall)
            .unwrap()
            .add(Good::Stone, 1);
        app.world_mut()
            .entity_mut(clock)
            .get_mut::<WorldTime>()
            .unwrap()
            .day = 2;
        app.update();
        assert_eq!(app.world().get::<VillageRoad>(road).unwrap().width, 4.0);
        assert_eq!(
            app.world().get::<VillageRoad>(road).unwrap().surface,
            RoadSurface::Stone
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Stone),
            0
        );
    }

    #[test]
    fn village_promotion_is_sustained_and_uses_real_market_volume() {
        let mut app = development_test_app();
        app.add_systems(Update, update_settlement_developments);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut market = MootMarket::founding();
        let seller = shared::economy::MarketSeller::Business(shared::components::BuildingId(42));
        while market.total_volume() < TOWN_MIN_MARKET_VOLUME {
            market.consign(seller, Good::Wood, 1, Good::Wood.base_price());
            let sold = market.purchase(Good::Wood, 1, u64::MAX, None, None);
            assert_eq!(sold.trade.units, 1, "market must keep trading");
        }
        let mut economy = SettlementEconomy::default();
        economy.prosperity = TOWN_MIN_PROSPERITY;
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Tradeford".into(),
                    tier: SettlementTier::Village,
                    residents: TOWN_MIN_RESIDENTS - 1,
                    treasury: 0,
                },
                economy,
                market,
                SettlementDevelopment::from_foundation("Tradeford", Vec3::ZERO, 0),
            ))
            .id();
        for kind in [
            SettlementBuildingKind::Market,
            SettlementBuildingKind::Tavern,
        ] {
            app.world_mut().spawn(SettlementBuilding {
                kind,
                settlement: "Tradeford".into(),
                owner: None,
                quality: 0.5,
                workers: vec!["Worker".into()],
            });
        }

        for day in 1..=TOWN_REQUIRED_DAYS {
            app.world_mut()
                .entity_mut(clock)
                .get_mut::<WorldTime>()
                .unwrap()
                .day = u32::from(day);
            app.update();
        }
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().tier,
            SettlementTier::Village,
            "prosperity and trade cannot bypass the 30-resident Town gate"
        );
        app.world_mut()
            .get_mut::<Settlement>(settlement)
            .unwrap()
            .residents = TOWN_MIN_RESIDENTS;
        for day in (TOWN_REQUIRED_DAYS + 1)..=(TOWN_REQUIRED_DAYS * 2) {
            app.world_mut()
                .entity_mut(clock)
                .get_mut::<WorldTime>()
                .unwrap()
                .day = u32::from(day);
            app.update();
        }
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().tier,
            SettlementTier::Town
        );
    }

    #[test]
    fn town_promotion_reaches_city_after_real_amenity_and_sustained_pull() {
        let mut app = development_test_app();
        app.add_systems(Update, update_settlement_developments);
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut economy = SettlementEconomy::default();
        economy.prosperity = CITY_MIN_PROSPERITY;
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Bellcross".into(),
                    tier: SettlementTier::Town,
                    residents: CITY_MIN_RESIDENTS,
                    treasury: 0,
                },
                economy,
                MootMarket::founding(),
                SettlementDevelopment::from_foundation("Bellcross", Vec3::ZERO, 0),
            ))
            .id();
        app.world_mut().spawn(SettlementBuilding {
            kind: SettlementBuildingKind::Church,
            settlement: "Bellcross".into(),
            owner: None,
            quality: 0.5,
            workers: vec!["Keeper".into()],
        });

        for day in 1..=CITY_REQUIRED_DAYS {
            app.world_mut()
                .entity_mut(clock)
                .get_mut::<WorldTime>()
                .unwrap()
                .day = u32::from(day);
            app.update();
        }
        assert_eq!(
            app.world().get::<Settlement>(settlement).unwrap().tier,
            SettlementTier::City
        );
    }

    #[test]
    fn public_position_caps_expand_from_hamlet_to_village() {
        assert_eq!(SettlementTier::Hamlet.public_guard_positions(), 0);
        assert_eq!(SettlementTier::Hamlet.public_worker_positions(), 2);
        assert_eq!(SettlementTier::Village.public_guard_positions(), 2);
        assert_eq!(SettlementTier::Village.public_worker_positions(), 2);
    }
}
