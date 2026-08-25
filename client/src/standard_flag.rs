//! The battalion standard: a pole and crimson pennant over the flag bearer.
//!
//! This is how a formation reads as a UNIT from RTS height rather than a
//! crowd - and it doubles as the selection handle, because clicking the
//! bearer selects the whole battalion. Flags are POOLED, separate entities
//! that follow the bearer's smoothed body (never children of it - the same
//! three reasons as the selection ring: character material passes rewrite
//! descendants, the body root carries yaw, and its Y is network-smoothed).
//!
//! The pennant is unlit like the rings: a battle marker must read identically
//! at dawn, at noon and under a storm.

use std::f32::consts::FRAC_PI_2;

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use shared::components::{PlayerPosition, StandardBearer};

use crate::combat_mode::CRIMSON;
use crate::states::GameState;

/// Pole top sits well above helmet height so the pennant is never lost in
/// the crowd it marks.
const POLE_HEIGHT: f32 = 3.6;
const POLE_RADIUS: f32 = 0.055;
/// Pennant dimensions: a long tapered triangle, the classic lance pennon.
/// Sized to read at RTS command height, not at portrait distance.
const PENNANT_LENGTH: f32 = 1.55;
const PENNANT_DROP: f32 = 0.62;
/// Gentle idle sway, radians. Enough to look like cloth, not a metronome.
const SWAY_RADIANS: f32 = 0.08;
const SWAY_RATE: f32 = 1.7;

pub struct StandardFlagPlugin;

impl Plugin for StandardFlagPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sync_standard_flags
                .after(crate::hero::sync_hero_transforms)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), despawn_standard_flags);
    }
}

#[derive(Component)]
struct StandardFlag;

#[derive(Resource)]
struct StandardFlagAssets {
    pole_mesh: Handle<Mesh>,
    pennant_mesh: Handle<Mesh>,
    pole: Handle<StandardMaterial>,
    pennant: Handle<StandardMaterial>,
}

#[allow(clippy::too_many_arguments)]
fn sync_standard_flags(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Option<Res<StandardFlagAssets>>,
    time: Res<Time>,
    cameras: Query<&Transform, (With<Camera3d>, Without<StandardFlag>)>,
    // Current-frame smoothed Transform; GlobalTransform is a frame stale and
    // a flag trailing its bearer reads as the pole bending.
    bearers: Query<
        (&PlayerPosition, Option<&Transform>),
        (With<StandardBearer>, Without<StandardFlag>),
    >,
    mut flags: Query<(&mut Transform, &mut Visibility), With<StandardFlag>>,
) {
    let Some(assets) = assets else {
        commands.insert_resource(StandardFlagAssets {
            pole_mesh: meshes.add(Cylinder::new(POLE_RADIUS, POLE_HEIGHT)),
            pennant_mesh: meshes.add(Triangle2d::new(
                Vec2::new(0.0, 0.0),
                Vec2::new(0.0, -PENNANT_DROP),
                Vec2::new(PENNANT_LENGTH, -PENNANT_DROP * 0.45),
            )),
            pole: materials.add(StandardMaterial {
                base_color: Color::srgb(0.23, 0.16, 0.10),
                perceptual_roughness: 0.9,
                ..default()
            }),
            // Unlit and fog-exempt for the same reason as the rings: the
            // marker has to read under every sky the game can produce.
            pennant: materials.add(StandardMaterial {
                base_color: CRIMSON,
                unlit: true,
                fog_enabled: false,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
        });
        return;
    };

    // Where every flag stands this frame: the bearer's smoothed body.
    let wanted: Vec<Vec3> = bearers
        .iter()
        .map(|(position, visual)| {
            visual
                .map(|visual| visual.translation)
                .unwrap_or(position.0)
        })
        .collect();

    let pooled = flags.iter().count();
    for _ in pooled..wanted.len() {
        commands
            .spawn((StandardFlag, Transform::default(), Visibility::Hidden))
            .with_children(|flag| {
                flag.spawn((
                    Mesh3d(assets.pole_mesh.clone()),
                    MeshMaterial3d(assets.pole.clone()),
                    Transform::from_xyz(0.0, POLE_HEIGHT * 0.5, 0.0),
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
                flag.spawn((
                    Mesh3d(assets.pennant_mesh.clone()),
                    MeshMaterial3d(assets.pennant.clone()),
                    Transform::from_xyz(POLE_RADIUS, POLE_HEIGHT - 0.06, 0.0),
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            });
    }

    // Billboard around Y toward the camera so the pennant's face - its whole
    // silhouette - is always presented, with a slow cloth-like sway.
    let camera_yaw = cameras
        .iter()
        .next()
        .map(|camera| {
            let forward = camera.forward();
            (-forward.x).atan2(-forward.z)
        })
        .unwrap_or(0.0);
    let sway = (time.elapsed_secs() * SWAY_RATE).sin() * SWAY_RADIANS;
    let facing = Quat::from_rotation_y(camera_yaw + FRAC_PI_2 + sway);

    for (index, (mut transform, mut visibility)) in flags.iter_mut().enumerate() {
        match wanted.get(index) {
            Some(point) => {
                if transform.translation != *point {
                    transform.translation = *point;
                }
                transform.rotation = facing;
                if *visibility != Visibility::Visible {
                    *visibility = Visibility::Visible;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}

fn despawn_standard_flags(mut commands: Commands, flags: Query<Entity, With<StandardFlag>>) {
    for flag in flags.iter() {
        commands.entity(flag).despawn();
    }
}
