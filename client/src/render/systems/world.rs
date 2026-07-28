//! World systems
//!
//! Spawning world visuals and other static environment.

use bevy::light::{light_consts::lux, SunDisk};
use bevy::prelude::*;

use super::rendering::{FillLight, GraphicsSettings, SunLight};

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
pub fn spawn_world(
    mut commands: Commands,
    world_roots: Query<Entity, With<ClientWorldRoot>>,
    settings: Res<GraphicsSettings>,
) {
    if !world_roots.is_empty() {
        return;
    }

    let _root = commands
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
                // Overwritten every frame by the day/night cycle; keep the
                // initial value consistent with its noon output.
                illuminance: lux::DIRECT_SUNLIGHT,
                shadow_maps_enabled: settings.shadows_enabled,
                color: Color::srgb(1.0, 0.97, 0.9),
                ..default()
            },
            // Cascade count/range/resolution follow the shadow quality setting;
            // every cascade re-renders scene geometry each frame.
            settings.shadow_quality.build_cascades(),
            // Brighter sun disk for that harsh desert sun feel
            SunDisk {
                angular_size: 0.00930842, // Same as EARTH
                intensity: 1.8,           // 80% brighter - blazing desert sun!
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.3, 0.0)),
        ))
        .id();
    // Deliberately NOT a child of the world root: the sun's rotation changes
    // every frame, and a per-frame-dirty child keeps the root's whole subtree
    // (every prop and chunk) out of bevy's static-transform fast path.
    // Cleanup lives in cleanup_enter_main_menu alongside the root despawn.
    let _ = sun_light_entity;

    // --- Fill light (shadow lift / readability) ---
    let fill_light_entity = commands
        .spawn((
            FillLight,
            DirectionalLight {
                illuminance: 0.0, // Driven by day/night cycle
                shadow_maps_enabled: false,
                color: Color::srgb(0.62, 0.72, 0.92),
                ..default()
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
        ))
        .id();
    // Top-level for the same static-subtree reason as the sun.
    let _ = fill_light_entity;

    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.46, 0.55, 0.68),
        // Overwritten every frame by the day/night cycle; matches its noon output.
        brightness: 2600.0,
        affects_lightmapped_meshes: true,
    });

    // Initial sky color (will be updated by day/night cycle)
    // Atmosphere handles this, but set a fallback
    commands.insert_resource(ClearColor(Color::srgb(0.52, 0.68, 0.92)));

    info!("Spawned client world visuals");
}
