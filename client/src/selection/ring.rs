//! The selection ring: a soft mark on the ground under the selected unit.
//!
//! Rings are POOLED: the pool only grows, and spare rings hide rather than
//! despawn. Spawning per selection would churn mesh handles and, worse, flash
//! for a frame at the origin before the transform is set.
//!
//! It is a SEPARATE entity that follows the selection rather than a child of it,
//! for three reasons that all bite:
//!
//! - `hero::matte_character_materials` walks every descendant of a `Hero` and
//!   rewrites the material it finds. A ring parented to the hero would silently
//!   inherit a pass that does the opposite of what the ring wants.
//! - The hero root carries the character's yaw, which would spin the ring.
//! - The hero's rendered Y is smoothed toward the replicated position, so a
//!   fixed local lift would sink uphill and float downhill while walking.

use std::f32::consts::FRAC_PI_2;

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use shared::components::PlayerPosition;
use shared::terrain::WorldTerrain;

use super::Selection;

/// Marker for a ring root. There is one per pooled ring.
#[derive(Component)]
pub struct SelectionRing;

/// Shared handles, so every pooled ring reuses one mesh and material pair.
#[derive(Resource)]
pub struct RingAssets {
    shoulder_mesh: Handle<Mesh>,
    core_mesh: Handle<Mesh>,
    shoulder: Handle<StandardMaterial>,
    /// Warm core: this unit takes your orders.
    core_command: Handle<StandardMaterial>,
    /// Cool core: selected for inspection only.
    core_inspect: Handle<StandardMaterial>,
}

/// Which core a pooled ring is currently wearing, so the material is only
/// swapped when it actually changes.
#[derive(Component, PartialEq, Eq, Clone, Copy)]
pub struct RingTone(pub bool);

/// The ring is TWO concentric annuli: a dark shoulder under a light core.
///
/// A single-tone ring cannot work. A light ring disappears on snow and sand; a
/// dark one disappears on wet rock and in shadow. Pairing them means whichever
/// band loses contrast against the ground, the other still draws the shape --
/// the same reason a map pin has an outline. The shoulder is slightly wider so
/// it reads as an outline around the core rather than as a second ring.
const SHOULDER_INNER: f32 = 0.58;
const SHOULDER_OUTER: f32 = 0.82;
const CORE_INNER: f32 = 0.64;
const CORE_OUTER: f32 = 0.76;

/// Lift above the HIGHEST ground under the ring, in metres.
///
/// A tiny lift over the CENTRE height is not enough, and the failure mode is
/// distinctive: the terrain has fine crumpled micro-relief, transparent
/// materials do not write depth but still TEST it, so the arcs of the ring that
/// fall below a local bump silently fail the depth test and the ring renders as
/// a partial arc. It looks like a broken mesh, not like z-fighting.
///
/// So the height is sampled around the ring's circumference and the ring is
/// placed above the maximum -- see [`ground_under_ring`]. 12cm on top of that is
/// enough to clear the remaining bilinear-versus-triangulated mismatch between
/// `get_height` and the rendered chunk, while still reading as lying on the soil.
const RING_LIFT: f32 = 0.12;

/// Samples taken around the circumference to find the ground under the ring.
///
/// Eight is enough to catch the bumps that matter at this radius and costs eight
/// heightfield lookups for one selected unit.
const GROUND_SAMPLES: usize = 8;

/// Highest ground under the ring's footprint, so no arc of it sinks.
fn ground_under_ring(terrain: &WorldTerrain, centre: Vec3, radius: f32) -> f32 {
    let mut highest = terrain.get_height(centre.x, centre.z);
    for i in 0..GROUND_SAMPLES {
        let angle = std::f32::consts::TAU * i as f32 / GROUND_SAMPLES as f32;
        let x = centre.x + angle.cos() * radius;
        let z = centre.z + angle.sin() * radius;
        highest = highest.max(terrain.get_height(x, z));
    }
    highest
}

/// Grow the ring modestly as the camera pulls back, then STOP DRAWING IT.
///
/// The obvious fix for "a 0.8m ring is sub-pixel at distance" is to hold a
/// constant size on screen, and that is wrong: at max zoom a screen-constant
/// ring is over a kilometre of ground, clipping through mountains. MSAA is also
/// off, so a sub-2px stroke shimmers rather than reading as a line. And past
/// ~512m the visible ground is the far mesh, which sits BELOW the detail
/// surface, so a ground-conformed ring is not even truthful out there.
///
/// So the ring is a close-range affordance, and past `RING_HIDE_ZOOM` the HUD's
/// selection plate is what tells you what is selected. That is the honest answer
/// to "the ring is now smaller than a pixel".
const RING_SCALE_FROM: f32 = 120.0;
const RING_SCALE_MAX: f32 = 4.0;
const RING_HIDE_ZOOM: f32 = 520.0;

/// Shared material setup for both bands.
fn ring_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        // Reads the same at dawn, at noon and under a storm. A lit ring goes
        // invisible exactly when the world is dark.
        unlit: true,
        // MANDATORY, and NOT implied by `unlit`: fog is applied outside the
        // lighting branch, and this repo puts DistanceFog on the camera. Without
        // this the ring desaturates toward sky-blue at exactly the zooms where it
        // has to read.
        fog_enabled: false,
        alpha_mode: AlphaMode::Blend,
        // Visible when the camera dips below the ring on a hillside.
        double_sided: true,
        cull_mode: None,
        // Blend materials land in the Transparent3d phase, where depth write is
        // off and the compare is GreaterEqual, so coplanar fragments already pass
        // and the lift above does most of the work. This is a nudge on top.
        // NOTE: truncated to i32 downstream, so fractional values do nothing.
        depth_bias: 4.0,
        ..default()
    }
}

pub(super) fn sync_selection_ring(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Option<Res<RingAssets>>,
    selection: Res<Selection>,
    terrain: Option<Res<WorldTerrain>>,
    camera: Query<&crate::camera_rts::CommanderCamera>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    positions: Query<(
        &PlayerPosition,
        Option<&GlobalTransform>,
        Option<&shared::components::CommandedBy>,
    )>,
    mut rings: Query<(Entity, &mut Transform, &mut Visibility, &Children), With<SelectionRing>>,
    mut cores: Query<(&mut MeshMaterial3d<StandardMaterial>, &mut RingTone)>,
) {
    let Some(assets) = assets else {
        // Quiet on purpose. The ring MARKS the selection; the HUD plate is what
        // states it loudly. Raising these alphas is the fastest way to make the
        // world look like a debug view.
        commands.insert_resource(RingAssets {
            shoulder_mesh: meshes.add(Annulus::new(SHOULDER_INNER, SHOULDER_OUTER)),
            core_mesh: meshes.add(Annulus::new(CORE_INNER, CORE_OUTER)),
            shoulder: materials.add(ring_material(Color::srgba(0.106, 0.094, 0.082, 0.34))),
            // Commandable rings carry a warm cast -- the same ember family the
            // HUD reserves for selection -- so "I can move this" is legible
            // without reading the plate. Inspect-only rings stay neutral.
            core_command: materials.add(ring_material(Color::srgba(0.984, 0.898, 0.792, 0.46))),
            core_inspect: materials.add(ring_material(Color::srgba(0.945, 0.949, 0.953, 0.30))),
        });
        return;
    };

    let zoom = camera
        .iter()
        .next()
        .map(|c| c.zoom)
        .unwrap_or(RING_SCALE_FROM);
    let scale_factor = (zoom / RING_SCALE_FROM).clamp(1.0, RING_SCALE_MAX);

    let my_account = account.as_ref().map(|input| input.name.trim().to_lowercase());

    // Where every ring belongs this frame, and whether that unit takes orders.
    // Empty past the hide distance, so the whole pool simply hides.
    let wanted: Vec<(Vec3, bool)> = if zoom > RING_HIDE_ZOOM {
        Vec::new()
    } else {
        selection
            .entities
            .iter()
            .filter_map(|entity| positions.get(*entity).ok())
            .map(|(position, visual, commanded)| {
                let commandable = super::can_command(commanded, my_account.as_deref());
                // Follow the SMOOTHED transform, not the replicated position.
                // `PlayerPosition` is a staircase at network rate while the body
                // is interpolated every frame, so a ring drawn from it walks in
                // steps behind a character that glides -- which is the lag you
                // see. The body and its ring must be driven by the same number.
                let mut point = visual
                    .map(|visual| visual.translation())
                    .unwrap_or(position.0);
                // Sit on the GROUND, not on the entity's replicated Y: feet are
                // terrain-snapped server-side but the client can be a frame
                // behind, and a ring that lags into a hillside is worse than one
                // that is always on it.
                if let Some(terrain) = terrain.as_deref() {
                    point.y = ground_under_ring(terrain, point, SHOULDER_OUTER * scale_factor);
                }
                point.y += RING_LIFT;
                (point, commandable)
            })
            .collect()
    };

    // Grow the pool to fit. It never shrinks: a player who box-selects twenty
    // units once will do it again, and despawn-then-respawn costs more than
    // twenty hidden transforms.
    let pooled = rings.iter().count();
    for _ in pooled..wanted.len() {
        commands.spawn((
            SelectionRing,
            // Annulus meshes are built in the XY plane facing +Z, so the root is
            // rotated -90 degrees about X to lay them flat.
            Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            Visibility::Hidden,
            children![
                (
                    Mesh3d(assets.shoulder_mesh.clone()),
                    MeshMaterial3d(assets.shoulder.clone()),
                    NotShadowCaster,
                    NotShadowReceiver,
                ),
                (
                    Mesh3d(assets.core_mesh.clone()),
                    MeshMaterial3d(assets.core_command.clone()),
                    RingTone(true),
                    // Lifted a hair along the ring's own local +Z (which points
                    // up after the root's rotation) so the core always resolves
                    // in front of its own shoulder.
                    Transform::from_xyz(0.0, 0.0, 0.002),
                    NotShadowCaster,
                    NotShadowReceiver,
                ),
            ],
        ));
    }

    let scale = Vec3::splat(scale_factor);
    for (index, (_entity, mut transform, mut visibility, children)) in
        rings.iter_mut().enumerate()
    {
        match wanted.get(index) {
            Some((point, commandable)) => {
                if transform.translation != *point {
                    transform.translation = *point;
                }
                // Swap the core material only when the tone actually changes:
                // writing a material handle every frame re-uploads it.
                for child in children.iter() {
                    if let Ok((mut material, mut tone)) = cores.get_mut(child) {
                        if tone.0 != *commandable {
                            tone.0 = *commandable;
                            material.0 = if *commandable {
                                assets.core_command.clone()
                            } else {
                                assets.core_inspect.clone()
                            };
                        }
                    }
                }
                if transform.scale != scale {
                    transform.scale = scale;
                }
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
