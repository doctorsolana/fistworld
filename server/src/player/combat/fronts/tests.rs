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
    for entity in &soldiers {
        app.world_mut()
            .entity_mut(*entity)
            .insert(PersonId(entity.to_bits()));
    }
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
fn three_attackers_approach_directly_and_each_reaches_contact() {
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
            mode: shared::protocol::AttackMode::Focus,
        },
    );
    tick(&mut app, 3);
    // A wing must approach the fight from its current side immediately;
    // there is no whole-battalion detour to a reserved rectangular flank.
    for unit in &attackers {
        let entity = unit[0];
        let at = app.world().get::<PlayerPosition>(entity).unwrap().0.xz();
        let goal = app
            .world()
            .get::<crate::player::hero::MoveTarget>(entity)
            .unwrap()
            .0
            .xz();
        assert!(
            goal.y > at.y,
            "approach should advance toward the enemy: {at:?} -> {goal:?}"
        );
    }
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
fn explicitly_released_soldiers_can_attack_an_exposed_opponent() {
    let mut app = lab();
    let defenders = block(&mut app, "bob", Vec2::new(0.0, 0.0), -Vec2::Y, 50);
    let attackers = block(&mut app, "alice", Vec2::new(0.0, -1.55), Vec2::Y, 50);
    let raiders = block(&mut app, "alice", Vec2::new(0.0, 13.0), -Vec2::Y, 5);
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Dismiss {
            members: raiders[..3].to_vec(),
        },
    );
    order(
        &mut app,
        "alice",
        &raiders[..1],
        UnitCommand::Attack {
            target: defenders[0],
            mode: shared::protocol::AttackMode::Focus,
        },
    );
    order(
        &mut app,
        "alice",
        &raiders[1..3],
        UnitCommand::Attack {
            target: defenders[0],
            mode: shared::protocol::AttackMode::Focus,
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
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Dismiss {
            members: raiders[..1].to_vec(),
        },
    );
    order(
        &mut app,
        "alice",
        &raiders[..1],
        UnitCommand::Attack {
            target: defenders[0],
            mode: shared::protocol::AttackMode::Focus,
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
    order(
        &mut app,
        "alice",
        &a,
        UnitCommand::Attack {
            target: b[0],
            mode: shared::protocol::AttackMode::Focus,
        },
    );
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

#[test]
fn army_engagement_spreads_three_blocks_but_focus_keeps_the_clicked_target() {
    let mut app = lab();
    let mut attackers = vec![];
    let mut defenders = vec![];
    for x in [-18.0, 0.0, 18.0] {
        attackers.extend(block(&mut app, "alice", Vec2::new(x, -25.0), Vec2::Y, 50));
        defenders.push(block(&mut app, "bob", Vec2::new(x, 15.0), -Vec2::Y, 50));
    }
    let clicked = defenders[1][0];
    for (mode, expected) in [
        (shared::protocol::AttackMode::EngageLine, 3),
        (shared::protocol::AttackMode::Focus, 1),
    ] {
        order(
            &mut app,
            "alice",
            &attackers,
            UnitCommand::Attack {
                target: clicked,
                mode,
            },
        );
        let targets: std::collections::BTreeSet<_> = attackers
            .iter()
            .filter_map(|e| {
                let member = app.world().get::<FormationMember>(*e)?;
                match app.world().resource::<CombatFormations>().fronts[&member.group].intent {
                    Intent::Attack(Enemy::Battalion(id)) => Some(id),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(targets.len(), expected);
        assert!(targets.contains(&app.world().get::<MemberOfBattalion>(clicked).unwrap().0));
    }
}

#[test]
fn a_nearby_soldier_fights_without_returning_to_their_assigned_file() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::new(0.0, -8.0), Vec2::Y, 50);
    let b = block(&mut app, "bob", Vec2::new(0.0, 4.0), -Vec2::Y, 50);
    // A back-rank member arrives on an exposed flank. A rigid slot would send
    // them back across their own ranks before they could participate.
    let point = app.world().get::<PlayerPosition>(b[9]).unwrap().0 + Vec3::X * -1.7;
    app.world_mut().get_mut::<PlayerPosition>(a[49]).unwrap().0 = point;
    order(
        &mut app,
        "alice",
        &a,
        UnitCommand::Attack {
            target: b[0],
            mode: shared::protocol::AttackMode::Focus,
        },
    );
    tick(&mut app, 12);
    assert!(
        app.world().get::<EngagedWith>(a[49]).is_some(),
        "a reachable opponent takes priority over rank alignment"
    );
    assert!(
        app.world()
            .get::<PlayerPosition>(a[49])
            .unwrap()
            .0
            .xz()
            .distance(point.xz())
            < 1.0
    );
}

#[test]
fn a_mid_fight_move_clears_combat_and_regroups_every_survivor() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    block(&mut app, "bob", Vec2::Y * 1.65, -Vec2::Y, 50);
    tick(&mut app, 24);
    assert!(a
        .iter()
        .any(|e| app.world().get::<EngagedWith>(*e).is_some()));
    order(
        &mut app,
        "alice",
        &a,
        UnitCommand::Move {
            target: Vec3::new(0.0, 80.0, -25.0),
            frontage: None,
            mode: shared::protocol::MovementMode::Retreat,
        },
    );
    let destinations: Vec<_> = a
        .iter()
        .map(|entity| {
            (
                *entity,
                app.world()
                    .get::<crate::player::orders::MarchOrder>(*entity)
                    .unwrap()
                    .destination,
            )
        })
        .collect();
    tick(&mut app, 900);
    for (entity, destination) in destinations {
        assert!(app.world().get::<EngagedWith>(entity).is_none());
        let actual = app.world().get::<PlayerPosition>(entity).unwrap().0;
        assert!(
            actual.xz().distance(destination.xz()) < 0.3,
            "retreat must reach each soldier's slot: {actual:?} vs {destination:?}"
        );
    }
}

#[test]
fn an_idle_formation_restores_a_settled_member_displaced_by_a_late_arrival() {
    let mut app = lab();
    let people = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    let home = app.world().get::<PlayerPosition>(people[0]).unwrap().0;
    app.world_mut()
        .get_mut::<PlayerPosition>(people[0])
        .unwrap()
        .0
        .x -= 0.6;
    tick(&mut app, 90);
    let actual = app.world().get::<PlayerPosition>(people[0]).unwrap().0;
    assert!(
        actual.xz().distance(home.xz()) < 0.2,
        "settled soldier remained displaced: {actual:?} vs {home:?}"
    );
    let group = app.world().get::<FormationMember>(people[0]).unwrap().group;
    assert!(
        !app.world().resource::<CombatFormations>().fronts[&group].active,
        "restoring spacing must not invent a combat engagement"
    );
}

#[test]
fn a_rear_rank_moves_up_through_a_gap_instead_of_waiting_out_of_range() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    let b = block(&mut app, "bob", Vec2::Y * 1.65, -Vec2::Y, 50);
    for e in a.iter().chain(&b) {
        app.world_mut().get_mut::<Health>(*e).unwrap().current = 10000.0;
    }
    // Clear the reserve's own lane through the front rows. A supporting soldier is initially
    // farther than eight metres from the opposing front.
    for i in [1, 11, 21, 31] {
        app.world_mut().get_mut::<Health>(a[i]).unwrap().current = 0.0;
    }
    let rear = a[41];
    // Keep this lane open while the reserve advances. Other soldiers should
    // not race into it first and invalidate the reachability fixture.
    for e in a.iter().chain(&b).filter(|e| **e != rear) {
        app.world_mut()
            .entity_mut(*e)
            .insert(BattalionStance::HoldLine);
    }
    let start = app.world().get::<PlayerPosition>(rear).unwrap().0;
    let mut fought = false;
    for _ in 0..100 {
        tick(&mut app, 6);
        fought |= app.world().get::<EngagedWith>(rear).is_some();
    }
    let end = app.world().get::<PlayerPosition>(rear).unwrap().0;
    assert!(
        end.z > start.z + 3.0,
        "rear rank should move up: {start:?} -> {end:?}"
    );
    assert!(fought, "a reserve with an open lane should join the fight");
}

#[test]
fn screened_reserves_keep_their_files_until_an_approach_opens() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    let b = block(&mut app, "bob", Vec2::Y * 1.65, -Vec2::Y, 50);
    for e in a.iter().chain(&b) {
        app.world_mut().get_mut::<Health>(*e).unwrap().current = 10000.0;
    }
    // A stable occupied screen isolates reserve following from front-line
    // movement that could legitimately expose a local approach.
    for e in a[..40].iter().chain(&b) {
        app.world_mut()
            .entity_mut(*e)
            .insert(BattalionStance::HoldLine);
    }
    let rear: Vec<_> = a[42..48]
        .iter()
        .map(|e| (*e, app.world().get::<PlayerPosition>(*e).unwrap().0.xz()))
        .collect();
    tick(&mut app, 180);
    assert!(a[..10]
        .iter()
        .any(|e| app.world().get::<EngagedWith>(*e).is_some()));
    for (entity, start) in rear {
        let end = app.world().get::<PlayerPosition>(entity).unwrap().0.xz();
        assert!(
            (end.x - start.x).abs() < 0.6,
            "screened rear soldier must not overtake its file: {start:?} -> {end:?}"
        );
    }
}

#[test]
fn an_open_ground_attack_keeps_rank_spacing_before_contact() {
    let mut app = lab();
    let a = block(&mut app, "alice", Vec2::ZERO, Vec2::Y, 50);
    let b = block(&mut app, "bob", Vec2::Y * 50.0, -Vec2::Y, 50);
    let starts: Vec<_> = a
        .iter()
        .map(|e| app.world().get::<PlayerPosition>(*e).unwrap().0.xz())
        .collect();
    order(
        &mut app,
        "alice",
        &a,
        UnitCommand::Attack {
            target: b[0],
            mode: shared::protocol::AttackMode::Focus,
        },
    );
    tick(&mut app, 360);
    let lead_delta = app.world().get::<PlayerPosition>(a[4]).unwrap().0.xz() - starts[4];
    assert!(lead_delta.y > 15.0, "the battalion must keep advancing");
    for (entity, start) in a.iter().zip(starts) {
        let delta = app.world().get::<PlayerPosition>(*entity).unwrap().0.xz() - start;
        assert!(
            delta.distance(lead_delta) < 0.4,
            "march stretched a rank: {delta:?} vs {lead_delta:?}"
        );
    }
}
