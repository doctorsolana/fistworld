//! Horse records and shared placement/ground-clearance boundary.
use bevy::prelude::*;
use shared::{components::*, region::RegionCoord, terrain::WorldTerrain};

pub(crate) const MAX_HORSES: usize = 128;
pub(crate) const MAX_ACTIVE_HORSES: usize = 32;
pub(crate) const OBSERVATION_RADIUS: f32 = 180.0;
#[derive(Component)]
pub(crate) struct ActiveWildHorse;

#[derive(Resource, Default)]
struct HorseIds(u64);

/// One session identity namespace for both wildlife and provisioned army mounts.
pub(crate) fn allocate_id(world: &mut World) -> u64 {
    world.init_resource::<HorseIds>();
    let mut ids = world.resource_mut::<HorseIds>();
    ids.0 = ids
        .0
        .checked_add(1)
        .expect("horse identity space exhausted");
    ids.0
}
#[derive(Component)]
pub(crate) struct WildHorse {
    pub(crate) home: Vec3,
    pub(crate) next_decision: f64,
    pub(crate) serial: u64,
    pub(crate) target: Option<Vec3>,
}
pub(crate) fn now(world: &mut World) -> f64 {
    world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map_or(0., |c| {
            f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
        })
}
pub(crate) fn grounded(world: &World, p: Vec3) -> Vec3 {
    Vec3::new(
        p.x,
        world
            .get_resource::<WorldTerrain>()
            .map_or(p.y, |t| t.get_height(p.x, p.z)),
        p.z,
    )
}
pub(crate) fn clear(world: &World, a: Vec3, b: Vec3, radius: f32) -> bool {
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
    crate::player::siege::ground_clear(
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
    for (_, at, wild) in world
        .query::<(&Horse, &PlayerPosition, Has<WildHorse>)>()
        .iter(world)
    {
        count += usize::from(wild);
        if at.0.xz().distance_squared(position.xz()) < (HORSE_CLEARANCE * 2.).powi(2) {
            return Err("Leave room beside the other horse");
        }
    }
    if count >= MAX_HORSES {
        return Err("The world horse population limit has been reached");
    }
    let id = allocate_id(world);
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
pub(crate) fn set_activity(world: &mut World, entity: Entity, activity: HorseActivity, now: f64) {
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
