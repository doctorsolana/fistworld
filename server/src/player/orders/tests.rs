use super::*;
use crate::player::army::{apply_army_order, BattalionLedger};
use shared::protocol::{ArmyOrder, FormationFrontage};
fn soldier(world: &mut World, account: &str, point: Vec3) -> Entity {
    world
        .spawn((
            CharacterKind::Villager,
            CommandedBy(account.into()),
            PlayerPosition(point),
            PlayerRotation(0.0),
            CharacterMotion::STATIONARY,
            CharacterActivity::Idle,
            Health::default(),
            CharacterAttributes::default(),
        ))
        .id()
}
fn selected(entity: Entity, command: UnitCommand) -> UnitOrder {
    UnitOrder {
        selection: UnitSelection {
            units: vec![entity],
            battalions: vec![],
        },
        command,
    }
}
#[test]
fn mixed_verbs_obey_last_intent_and_keep_weapon_cooldowns() {
    let mut world = World::new();
    let a = soldier(&mut world, "alice", Vec3::ZERO);
    let b = soldier(&mut world, "bob", Vec3::X);
    let attack = selected(a, UnitCommand::Attack { target: b });
    let movement = UnitOrder::move_to(vec![a], Vec3::X * 30.0);
    for (first, last, attacks) in [
        (attack.clone(), movement.clone(), false),
        (movement, attack, true),
    ] {
        assert_eq!(apply_unit_order(&mut world, "alice", first).0, 1);
        assert_eq!(apply_unit_order(&mut world, "alice", last).0, 1);
        assert_eq!(world.get::<AttackOrder>(a).is_some(), attacks);
        assert_eq!(world.get::<MarchOrder>(a).is_some(), !attacks);
    }
    apply_unit_order(&mut world, "alice", selected(a, UnitCommand::Hold));
    assert!(world.get::<AttackOrder>(a).is_none());
    assert!(world.get::<MoveTarget>(a).is_none());
    assert_eq!(world.get::<CommandStance>(a), Some(&CommandStance::Hold));
}
#[test]
fn authority_dead_embarked_and_invalid_destinations_leave_previous_intent_alone() {
    let mut world = World::new();
    let own = soldier(&mut world, "alice", Vec3::ZERO);
    let other = soldier(&mut world, "bob", Vec3::X);
    world.entity_mut(own).insert(MoveTarget(Vec3::Z));
    assert_eq!(
        apply_unit_order(
            &mut world,
            "alice",
            UnitOrder::move_to(vec![other], Vec3::ZERO)
        )
        .0,
        0
    );
    for command in [
        UnitCommand::Move {
            target: Vec3::NAN,
            frontage: None,
            mode: MovementMode::Move,
        },
        UnitCommand::Move {
            target: Vec3::ZERO,
            frontage: Some(FormationFrontage {
                facing: Vec2::ZERO,
                width: 20.0,
            }),
            mode: MovementMode::Move,
        },
        UnitCommand::Attack { target: own },
    ] {
        assert_eq!(
            apply_unit_order(&mut world, "alice", selected(own, command)).0,
            0
        );
        assert_eq!(world.get::<MoveTarget>(own).unwrap().0, Vec3::Z);
    }
    world.get_mut::<Health>(own).unwrap().current = 0.0;
    assert_eq!(
        apply_unit_order(&mut world, "alice", UnitOrder::move_to(vec![own], Vec3::X)).0,
        0
    );
}
#[test]
fn membership_batches_see_previous_assignment_and_never_leave_orphans() {
    for disband in [false, true] {
        let mut world = World::new();
        world.init_resource::<BattalionLedger>();
        let unit = soldier(&mut world, "alice", Vec3::ZERO);
        apply_army_order(&mut world, "alice", ArmyOrder::Muster { members: vec![] });
        let bat = world
            .query_filtered::<Entity, With<Battalion>>()
            .single(&world)
            .unwrap();
        let orders = [
            ArmyOrder::Assign {
                battalion: bat,
                members: vec![unit],
            },
            if disband {
                ArmyOrder::Disband { battalion: bat }
            } else {
                ArmyOrder::Dismiss {
                    members: vec![unit],
                }
            },
        ];
        for order in orders {
            apply_army_order(&mut world, "alice", order);
        }
        assert!(world.get::<MemberOfBattalion>(unit).is_none());
        assert!(world.get::<StandardBearer>(unit).is_none());
        assert_eq!(world.get_entity(bat).is_err(), disband);
    }
}
#[test]
fn five_battalions_expand_to_all_250_unique_slots_without_crossing_blocks() {
    let mut world = World::new();
    world.init_resource::<BattalionLedger>();
    for group in 0..5 {
        let members = (0..50)
            .map(|i| {
                soldier(
                    &mut world,
                    "alice",
                    Vec3::new(
                        group as f32 * 20.0 + (i % 10) as f32 * 1.4,
                        0.0,
                        (i / 10) as f32 * 1.7,
                    ),
                )
            })
            .collect();
        assert_eq!(
            apply_army_order(&mut world, "alice", ArmyOrder::Muster { members }).0,
            50
        );
    }
    let battalions = world
        .query::<&Battalion>()
        .iter(&world)
        .map(|b| b.id)
        .collect();
    let order = UnitOrder {
        selection: UnitSelection {
            units: vec![],
            battalions,
        },
        command: UnitCommand::Move {
            target: Vec3::new(40.0, 0.0, 60.0),
            frontage: Some(FormationFrontage {
                facing: Vec2::Y,
                width: 83.0,
            }),
            mode: MovementMode::Move,
        },
    };
    assert_eq!(apply_unit_order(&mut world, "alice", order).0, 250);
    let mut slots = HashSet::new();
    let mut groups = HashSet::new();
    for march in world.query::<&MarchOrder>().iter(&world) {
        assert!(slots.insert((march.destination.x.to_bits(), march.destination.z.to_bits())));
        groups.insert(march.group);
    }
    assert_eq!(slots.len(), 250);
    assert_eq!(groups.len(), 5);
}

#[test]
fn an_obstructed_battalion_reaches_its_slots_through_the_real_mover() {
    use crate::player::hero::{settle_villagers_without_targets, step_units};
    use shared::region::RegionCoord;
    use shared::spatial::{ObstacleEntry, SpatialObstacleGrid};
    use shared::terrain::WorldTerrain;
    let mut app = App::new();
    app.init_resource::<BattalionLedger>();
    let members: Vec<_> = (0..50)
        .map(|i| {
            let e = soldier(
                app.world_mut(),
                "alice",
                Vec3::new(
                    -20.0 - (i / 10) as f32 * 1.7,
                    0.0,
                    (i % 10) as f32 * 1.4 - 6.3,
                ),
            );
            app.world_mut().entity_mut(e).insert(RegionCoord::default());
            e
        })
        .collect();
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Muster {
            members: members.clone(),
        },
    );
    let order = UnitOrder {
        selection: UnitSelection {
            units: members.clone(),
            battalions: vec![],
        },
        command: UnitCommand::Move {
            target: Vec3::new(25.0, 0.0, 0.0),
            frontage: Some(FormationFrontage {
                facing: Vec2::X,
                width: 12.6,
            }),
            mode: MovementMode::Move,
        },
    };
    assert_eq!(apply_unit_order(app.world_mut(), "alice", order).0, 50);
    let mut obstacles = SpatialObstacleGrid::default();
    obstacles.insert(ObstacleEntry {
        center: Vec2::ZERO,
        half_extents: Vec2::new(2.0, 10.0),
        rotation: 0.0,
        obstacle_type: 0,
    });
    app.insert_resource(obstacles);
    app.add_systems(
        Update,
        (
            advance_marches,
            step_units,
            settle_villagers_without_targets,
        )
            .chain(),
    );
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0.0, 80.0, 0.0), Vec2::splat(100.0), 0.0, 4.0);
    app.insert_resource(terrain);
    app.world_mut().spawn(TimeWarp::clamped(10.0));
    let goals: Vec<_> = members
        .iter()
        .map(|e| (*e, app.world().get::<MarchOrder>(*e).unwrap().destination))
        .collect();
    for _ in 0..1500 {
        app.update();
        for e in &members {
            let p = app.world().get::<PlayerPosition>(*e).unwrap().0.xz();
            assert!(!app
                .world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(p));
        }
        if goals.iter().all(|(e, p)| {
            app.world()
                .get::<PlayerPosition>(*e)
                .unwrap()
                .0
                .xz()
                .distance(p.xz())
                < 0.3
        }) {
            return;
        }
    }
    let stuck: Vec<_> = goals
        .iter()
        .filter(|(e, p)| {
            app.world()
                .get::<PlayerPosition>(*e)
                .unwrap()
                .0
                .xz()
                .distance(p.xz())
                > 0.3
        })
        .collect();
    panic!("{} soldiers failed to arrive", stuck.len());
}
