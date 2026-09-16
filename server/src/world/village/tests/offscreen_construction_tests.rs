//! Unobserved permit admission and physical construction ownership.
use super::*;
use crate::world::regions::RegionRegistry;
use shared::region::RegionCoord;

#[test]
fn unobserved_residents_can_approve_permits_without_cancelling_existing_journeys() {
    let mut app = village_test_app();
    app.init_resource::<Time>()
        .init_resource::<VillageClock>()
        .init_resource::<MootQueueClock>()
        .init_resource::<RegionRegistry>()
        .insert_resource(WorldTerrain::default())
        .add_systems(Update, consider_permits);
    let terrain = app.world().resource::<WorldTerrain>();
    let position = Vec3::new(1720., terrain.get_height(1720., 0.), 0.);
    let version = terrain.modification_version();
    let hall = app
        .world_mut()
        .spawn((
            Settlement {
                name: "Unobserved".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 4,
                treasury: 0,
            },
            PlayerPosition(position),
        ))
        .id();
    app.world_mut()
        .resource_mut::<VillageClock>()
        .failed_fishing_terrain_versions
        .insert(hall, version);
    let people: Vec<_> = ["Ada", "Bea", "Cy", "Traveller"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let at = position + Vec3::new(index as f32 * 2., 0., -12.);
            app.world_mut()
                .spawn((
                    CharacterName(name.into()),
                    CharacterKind::Villager,
                    PlayerPosition(at),
                    PlayerRotation(0.),
                    RegionCoord::from_world_pos(at),
                    VillagerIntent::Resident { settlement: hall },
                    GoodsInventory::new(shared::economy::capacity::VILLAGER),
                ))
                .id()
        })
        .collect();
    let traveller = people[3];
    let traveller_position = app.world().get::<PlayerPosition>(traveller).unwrap().0;
    app.world_mut().entity_mut(traveller).insert((
        TravelRoute {
            goal: position + Vec3::X * 40.,
            waypoints: vec![crate::world::village_roads::RouteWaypoint {
                position: position + Vec3::X * 40.,
                on_road: false,
            }],
            next: 0,
            geometry_version: 0,
        },
        Wallet::new(10_000),
    ));

    let mut builders = Vec::new();
    for _ in 0..12 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
        app.update();
        let world = app.world_mut();
        builders = world
            .query::<&UnderConstruction>()
            .iter(world)
            .filter_map(|site| site.builder)
            .collect();
        if builders.len() == 2 {
            break;
        }
    }
    assert_eq!(app.world().resource::<RegionRegistry>().observed_count(), 0);
    assert_eq!(
        builders.len(),
        2,
        "unobserved housing/food demand still admits work"
    );
    for builder in &builders {
        assert_ne!(*builder, traveller);
        assert!(app.world().get::<MootQueueTicket>(*builder).is_some());
        assert!(
            app.world()
                .get::<ConstructionMaterialRoutine>(*builder)
                .is_none(),
            "approval cannot skip collecting the physical permit"
        );
    }
    assert!(app.world().get::<TravelRoute>(traveller).is_some());
    assert_eq!(
        app.world().get::<PlayerPosition>(traveller).unwrap().0,
        traveller_position
    );
    assert_eq!(
        app.world().get::<Wallet>(traveller).unwrap().balance(),
        10_000
    );
}
