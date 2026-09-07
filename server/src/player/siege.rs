//! Authoritative siege orders and lifecycle. Projectiles outlive their launcher;
//! clients only sample the launch/impact timeline and articulated presentation.
use crate::player::{
    hero::MoveTarget,
    orders::{FormationRoutes, MarchOrder},
};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use bevy::prelude::*;
use shared::{components::*, protocol::UnitCommand, region::RegionCoord, terrain::WorldTerrain};
mod fire;
pub use fire::{advance_catapults, resolve_siege_projectiles};

#[derive(Component, Clone, Copy)]
pub enum SiegeTarget {
    Person(Entity),
    Ground(Vec3),
}
#[derive(Component)]
pub struct CatapultWreck(pub f64);

fn catapult_bundle(account: &str, position: Vec3) -> impl Bundle {
    (
        Catapult {
            ammunition: CATAPULT_AMMUNITION,
        },
        CatapultStatus::default(),
        CommandedBy(account.into()),
        Health::new(350.0),
        PlayerPosition(position),
        PlayerRotation(0.0),
        CharacterMotion::STATIONARY,
        RegionCoord::from_world_pos(position),
        lightyear::prelude::Replicate::to_clients(lightyear::prelude::NetworkTarget::All),
    )
}
pub fn spawn_catapult(commands: &mut Commands, account: &str, position: Vec3) -> Entity {
    commands.spawn(catapult_bundle(account, position)).id()
}

pub fn order_catapult(
    world: &mut World,
    account: &str,
    entity: Entity,
    command: UnitCommand,
) -> Result<(), &'static str> {
    if world.get::<Catapult>(entity).is_none()
        || world
            .get::<CommandedBy>(entity)
            .is_none_or(|o| o.0 != account)
        || world.get::<Health>(entity).is_none_or(|h| h.is_dead())
    {
        return Err("Catapult unavailable");
    }
    let from = world
        .get::<PlayerPosition>(entity)
        .ok_or("Catapult unavailable")?
        .0;
    let target = match command {
        UnitCommand::Attack { target, .. } => Some((
            SiegeTarget::Person(target),
            world
                .get::<PlayerPosition>(target)
                .ok_or("Target unavailable")?
                .0,
        )),
        UnitCommand::AttackGround { target } => Some((SiegeTarget::Ground(target), target)),
        _ => None,
    };
    if let Some((_, at)) = target {
        if !siege_in_range(from, at) {
            return Err("Catapult range is 16-125 m; reposition before firing");
        }
        if world.get::<Catapult>(entity).unwrap().ammunition == 0 {
            return Err("Catapult is out of stones");
        }
    }
    if let UnitCommand::Move { target, .. } = command {
        if !target.is_finite() || !placement_clear(world, target) {
            return Err("Catapult needs dry navigable ground");
        }
    }
    // A new order cancels a pending wind-up, never an already released stone
    // or the reload deadline. Order-spamming cannot buy a faster weapon.
    let was_winding = world
        .get::<CatapultStatus>(entity)
        .is_some_and(|s| s.phase == SiegePhase::Winding);
    world.entity_mut(entity).remove::<(
        SiegeTarget,
        MarchOrder,
        MoveTarget,
        TravelRoute,
        NavigationRoutePending,
        NavigationRouteFailed,
    )>();
    let mut status = *world.get::<CatapultStatus>(entity).unwrap();
    status.aim = target.map(|(_, p)| p);
    if was_winding {
        status.fire_at = 0.0;
    }
    if let Some((target, _)) = target {
        world.entity_mut(entity).insert(target);
        status.phase = SiegePhase::Turning;
    } else if let UnitCommand::Move { target, .. } = command {
        let terrain = world.resource::<WorldTerrain>();
        let to = Vec3::new(target.x, terrain.get_height(target.x, target.z), target.z);
        let facing = (to - from).xz().normalize_or_zero();
        world.init_resource::<FormationRoutes>();
        let group = world
            .resource_mut::<FormationRoutes>()
            .register_with_clearance(vec![from.xz(), to.xz()], to.xz(), CATAPULT_CLEARANCE);
        world.entity_mut(entity).insert((
            MarchOrder {
                destination: to,
                facing,
                group,
            },
            MoveTarget(to),
        ));
        status.phase = SiegePhase::Moving;
    } else {
        status.phase = SiegePhase::Ready;
    }
    world.entity_mut(entity).insert(status);
    Ok(())
}

/// The same swept footprint certifies route edges and actual movement.
/// Centre + perimeter samples conservatively cover the timber carriage.
pub(crate) fn ground_clear(
    a: Vec2,
    b: Vec2,
    radius: f32,
    terrain: Option<&WorldTerrain>,
    buildings: Option<&shared::spatial::SpatialObstacleGrid>,
    colliders: Option<&crate::collision::library::StaticColliders>,
    derived: Option<&crate::collision::library::DerivedColliderLibrary>,
) -> bool {
    if radius == 0.0 {
        return crate::player::hero::navigation_segment_clear(a, b, buildings, colliders, derived)
            && terrain.is_none_or(|t| crate::player::hero::terrain_segment_walkable(t, a, b));
    }
    if buildings.is_some_and(|g| g.segment_blocked_with_clearance(a, b, radius))
        || colliders.zip(derived).is_some_and(|(c, d)| {
            crate::player::hero::static_prop_blocks_swept_disc(a, b, c, d, radius)
        })
    {
        return false;
    }
    let point_clear = |offset: Vec2| {
        terrain.is_none_or(|t| {
            crate::player::hero::terrain_segment_walkable(t, a + offset, b + offset)
        })
    };
    point_clear(Vec2::ZERO)
        && (radius <= 0.0
            || (0..8).all(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                point_clear(Vec2::new(angle.cos(), angle.sin()) * radius)
            }))
}
pub fn placement_clear(world: &World, position: Vec3) -> bool {
    if let Some(terrain) = world.get_resource::<WorldTerrain>() {
        let bounds = terrain.generator.active_map_bounds();
        if (position.xz() - Vec2::splat(CATAPULT_CLEARANCE))
            .cmplt(Vec2::from_array(bounds.min))
            .any()
            || (position.xz() + Vec2::splat(CATAPULT_CLEARANCE))
                .cmpgt(Vec2::from_array(bounds.max))
                .any()
        {
            return false;
        }
    }
    position.is_finite()
        && position.abs().max_element() < 1_000_000.0
        && ground_clear(
            position.xz(),
            position.xz(),
            CATAPULT_CLEARANCE,
            world.get_resource::<WorldTerrain>(),
            world.get_resource::<shared::spatial::SpatialObstacleGrid>(),
            world.get_resource::<crate::collision::library::StaticColliders>(),
            world.get_resource::<crate::collision::library::DerivedColliderLibrary>(),
        )
}
pub fn spawn_checked(world: &mut World, account: &str, position: Vec3) -> Result<(), &'static str> {
    if !placement_clear(world, position) {
        return Err("Catapult needs clear, dry ground");
    }
    let mut count = 0;
    for (owner, other) in world
        .query_filtered::<(&CommandedBy, &PlayerPosition), With<Catapult>>()
        .iter(world)
    {
        count += usize::from(owner.0 == account);
        if position.xz().distance_squared(other.0.xz()) < (CATAPULT_CLEARANCE * 2.0).powi(2) {
            return Err("Leave room beside the existing catapult");
        }
    }
    if count >= 8 {
        return Err("Limit of 8 catapults reached");
    }
    let y = world
        .resource::<WorldTerrain>()
        .get_height(position.x, position.z);
    // Immediate insertion makes the cap true even for several placement
    // messages drained in the same server frame.
    world.spawn(catapult_bundle(
        account,
        Vec3::new(position.x, y, position.z),
    ));
    Ok(())
}

#[cfg(test)]
mod tests;
