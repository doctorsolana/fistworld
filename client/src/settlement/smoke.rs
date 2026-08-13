//! Cheap, production-driven chimney smoke for settlement workplaces.
//!
//! The bakery GLB supplies only an `FX_ChimneySmoke` anchor. Simulation truth
//! decides when it fires, while this client-only pool owns soft presentation:
//! real-time pacing, wind drift, growth and fade. Time warp therefore makes the
//! bakery produce faster without turning its chimney into a particle cannon.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use shared::components::{
    BuildingId, CloudSeed, SettlementBuilding, SettlementBuildingKind, WorkplaceOperation,
    WorldTime,
};

use crate::states::GameState;
use crate::streaming::{camera_view_distance, streaming_anchor, AnchorCamera, AnchorPlayer};

use super::BuildingVisual;

const CHIMNEY_ANCHOR: &str = "FX_ChimneySmoke";
const MATERIAL_STAGES: usize = 6;
const MAX_SMOKE_PARTICLES: usize = 256;
const SMOKE_TEXTURE: &str = "fx/smoke_puff.png";

pub(super) struct BakerySmokePlugin;

impl Plugin for BakerySmokePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SmokePool>()
            .add_systems(Startup, setup_smoke_assets)
            .add_systems(
                Update,
                (
                    discover_bakery_smoke_emitters,
                    update_smoke_particles,
                    emit_bakery_smoke,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(OnExit(GameState::Playing), clear_smoke_pool);
    }
}

/// Every puff shares one two-triangle card, one texture and one of six fade
/// materials.
#[derive(Resource)]
struct SmokeAssets {
    mesh: Handle<Mesh>,
    materials: [Handle<StandardMaterial>; MATERIAL_STAGES],
}

#[derive(Resource, Default)]
struct SmokePool {
    allocated: usize,
}

#[derive(Component)]
struct BakerySmokeEmitter {
    anchor: Entity,
    intensity: f32,
    accumulator: f32,
    seed: u64,
    sequence: u32,
}

#[derive(Component)]
struct SmokeParticle {
    age: f32,
    lifetime: f32,
    velocity: Vec3,
    start_scale: f32,
    end_scale: f32,
    aspect: Vec3,
    curl_phase: f32,
    spin: f32,
    roll: f32,
    material_stage: usize,
}

/// An exhausted puff remains allocated and hidden for the next chimney.
#[derive(Component)]
struct InactiveSmoke;

fn setup_smoke_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A camera-facing card is two triangles instead of the old faceted sphere.
    // Its authored density texture supplies the apparent internal volume.
    let mesh = meshes.add(Rectangle::new(1.0, 1.0));
    let texture = asset_server.load(SMOKE_TEXTURE);
    let colors = [
        Color::srgba(0.46, 0.42, 0.38, 0.42),
        Color::srgba(0.50, 0.47, 0.43, 0.36),
        Color::srgba(0.54, 0.52, 0.49, 0.29),
        Color::srgba(0.58, 0.57, 0.54, 0.21),
        Color::srgba(0.62, 0.62, 0.59, 0.13),
        Color::srgba(0.67, 0.67, 0.64, 0.045),
    ];
    let materials = colors.map(|base_color| {
        materials.add(StandardMaterial {
            base_color,
            base_color_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            // Stable readability in both noon glare and moonlight. The low
            // alpha and overlapping faceted silhouettes provide the volume.
            unlit: true,
            ..default()
        })
    });
    commands.insert_resource(SmokeAssets { mesh, materials });
}

/// Scene instantiation is asynchronous, so this deliberately retries bakeries
/// until the authored anchor appears in their descendant hierarchy.
fn discover_bakery_smoke_emitters(
    mut commands: Commands,
    bakeries: Query<
        (
            Entity,
            &SettlementBuilding,
            Option<&BuildingId>,
            Option<&WorkplaceOperation>,
        ),
        (With<BuildingVisual>, Without<BakerySmokeEmitter>),
    >,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for (bakery, building, building_id, operation) in bakeries.iter() {
        if building.kind != SettlementBuildingKind::Bakery {
            continue;
        }
        let mut stack = vec![bakery];
        let mut anchor = None;
        while let Some(entity) = stack.pop() {
            if names
                .get(entity)
                .is_ok_and(|name| name.as_str() == CHIMNEY_ANCHOR)
            {
                anchor = Some(entity);
                break;
            }
            if let Ok(descendants) = children.get(entity) {
                stack.extend(descendants.iter());
            }
        }
        let Some(anchor) = anchor else {
            continue;
        };
        let seed = building_id.map_or_else(|| bakery.to_bits(), |id| id.0);
        let active = operation.is_some_and(|operation| operation.is_active());
        commands.entity(bakery).insert(BakerySmokeEmitter {
            anchor,
            intensity: if active { 1.0 } else { 0.0 },
            // Stagger chimneys and give an already-working captured bakery a
            // visible first puff promptly without an all-at-once startup burst.
            accumulator: sample(seed, 0, 0) * 0.55,
            seed,
            sequence: 0,
        });
    }
}

fn update_smoke_particles(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SmokeAssets>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut particles: Query<
        (
            Entity,
            &mut SmokeParticle,
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<InactiveSmoke>,
    >,
) {
    let dt = time.delta_secs().min(0.2);
    let camera_position = cameras.iter().next().map(GlobalTransform::translation);
    for (entity, mut particle, mut transform, mut visibility, mut material) in particles.iter_mut()
    {
        particle.age += dt;
        if particle.age >= particle.lifetime {
            *visibility = Visibility::Hidden;
            commands.entity(entity).insert(InactiveSmoke);
            continue;
        }

        let progress = (particle.age / particle.lifetime).clamp(0.0, 1.0);
        // Buoyancy gently increases as the hot puff becomes diffuse. A small
        // perpendicular curl stops the stream looking like beads on a wire.
        particle.velocity.y += 0.035 * dt;
        let horizontal = Vec2::new(particle.velocity.x, particle.velocity.z);
        let cross = Vec3::new(-horizontal.y, 0.0, horizontal.x).normalize_or_zero();
        let curl = (particle.curl_phase + particle.age * 1.7).sin() * (0.09 + 0.08 * progress);
        transform.translation += (particle.velocity + cross * curl) * dt;
        particle.roll += particle.spin * dt;

        let growth = 1.0 - (1.0 - progress).powi(2);
        let scale = particle.start_scale.lerp(particle.end_scale, growth);
        transform.scale = Vec3::new(particle.aspect.x * scale, particle.aspect.y * scale, 1.0);
        if let Some(camera_position) = camera_position {
            transform.look_at(camera_position, Vec3::Y);
            transform.rotate_local_z(particle.roll);
        }

        let stage = smoke_material_stage(progress);
        if stage != particle.material_stage {
            particle.material_stage = stage;
            material.0 = assets.materials[stage].clone();
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn emit_bakery_smoke(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SmokeAssets>,
    mut pool: ResMut<SmokePool>,
    player: AnchorPlayer,
    camera: AnchorCamera,
    world_time: Query<&WorldTime>,
    cloud_seed: Query<&CloudSeed>,
    anchors: Query<&GlobalTransform>,
    mut emitters: Query<(Entity, Option<&WorkplaceOperation>, &mut BakerySmokeEmitter)>,
    mut inactive: Query<
        (
            Entity,
            &mut SmokeParticle,
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        With<InactiveSmoke>,
    >,
) {
    let Some(view_anchor) = streaming_anchor(&player, &camera) else {
        return;
    };
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let absolute_seconds = clock.day as f32 * clock.cycle_duration() + clock.seconds_in_cycle;
    let seed_phase = crate::wind::wind_seed_phase(
        cloud_seed
            .iter()
            .next()
            .map_or(0, |cloud_seed| cloud_seed.seed),
    );
    let direction = crate::wind::wind_direction(absolute_seconds, seed_phase);
    let (_, wind_speed) = crate::wind::wind_state(absolute_seconds, seed_phase);
    let visible_radius = (camera_view_distance(&camera) * 1.4 + 40.0).clamp(120.0, 480.0);
    let visible_radius_sq = visible_radius * visible_radius;
    let dt = time.delta_secs().min(0.2);
    let mut available = inactive.iter_mut();

    for (bakery, operation, mut emitter) in emitters.iter_mut() {
        let Ok(anchor_transform) = anchors.get(emitter.anchor) else {
            // Handles a scene hot-reload: rediscover the replacement anchor.
            commands.entity(bakery).remove::<BakerySmokeEmitter>();
            continue;
        };
        let active_workers = operation.map_or(0, |operation| operation.active_workers);
        let target = if active_workers > 0 {
            1.0 + f32::from(active_workers.saturating_sub(1).min(2)) * 0.12
        } else {
            0.0
        };
        let rate = if target > emitter.intensity {
            1.8
        } else {
            0.55
        };
        emitter.intensity = move_towards(emitter.intensity, target, rate * dt);

        let origin = anchor_transform.translation();
        if Vec2::new(origin.x - view_anchor.x, origin.z - view_anchor.z).length_squared()
            > visible_radius_sq
        {
            // No accumulated debt: panning toward a bakery should reveal its
            // current smoke, not replay every off-screen puff in one frame.
            emitter.accumulator = emitter.accumulator.min(0.2);
            continue;
        }

        emitter.accumulator += dt * emitter.intensity;
        let mut emitted_this_frame = 0;
        loop {
            let interval = 0.30 + sample(emitter.seed, emitter.sequence, 1) * 0.18;
            if emitter.accumulator < interval || emitted_this_frame >= 2 {
                break;
            }
            emitter.accumulator -= interval;
            let particle = make_particle(emitter.seed, emitter.sequence, direction, wind_speed);
            let transform = particle_transform(origin, &particle);
            emitter.sequence = emitter.sequence.wrapping_add(1);
            emitted_this_frame += 1;

            if let Some((entity, mut old, mut old_transform, mut visibility, mut material)) =
                available.next()
            {
                *old = particle;
                *old_transform = transform;
                *visibility = Visibility::Inherited;
                material.0 = assets.materials[0].clone();
                commands.entity(entity).remove::<InactiveSmoke>();
            } else if pool.allocated < MAX_SMOKE_PARTICLES {
                pool.allocated += 1;
                commands.spawn((
                    Name::new("Pooled bakery chimney smoke"),
                    particle,
                    Mesh3d(assets.mesh.clone()),
                    MeshMaterial3d(assets.materials[0].clone()),
                    transform,
                    Visibility::Inherited,
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            } else {
                // Preserve a little accumulated time so a slot becoming free
                // produces one puff, never a catch-up burst.
                emitter.accumulator = emitter.accumulator.min(interval);
                break;
            }
        }
    }
}

fn make_particle(seed: u64, sequence: u32, wind: Vec2, wind_speed: f32) -> SmokeParticle {
    let lateral = (sample(seed, sequence, 2) - 0.5) * 0.12;
    let cross = Vec2::new(-wind.y, wind.x);
    let horizontal =
        wind * wind_speed * (0.13 + sample(seed, sequence, 3) * 0.055) + cross * lateral;
    SmokeParticle {
        age: 0.0,
        lifetime: 5.3 + sample(seed, sequence, 4) * 1.8,
        velocity: Vec3::new(
            horizontal.x,
            0.64 + sample(seed, sequence, 5) * 0.23,
            horizontal.y,
        ),
        start_scale: 0.48 + sample(seed, sequence, 6) * 0.16,
        end_scale: 2.05 + sample(seed, sequence, 7) * 0.7,
        aspect: Vec3::new(
            0.78 + sample(seed, sequence, 8) * 0.46,
            0.84 + sample(seed, sequence, 9) * 0.45,
            1.0,
        ),
        curl_phase: sample(seed, sequence, 11) * std::f32::consts::TAU,
        spin: (sample(seed, sequence, 12) - 0.5) * 0.85,
        roll: sample(seed, sequence, 13) * std::f32::consts::TAU,
        material_stage: 0,
    }
}

fn particle_transform(origin: Vec3, particle: &SmokeParticle) -> Transform {
    Transform::from_translation(
        origin
            + Vec3::new(
                (particle.curl_phase * 1.7).sin() * 0.08,
                0.05,
                (particle.curl_phase * 1.3).cos() * 0.08,
            ),
    )
    .with_rotation(Quat::from_rotation_z(particle.roll))
    .with_scale(Vec3::new(
        particle.aspect.x * particle.start_scale,
        particle.aspect.y * particle.start_scale,
        1.0,
    ))
}

fn smoke_material_stage(progress: f32) -> usize {
    ((progress.clamp(0.0, 0.999_999) * MATERIAL_STAGES as f32) as usize).min(MATERIAL_STAGES - 1)
}

fn move_towards(current: f32, target: f32, maximum_delta: f32) -> f32 {
    if (target - current).abs() <= maximum_delta {
        target
    } else {
        current + (target - current).signum() * maximum_delta
    }
}

/// Deterministic SplitMix sample: visual variety without a per-frame RNG or a
/// dependency on frame ordering.
fn sample(seed: u64, sequence: u32, channel: u32) -> f32 {
    let mut x = seed
        ^ u64::from(sequence).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ u64::from(channel).wrapping_mul(0xD1B5_4A32_D192_ED03);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x as f64 / u64::MAX as f64) as f32
}

fn clear_smoke_pool(
    mut commands: Commands,
    particles: Query<Entity, With<SmokeParticle>>,
    mut pool: ResMut<SmokePool>,
) {
    for entity in particles.iter() {
        commands.entity(entity).despawn();
    }
    pool.allocated = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_fade_stages_cover_the_whole_lifetime() {
        assert_eq!(smoke_material_stage(0.0), 0);
        assert_eq!(smoke_material_stage(0.5), 3);
        assert_eq!(smoke_material_stage(1.0), MATERIAL_STAGES - 1);
    }

    #[test]
    fn deterministic_particles_vary_without_frame_state() {
        let wind = Vec2::new(0.8, 0.6).normalize();
        let first = make_particle(42, 7, wind, 3.0);
        let repeated = make_particle(42, 7, wind, 3.0);
        let next = make_particle(42, 8, wind, 3.0);
        assert_eq!(first.lifetime, repeated.lifetime);
        assert_eq!(first.velocity, repeated.velocity);
        assert_ne!(first.lifetime, next.lifetime);
    }
}
