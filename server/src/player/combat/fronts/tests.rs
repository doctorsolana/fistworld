use super::*;
use crate::player::{
    army::{apply_army_order, BattalionLedger},
    combat::{pursue_attack_orders, separate_melee_bodies, AttackOrder},
    hero::step_units,
    orders::apply_unit_order,
};
use shared::{
    protocol::{ArmyOrder, UnitCommand, UnitOrder, UnitSelection},
    region::RegionCoord,
};

fn lab() -> App {
    let mut app = App::new();
    app.init_resource::<CombatSpace>()
        .init_resource::<CombatFormations>()
        .init_resource::<BattalionLedger>();
    let mut terrain = shared::terrain::WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0.0, 80.0, 0.0), Vec2::splat(120.0), 0.0, 4.0);
    app.insert_resource(terrain);
    app.world_mut().spawn(WorldTime::new_default());
    app.add_systems(
        Update,
        (
            crate::player::orders::advance_marches,
            rebuild_combat_space,
            advance_battle_fronts,
            crate::player::combat::steer_skirmishers,
            step_units,
            rebuild_combat_space,
            assign_formation_contacts,
            pursue_attack_orders,
            separate_melee_bodies,
        )
            .chain(),
    );
    app
}
fn block(app: &mut App, account: &str, anchor: Vec2, facing: Vec2, n: usize) -> Vec<Entity> {
    let right = Vec2::new(facing.y, -facing.x);
    let soldiers: Vec<_> = (0..n)
        .map(|i| {
            let p = anchor + right * ((i % 10) as f32 - 4.5) * FILE_SPACING
                - facing * (i / 10) as f32 * RANK_SPACING;
            app.world_mut()
                .spawn((
                    CharacterKind::Villager,
                    CommandedBy(account.into()),
                    PlayerPosition(Vec3::new(p.x, 80.0, p.y)),
                    PlayerRotation(f32::atan2(-facing.x, -facing.y)),
                    RegionCoord::default(),
                    CharacterMotion::STATIONARY,
                    CharacterActivity::Idle,
                    CharacterAttributes::default(),
                    Health::new(100.0),
                ))
                .id()
        })
        .collect();
    apply_army_order(
        app.world_mut(),
        account,
        ArmyOrder::Muster {
            members: soldiers.clone(),
        },
    );
    order(app, account, &soldiers, UnitCommand::Hold);
    soldiers
}
fn order(app: &mut App, account: &str, soldiers: &[Entity], command: UnitCommand) {
    assert_eq!(
        apply_unit_order(
            app.world_mut(),
            account,
            UnitOrder {
                selection: UnitSelection {
                    units: soldiers.to_vec(),
                    battalions: vec![]
                },
                command
            }
        )
        .0,
        soldiers.len()
    );
}
fn tick(app: &mut App, n: usize) {
    for _ in 0..n {
        for mut t in app
            .world_mut()
            .query::<&mut WorldTime>()
            .iter_mut(app.world_mut())
        {
            t.seconds_in_cycle += 1.0 / 60.0;
        }
        app.update();
    }
}
#[test]
fn a_front_rank_screens_the_rank_behind_it() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    let b = block(&mut app, "bob", Vec2::Y * 1.55, -Vec2::Y, 50);
    tick(&mut app, 3);
    assert!(a[..10]
        .iter()
        .all(|e| app.world().get::<AttackOrder>(*e).is_some()));
    assert!(a[10..]
        .iter()
        .all(|e| app.world().get::<AttackOrder>(*e).is_none()));
    assert!(b[10..]
        .iter()
        .all(|e| app.world().get::<AttackOrder>(*e).is_none()));
}
#[test]
fn a_casualty_advances_only_its_own_file() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    block(&mut app, "bob", Vec2::Y * 1.55, -Vec2::Y, 50);
    tick(&mut app, 3);
    let group = app.world().get::<FormationMember>(a[0]).unwrap().group;
    let columns = app.world().resource::<CombatFormations>().fronts[&group]
        .columns
        .clone();
    app.world_mut()
        .get_mut::<Health>(columns[4][0])
        .unwrap()
        .current = 0.0;
    tick(&mut app, 8);
    let after = &app.world().resource::<CombatFormations>().fronts[&group].columns;
    for i in 0..10 {
        assert_eq!(
            after[i],
            if i == 4 {
                columns[i][1..].to_vec()
            } else {
                columns[i].clone()
            }
        );
    }
}
#[test]
fn three_attackers_reserve_distinct_faces_of_one_battalion() {
    let mut app = lab();
    let defenders = block(&mut app, "bob", Vec2::new(0.0, 10.0), -Vec2::Y, 50);
    let mut attackers = Vec::new();
    for x in [-22.0, 0.0, 22.0] {
        attackers.push(block(&mut app, "alice", Vec2::new(x, -12.0), Vec2::Y, 50));
    }
    let selected: Vec<_> = attackers.iter().flatten().copied().collect();
    order(
        &mut app,
        "alice",
        &selected,
        UnitCommand::Attack {
            target: defenders[0],
        },
    );
    tick(&mut app, 3);
    let state = app.world().resource::<CombatFormations>();
    let mut faces = std::collections::BTreeSet::new();
    for unit in &attackers {
        let id = app.world().get::<FormationMember>(unit[0]).unwrap().group;
        assert!(faces.insert(state.fronts[&id].sector.unwrap()));
    }
    assert_eq!(faces.len(), 3);
    let centre = app
        .world()
        .get::<FormationMember>(attackers[1][0])
        .unwrap()
        .group;
    assert_eq!(
        state.fronts[&centre].sector,
        Some(Face::Front),
        "the centre takes the front regardless of command order"
    );
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..300 {
        tick(&mut app, 6);
        for (i, unit) in attackers.iter().enumerate() {
            if unit
                .iter()
                .any(|e| app.world().get::<AttackOrder>(*e).is_some())
            {
                seen.insert(i);
            }
        }
    }
    assert_eq!(
        seen.len(),
        3,
        "every attacking battalion must reach contact"
    );
}
#[test]
fn a_weapon_cannot_reach_through_a_friendly_body() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 2);
    let b = block(&mut app, "bob", Vec2::Y, -Vec2::Y, 2);
    app.world_mut().get_mut::<PlayerPosition>(a[0]).unwrap().0 = Vec3::ZERO;
    app.world_mut().get_mut::<PlayerPosition>(a[1]).unwrap().0 = Vec3::new(0.0, 0.0, 0.8);
    app.world_mut().get_mut::<PlayerPosition>(b[0]).unwrap().0 = Vec3::new(0.0, 0.0, 1.6);
    app.update();
    assert!(!app
        .world()
        .resource::<CombatSpace>()
        .clear_strike(a[0], b[0]));
}

#[test]
fn partial_selection_detaches_and_attacks_an_exposed_opponent() {
    let mut app = lab();
    let defenders = block(&mut app, "bob", Vec2::new(0.0, 0.0), -Vec2::Y, 50);
    let attackers = block(&mut app, "alice", Vec2::new(0.0, -1.55), Vec2::Y, 50);
    let raiders = block(&mut app, "alice", Vec2::new(0.0, 13.0), -Vec2::Y, 5);
    order(
        &mut app,
        "alice",
        &raiders[..1],
        UnitCommand::Attack {
            target: defenders[0],
        },
    );
    order(
        &mut app,
        "alice",
        &raiders[1..3],
        UnitCommand::Attack {
            target: defenders[0],
        },
    );
    for e in &raiders[..3] {
        assert!(app.world().get::<FormationMember>(*e).is_none());
        assert!(app
            .world()
            .get::<crate::player::combat::SkirmishOrder>(*e)
            .is_some());
    }
    let group = app
        .world()
        .get::<FormationMember>(defenders[0])
        .unwrap()
        .group;
    let facing = app.world().resource::<CombatFormations>().fronts[&group].facing;
    let mut engaged = std::collections::HashSet::new();
    for _ in 0..150 {
        tick(&mut app, 6);
        for e in &raiders[..3] {
            if app.world().get::<CharacterActivity>(*e) == Some(&CharacterActivity::Fighting) {
                engaged.insert(*e);
            }
        }
    }
    assert_eq!(
        engaged.len(),
        3,
        "each independent attacker must find a reachable opponent"
    );
    assert!(
        app.world().resource::<CombatFormations>().fronts[&group]
            .facing
            .dot(facing)
            > 0.99,
        "a few rear attackers must not turn the entire battalion"
    );
    assert!(attackers.iter().any(|e|app.world().get::<CharacterActivity>(*e)==Some(&CharacterActivity::Fighting)),"original front remains engaged");
}
#[test]
fn attack_move_keeps_its_destination_while_the_front_is_engaged() {
    use shared::protocol::{FormationFrontage, MovementMode};
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    let b = block(&mut app, "bob", Vec2::Y * 2.4, -Vec2::Y, 50);
    order(
        &mut app,
        "alice",
        &a,
        UnitCommand::Move {
            target: Vec3::new(0.0, 80.0, 35.0),
            frontage: Some(FormationFrontage {
                facing: Vec2::Y,
                width: 12.61,
            }),
            mode: MovementMode::AttackMove,
        },
    );
    tick(&mut app, 20);
    assert!(a.iter().all(|e| app
        .world()
        .get::<crate::player::orders::MarchOrder>(*e)
        .is_some()));
    assert!(a
        .iter()
        .any(|e| app.world().get::<PausedFormationMarch>(*e).is_some()));
    for e in &b {
        app.world_mut().get_mut::<Health>(*e).unwrap().current = 0.0;
    }
    tick(&mut app, 20);
    assert!(a
        .iter()
        .all(|e| app.world().get::<PausedFormationMarch>(*e).is_none()));
    assert!(a.iter().all(|e| app
        .world()
        .get::<crate::player::orders::MarchOrder>(*e)
        .is_some()));
}

#[test]
fn a_distant_independent_attack_keeps_a_navigation_goal() {
    let mut app = lab();
    let defenders = block(&mut app, "bob", Vec2::new(0.0, 55.0), -Vec2::Y, 5);
    let raiders = block(&mut app, "alice", Vec2::new(0.0, -55.0), Vec2::Y, 5);
    order(
        &mut app,
        "alice",
        &raiders[..1],
        UnitCommand::Attack {
            target: defenders[0],
        },
    );
    tick(&mut app, 8);
    assert!(app
        .world()
        .get::<crate::player::hero::MoveTarget>(raiders[0])
        .is_some());
    assert!(
        app.world()
            .get::<crate::player::combat::DirectCombatApproach>(raiders[0])
            .is_none(),
        "distant approaches use ordinary budgeted routing"
    );
}

#[test]
fn a_battle_approach_uses_one_shared_route_around_a_wall() {
    use crate::player::orders::MarchOrder;
    use shared::spatial::{ObstacleEntry, SpatialObstacleGrid};
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::new(0.0, -20.0), Vec2::Y, 20);
    let b = block(&mut app, "bob", Vec2::new(0.0, 20.0), -Vec2::Y, 20);
    let mut obstacles = SpatialObstacleGrid::default();
    obstacles.insert(ObstacleEntry {
        center: Vec2::ZERO,
        half_extents: Vec2::new(9.0, 2.0),
        rotation: 0.0,
        obstacle_type: 0,
    });
    app.insert_resource(obstacles);
    order(&mut app, "alice", &a, UnitCommand::Attack { target: b[0] });
    tick(&mut app, 1);
    let groups: std::collections::HashSet<_> = a
        .iter()
        .filter_map(|e| app.world().get::<MarchOrder>(*e).map(|m| m.group))
        .collect();
    assert_eq!(groups.len(), 1, "obstructed combat members share one field");
    let mut reached = false;
    for _ in 0..900 {
        tick(&mut app, 6);
        for e in &a {
            let p = app.world().get::<PlayerPosition>(*e).unwrap().0.xz();
            assert!(!app
                .world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(p));
            reached |=
                app.world().get::<CharacterActivity>(*e) == Some(&CharacterActivity::Fighting);
        }
        if reached {
            break;
        }
    }
    assert!(
        reached,
        "the battle order must reach contact beyond the obstacle"
    );
}
