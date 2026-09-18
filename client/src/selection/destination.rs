//! A brief ground pulse where a move order was placed.
//!
//! Right-click already commits the order, but nothing on screen confirmed the
//! click landed where you meant: on broken ground, behind a tree or at the edge
//! of the frame you had to watch the unit start walking to know. This draws a
//! short expanding ring at the destination, in the same warm ember the HUD
//! reserves for "this is yours to command".
//!
//! Two rings, the second delayed, read as a deliberate pulse rather than a
//! sprite popping in. Both fade as they expand, so the marker never lingers as
//! clutter and never has to be cleaned up by the player.
//!
//! Lives in world space, not the HUD, because the whole point is that it sits
//! on the ground you clicked. It follows `ring.rs`'s material contract: unlit so
//! it reads at dawn and under a storm, blended and double sided so a hillside
//! cannot swallow it, and depth-biased so it does not z-fight the terrain.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use shared::terrain::WorldTerrain;

/// Destinations awaiting a pulse. `order.rs` pushes, this module drains, so the
/// order path needs no knowledge of meshes or materials.
#[derive(Resource, Default)]
pub struct DestinationPulses(pub Vec<Vec3>);

/// Shared mesh and the base colour every pulse clones its material from.
#[derive(Resource)]
struct PulseAssets {
    mesh: Handle<Mesh>,
}

#[derive(Component)]
struct Pulse {
    /// Seconds since this ring was spawned, including its stagger delay.
    age: f32,
    /// Seconds to wait before this ring starts expanding.
    delay: f32,
    /// Where the order was placed, on the flat. The ring re-samples the ground
    /// under itself every frame as it grows, so the height it needs changes.
    centre: Vec2,
    material: Handle<StandardMaterial>,
}

/// Ring radius in metres at birth and at death. The unit itself is about a
/// metre wide, so the pulse starts a little larger than the target and opens to
/// roughly a house's width -- big enough to find at a glance, small enough not
/// to claim ground the order did not.
const START_RADIUS: f32 = 0.9;
const END_RADIUS: f32 = 3.4;
const LIFETIME: f32 = 0.62;
/// The second ring starts this many seconds after the first.
const SECOND_RING_DELAY: f32 = 0.12;
/// Metres above the HIGHEST ground under the ring's footprint.
///
/// A flat ring placed at the height of its centre sinks into any slope: the
/// first version cut a third of the arc away on a gentle hillside. `ring.rs`
/// already solved this for the selection ring, so this reuses its sampler and
/// its clearance -- enough to cover the mismatch between the sampled
/// heightfield and the triangulated chunk, while still reading as lying on soil.
const GROUND_LIFT: f32 = super::ring::RING_LIFT;
/// The ember the HUD uses for commandable selection rings, so a destination and
/// the ring around the unit that will walk there are visibly the same language.
const EMBER: Srgba = Srgba::new(0.984, 0.898, 0.792, 1.0);

pub(super) fn install(app: &mut App) {
    app.init_resource::<DestinationPulses>()
        .add_systems(Startup, setup_assets)
        .add_systems(Update, (spawn_pulses, animate_pulses).chain());
}

fn setup_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // A unit annulus scaled per frame: one mesh for every pulse ever drawn,
    // rather than a new ring mesh per click.
    commands.insert_resource(PulseAssets {
        mesh: meshes.add(Annulus::new(0.93, 1.0)),
    });
}

fn spawn_pulses(
    mut commands: Commands,
    mut pending: ResMut<DestinationPulses>,
    assets: Option<Res<PulseAssets>>,
    terrain: Option<Res<WorldTerrain>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if pending.0.is_empty() {
        return;
    }
    let Some(assets) = assets else {
        pending.0.clear();
        return;
    };
    for target in pending.0.drain(..) {
        // Trust the terrain over the picked point: the ray can land on a prop
        // or a unit's collider, and a ring floating at shoulder height reads as
        // a bug rather than a destination.
        let flat = target.xz();
        let ground = terrain.as_ref().map_or(target.y, |terrain| {
            super::ring::ground_under_ring(terrain, target, START_RADIUS)
        });
        let centre = Vec3::new(flat.x, ground + GROUND_LIFT, flat.y);
        for (index, delay) in [0.0, SECOND_RING_DELAY].into_iter().enumerate() {
            // One material per ring: the fade animates base_color's alpha, and
            // sharing a handle would fade every live pulse together.
            let material = materials.add(super::ring::ring_material(Color::srgba(
                EMBER.red,
                EMBER.green,
                EMBER.blue,
                0.0,
            )));
            commands.spawn((
                Name::new(if index == 0 {
                    "Destination pulse"
                } else {
                    "Destination pulse (echo)"
                }),
                Pulse {
                    age: 0.0,
                    delay,
                    centre: flat,
                    material: material.clone(),
                },
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(material),
                // Flat on the ground: the annulus is built in the XY plane.
                Transform::from_translation(centre)
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::splat(START_RADIUS)),
                // A destination marker must never darken the world it marks.
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
}

fn animate_pulses(
    mut commands: Commands,
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pulses: Query<(Entity, &mut Pulse, &mut Transform)>,
) {
    for (entity, mut pulse, mut transform) in pulses.iter_mut() {
        pulse.age += time.delta_secs();
        let live = pulse.age - pulse.delay;
        if live < 0.0 {
            continue;
        }
        if live >= LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        let t = live / LIFETIME;
        // Ease out: the ring opens fast and settles, which reads as an impact
        // rather than a constant-speed expansion.
        let eased = 1.0 - (1.0 - t).powi(3);
        let radius = START_RADIUS + (END_RADIUS - START_RADIUS) * eased;
        transform.scale = Vec3::splat(radius);
        // Re-conform as it grows: a ring that cleared the ground at 0.9 m can
        // still be swallowed by the same slope once it reaches 3.4 m.
        if let Some(terrain) = terrain.as_ref() {
            let centre = Vec3::new(pulse.centre.x, 0.0, pulse.centre.y);
            transform.translation.y =
                super::ring::ground_under_ring(terrain, centre, radius) + GROUND_LIFT;
        }
        // Fade in over the first fifth so the ring arrives rather than blinks,
        // then out over the rest.
        let alpha = if t < 0.2 { t / 0.2 } else { 1.0 - (t - 0.2) / 0.8 };
        if let Some(mut material) = materials.get_mut(&pulse.material) {
            material.base_color = Color::srgba(EMBER.red, EMBER.green, EMBER.blue, alpha * 0.72);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// A bare world, not `MinimalPlugins`: the time plugin overwrites the clock
    /// on every update, so an advanced delta would never reach the system.
    fn world_with_pulse(delay: f32) -> (World, Entity, Handle<StandardMaterial>) {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.init_resource::<Assets<StandardMaterial>>();
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let entity = world
            .spawn((
                Pulse {
                    age: 0.0,
                    delay,
                    centre: Vec2::ZERO,
                    material: material.clone(),
                },
                Transform::from_scale(Vec3::splat(START_RADIUS)),
            ))
            .id();
        (world, entity, material)
    }

    fn advance(world: &mut World, seconds: f32) {
        world
            .resource_mut::<Time<()>>()
            .advance_by(std::time::Duration::from_secs_f32(seconds));
        world.run_system_once(animate_pulses).unwrap();
        world.flush();
    }

    #[test]
    fn a_pulse_expands_and_fades_within_its_lifetime() {
        let (mut world, entity, material) = world_with_pulse(0.0);

        advance(&mut world, LIFETIME * 0.5);
        let scale = world.get::<Transform>(entity).unwrap().scale.x;
        assert!(
            scale > START_RADIUS && scale < END_RADIUS,
            "expanding, got {scale}"
        );
        let alpha = world
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .base_color
            .alpha();
        assert!(alpha > 0.0, "still visible at mid-life, got {alpha}");

        // Past its lifetime it cleans itself up: no player action, no clutter.
        advance(&mut world, LIFETIME);
        assert!(
            world.get_entity(entity).is_err(),
            "the pulse must despawn itself"
        );
    }

    #[test]
    fn a_delayed_echo_waits_before_it_expands() {
        let (mut world, entity, _) = world_with_pulse(SECOND_RING_DELAY);
        advance(&mut world, SECOND_RING_DELAY * 0.5);
        assert_eq!(
            world.get::<Transform>(entity).unwrap().scale.x,
            START_RADIUS,
            "the echo must not move before its delay elapses"
        );
        // ...and then it does move, so the delay is a stagger and not a stall.
        advance(&mut world, SECOND_RING_DELAY + LIFETIME * 0.4);
        assert!(
            world.get::<Transform>(entity).unwrap().scale.x > START_RADIUS,
            "the echo must expand once its delay has passed"
        );
    }
}
