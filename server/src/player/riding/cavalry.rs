//! Issued mounts are equipment of a soldier, not part of the ambient herd budget.
use super::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::region::RegionCoord;

#[derive(Component)]
pub(super) struct CavalryMount;

/// Provision an already conscripted soldier at a certified, spacious staging point.
/// Called by development scenarios; public equipment orders never mint horses.
pub fn equip_cavalry(world: &mut World, rider: Entity) -> Result<Entity, &'static str> {
    if world.get::<Mounted>(rider).is_some() {
        return Err("Soldier already has a mount");
    }
    let person = *world
        .get::<PersonId>(rider)
        .ok_or("Soldier has no identity")?;
    let owner = world
        .get::<CommandedBy>(rider)
        .cloned()
        .ok_or("Soldier has no commander")?;
    if world.get::<CharacterKind>(rider).is_none()
        || world.get::<Health>(rider).is_none_or(|h| h.is_dead())
        || world.get::<AboardBoat>(rider).is_some()
    {
        return Err("Soldier unavailable");
    }
    let at = world
        .get::<PlayerPosition>(rider)
        .ok_or("Soldier has no position")?
        .0;
    if !clear(world, at, at, HORSE_CLEARANCE) {
        return Err("Mount needs clear, dry ground");
    }
    if world
        .query::<(&Horse, &PlayerPosition)>()
        .iter(world)
        .any(|(_, p)| p.0.xz().distance_squared(at.xz()) < (2.0 * HORSE_CLEARANCE).powi(2))
    {
        return Err("Mounts need at least 3.1 m of room");
    }
    let yaw = world.get::<PlayerRotation>(rider).map_or(0., |r| r.0);
    let id = crate::world::wildlife::allocate_id(world);
    let now = now(world);
    let horse = world
        .spawn((
            Horse {
                id,
                rider: Some(person),
            },
            CavalryMount,
            owner,
            HorseAnimation {
                activity: HorseActivity::Idle,
                since: now,
            },
            PlayerPosition(at),
            PlayerRotation(yaw),
            CharacterMotion::STATIONARY,
            RegionCoord::from_world_pos(at),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    world.entity_mut(rider).insert((
        SoldierRole::Cavalry,
        Mounted {
            horse: id,
            gait: HorseGait::Gallop,
            phase: RidingPhase::Riding,
            since: now,
        },
    ));
    Ok(horse)
}

pub(crate) fn remove_equipment(world: &mut World, rider: Entity) {
    let Some(mounted) = world.get::<Mounted>(rider).copied() else {
        return;
    };
    let horse = world
        .query::<(Entity, &Horse, &CavalryMount)>()
        .iter(world)
        .find(|(_, h, _)| h.id == mounted.horse)
        .map(|(e, _, _)| e);
    if let Some(horse) = horse {
        world.despawn(horse);
        world
            .entity_mut(rider)
            .remove::<(Mounted, DismountLanding)>();
    }
}
