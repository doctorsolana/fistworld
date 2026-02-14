//! World systems
//!
//! Spawning world visuals and other static environment.

use bevy::light::{light_consts::lux, CascadeShadowConfigBuilder, SunDisk};
use bevy::prelude::*;

use super::rendering::{FillLight, SunLight};

// =============================================================================
// COMPONENTS
// =============================================================================

/// Root entity for all client-side world visuals
#[derive(Component)]
pub struct ClientWorldRoot;

// =============================================================================
// SPAWNING
// =============================================================================

/// Spawn the visual world
pub fn spawn_world(mut commands: Commands, world_roots: Query<Entity, With<ClientWorldRoot>>) {
    if !world_roots.is_empty() {
        return;
    }

    let root = commands
        // IMPORTANT: this is the parent of terrain chunks / props / lights.
        // It must have GlobalTransform or Bevy will emit B0004 warnings for children.
        .spawn((
            ClientWorldRoot,
            Transform::default(),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ))
        .id();

    // --- Sun light (driven by day/night cycle) ---
    // Brighter, more intense desert sun
    let sun_light_entity = commands
        .spawn((
            SunLight,
            DirectionalLight {
                // Use unfiltered sunlight intensity; the atmosphere will handle scattering.
                illuminance: lux::RAW_SUNLIGHT,
                shadows_enabled: true,
                // Neutral sun color for a cleaner blue sky.
                color: Color::WHITE,
                ..default()
            },
            // Performance: keep shadows enabled, but make them cheaper.
            //
            // Tradeoff: fewer cascades = cheaper, larger max distance = more coverage.
            // This gives "cheap far shadows" while keeping a reasonable near range.
            CascadeShadowConfigBuilder {
                num_cascades: 3,
                maximum_distance: 220.0,
                first_cascade_far_bound: 12.0,
                ..default()
            }
            .build(),
            // Brighter sun disk for that harsh desert sun feel
            SunDisk {
                angular_size: 0.00930842, // Same as EARTH
                intensity: 1.8,           // 80% brighter - blazing desert sun!
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.3, 0.0)),
        ))
        .id();
    commands.entity(root).add_child(sun_light_entity);

    // --- Fill light (shadow lift / readability) ---
    let fill_light_entity = commands
        .spawn((
            FillLight,
            DirectionalLight {
                illuminance: 0.0, // Driven by day/night cycle
                shadows_enabled: false,
                color: Color::WHITE,
                ..default()
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
        ))
        .id();
    commands.entity(root).add_child(fill_light_entity);

    // Initial ambient (will be updated by day/night cycle)
    // Neutral daylight baseline.
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.82, 0.92),
        brightness: 80.0, // Brighter for desert environment
        affects_lightmapped_meshes: true,
    });

    // Initial sky color (will be updated by day/night cycle)
    // Atmosphere handles this, but set a fallback
    commands.insert_resource(ClearColor(Color::srgb(0.52, 0.68, 0.92)));

    info!("Spawned client world visuals");
}
