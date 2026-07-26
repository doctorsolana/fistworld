//! Particle systems
//!
//! Sand/dust trail particles for vehicles.

use bevy::prelude::*;

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
