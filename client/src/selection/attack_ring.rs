//! The attack marker: a crimson ring under whoever you are about to hurt.
//!
//! Two tones of the same mark. While combat mode is armed, the character under
//! the cursor wears a faint ring — "this click is a knife". Once an attack
//! order is sent, the target wears a strong pulsing ring until it dies or the
//! order is replaced. Pooled exactly like [`super::ring`], and a separate
//! entity from the victim for the same three reasons the selection ring is
//! (material rewrites, inherited yaw, smoothed Y).

use std::f32::consts::FRAC_PI_2;

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use shared::components::{CharacterActivity, CommandedBy, PlayerPosition};
use shared::terrain::WorldTerrain;

use super::ring::{
    ground_under_ring, ring_material, RING_HIDE_ZOOM, RING_LIFT, RING_SCALE_FROM, RING_SCALE_MAX,
};
use super::{can_command, Selectable, SelectableShape};
use crate::combat_mode::{CombatMode, CombatTargets};

/// Marker for a pooled attack ring root.
#[derive(Component)]
pub struct AttackRing;

/// Whether a pooled ring currently wears the ordered (strong) or hovered
/// (faint) core, so materials are only swapped on change.
#[derive(Component, PartialEq, Eq, Clone, Copy)]
pub struct AttackTone(pub bool);

#[derive(Resource)]
pub struct AttackRingAssets {
    shoulder_mesh: Handle<Mesh>,
    core_mesh: Handle<Mesh>,
    shoulder: Handle<StandardMaterial>,
    core_ordered: Handle<StandardMaterial>,
    core_hovered: Handle<StandardMaterial>,
}

/// Slightly wider than the selection ring so a selected attacker standing
/// beside its victim reads as two different marks, not a rendering glitch.
const SHOULDER_INNER: f32 = 0.66;
const SHOULDER_OUTER: f32 = 0.92;
const CORE_INNER: f32 = 0.72;
const CORE_OUTER: f32 = 0.86;

/// The ordered ring breathes so a standing order stays alive to the eye.
const PULSE_RATE: f32 = 4.0;
const PULSE_DEPTH: f32 = 0.05;

/// While combat mode is armed, mark the enemy under the cursor. Purely a
/// preview: the order itself is issued by the right-click system.
pub(super) fn hover_attack_target(
    mode: Res<CombatMode>,
    mut targets: ResMut<CombatTargets>,
    input_state: Res<crate::input::InputState>,
    ui_blockers: Query<&Interaction>,
    ray: Res<crate::camera_rts::CursorRay>,
    terrain_hit: Res<crate::camera_rts::CursorTerrainHit>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    candidates: Query<(
        Entity,
        &Selectable,
        &PlayerPosition,
        Option<&Transform>,
        Option<&CommandedBy>,
        Option<&CharacterActivity>,
    )>,
) {
    let hovered = find_enemy_under_cursor(
        &mode,
        &input_state,
        &ui_blockers,
        &ray,
        &terrain_hit,
        account.as_deref(),
        &candidates,
    );
    // Diff-gated: this runs every frame and the resource must not look
    // perpetually busy to change detection.
    if targets.hovered != hovered {
        targets.hovered = hovered;
    }
}

/// The shared "what enemy does this ray hit" question, used by the hover
/// preview above and by the right-click order itself so the mark you saw and
/// the person you hit can never disagree.
pub(crate) fn find_enemy_under_cursor(
    mode: &CombatMode,
    input_state: &crate::input::InputState,
    ui_blockers: &Query<&Interaction>,
    ray: &crate::camera_rts::CursorRay,
    terrain_hit: &crate::camera_rts::CursorTerrainHit,
    account: Option<&crate::ui::name_entry::PlayerNameInput>,
    candidates: &Query<(
        Entity,
        &Selectable,
        &PlayerPosition,
        Option<&Transform>,
        Option<&CommandedBy>,
        Option<&CharacterActivity>,
    )>,
) -> Option<Entity> {
    if !mode.0 {
        return None;
    }
    if input_state.ui_blocking() || crate::ui::pointer_over_ui(ui_blockers) {
        return None;
    }
    let ray = ray.0?;
    let origin = ray.origin;
    let direction = ray.direction.as_vec3();
    let my_account = account.map(|input| input.name.trim().to_lowercase());

    // Behind-a-hill rejection, same tolerance as single-click selection.
    let terrain_distance = terrain_hit
        .0
        .map(|point| (point - origin).dot(direction))
        .filter(|distance| *distance > 0.0);

    let mut best: Option<(Entity, f32)> = None;
    for (entity, selectable, position, visual, commanded, activity) in candidates.iter() {
        // People only: combat mode must not paint buildings or boats red.
        if selectable.shape != SelectableShape::Person {
            continue;
        }
        if activity.is_some_and(|activity| *activity == CharacterActivity::Indoors) {
            continue;
        }
        // Your own take orders, not damage.
        if can_command(commanded, my_account.as_deref()) {
            continue;
        }
        let base = super::pick::selectable_base(selectable, position, visual);
        let Some(distance) =
            super::pick::selectable_ray_distance(selectable, base, origin, direction)
        else {
            continue;
        };
        if terrain_distance.is_some_and(|ground| distance > ground + selectable.height.max(1.0)) {
            continue;
        }
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((entity, distance));
        }
    }
    best.map(|(entity, _)| entity)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_attack_rings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Option<Res<AttackRingAssets>>,
    time: Res<Time>,
    mode: Res<CombatMode>,
    mut targets: ResMut<CombatTargets>,
    terrain: Option<Res<WorldTerrain>>,
    camera: Query<&crate::camera_rts::CommanderCamera>,
    // Current-frame smoothed Transform, same reasoning as the selection ring.
    victims: Query<
        (&PlayerPosition, Option<&Transform>),
        (With<shared::components::CharacterKind>, Without<AttackRing>),
    >,
    mut rings: Query<(&mut Transform, &mut Visibility, &Children), With<AttackRing>>,
    mut cores: Query<(&mut MeshMaterial3d<StandardMaterial>, &mut AttackTone)>,
) {
    let Some(assets) = assets else {
        commands.insert_resource(AttackRingAssets {
            shoulder_mesh: meshes.add(Annulus::new(SHOULDER_INNER, SHOULDER_OUTER)),
            core_mesh: meshes.add(Annulus::new(CORE_INNER, CORE_OUTER)),
            // The dark shoulder leans red-black rather than the selection
            // ring's neutral soil, so even the outline says danger.
            shoulder: materials.add(ring_material(Color::srgba(0.16, 0.05, 0.04, 0.4))),
            core_ordered: materials.add(ring_material(Color::srgba(0.86, 0.22, 0.15, 0.62))),
            core_hovered: materials.add(ring_material(Color::srgba(0.86, 0.25, 0.18, 0.3))),
        });
        return;
    };

    // A pair goes stale when either side stops existing on this client: the
    // victim died (or left replication range), or the attacker did. The
    // marker otherwise outlives the click on purpose - it is the standing
    // "my people are on this" flag.
    let stale_check = |(attacker, target): &(Entity, Entity)| {
        victims.contains(*attacker) && victims.contains(*target)
    };
    if !targets.ordered.iter().all(stale_check) {
        targets.ordered.retain(stale_check);
    }
    if targets
        .hovered
        .is_some_and(|entity| !victims.contains(entity))
    {
        targets.hovered = None;
    }

    let zoom = camera
        .iter()
        .next()
        .map(|camera| camera.zoom)
        .unwrap_or(RING_SCALE_FROM);
    let scale_factor = (zoom / RING_SCALE_FROM).clamp(1.0, RING_SCALE_MAX);

    // One ring per distinct target, however many of our units attack it.
    // Strong marks first so pool slot N keeps meaning "the Nth ordered
    // target" while a hover comes and goes.
    let mut ordered_targets: Vec<Entity> = Vec::new();
    for (_, target) in targets.ordered.iter() {
        if !ordered_targets.contains(target) {
            ordered_targets.push(*target);
        }
    }
    let mut wanted: Vec<(Entity, bool)> = ordered_targets
        .iter()
        .map(|entity| (*entity, true))
        .collect();
    if mode.0 {
        if let Some(hovered) = targets.hovered {
            if !ordered_targets.contains(&hovered) {
                wanted.push((hovered, false));
            }
        }
    }
    let placed: Vec<(Vec3, bool)> = if zoom > RING_HIDE_ZOOM {
        Vec::new()
    } else {
        wanted
            .iter()
            .filter_map(|(entity, ordered)| {
                let (position, visual) = victims.get(*entity).ok()?;
                let mut point = visual
                    .map(|visual| visual.translation)
                    .unwrap_or(position.0);
                if let Some(terrain) = terrain.as_deref() {
                    point.y = ground_under_ring(terrain, point, SHOULDER_OUTER * scale_factor);
                }
                point.y += RING_LIFT;
                Some((point, *ordered))
            })
            .collect()
    };

    let pooled = rings.iter().count();
    for _ in pooled..placed.len() {
        commands.spawn((
            AttackRing,
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
                    MeshMaterial3d(assets.core_ordered.clone()),
                    AttackTone(true),
                    Transform::from_xyz(0.0, 0.0, 0.002),
                    NotShadowCaster,
                    NotShadowReceiver,
                ),
            ],
        ));
    }

    let pulse = 1.0 + PULSE_DEPTH * (time.elapsed_secs() * PULSE_RATE).sin();
    for (index, (mut transform, mut visibility, children)) in rings.iter_mut().enumerate() {
        match placed.get(index) {
            Some((point, ordered)) => {
                if transform.translation != *point {
                    transform.translation = *point;
                }
                // Ordered rings breathe; the hover preview holds still so the
                // moment of commitment is visible as motion starting.
                let scale = Vec3::splat(if *ordered {
                    scale_factor * pulse
                } else {
                    scale_factor
                });
                if transform.scale != scale {
                    transform.scale = scale;
                }
                for child in children.iter() {
                    if let Ok((mut material, mut tone)) = cores.get_mut(child) {
                        if tone.0 != *ordered {
                            tone.0 = *ordered;
                            material.0 = if *ordered {
                                assets.core_ordered.clone()
                            } else {
                                assets.core_hovered.clone()
                            };
                        }
                    }
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
