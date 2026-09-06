//! Opt-in connected army fixture. Commands and movement use production code.
use bevy::{ecs::system::SystemState, prelude::*};
use shared::{
    army_lab::ArmyLabScenario,
    components::*,
    formation::{FormationGroup, FormationSoldier},
    protocol::{ArmyOrder, FormationFrontage},
    terrain::WorldTerrain,
};
#[derive(Resource)]
struct Staged;

pub fn stage_connected_army(world: &mut World) {
    if world.contains_resource::<Staged>() {
        return;
    }
    let Some(scenario) = ArmyLabScenario::from_env() else {
        world.insert_resource(Staged);
        return;
    };
    if !world
        .resource::<crate::persistence::profiles::PlayerProfiles>()
        .peer_to_name
        .values()
        .any(|name| name == &scenario.account)
    {
        return;
    }
    assert_eq!(
        world.resource::<WorldTerrain>().generator.active_map_id(),
        scenario.map
    );
    let mut state = SystemState::<(Commands, Res<WorldTerrain>)>::new(world);
    let (mut commands, terrain) = state.get_mut(world).expect("army lab resources");
    let mut groups = Vec::new();
    for group in 0..scenario.battalions {
        let mut soldiers = Vec::new();
        for index in 0..scenario.soldiers_per_battalion {
            let point =
                Vec3::from_array(scenario.origin) + Vec3::new(group as f32 * 20.0, 0.0, 0.0);
            let seed = 800_000 + (group * scenario.soldiers_per_battalion + index) as u64;
            let entity = crate::player::hero::spawn_villager(&mut commands, &terrain, seed, point);
            let mut unit = commands.entity(entity);
            crate::player::army::discharge_from_village_life(&mut unit);
            unit.insert((CommandedBy(scenario.account.clone()), PersonId(seed)));
            soldiers.push(FormationSoldier {
                entity,
                identity: seed,
                position: point,
                strength: CharacterAttributes::from_seed(seed).physique(),
            });
        }
        groups.push(FormationGroup {
            key: group as u64,
            soldiers,
        });
    }
    let blocks = shared::formation::layout(
        groups,
        Vec3::from_array(scenario.origin),
        Some(FormationFrontage {
            facing: Vec2::Y,
            width: scenario.deployments[0].width,
        }),
    );
    let mut memberships = Vec::new();
    for block in blocks {
        let mut members = Vec::new();
        for (entity, mut point) in block.slots {
            point.y = terrain.get_height(point.x, point.z);
            commands.entity(entity).insert((
                PlayerPosition(point),
                PlayerRotation(std::f32::consts::PI),
                shared::region::RegionCoord::from_world_pos(point),
            ));
            members.push(entity);
        }
        memberships.push(members);
    }
    state.apply(world);
    for members in memberships {
        assert_eq!(
            crate::player::army::apply_army_order(
                world,
                &scenario.account,
                ArmyOrder::Muster { members }
            )
            .0,
            scenario.soldiers_per_battalion
        );
    }
    if let Some(battle) = &scenario.battle {
        stage_defenders(world, &scenario, battle);
    }
    world.insert_resource(Staged);
    info!(
        "Army lab staged {} soldiers in {} battalions for {}",
        scenario.total(),
        scenario.battalions,
        scenario.account
    );
}

fn stage_defenders(
    world: &mut World,
    scenario: &ArmyLabScenario,
    battle: &shared::army_lab::BattleScenario,
) {
    let account = "battlelab_enemy";
    let mut state = SystemState::<(Commands, Res<WorldTerrain>)>::new(world);
    let (mut commands, terrain) = state.get_mut(world).unwrap();
    let facing = Vec2::from_array(battle.defender_facing).normalize();
    let mut groups = Vec::new();
    for group in 0..battle.defender_battalions {
        let mut soldiers = Vec::new();
        for index in 0..battle.defenders_per_battalion {
            let seed = 900_000 + (group * battle.defenders_per_battalion + index) as u64;
            let p = Vec3::from_array(battle.defender_origin);
            let entity = crate::player::hero::spawn_villager(&mut commands, &terrain, seed, p);
            let mut unit = commands.entity(entity);
            crate::player::army::discharge_from_village_life(&mut unit);
            unit.insert((CommandedBy(account.into()), PersonId(seed)));
            soldiers.push(FormationSoldier {
                entity,
                identity: seed,
                position: p,
                strength: CharacterAttributes::from_seed(seed).physique(),
            });
        }
        groups.push(FormationGroup {
            key: group as u64,
            soldiers,
        });
    }
    for index in 0..battle.independent_attackers {
        let seed = 850_000 + index as u64;
        let p = Vec3::from_array(battle.defender_origin)
            - Vec3::new(facing.x, 0.0, facing.y) * 14.0
            + Vec3::X * (index as f32 - 1.0) * 2.0;
        let entity = crate::player::hero::spawn_villager(&mut commands, &terrain, seed, p);
        let mut unit = commands.entity(entity);
        crate::player::army::discharge_from_village_life(&mut unit);
        unit.insert((
            CommandedBy(scenario.account.clone()),
            PersonId(seed),
            crate::player::orders::CommandStance::Hold,
        ));
    }
    let blocks = shared::formation::layout(
        groups,
        Vec3::from_array(battle.defender_origin),
        Some(FormationFrontage {
            facing,
            width: 12.6 * battle.defender_battalions as f32
                + 5.0 * battle.defender_battalions.saturating_sub(1) as f32
                + 0.01,
        }),
    );
    let mut memberships = Vec::new();
    for block in blocks {
        let mut members = Vec::new();
        for (entity, mut p) in block.slots {
            p.y = terrain.get_height(p.x, p.z);
            commands.entity(entity).insert((
                PlayerPosition(p),
                PlayerRotation(f32::atan2(-facing.x, -facing.y)),
                shared::region::RegionCoord::from_world_pos(p),
            ));
            members.push(entity);
        }
        memberships.push(members);
    }
    state.apply(world);
    for members in memberships {
        crate::player::army::apply_army_order(
            world,
            account,
            ArmyOrder::Muster {
                members: members.clone(),
            },
        );
        crate::player::orders::apply_unit_order(
            world,
            account,
            shared::protocol::UnitOrder {
                selection: shared::protocol::UnitSelection {
                    units: members,
                    battalions: vec![],
                },
                command: shared::protocol::UnitCommand::Hold,
            },
        );
    }
    info!(
        "Battle lab: {} attackers / {} defenders",
        scenario.total(),
        battle.defender_battalions * battle.defenders_per_battalion
    );
}
