//! Explicit hero mounting verbs. Cavalry uses the ordinary army order stream.
use super::*;
use crate::player::{
    hero::MoveTarget,
    orders::{self, FormationRoutes, MarchOrder},
};
use shared::{
    protocol::{MovementMode, UnitCommand},
    region::RegionCoord,
};

fn horse_for(world: &mut World, id: u64) -> Option<Entity> {
    world
        .query::<(Entity, &Horse)>()
        .iter(world)
        .find(|(_, h)| h.id == id)
        .map(|(e, _)| e)
}
pub(super) fn stop(world: &mut World, entity: Entity) {
    orders::interrupt_previous_order(world, entity);
}

pub fn order(
    world: &mut World,
    account: &str,
    entity: Entity,
    command: UnitCommand,
) -> Result<(), &'static str> {
    if !orders::owned_character(world, entity, account)
        || world.get::<CharacterKind>(entity) != Some(&CharacterKind::Hero)
    {
        return Err("Select your living hero to ride");
    }
    let now = now(world);
    if let UnitCommand::Mount { horse } = command {
        if world.get::<Mounted>(entity).is_some() {
            return Err("Already mounted");
        }
        if world.get::<MemberOfBattalion>(entity).is_some()
            || world.get::<EngagedWith>(entity).is_some()
            || world.get::<BuildingDoorUse>(entity).is_some()
            || world
                .get::<shared::economy::PorterCartState>(entity)
                .is_some()
            || world
                .get::<shared::economy::CarriedLoad>(entity)
                .is_some_and(|l| !l.is_empty())
        {
            return Err("Finish fighting, carrying or entering a building before mounting");
        }
        let h = *world.get::<Horse>(horse).ok_or("Horse unavailable")?;
        if h.rider.is_some() {
            return Err("That horse already has a rider");
        }
        let person = *world
            .get::<PersonId>(entity)
            .ok_or("Hero identity unavailable")?;
        let at = world
            .get::<PlayerPosition>(horse)
            .ok_or("Horse unavailable")?
            .0;
        let from = world.get::<PlayerPosition>(entity).unwrap().0;
        if from.distance(at) > HORSE_MOUNT_REACH {
            return Err("Move your hero closer to the horse");
        }
        let yaw = world.get::<PlayerRotation>(horse).map_or(0., |r| r.0);
        let landing = grounded(
            world,
            at + Quat::from_rotation_y(yaw) * HORSE_DISMOUNT_OFFSET,
        );
        if !clear(world, from, landing, 0.24) || !clear(world, at, landing, 0.24) {
            return Err("The horse's left side is blocked");
        }
        stop(world, entity);
        world.entity_mut(entity).insert((
            Mounted {
                horse: h.id,
                gait: HorseGait::Canter,
                phase: RidingPhase::Mounting,
                since: now,
            },
            PlayerPosition(at),
            PlayerRotation(yaw),
            RegionCoord::from_world_pos(at),
        ));
        world.get_mut::<Horse>(horse).unwrap().rider = Some(person);
        if let Some(mut wild) = world.get_mut::<WildHorse>(horse) {
            wild.target = None;
        }
        world.entity_mut(horse).insert(CharacterMotion::STATIONARY);
        set_activity(world, horse, HorseActivity::Idle, now);
        return Ok(());
    }
    let mut mounted = *world
        .get::<Mounted>(entity)
        .ok_or("Your hero is not riding")?;
    if mounted.phase != RidingPhase::Riding {
        return Err("Wait until mounting or dismounting finishes");
    }
    let horse = horse_for(world, mounted.horse).ok_or("Horse unavailable")?;
    let from = world.get::<PlayerPosition>(entity).unwrap().0;
    match command {
        UnitCommand::Dismount => {
            let yaw = world.get::<PlayerRotation>(entity).map_or(0., |r| r.0);
            let landing = grounded(
                world,
                from + Quat::from_rotation_y(yaw) * HORSE_DISMOUNT_OFFSET,
            );
            if !clear(world, from, landing, 0.24) {
                return Err("Move somewhere with room to dismount on the left");
            }
            stop(world, entity);
            mounted.phase = RidingPhase::Dismounting;
            mounted.since = now;
            world
                .entity_mut(entity)
                .insert((mounted, DismountLanding(landing)));
            set_activity(world, horse, HorseActivity::Idle, now);
        }
        UnitCommand::RideGait { gait } => {
            mounted.gait = gait;
            world.entity_mut(entity).insert(mounted);
        }
        UnitCommand::Hold => stop(world, entity),
        UnitCommand::Move {
            target,
            mode: MovementMode::Move,
            ..
        } => {
            if !clear(world, target, target, HORSE_CLEARANCE) {
                return Err("Choose dry ground with room for a horse");
            }
            let target = grounded(world, target);
            stop(world, entity);
            world.init_resource::<FormationRoutes>();
            let group = world
                .resource_mut::<FormationRoutes>()
                .register_with_clearance(
                    vec![from.xz(), target.xz()],
                    target.xz(),
                    HORSE_CLEARANCE,
                );
            world.entity_mut(entity).insert((
                MarchOrder {
                    destination: target,
                    facing: (target - from).xz().normalize_or_zero(),
                    group,
                },
                MoveTarget(target),
            ));
        }
        _ => return Err("Dismount before issuing combat orders"),
    }
    Ok(())
}
