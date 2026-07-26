//! First-person slash trail: a translucent crescent that flashes across the
//! view during a sword swing's strike frames — the "swoosh". Pure vertex-
//! colored geometry on an unlit material; one short-lived entity per swing.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use shared::components::{EquippedWeapon, LocalPlayer};

use crate::weapons::MeleeSwingState;

use super::offhand::SWING_WINDUP_END;

/// How long the flash lives.
const TRAIL_LIFETIME: f32 = 0.18;

#[derive(Resource)]
pub struct SlashTrailAssets {
    pub mesh: Handle<Mesh>,
}

#[derive(Component)]
pub struct SlashTrail {
    pub spawn_time: f32,
    pub material: Handle<StandardMaterial>,
}

/// Crescent ribbon in the camera XY plane: the sector a blade sweeps when it
/// pivots from up-right, over the top, to down-left. Vertex alpha ramps
/// toward the sweep's end (where the blade lands) so the leading edge is the
/// brightest part of the flash.
pub fn build_slash_trail_mesh(meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    const SEGMENTS: usize = 28;
    const START_ANGLE: f32 = 1.05; // up-right
    const END_ANGLE: f32 = 3.65; // down-left (sweeping over the top)
    const INNER: f32 = 0.16;
    const OUTER: f32 = 0.85;

    let mut positions = Vec::with_capacity((SEGMENTS + 1) * 2);
    let mut colors = Vec::with_capacity((SEGMENTS + 1) * 2);
    let mut uvs = Vec::with_capacity((SEGMENTS + 1) * 2);
    let mut indices: Vec<u32> = Vec::with_capacity(SEGMENTS * 6);

    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let angle = START_ANGLE + (END_ANGLE - START_ANGLE) * t;
        let (sin, cos) = angle.sin_cos();
        positions.push([cos * INNER, sin * INNER, 0.0]);
        positions.push([cos * OUTER, sin * OUTER, 0.0]);
        // Brightest at the sweep's end, fading tail behind; the inner edge
        // is dimmer than the blade-tip band.
        let along = t.powf(1.6);
        colors.push([1.0, 1.0, 1.0, along * 0.35]);
        colors.push([1.0, 1.0, 1.0, along]);
        uvs.push([t, 0.0]);
        uvs.push([t, 1.0]);
    }
    for i in 0..SEGMENTS as u32 {
        let a = i * 2;
        indices.extend_from_slice(&[a, a + 1, a + 2, a + 2, a + 1, a + 3]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(positions),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, VertexAttributeValues::Float32x2(uvs));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(colors),
    );
    let normals = vec![[0.0, 0.0, 1.0]; (SEGMENTS + 1) * 2];
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(normals),
    );
    mesh.insert_indices(Indices::U32(indices));
    meshes.add(mesh)
}

/// Spawn the flash the moment the local swing enters its strike frames.
pub fn spawn_slash_trails(
    mut commands: Commands,
    time: Res<Time>,
    melee_state: Res<MeleeSwingState>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    camera: Query<Entity, With<Camera3d>>,
    assets: Option<Res<SlashTrailAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spawned_for: Local<f32>,
) {
    let Some(assets) = assets else { return };
    let is_sword = local_player
        .single()
        .map(|weapon| weapon.weapon_type == shared::weapons::WeaponType::Sword)
        .unwrap_or(false);
    if !is_sword {
        return;
    }

    let now = time.elapsed_secs();
    let elapsed = now - melee_state.last_swing;
    let strike_start = melee_state.duration * SWING_WINDUP_END;
    if elapsed < strike_start
        || elapsed > melee_state.duration
        || *spawned_for == melee_state.last_swing
    {
        return;
    }
    *spawned_for = melee_state.last_swing;

    let Ok(camera_entity) = camera.single() else {
        return;
    };

    // Per-trail material so the fade animation can't fight other trails.
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.92, 0.97, 1.0, 0.85),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    // Diagonal slash plane; mirrored swings flip across X.
    let mirror = if melee_state.mirror { -1.0 } else { 1.0 };
    let trail = commands
        .spawn((
            SlashTrail {
                spawn_time: now,
                material: material.clone(),
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(0.06 * mirror, -0.18, -0.85)
                .with_rotation(Quat::from_rotation_z(-0.30 * mirror))
                .with_scale(Vec3::new(mirror, 1.0, 1.0)),
            NotShadowCaster,
            Visibility::Inherited,
            InheritedVisibility::default(),
            GlobalTransform::default(),
        ))
        .id();
    commands.entity(camera_entity).add_child(trail);
}

/// Fade + carry the flash forward, then despawn.
pub fn animate_slash_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut trails: Query<(Entity, &SlashTrail, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = time.elapsed_secs();
    for (entity, trail, mut transform) in trails.iter_mut() {
        let age = now - trail.spawn_time;
        if age >= TRAIL_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        let t = (age / TRAIL_LIFETIME).clamp(0.0, 1.0);
        // Keep rotating with the blade's momentum while dissolving.
        let spin = -0.55 * time.delta_secs() * transform.scale.x.signum();
        transform.rotate_local_z(spin);
        if let Some(material) = materials.get_mut(&trail.material) {
            material.base_color.set_alpha(0.85 * (1.0 - t) * (1.0 - t));
        }
    }
}
