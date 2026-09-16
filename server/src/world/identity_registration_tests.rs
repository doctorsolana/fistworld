//! Durable citizenship follows registration, not a chosen travel destination.
use super::*;

fn hall(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            SettlementId(41),
            Settlement {
                name: "Registerford".into(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
        ))
        .id()
}

#[test]
fn only_registered_intents_publish_durable_residency() {
    let mut app = App::new();
    app.add_systems(Update, reconcile_stable_world_relationships);
    let hall = hall(&mut app);
    let person = app
        .world_mut()
        .spawn((
            CharacterName("Newcomer".into()),
            PersonId(7),
            VillagerIntent::Idle,
        ))
        .id();
    for intent in [
        VillagerIntent::Idle,
        VillagerIntent::ArrivingBySea { settlement: hall },
        VillagerIntent::Travelling { settlement: hall },
    ] {
        app.world_mut().entity_mut(person).insert(intent);
        app.update();
        assert!(app.world().get::<ResidentOf>(person).is_none());
    }
    // A stale external/loaded marker must also be removed without relying on
    // another intent change to wake the relationship reconciler.
    app.world_mut()
        .entity_mut(person)
        .insert(ResidentOf(SettlementId(41)));
    app.update();
    assert!(app.world().get::<ResidentOf>(person).is_none());

    for intent in [
        VillagerIntent::Resident { settlement: hall },
        VillagerIntent::Building {
            settlement: hall,
            site: Entity::PLACEHOLDER,
        },
        VillagerIntent::RoadBuilding {
            settlement: hall,
            road: Entity::PLACEHOLDER,
        },
    ] {
        app.world_mut().entity_mut(person).insert(intent);
        app.update();
        assert_eq!(
            app.world().get::<ResidentOf>(person),
            Some(&ResidentOf(SettlementId(41)))
        );
    }
    app.world_mut()
        .entity_mut(person)
        .insert(VillagerIntent::Travelling { settlement: hall });
    app.update();
    assert!(app.world().get::<ResidentOf>(person).is_none());
}

#[test]
fn pending_migrants_cannot_inherit_legacy_jobs_or_property_by_name() {
    let mut app = App::new();
    app.add_systems(
        Update,
        (
            reconcile_stable_world_relationships,
            reconcile_stable_civic_employment,
        )
            .chain(),
    );
    let hall = hall(&mut app);
    app.world_mut().entity_mut(hall).insert(MootAdministration {
        lead_steward: Some("Same name".into()),
        ..default()
    });
    let migrant = app
        .world_mut()
        .spawn((
            CharacterName("Same name".into()),
            PersonId(8),
            VillagerIntent::Travelling { settlement: hall },
            ResidentOf(SettlementId(41)),
        ))
        .id();
    let site = app
        .world_mut()
        .spawn((
            BuildingId(5),
            SettlementBuilding {
                kind: SettlementBuildingKind::LumberjackHut,
                settlement: "Registerford".into(),
                owner: Some("Same name".into()),
                quality: 1.,
                workers: vec!["Same name".into()],
            },
        ))
        .id();
    app.update();
    assert!(app.world().get::<ResidentOf>(migrant).is_none());
    assert!(app.world().get::<EmployedAt>(migrant).is_none());
    assert!(app.world().get::<CivicEmployment>(migrant).is_none());
    assert!(app.world().get::<OwnedBy>(site).is_none());

    // A registered namesake can inherit the unambiguous old record. The
    // travelling newcomer neither receives it nor makes that name ambiguous.
    let resident = app
        .world_mut()
        .spawn((
            CharacterName("Same name".into()),
            PersonId(9),
            VillagerIntent::Resident { settlement: hall },
        ))
        .id();
    app.world_mut()
        .get_mut::<SettlementBuilding>(site)
        .unwrap()
        .set_changed();
    app.world_mut()
        .get_mut::<MootAdministration>(hall)
        .unwrap()
        .set_changed();
    app.update();
    assert_eq!(
        app.world().get::<OwnedBy>(site),
        Some(&OwnedBy(PersonId(9)))
    );
    assert_eq!(
        app.world().get::<EmployedAt>(resident),
        Some(&EmployedAt(BuildingId(5)))
    );
    assert_eq!(
        app.world()
            .get::<CivicEmployment>(resident)
            .unwrap()
            .settlement,
        SettlementId(41)
    );
    assert!(app.world().get::<EmployedAt>(migrant).is_none());
    assert!(app.world().get::<CivicEmployment>(migrant).is_none());
}

#[test]
fn explicit_hero_membership_without_village_intent_is_unchanged() {
    let mut app = App::new();
    app.add_systems(Update, reconcile_stable_world_relationships);
    hall(&mut app);
    let hero = app
        .world_mut()
        .spawn((
            CharacterKind::Hero,
            CharacterName("Hero".into()),
            PersonId(10),
            ResidentOf(SettlementId(41)),
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<ResidentOf>(hero),
        Some(&ResidentOf(SettlementId(41)))
    );
}
