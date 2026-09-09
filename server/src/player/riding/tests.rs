use super::*;
use crate::world::wildlife::spawn_checked;
use shared::protocol::{MovementMode, UnitCommand};
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

#[test]
fn cavalry_musters_and_uses_the_normal_battalion_order_stream() {
    use crate::player::{army::{apply_army_order, BattalionLedger}, orders::apply_unit_order};
    use shared::protocol::{ArmyOrder, AttackMode, UnitOrder, UnitSelection};
    let mut world = World::new();
    world.init_resource::<BattalionLedger>();
    let a = hero(&mut world, "alice", 1, Vec3::ZERO);
    let b = hero(&mut world, "alice", 2, Vec3::X * 4.);
    for e in [a, b] {
        world.entity_mut(e).insert(CharacterKind::Villager);
        equip_cavalry(&mut world, e).unwrap();
    }
    let (count, _) = apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![a,b] });
    assert_eq!(count, 2);
    let battalion = world.query::<(Entity, &Battalion)>().iter(&world).next().unwrap().0;
    assert_eq!(world.get::<SoldierRole>(battalion), Some(&SoldierRole::Cavalry));
    // Clicking one member expands to both, exactly like infantry selection.
    let order = |command| UnitOrder { selection: UnitSelection { units: vec![a], battalions: vec![] }, command };
    let (count, detail) = apply_unit_order(&mut world, "alice", order(UnitCommand::Move {
        target: Vec3::Z * 20., frontage: None, mode: MovementMode::Move,
    }));
    assert_eq!(count, 2, "{detail}");
    let goal_a = world.get::<crate::player::orders::MarchOrder>(a).unwrap().destination;
    let goal_b = world.get::<crate::player::orders::MarchOrder>(b).unwrap().destination;
    assert!(goal_a.distance(goal_b) >= 3.1);
    let enemy = hero(&mut world, "bob", 3, Vec3::Z * 30.);
    let (count, detail) = apply_unit_order(&mut world, "alice", order(UnitCommand::Attack { target: enemy, mode: AttackMode::Focus }));
    assert_eq!(count, 2, "{detail}");
    assert!(world.get::<crate::player::combat::fronts::FormationMember>(a).is_some());
    assert!(world.get::<Mounted>(a).is_some());
    let (count, _) = apply_unit_order(&mut world, "alice", order(UnitCommand::Hold));
    assert_eq!(count, 2);
}

#[test]
fn supplied_mounts_are_cleaned_on_death_and_equipment_release() {
    let mut app = App::new();
    app.add_systems(Update, tick);
    let rider = hero(app.world_mut(), "alice", 11, Vec3::ZERO);
    let horse = equip_cavalry(app.world_mut(), rider).unwrap();
    app.world_mut().get_mut::<Health>(rider).unwrap().current = 0.;
    app.update();
    assert!(app.world().get::<Mounted>(rider).is_none());
    assert!(app.world().get_entity(horse).is_err());
    let rider = hero(app.world_mut(), "alice", 12, Vec3::ZERO);
    let horse = equip_cavalry(app.world_mut(), rider).unwrap();
    crate::player::archery::inherit_equipment(app.world_mut(), rider, SoldierRole::Infantry, FirePolicy::default());
    assert!(app.world().get::<Mounted>(rider).is_none());
    assert!(app.world().get_entity(horse).is_err());
}

#[test]
fn cavalry_is_not_a_free_horse_supply_and_unmounted_recruits_cannot_join() {
    use crate::player::army::{apply_army_order, BattalionLedger};
    use shared::protocol::ArmyOrder;
    let mut world = World::new();
    world.init_resource::<BattalionLedger>();
    let foot = hero(&mut world, "alice", 21, Vec3::ZERO);
    apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![foot] });
    let battalion = world.query::<(Entity, &Battalion)>().iter(&world).next().unwrap().0;
    assert_eq!(apply_army_order(&mut world, "alice", ArmyOrder::SetRole { battalion, role: SoldierRole::Cavalry }).0, 0);
    assert!(world.get::<Mounted>(foot).is_none());
    let cavalry = hero(&mut world, "alice", 22, Vec3::X * 6.);
    equip_cavalry(&mut world, cavalry).unwrap();
    apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![cavalry] });
    let cavalry_battalion = world.query::<(Entity, &Battalion, &SoldierRole)>().iter(&world).find(|(_, _, r)| **r == SoldierRole::Cavalry).unwrap().0;
    assert_eq!(apply_army_order(&mut world, "alice", ArmyOrder::Assign { battalion: cavalry_battalion, members: vec![foot] }).0, 0);
    assert_eq!(world.get::<SoldierRole>(foot), Some(&SoldierRole::Infantry));
}

#[test]
fn a_despawned_rider_cannot_leave_an_issued_mount_behind() {
    let mut app = App::new();
    app.add_systems(Update, tick);
    let rider = hero(app.world_mut(), "alice", 31, Vec3::ZERO);
    let horse = equip_cavalry(app.world_mut(), rider).unwrap();
    app.world_mut().despawn(rider);
    app.update();
    assert!(app.world().get_entity(horse).is_err());
}

#[test]
fn public_equipment_and_mixed_membership_edits_cannot_discard_issued_horses() {
    use crate::player::army::{apply_army_order, BattalionLedger};
    use shared::protocol::ArmyOrder;
    let mut world = World::new();
    world.init_resource::<BattalionLedger>();
    let foot = hero(&mut world, "alice", 41, Vec3::ZERO);
    let rider = hero(&mut world, "alice", 42, Vec3::X * 6.);
    let horse = equip_cavalry(&mut world, rider).unwrap();
    apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![rider] });
    let cavalry = world.query::<(Entity, &Battalion)>().iter(&world).next().unwrap().0;
    assert_eq!(apply_army_order(&mut world, "alice", ArmyOrder::SetRole { battalion: cavalry, role: SoldierRole::Infantry }).0, 0);
    assert_eq!(apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![rider, foot] }).0, 0);
    apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![foot] });
    let infantry = world.query::<(Entity, &Battalion, &SoldierRole)>().iter(&world).find(|(_, _, role)| **role == SoldierRole::Infantry).unwrap().0;
    assert_eq!(apply_army_order(&mut world, "alice", ArmyOrder::Assign { battalion: infantry, members: vec![rider] }).0, 0);
    assert!(world.get::<Mounted>(rider).is_some());
    assert!(world.get::<Horse>(horse).is_some());
}

#[test]
fn detached_cavalry_certifies_obstructed_attacks_with_a_mounted_formation_route() {
    use crate::player::{combat::{fronts::{CombatSpace, Enemy, rebuild_combat_space}, SkirmishOrder, steer_skirmishers}, orders::{FormationRoutes, MarchOrder}};
    let mut app = App::new();
    app.init_resource::<CombatSpace>().init_resource::<FormationRoutes>();
    let mut obstacles = shared::spatial::SpatialObstacleGrid::default();
    for x in [-1.0, 1.0] {
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(x, 6.), half_extents: Vec2::new(0.1, 3.), rotation: 0., obstacle_type: 0,
        });
    }
    app.insert_resource(obstacles);
    app.world_mut().spawn(WorldTime::new_default());
    let rider = hero(app.world_mut(), "alice", 51, Vec3::ZERO);
    equip_cavalry(app.world_mut(), rider).unwrap();
    let target = hero(app.world_mut(), "bob", 52, Vec3::Z * 14.);
    app.world_mut().entity_mut(rider).insert(SkirmishOrder { target: Enemy::Person(target) });
    app.add_systems(Update, (rebuild_combat_space, steer_skirmishers).chain());
    app.update();
    assert!(app.world().get::<crate::player::combat::DirectCombatApproach>(rider).is_none(), "A foot-wide passage is not a clear mounted approach");
    assert!(app.world().get::<MarchOrder>(rider).is_some(), "Blocked mounted attacks must enter the clearance-aware bounded planner");
}
