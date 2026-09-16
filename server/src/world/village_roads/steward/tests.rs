use super::*;
use crate::world::regional_roads::{RegionalRoadSection, RegionalRoadWorker};
use shared::components::{CivicEmployment, CivicRole, SettlementId, SettlementTier};

#[test]
fn local_audit_retires_its_stalled_connector_but_keeps_a_paid_regional_prefix() {
    let mut app = App::new();
    app.add_systems(Update, audit_village_roads);
    let mut clock = WorldTime::new_default();
    clock.set_normalized_time(9.0 / 24.0);
    let clock = app.world_mut().spawn(clock).id();
    let id = SettlementId(71);
    let hall = app
        .world_mut()
        .spawn((
            id,
            Settlement {
                name: "Auditford".into(),
                tier: SettlementTier::Village,
                residents: 3,
                treasury: 1000,
            },
            PlayerPosition(Vec3::ZERO),
            PlayerRotation(0.0),
            MootAdministration::default(),
            MootAdministrationRuntime {
                last_audit_at: None,
                road_progress: HashMap::new(),
            },
        ))
        .id();
    app.world_mut().spawn((
        CharacterName("Clerk".into()),
        VillagerIntent::Resident { settlement: hall },
        MootSteward { settlement: hall },
        CivicEmployment {
            settlement: id,
            role: CivicRole::MootSteward,
        },
    ));
    let make_road = |offset: f32| VillageRoad {
        settlement: "Auditford".into(),
        builder: "Builder".into(),
        points: (0..4).map(|x| Vec2::new(x as f32 * 2.0, offset)).collect(),
        built_through: 2,
        width: 2.6,
        reserved_width: 2.6,
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    let local = app
        .world_mut()
        .spawn((make_road(10.0), shared::components::RoadOf(id)))
        .id();
    app.world_mut().spawn((
        VillagerIntent::RoadBuilding {
            settlement: hall,
            road: local,
        },
        RoadBuilderRoutine::regional(local, hall),
        PlayerPosition(Vec3::new(2.0, 0.0, 10.0)),
    ));
    let project = app.world_mut().spawn_empty().id();
    let regional = app
        .world_mut()
        .spawn((
            make_road(20.0),
            shared::components::RoadOf(id),
            RegionalRoadSection { project },
        ))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            VillagerIntent::RoadBuilding {
                settlement: hall,
                road: regional,
            },
            RoadBuilderRoutine::regional(regional, hall),
            PlayerPosition(Vec3::new(2.0, 0.0, 20.0)),
            RegionalRoadWorker { project },
        ))
        .id();
    app.update();
    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .advance(360.0, 360.0);
    app.update();
    assert!(
        app.world().get_entity(local).is_err(),
        "control proves the real 300-second local recovery ran"
    );
    let preserved = app
        .world()
        .get::<VillageRoad>(regional)
        .expect("regional owner, not local steward, may retire paid work");
    assert_eq!(preserved.built_through, 2);
    assert_eq!(preserved.points.len(), 4);
    assert!(app.world().get::<RegionalRoadWorker>(worker).is_some());
    assert!(app.world().get::<RoadBuilderRoutine>(worker).is_some());
    // Even an owner handoff waits for the regional coordinator's refund and
    // prefix-retention policy; it is not an abandoned local connector.
    app.world_mut()
        .entity_mut(worker)
        .remove::<RoadBuilderRoutine>();
    app.world_mut()
        .get_mut::<WorldTime>(clock)
        .unwrap()
        .advance(61.0, 61.0);
    app.update();
    assert!(app.world().get::<VillageRoad>(regional).is_some());
}
