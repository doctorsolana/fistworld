//! Connected verification fixture. Only the lab chooses targets; all firing,
//! movement, damage and standing responses are production systems.
use bevy::prelude::*;
use shared::{components::*, protocol::*};
#[derive(Resource)]
struct BombardmentLab {
    targets: Vec<(Entity, Entity)>,
    fired: bool,
    stopped: bool,
    account: String,
}
pub(super) fn stage(world: &mut World, account: &str) {
    let mut battalions: Vec<_> = world
        .query::<(Entity, &Battalion, &CommandedBy)>()
        .iter(world)
        .filter(|(_, _, o)| o.0 == account)
        .map(|(e, b, _)| (e, b.id))
        .collect();
    battalions.sort_by_key(|(_, id)| *id);
    let mut targets = Vec::new();
    for (battalion, id) in battalions.into_iter().take(2) {
        let centre = shared::formation::centre(
            world
                .query::<(&MemberOfBattalion, &PlayerPosition)>()
                .iter(world)
                .filter(|(m, _)| m.0 == id)
                .map(|(_, p)| p.0),
        );
        let mut p = centre - Vec3::Z * 65.0;
        p.y = world
            .resource::<shared::terrain::WorldTerrain>()
            .get_height(p.x, p.z);
        let catapult =
            crate::player::siege::spawn_catapult(&mut world.commands(), "battlelab_enemy", p);
        world.flush();
        world
            .entity_mut(catapult)
            .insert(PlayerRotation(std::f32::consts::PI));
        targets.push((battalion, catapult));
    }
    // Keep every fixture member alive so position invariance can be checked
    // for the entire held line, including people nearest the stone.
    let soldiers: Vec<_> = world
        .query_filtered::<Entity, (With<MemberOfBattalion>, With<CharacterKind>)>()
        .iter(world)
        .collect();
    for e in soldiers {
        world.entity_mut(e).insert(Health::new(300.0));
    }
    world.insert_resource(BombardmentLab {
        targets,
        fired: false,
        stopped: false,
        account: account.into(),
    });
}
pub fn drive_management_bombardment(world: &mut World) {
    let Some(lab) = world.get_resource::<BombardmentLab>() else {
        return;
    };
    if lab.stopped {
        return;
    }
    let targets = lab.targets.clone();
    let account = lab.account.clone();
    if !lab.fired {
        if targets.len() != 2
            || world.get::<BattalionStance>(targets[1].0) != Some(&BattalionStance::HoldLine)
        {
            return;
        }
        for (battalion, catapult) in targets {
            let id = world.get::<Battalion>(battalion).unwrap().id;
            let aim = shared::formation::centre(
                world
                    .query::<(&MemberOfBattalion, &PlayerPosition, &CommandedBy)>()
                    .iter(world)
                    .filter(|(m, _, o)| m.0 == id && o.0 == account)
                    .map(|(_, p, _)| p.0),
            );
            assert!(crate::player::siege::order_catapult(
                world,
                "battlelab_enemy",
                catapult,
                UnitCommand::AttackGround { target: aim }
            )
            .is_ok());
        }
        world.resource_mut::<BombardmentLab>().fired = true;
        info!("Management lab: both battalions under real catapult fire");
    } else if targets.iter().all(|(_, e)| {
        world
            .get::<Catapult>(*e)
            .is_some_and(|c| c.ammunition == 19)
    }) {
        for (_, catapult) in targets {
            crate::player::siege::order_catapult(
                world,
                "battlelab_enemy",
                catapult,
                UnitCommand::Hold,
            )
            .unwrap();
        }
        world.resource_mut::<BombardmentLab>().stopped = true;
    }
}
