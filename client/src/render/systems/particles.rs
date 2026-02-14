//! Particle systems
//!
//! Sand/dust trail particles for vehicles.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::vehicle::{vehicle_def, Vehicle, VehicleState};

// =============================================================================
// COMPONENTS & RESOURCES
// =============================================================================

/// Sand/dust particle for vehicle trails
#[derive(Component)]
pub struct SandParticle {
    pub lifetime: f32,      // Remaining lifetime in seconds
    pub max_lifetime: f32,  // Original lifetime for fade calculation
    pub velocity: Vec3,     // Current velocity
    pub initial_scale: f32, // Starting scale
}

/// Pre-made assets for particles (avoid recreating each frame)
#[derive(Resource)]
pub struct ParticleAssets {
    pub sand_mesh: Handle<Mesh>,
    pub sand_materials: Vec<Handle<StandardMaterial>>,
}

// =============================================================================
// SETUP
// =============================================================================

/// Spawn particle assets on startup
pub fn setup_particle_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Low-poly sphere for particles
    let sand_mesh = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());

    // Multiple sand colors for variety
    let sand_colors = [
        Color::srgba(0.85, 0.75, 0.55, 0.8),  // Golden sand
        Color::srgba(0.80, 0.70, 0.50, 0.7),  // Darker sand
        Color::srgba(0.90, 0.80, 0.60, 0.75), // Light sand
        Color::srgba(0.75, 0.65, 0.45, 0.7),  // Brown sand
    ];

    let sand_materials: Vec<_> = sand_colors
        .iter()
        .map(|&color| {
            materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: true, // Unlit for better visibility
                ..default()
            })
        })
        .collect();

    commands.insert_resource(ParticleAssets {
        sand_mesh,
        sand_materials,
    });
}

// =============================================================================
// SPAWNING
// =============================================================================

const MAX_SAND_PARTICLES: usize = 50;

/// Spawn sand particles behind moving vehicles
pub fn spawn_sand_particles(
    mut commands: Commands,
    particle_assets: Option<Res<ParticleAssets>>,
    vehicles: Query<(&Vehicle, &VehicleState, &Transform)>,
    existing_particles: Query<(), With<SandParticle>>,
    time: Res<Time>,
) {
    let Some(assets) = particle_assets else {
        return;
    };

    // Don't spawn if already at cap
    if existing_particles.iter().len() >= MAX_SAND_PARTICLES {
        return;
    }

    for (vehicle, state, transform) in vehicles.iter() {
        let def = vehicle_def(vehicle.vehicle_type);
        // Only spawn particles when grounded and moving
        if !state.grounded {
            continue;
        }

        // Use horizontal speed only: hover/settling vertical jitter shouldn't create dust.
        let horizontal_velocity = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
        let speed = horizontal_velocity.length();
        if speed < 2.0 {
            continue; // Too slow for dust
        }

        // Spawn rate increases with speed
        // At 10 m/s: ~15 particles/sec, at max speed: ~40 particles/sec
        let speed_factor = (speed / def.max_speed).clamp(0.0, 1.0);
        let spawn_rate = 15.0 + speed_factor * 25.0;
        let spawn_chance = spawn_rate * time.delta_secs();

        // Use a simple pseudo-random based on time
        let random_val = (time.elapsed_secs() * 1000.0).fract();
        if random_val > spawn_chance {
            continue;
        }

        // Spawn position: behind the vehicle, slightly to the sides
        let bike_rotation = transform.rotation;
        let back_offset = bike_rotation * Vec3::new(0.0, 0.0, def.size.z * 0.6); // Behind the vehicle

        // Add randomness to spawn position
        let random_x = ((time.elapsed_secs() * 3456.789).fract() - 0.5) * (def.size.x * 0.4);
        let random_z = ((time.elapsed_secs() * 7891.234).fract() - 0.5) * (def.size.z * 0.2);
        let side_offset = bike_rotation * Vec3::new(random_x, 0.0, random_z);

        let spawn_pos = transform.translation + back_offset + side_offset;

        // Particle velocity: upward and backward with some spread
        let up_speed = 2.0 + speed_factor * 3.0;
        let back_speed = speed * 0.3;
        let random_spread_x = ((time.elapsed_secs() * 5678.123).fract() - 0.5) * 2.0;
        let random_spread_z = ((time.elapsed_secs() * 9012.456).fract() - 0.5) * 2.0;

        let velocity = Vec3::new(random_spread_x, up_speed, random_spread_z)
            + bike_rotation * Vec3::new(0.0, 0.0, back_speed);

        // Particle size scales with speed
        let base_scale = 0.08 + speed_factor * 0.15;
        let scale_variation = ((time.elapsed_secs() * 2345.678).fract() - 0.5) * 0.04;
        let initial_scale = base_scale + scale_variation;

        // Lifetime: faster = longer visible trail
        let lifetime = 0.8 + speed_factor * 0.7;

        // Pick random material
        let mat_idx = ((time.elapsed_secs() * 8765.432).fract()
            * assets.sand_materials.len() as f32) as usize;
        let material = assets.sand_materials[mat_idx % assets.sand_materials.len()].clone();

        commands.spawn((
            SandParticle {
                lifetime,
                max_lifetime: lifetime,
                velocity,
                initial_scale,
            },
            Mesh3d(assets.sand_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(spawn_pos).with_scale(Vec3::splat(initial_scale)),
            NotShadowCaster,
        ));
    }
}

// =============================================================================
// UPDATE
// =============================================================================

/// Update sand particles: move, scale up, fade out, despawn
pub fn update_sand_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut SandParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let gravity = Vec3::new(0.0, -6.0, 0.0); // Lighter gravity for floaty dust

    for (entity, mut particle, mut transform) in particles.iter_mut() {
        // Update lifetime
        particle.lifetime -= dt;

        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Apply gravity and drag to velocity
        particle.velocity += gravity * dt;
        particle.velocity *= 0.97_f32.powf(dt * 60.0); // Air drag

        // Move particle
        transform.translation += particle.velocity * dt;

        // Scale up as it disperses (dust cloud effect)
        let life_progress = 1.0 - (particle.lifetime / particle.max_lifetime);
        let scale_multiplier = 1.0 + life_progress * 2.5; // Grows to 3.5x original size
        transform.scale = Vec3::splat(particle.initial_scale * scale_multiplier);

        // Fade by scaling down towards end of life
        let alpha = (particle.lifetime / particle.max_lifetime).powf(0.5) * 0.8;
        let fade_scale = alpha.max(0.1);
        transform.scale *= fade_scale;
    }
}
