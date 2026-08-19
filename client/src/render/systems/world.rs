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

    // IMPORTANT: this is the parent of terrain chunks / props (not the lights).
    // It must have GlobalTransform or Bevy will emit B0004 warnings for children.
    commands.spawn((
        ClientWorldRoot,
        Transform::default(),
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
    ));

    // --- Sun light ---
    // Spawn values are placeholders: update_day_night_cycle overwrites
    // rotation, color and illuminance every frame once WorldTime replicates.
    // Deliberately NOT a child of the world root: the sun's rotation changes
    // every frame, and a per-frame-dirty child keeps the root's whole subtree
    // (every prop and chunk) out of bevy's static-transform fast path.
    // Cleanup lives in cleanup_enter_main_menu alongside the root despawn.
    commands.spawn((
        SunLight,
        DirectionalLight {
            illuminance: lux::DIRECT_SUNLIGHT,
            shadow_maps_enabled: settings.shadows_enabled,
            color: Color::srgb(1.0, 0.97, 0.9),
            ..default()
        },
        // Cascade count/range/resolution follow the shadow quality setting;
        // every cascade re-renders scene geometry each frame.
        settings.shadow_quality.build_cascades(),
        SunDisk {
            angular_size: 0.00930842, // same as Earth's sun
            intensity: 1.8,
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.3, 0.0)),
    ));

    // --- Fill light (shadow lift by day, cool moon key at night) ---
    // Top-level for the same static-subtree reason as the sun; also fully
    // driven by update_day_night_cycle.
    commands.spawn((
        FillLight,
        DirectionalLight {
            illuminance: 0.0,
            shadow_maps_enabled: false,
            color: Color::srgb(0.60, 0.72, 0.95),
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
    ));

    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.46, 0.55, 0.68),
        // Placeholder; update_day_night_cycle owns this every frame.
        brightness: 2600.0,
        affects_lightmapped_meshes: true,
    });

    // Fallback sky color for the atmosphere-off graphics setting; with the
    // atmosphere on, the procedural sky covers every sky pixel. No system
    // updates this at runtime (the menu/disconnect paths set BLACK).
    commands.insert_resource(ClearColor(Color::srgb(0.52, 0.68, 0.92)));

    info!("Spawned client world visuals");
}
