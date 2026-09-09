use super::*;
fn hero(world: &mut World, name: &str, id: u64, p: Vec3) -> Entity {
    world
        .spawn((
            CharacterKind::Hero,
            CommandedBy(name.into()),
            PersonId(id),
            PlayerPosition(p),
            PlayerRotation(0.),
            CharacterMotion::STATIONARY,
            Health::default(),
        ))
        .id()
}
#[test]
fn mounting_is_owned_nearby_and_exclusive() {
    let mut world = World::new();
    let h = spawn_checked(&mut world, Vec3::ZERO).unwrap();
    let a = hero(&mut world, "alice", 1, Vec3::X);
    let b = hero(&mut world, "bob", 2, Vec3::X);
    assert!(order(&mut world, "bob", a, UnitCommand::Mount { horse: h }).is_err());
    world.get_mut::<PlayerPosition>(a).unwrap().0 = Vec3::X * 10.;
    assert!(order(&mut world, "alice", a, UnitCommand::Mount { horse: h }).is_err());
    world.get_mut::<PlayerPosition>(a).unwrap().0 = Vec3::X;
    order(&mut world, "alice", a, UnitCommand::Mount { horse: h }).unwrap();
    assert_eq!(world.get::<Horse>(h).unwrap().rider, Some(PersonId(1)));
    assert!(order(&mut world, "bob", b, UnitCommand::Mount { horse: h }).is_err());
    assert!(order(
        &mut world,
        "alice",
        a,
        UnitCommand::Move {
            target: Vec3::X * 5.,
            frontage: None,
            mode: MovementMode::Move
        }
    )
    .is_err());
}
#[test]
fn dismount_completes_and_death_releases_the_horse() {
    let mut app = App::new();
    app.add_systems(Update, tick);
    let h = spawn_checked(app.world_mut(), Vec3::ZERO).unwrap();
    let a = hero(app.world_mut(), "alice", 1, Vec3::X);
    order(app.world_mut(), "alice", a, UnitCommand::Mount { horse: h }).unwrap();
    app.world_mut().get_mut::<Mounted>(a).unwrap().since = -2.;
    app.update();
    assert_eq!(
        app.world().get::<Mounted>(a).unwrap().phase,
        RidingPhase::Riding
    );
    order(app.world_mut(), "alice", a, UnitCommand::Dismount).unwrap();
    app.world_mut().get_mut::<Mounted>(a).unwrap().since = -2.;
    app.update();
    assert!(app.world().get::<Mounted>(a).is_none());
    assert_eq!(app.world().get::<Horse>(h).unwrap().rider, None);
    assert_eq!(
        app.world().get::<PlayerPosition>(a).unwrap().0,
        HORSE_DISMOUNT_OFFSET
    );
    order(app.world_mut(), "alice", a, UnitCommand::Mount { horse: h }).unwrap();
    app.world_mut().entity_mut(a).insert(OfflineHero);
    app.update();
    assert!(app.world().get::<Mounted>(a).is_none());
    assert_eq!(app.world().get::<Horse>(h).unwrap().rider, None);
}
