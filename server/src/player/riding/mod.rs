//! Server-owned single-hero riding. Horse and rider share one navigation route;
//! the replicated hero position remains on the ground, while the client places
//! its visual on the animated mount anchor. Horses are transient world actors.
use super::{
    hero::{MoveTarget, OfflineHero},
    orders::{self, FormationRoutes, MarchOrder},
};
use bevy::prelude::*;
use shared::{
    components::*,
    protocol::{MovementMode, UnitCommand},
    region::RegionCoord,
    terrain::WorldTerrain,
};
mod wild;
pub use wild::tick;

#[derive(Resource, Default)]
struct HorseIds(u64);
#[derive(Component)]
struct WildHorse {
    home: Vec3,
    next_decision: f64,
    serial: u64,
    target: Option<Vec3>,
}
#[derive(Component)]
struct DismountLanding(Vec3);

fn now(world: &mut World) -> f64 {
    world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map_or(0., |c| {
            f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
        })
}
fn grounded(world: &World, p: Vec3) -> Vec3 {
    Vec3::new(
        p.x,
        world
            .get_resource::<WorldTerrain>()
            .map_or(p.y, |t| t.get_height(p.x, p.z)),
        p.z,
    )
}
fn clear(world: &World, a: Vec3, b: Vec3, radius: f32) -> bool {
    if !a.is_finite() || !b.is_finite() || b.abs().max_element() > 1_000_000. {
        return false;
    }
    if let Some(terrain) = world.get_resource::<WorldTerrain>() {
        let bounds = terrain.generator.active_map_bounds();
        if (b.xz() - Vec2::splat(radius))
            .cmplt(Vec2::from_array(bounds.min))
            .any()
            || (b.xz() + Vec2::splat(radius))
                .cmpgt(Vec2::from_array(bounds.max))
                .any()
        {
            return false;
        }
    }
    super::siege::ground_clear(
        a.xz(),
        b.xz(),
        radius,
        world.get_resource::<WorldTerrain>(),
        world.get_resource::<shared::spatial::SpatialObstacleGrid>(),
        world.get_resource::<crate::collision::library::StaticColliders>(),
        world.get_resource::<crate::collision::library::DerivedColliderLibrary>(),
    )
}
pub fn spawn_checked(world: &mut World, position: Vec3) -> Result<Entity, &'static str> {
    if !clear(world, position, position, HORSE_CLEARANCE) {
        return Err("Horse needs clear, dry ground");
    }
    let mut count = 0;
    for (_, at) in world.query::<(&Horse, &PlayerPosition)>().iter(world) {
        count += 1;
        if at.0.xz().distance_squared(position.xz()) < (HORSE_CLEARANCE * 2.).powi(2) {
            return Err("Leave room beside the other horse");
        }
    }
    if count >= 32 {
        return Err("This world already has 32 horses");
    }
    world.init_resource::<HorseIds>();
    let id = {
        let mut ids = world.resource_mut::<HorseIds>();
        ids.0 += 1;
        ids.0
    };
    let position = grounded(world, position);
    let now = now(world);
    Ok(world
        .spawn((
            Horse { id, rider: None },
            HorseAnimation {
                activity: HorseActivity::Idle,
                since: now,
            },
            WildHorse {
                home: position,
                next_decision: now + 2. + (id % 4) as f64,
                serial: id,
                target: None,
            },
            PlayerPosition(position),
            PlayerRotation(0.),
            CharacterMotion::STATIONARY,
            RegionCoord::from_world_pos(position),
            lightyear::prelude::Replicate::to_clients(lightyear::prelude::NetworkTarget::All),
        ))
        .id())
}
fn horse_for(world: &mut World, id: u64) -> Option<Entity> {
    world
        .query::<(Entity, &Horse)>()
        .iter(world)
        .find(|(_, h)| h.id == id)
        .map(|(e, _)| e)
}
fn set_activity(world: &mut World, entity: Entity, activity: HorseActivity, now: f64) {
    if world
        .get::<HorseAnimation>(entity)
        .is_none_or(|a| a.activity != activity)
    {
        world.entity_mut(entity).insert(HorseAnimation {
            activity,
            since: now,
        });
    }
}
fn stop(world: &mut World, entity: Entity) {
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
        world.get_mut::<WildHorse>(horse).unwrap().target = None;
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

#[cfg(test)]
mod tests;
