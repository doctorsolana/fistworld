use super::*;
use crate::collision::building_index::sync_building_spatial_index;
use crate::world::navgrid::{sync_obstacle_grid, ObstacleGridState};
use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
use shared::spatial::SpatialObstacleGrid;

fn frontage_road(at: Vec3) -> VillageRoad {
    VillageRoad {
        settlement: "Yard test".into(),
        builder: "Builder".into(),
        points: vec![
            at.xz() + Vec2::new(-10., -7.),
            at.xz() + Vec2::new(10., -7.),
        ],
        built_through: 2,
        width: 1.8,
        reserved_width: 3.6,
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    }
}

fn fitted_yard(at: Vec3, yaw: f32) -> HouseholdYard {
    let mut land = HouseholdYardLand::default();
    land.reserve_house(HouseAppearance::default(), at, yaw);
    land.reserve_road(&frontage_road(at));
    land.fit_yard(
        HouseAppearance::default(),
        at,
        yaw,
        household_yard_seed(at),
        |_, _| true,
        |_| Some(at.y),
    )
    .unwrap()
}

fn yard_app() -> (App, Entity, HouseholdYard, Vec3) {
    let at = Vec3::new(1700., 80., 0.);
    let source = WorldTerrain::default();
    let mut map = source.generator.loaded_map().clone();
    map.objects_by_chunk.clear();
    if let Some(recipe) = &mut map.definition.generated {
        recipe.scatter_vegetation = false;
    }
    let mut terrain = WorldTerrain::from_loaded_map(map);
    terrain.apply_flatten_rect(at, Vec2::splat(30.), 0., 4.);
    let yard = fitted_yard(at, 0.);
    let mut app = App::new();
    app.insert_resource(terrain);
    app.init_resource::<BuildingSpatialIndex>();
    app.init_resource::<SpatialObstacleGrid>();
    app.init_resource::<ObstacleGridState>();
    app.add_systems(
        Update,
        (
            sync_building_spatial_index,
            refresh_household_yards,
            sync_obstacle_grid,
        )
            .chain(),
    );
    let entity = app
        .world_mut()
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Yard test".into(),
                owner: None,
                quality: 1.,
                workers: vec![],
            },
            HouseAppearance::default(),
            PlayerPosition(at),
            PlayerRotation(0.),
            PlacedBuilding {
                building_type: BuildingType::LogCabin,
                rotation: 0.,
            },
            BuildingPosition(at),
            yard.clone(),
        ))
        .id();
    app.world_mut().spawn(frontage_road(at));
    (app, entity, yard, at)
}

#[test]
fn later_reserved_road_releases_the_old_yard_and_its_navigation() {
    let (mut app, house, yard, at) = yard_app();
    app.update();
    assert_eq!(app.world().get::<HouseholdYard>(house), Some(&yard));
    let outer = (yard.outer_edge().0 + yard.outer_edge().1) * 0.5 + at.xz();
    assert!(app
        .world()
        .resource::<SpatialObstacleGrid>()
        .point_blocked(outer));
    let center = outer;
    let road = app
        .world_mut()
        .spawn(VillageRoad {
            settlement: "Yard test".into(),
            builder: "Builder".into(),
            points: vec![center - Vec2::Y * 9., center + Vec2::Y * 9.],
            built_through: 0,
            width: 1.,
            reserved_width: 3.,
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        })
        .id();
    app.update();
    assert_ne!(
        app.world().get::<HouseholdYard>(house),
        Some(&yard),
        "the unbuilt road reservation takes priority"
    );
    assert!(
        !app.world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(outer),
        "revocation and navigation update occur in the same schedule"
    );
    app.world_mut()
        .get_mut::<VillageRoad>(road)
        .unwrap()
        .built_through = 2;
    app.update();
    assert!(
        !app.world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(outer),
        "new built frontage may redesign the yard but cannot reclaim the road"
    );
}

#[test]
fn house_upgrade_refits_the_actual_envelope_and_demolition_removes_fences() {
    let (mut app, house, _, at) = yard_app();
    app.update();
    let upgraded = HouseAppearance {
        line: HouseLine::Cabin,
        level: HouseLevel::UpperStorey,
    };
    app.world_mut().entity_mut(house).insert((
        upgraded,
        PlacedBuilding {
            building_type: upgraded.building_type(),
            rotation: 0.,
        },
    ));
    app.update();
    {
        let yard = app
            .world()
            .get::<HouseholdYard>(house)
            .expect("the upgraded home still has ample useful street-side land");
        let mut land = HouseholdYardLand::default();
        land.reserve_house(upgraded, at, 0.);
        land.reserve_road(&frontage_road(at));
        assert!(
            yard.fits_site(at, 0., |p, r| land.is_clear(p, r), |_| Some(at.y)),
            "no old fence can remain through the upgraded house"
        );
        assert_eq!(yard.house, Some(upgraded));
    }
    app.world_mut().despawn(house);
    app.update();
    assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
}

#[test]
fn a_plain_placed_building_revokes_conflicting_yard_land() {
    let (mut app, house, yard, at) = yard_app();
    app.update();
    let point = at + Vec3::new(yard.center().x, 0.0, yard.center().y);
    let building = BuildingType::LogCabin;
    app.world_mut().spawn((
        PlacedBuilding {
            building_type: building,
            rotation: 0.0,
        },
        BuildingPosition(point),
    ));
    app.update();
    assert_ne!(app.world().get::<HouseholdYard>(house), Some(&yard));
    if let Some(next) = app.world().get::<HouseholdYard>(house) {
        let mut land = HouseholdYardLand::default();
        let definition = building.definition();
        land.reserve_rect(
            definition.world_footprint_center(point, 0.0),
            definition.footprint * 0.5,
            0.0,
        );
        assert!(next.fits_site(at, 0.0, |p, r| land.is_clear(p, r), |_| Some(at.y)));
    }
}

#[test]
fn live_grants_never_embed_a_person_or_horse_in_a_new_fence() {
    let yard = fit_household_yard(
        HouseAppearance::default(),
        Vec3::ZERO,
        0.7,
        0,
        |_, _| true,
        |_| Some(0.),
    )
    .unwrap();
    let obstacles = yard.ground_obstacles(Vec3::ZERO, 0.7);
    let fence = &obstacles[1];
    assert!(overlaps_body(&obstacles, fence.center, false));
    let beside =
        fence.center + shared::rotation::local_to_world_xz(Vec2::new(0., 0.8), fence.rotation);
    assert!(!overlaps_body(&obstacles, beside, false));
    assert!(overlaps_body(&obstacles, beside, true));
    assert!(!overlaps_body(&obstacles, Vec2::splat(100.), true));
}

#[test]
fn fitting_a_town_publishes_navigation_in_groups_not_every_pair_of_houses() {
    let (mut app, first, _, at) = yard_app();
    app.world_mut().entity_mut(first).remove::<HouseholdYard>();
    let building = app
        .world()
        .get::<SettlementBuilding>(first)
        .unwrap()
        .clone();
    app.world_mut()
        .resource_mut::<WorldTerrain>()
        .apply_flatten_rect(at + Vec3::new(48., 0., 36.), Vec2::splat(120.), 0., 4.);
    for i in 1..18 {
        let point = at + Vec3::new((i % 5) as f32 * 24., 0., (i / 5) as f32 * 24.);
        app.world_mut().spawn(frontage_road(point));
        app.world_mut().spawn((
            building.clone(),
            HouseAppearance::default(),
            PlayerPosition(point),
            PlayerRotation(0.),
            PlacedBuilding {
                building_type: BuildingType::LogCabin,
                rotation: 0.,
            },
            BuildingPosition(point),
        ));
    }
    app.update();
    let count = |world: &mut World| world.query::<&HouseholdYard>().iter(world).count();
    assert_eq!(count(app.world_mut()), 0);
    let initial_version = app.world().resource::<SpatialObstacleGrid>().version;
    for _ in 1..7 {
        app.update();
        assert_eq!(count(app.world_mut()), 0);
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            initial_version
        );
    }
    app.update();
    assert_eq!(count(app.world_mut()), 16);
    let batch_version = app.world().resource::<SpatialObstacleGrid>().version;
    assert_ne!(batch_version, initial_version);
    app.update();
    assert_eq!(count(app.world_mut()), 18);
    assert_ne!(
        app.world().resource::<SpatialObstacleGrid>().version,
        batch_version
    );
}

#[test]
fn a_staged_fence_waits_for_a_standing_body_then_publishes_without_a_refit() {
    let (mut app, house, _, at) = yard_app();
    app.world_mut().entity_mut(house).remove::<HouseholdYard>();
    let yard = fitted_yard(at, 0.);
    let point = yard.ground_obstacles(at, 0.)[1].center;
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(point.x, at.y, point.y)),
        ))
        .id();
    app.update();
    assert!(app.world().get::<HouseholdYard>(house).is_none());
    assert!(!app
        .world()
        .resource::<SpatialObstacleGrid>()
        .point_blocked(point));
    app.world_mut().get_mut::<PlayerPosition>(person).unwrap().0 += Vec3::splat(100.);
    for _ in 0..59 {
        app.update();
    }
    assert!(app.world().get::<HouseholdYard>(house).is_none());
    app.update();
    assert_eq!(app.world().get::<HouseholdYard>(house), Some(&yard));
    assert!(app
        .world()
        .resource::<SpatialObstacleGrid>()
        .point_blocked(point));
}

#[test]
fn a_house_waits_for_real_local_road_frontage() {
    let (mut app, house, _, at) = yard_app();
    app.world_mut().entity_mut(house).remove::<HouseholdYard>();
    let road = app
        .world_mut()
        .query::<(Entity, &VillageRoad)>()
        .iter(app.world())
        .next()
        .unwrap()
        .0;
    {
        let mut source = app.world_mut().get_mut::<VillageRoad>(road).unwrap();
        source.points = [-100., -70., -10., 10.]
            .map(|x| at.xz() + Vec2::new(x, -7.))
            .to_vec();
        source.built_through = 0;
    }
    for _ in 0..3 {
        app.update();
    }
    assert!(
        app.world().get::<HouseholdYard>(house).is_none(),
        "a surveyed line is not a lived-in street"
    );
    app.world_mut()
        .get_mut::<VillageRoad>(road)
        .unwrap()
        .built_through = 2;
    app.update();
    assert!(
        app.world().get::<HouseholdYard>(house).is_none(),
        "distant construction does not grant this yard"
    );
    app.world_mut()
        .get_mut::<VillageRoad>(road)
        .unwrap()
        .built_through = 4;
    app.update();
    let yard = app
        .world()
        .get::<HouseholdYard>(house)
        .expect("built frontage wakes the waiting home");
    assert!(yard.entry.is_some());
    assert_eq!(yard.house, Some(HouseAppearance::default()));
}

#[test]
fn frontage_progress_only_wakes_houses_beside_the_new_segments() {
    let mut locations = HouseLocations::default();
    let mut world = World::new();
    let old_house = world.spawn_empty().id();
    let new_house = world.spawn_empty().id();
    for (entity, x) in [(old_house, 0.), (new_house, 80.)] {
        locations.update(
            entity,
            HouseSource {
                origin: Vec3::new(x, 0., 0.),
                yaw: 0.,
                appearance: HouseAppearance::default(),
            },
        );
    }
    let mut road = frontage_road(Vec3::ZERO);
    road.points = [-10., 10., 60., 90.].map(|x| Vec2::new(x, -7.)).to_vec();
    road.built_through = 3;
    let old = RoadLand::from_road(&road);
    road.built_through = 4;
    let mut affected = HashSet::new();
    locations.changed_frontage(&old, &road, &mut affected);
    assert_eq!(affected, HashSet::from([new_house]));
    locations.remove(new_house);
    affected.clear();
    locations.changed_frontage(&old, &road, &mut affected);
    assert!(
        affected.is_empty(),
        "removed house roots leave the local event index"
    );
}

fn smaller_yard(yard: &HouseholdYard) -> HouseholdYard {
    let mut next = yard.clone();
    let shrink = |mut p: Vec2| {
        match yard.side {
            YardSide::Right => p.x = yard.minimum.x + (p.x - yard.minimum.x) * 0.70,
            YardSide::Left => p.x = yard.maximum.x + (p.x - yard.maximum.x) * 0.70,
            YardSide::Rear => p.y = yard.minimum.y + (p.y - yard.minimum.y) * 0.70,
        }
        p
    };
    next.boundary = yard.boundary_points().into_iter().map(shrink).collect();
    next.entry = yard.entry.map(shrink);
    next.minimum = next
        .boundary
        .iter()
        .copied()
        .fold(Vec2::splat(f32::INFINITY), Vec2::min);
    next.maximum = next
        .boundary
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    next
}

#[test]
fn a_body_blocked_replacement_retains_the_old_yard_and_survives_remote_edits() {
    let (mut app, house, grown, at) = yard_app();
    let old = smaller_yard(&grown);
    let mut priority = HouseholdYardLand::default();
    priority.reserve_house(HouseAppearance::default(), at, 0.);
    priority.reserve_road(&frontage_road(at));
    assert!(old.fits_site(at, 0., |p, r| priority.is_clear(p, r), |_| Some(at.y)));
    assert!(
        priority.yard_access_is_clear_for(
            house.to_bits(),
            &old,
            at,
            0.,
            |_, _| true,
            |_| Some(at.y)
        ),
        "the incumbent must be truly usable before the growth attempt"
    );
    assert!(worthwhile_replacement(&old, &grown));
    let point = grown
        .ground_obstacles(at, 0.)
        .iter()
        .map(|obstacle| obstacle.center)
        .find(|point| !old.contains_world_point(*point, at, 0., 0.6))
        .expect("an expanded boundary has a genuinely new obstacle");
    app.world_mut().entity_mut(house).insert(old.clone());
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(point.x, at.y, point.y)),
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<HouseholdYard>(house),
        Some(&old),
        "keep the old accepted layout while the new fence is occupied"
    );
    let version = app.world().resource::<SpatialObstacleGrid>().version;
    for _ in 0..20 {
        app.update();
    }
    app.world_mut()
        .resource_mut::<WorldTerrain>()
        .apply_flatten_rect(at + Vec3::X * 1000., Vec2::splat(3.), 0., 1.);
    app.update();
    assert_eq!(app.world().get::<HouseholdYard>(house), Some(&old));
    assert_eq!(
        app.world().resource::<SpatialObstacleGrid>().version,
        version
    );
    app.world_mut().get_mut::<PlayerPosition>(person).unwrap().0 += Vec3::splat(100.);
    for _ in 0..38 {
        app.update();
    }
    assert_eq!(
        app.world().get::<HouseholdYard>(house),
        Some(&old),
        "the existing retry deadline is retained"
    );
    app.update();
    assert_eq!(app.world().get::<HouseholdYard>(house), Some(&grown));
    assert_ne!(
        app.world().resource::<SpatialObstacleGrid>().version,
        version
    );
}

#[test]
fn stable_context_does_not_rewrite_yards_or_navigation() {
    let (mut app, house, _, _) = yard_app();
    app.update();
    let yard = app.world().get::<HouseholdYard>(house).unwrap().clone();
    let version = app.world().resource::<SpatialObstacleGrid>().version;
    for tick in 0..120 {
        app.world_mut()
            .get_mut::<SettlementBuilding>(house)
            .unwrap()
            .quality = 0.6 + (tick % 5) as f32 * 0.02;
        app.update();
    }
    assert_eq!(app.world().get::<HouseholdYard>(house), Some(&yard));
    assert_eq!(
        app.world().resource::<SpatialObstacleGrid>().version,
        version,
        "account/quality churn is not a geometry event"
    );
}

#[test]
fn replacement_hysteresis_keeps_small_boundary_changes_but_accepts_useful_growth() {
    let (_, _, yard, _) = yard_app();
    assert!(!worthwhile_replacement(&yard, &yard));
    let mut noisy = yard.clone();
    if let Some(entry) = &mut noisy.entry {
        entry.x += 0.2;
    }
    assert!(!worthwhile_replacement(&yard, &noisy));
    assert!(worthwhile_replacement(&smaller_yard(&yard), &yard));
}

#[test]
fn publishing_a_replacement_wakes_a_neighbour_waiting_for_the_released_land() {
    let (mut app, owner, chosen, at) = yard_app();
    assert!(matches!(chosen.side, YardSide::Left | YardSide::Right));
    // Start with an equally usable older plot on the other side of this home.
    // A nearby home can use that ground only after the real replacement lands.
    let mut old = chosen.clone();
    old.boundary = chosen
        .boundary_points()
        .into_iter()
        .rev()
        .map(|p| Vec2::new(-p.x, p.y))
        .collect();
    old.minimum = Vec2::new(-chosen.maximum.x, chosen.minimum.y);
    old.maximum = Vec2::new(-chosen.minimum.x, chosen.maximum.y);
    old.entry = chosen.entry.map(|p| Vec2::new(-p.x, p.y));
    old.approach = chosen.approach.map(|p| Vec2::new(-p.x, p.y));
    old.side = if chosen.side == YardSide::Left {
        YardSide::Right
    } else {
        YardSide::Left
    };
    let definition = BuildingType::LogCabin.definition();
    let sign = if old.side == YardSide::Left { -1. } else { 1. };
    let edge = if sign < 0. {
        old.minimum.x
    } else {
        old.maximum.x
    };
    let neighbour_at = at + Vec3::X * (edge + sign * (definition.footprint.x * 0.5 + 0.9));
    let outer = neighbour_at + Vec3::X * sign * (definition.footprint.x + 0.9);
    let rear = neighbour_at + Vec3::Z * (definition.footprint.y + 0.9);
    let building = app
        .world()
        .get::<SettlementBuilding>(owner)
        .unwrap()
        .clone();
    let neighbour = app
        .world_mut()
        .spawn((
            building,
            HouseAppearance::default(),
            PlayerPosition(neighbour_at),
            PlayerRotation(0.),
            PlacedBuilding {
                building_type: BuildingType::LogCabin,
                rotation: 0.,
            },
            BuildingPosition(neighbour_at),
        ))
        .id();
    app.world_mut().spawn(frontage_road(neighbour_at));
    for position in [outer, rear] {
        app.world_mut().spawn((
            PlacedBuilding {
                building_type: BuildingType::LogCabin,
                rotation: 0.,
            },
            BuildingPosition(position),
        ));
    }
    app.world_mut()
        .resource_mut::<WorldTerrain>()
        .apply_flatten_rect(at, Vec2::splat(70.), 0., 4.);
    let mut land = HouseholdYardLand::default();
    land.reserve_house(HouseAppearance::default(), at, 0.);
    land.reserve_house(HouseAppearance::default(), neighbour_at, 0.);
    for position in [outer, rear] {
        land.reserve_rect(
            definition.world_footprint_center(position, 0.),
            definition.footprint * 0.5,
            0.,
        );
    }
    land.reserve_road(&frontage_road(at));
    land.reserve_road(&frontage_road(neighbour_at));
    land.set_yard(owner.to_bits(), Some((&old, at, 0.)));
    assert!(old.fits_site(
        at,
        0.,
        |p, r| land.is_clear_for(owner.to_bits(), p, r),
        |_| Some(at.y)
    ));
    assert!(land.yard_access_is_clear_for(
        owner.to_bits(),
        &old,
        at,
        0.,
        |_, _| true,
        |_| Some(at.y)
    ));
    let replacement = land
        .fit_yard_for(
            owner.to_bits(),
            HouseAppearance::default(),
            at,
            0.,
            household_yard_seed(at),
            |_, _| true,
            |_| Some(at.y),
        )
        .unwrap();
    assert!(worthwhile_replacement(&old, &replacement));
    let occupied = replacement
        .ground_obstacles(at, 0.)
        .iter()
        .map(|o| o.center)
        .find(|p| !old.contains_world_point(*p, at, 0., 0.6))
        .unwrap();
    app.world_mut().entity_mut(owner).insert(old.clone());
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(occupied.x, at.y, occupied.y)),
        ))
        .id();
    app.update();
    assert_eq!(app.world().get::<HouseholdYard>(owner), Some(&old));
    let before = app
        .world()
        .get::<HouseholdYard>(neighbour)
        .map_or(0., HouseholdYard::area);
    app.world_mut().get_mut::<PlayerPosition>(person).unwrap().0 += Vec3::splat(100.);
    // No road, house, land or terrain input changes after the first update.
    // Publication itself must wake the previously constrained neighbour.
    for _ in 0..64 {
        app.update();
    }
    assert_ne!(app.world().get::<HouseholdYard>(owner), Some(&old));
    let after = app
        .world()
        .get::<HouseholdYard>(neighbour)
        .map_or(0., HouseholdYard::area);
    assert!(
        after > before + 1.0,
        "released old land must wake the neighbour: {before} -> {after}"
    );
    let version = app.world().resource::<SpatialObstacleGrid>().version;
    for _ in 0..90 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<SpatialObstacleGrid>().version,
        version,
        "release wakeups settle instead of making neighbouring gardens oscillate"
    );
}

#[test]
fn cancelling_a_staged_claim_wakes_only_waiting_local_homes() {
    let mut state = HouseholdYardState::default();
    let mut world = World::new();
    let owner = world.spawn_empty().id();
    let waiting = world.spawn_empty().id();
    let already_fitted = world.spawn_empty().id();
    let remote = world.spawn_empty().id();
    let at = Vec3::new(1700., 80., 0.);
    let yard = fitted_yard(at, 0.);
    let source = HouseSource {
        origin: at,
        yaw: 0.,
        appearance: HouseAppearance::default(),
    };
    for (entity, origin) in [
        (owner, at),
        (waiting, at + Vec3::X * 16.),
        (already_fitted, at + Vec3::Z * 16.),
        (remote, at + Vec3::X * 300.),
    ] {
        state.houses.update(
            entity,
            HouseSource {
                origin,
                ..source.clone()
            },
        );
    }
    let terrain = WorldTerrain::default();
    state.land.reserve_road(&frontage_road(at));
    for entity in [owner, already_fitted] {
        let context = SiteContext::new(entity, &source, &state.land, &terrain);
        state.staged.push(YardGrant {
            entity,
            context,
            validation: YardValidation::new(
                entity,
                &yard,
                at,
                0.,
                source.appearance,
                &state.land,
                &terrain,
            ),
            retry_step: 60,
        });
    }
    state
        .land
        .set_staged_yard(owner.to_bits(), Some((&yard, at, 0.)));
    state.cancel_staged(owner);
    assert_eq!(state.take_released_neighbours(), HashSet::from([waiting]));
    assert!(state.take_released_neighbours().is_empty());
    assert!(
        state.staged.iter().any(|g| g.entity == already_fitted),
        "released space does not invalidate another home's already fitted proposal"
    );
    assert!(
        state
            .land
            .is_clear_for(waiting.to_bits(), at.xz() + yard.center(), 0.1),
        "cancellation releases the staged parcel as well as its event"
    );
    state.cancel_staged(owner);
    assert!(
        state.take_released_neighbours().is_empty(),
        "cancelling an absent claim is a no-op"
    );
}
