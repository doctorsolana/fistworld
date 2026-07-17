//! Authoritative NPC ragdoll runtime (Oilman v1).

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::*;
use shared::components::{
    Health, Npc, NpcActivity, NpcActivityKind, NpcArchetype, NpcPosition, NpcRotation, NpcVelocity,
};
use shared::npc::{
    humanoid_body_bounding_radius, humanoid_body_shape, ragdoll_body_axis, HumanoidBodyShape,
    RagdollBodyDef, DEAD_NPC_DESPAWN_TIME, HUMANOID_RAGDOLL_BODIES,
};
use shared::protocol::{
    pack_quat_i16, NpcRagdollPoseBatch, NpcRagdollPoseSample, NpcRagdollStarted, RagdollBodyId,
    RagdollBodyPose, RagdollPoseChannel, ReliableChannel,
};
use std::collections::{HashMap, HashSet};

use crate::physics::layers;

const DEFAULT_CORPSE_CAP: usize = 64;
// Moving ragdolls stream at 30 Hz so interpolation preserves the gravity
// curve. Sleeping corpses fall back to a low-rate refresh, and both paths are
// filtered by per-client network visibility.
const DEFAULT_RAGDOLL_POSE_HZ: f32 = 30.0;
const CORPSE_CELL_SIZE: f32 = 8.0;
const PELVIS_LOCAL_Y_FROM_NPC_CENTER: f32 = 0.0;
// Yaw offset between the NPC's replicated yaw and the ragdoll spawn frame.
// MUST be 0: the old PI value mirrored the whole body layout left<->right
// (a 180° yaw turn puts "UpperArmL" on the character's anatomical RIGHT),
// so the client's LeftArm bone was driven by the right arm's physics body —
// arms swept up/backwards and every corpse curled. The mesh's own model-root
// PI flip is a render-side concern that the client handles via bind_rel.
const RAGDOLL_VISUAL_YAW_OFFSET: f32 = 0.0;
const SOFT_RAGDOLL_MODE: bool = true;
// Damping tuned for a real-looking fall: gravity is 18 m/s², so linear damping
// must stay well below ~1.0 or the corpse reaches a floaty terminal velocity
// (the old 8.5 capped falls at ~2 m/s). Angular damping sets "floppiness" —
// Source-style ragdolls sit around 0.5-1.0; higher reads as rigor mortis.
const RAGDOLL_LINEAR_DAMPING: f32 = 0.15;
const RAGDOLL_ANGULAR_DAMPING: f32 = 1.1;
// Velocity clamps are explosion/NaN safeties: above natural fall/tumble
// speeds (a 1m fall peaks ~6 m/s), below "corpse flies across the street".
const RAGDOLL_MAX_LINEAR_SPEED: f32 = 9.0;
const RAGDOLL_MAX_ANGULAR_SPEED: f32 = 18.0;
// Death impulse split, Source-style: most of it shoves the WHOLE body
// (mass-proportional, so it's a clean velocity change with no limb whip) and
// a small remainder kicks the hit body at the impact point for localized
// spin/flinch. The local kick is capped as a velocity change so light limbs
// (2.5 kg forearms) can't be launched.
const IMPULSE_UNIFORM_SHARE: f32 = 0.65;
const IMPULSE_LOCAL_SHARE: f32 = 0.35;
const IMPULSE_LOCAL_MAX_DV: f32 = 3.5;

#[derive(Component, Clone, Copy, Debug)]
pub struct RagdollPhysicsBody;

#[derive(Component, Clone, Copy, Debug)]
pub struct NpcDeathImpact {
    pub hit_point: Vec3,
    pub impulse: Vec3,
    pub body: Option<RagdollBodyId>,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct NpcRagdoll;

#[derive(Clone, Copy, Debug)]
pub struct RagdollBodyBinding {
    pub id: RagdollBodyId,
    pub entity: Entity,
    pub radius: f32,
}

#[derive(Component, Clone, Debug, Default)]
pub struct NpcRagdollBodies {
    pub bindings: Vec<RagdollBodyBinding>,
    pub next_seq: u32,
    pub last_pose_sent_at: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct CorpseLifecycle {
    pub started_at: f32,
    pub despawn_at: f32,
    pub age_order: u64,
}

#[derive(Resource, Clone, Debug)]
pub struct CorpseBudget {
    pub max_corpses: usize,
    next_age_order: u64,
}

impl Default for CorpseBudget {
    fn default() -> Self {
        let max_corpses = std::env::var("CITYSIM_CORPSE_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_CORPSE_CAP)
            .clamp(8, 2048);

        Self {
            max_corpses,
            next_age_order: 0,
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct RagdollPoseStream {
    accumulator: f32,
    interval_secs: f32,
}

impl Default for RagdollPoseStream {
    fn default() -> Self {
        let hz = std::env::var("CITYSIM_RAGDOLL_POSE_HZ")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(DEFAULT_RAGDOLL_POSE_HZ)
            .clamp(1.0, 60.0);
        Self {
            accumulator: 0.0,
            interval_secs: 1.0 / hz,
        }
    }
}

#[derive(Resource, Clone, Debug, Default)]
pub struct RagdollTelemetry {
    pub active_corpses: usize,
    pub total_evicted: u64,
    pub total_pose_msgs: u64,
    pub total_pose_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct CorpseBodyPoint {
    pub npc_entity: Entity,
    pub body_entity: Entity,
    pub body: RagdollBodyId,
    pub position: Vec3,
    pub radius: f32,
}

#[derive(Resource, Clone, Debug)]
pub struct CorpseCollisionIndex {
    cell_size: f32,
    cells: HashMap<(i32, i32), Vec<CorpseBodyPoint>>,
    by_npc: HashMap<Entity, Vec<CorpseBodyPoint>>,
    pub version: u64,
}

impl Default for CorpseCollisionIndex {
    fn default() -> Self {
        Self {
            cell_size: CORPSE_CELL_SIZE,
            cells: HashMap::new(),
            by_npc: HashMap::new(),
            version: 0,
        }
    }
}

impl CorpseCollisionIndex {
    #[inline]
    fn cell_key(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32,
        )
    }

    pub fn collect_nearby(&self, pos: Vec3, radius: f32, out: &mut Vec<CorpseBodyPoint>) {
        out.clear();
        let (cx, cz) = self.cell_key(pos);
        let cells = (radius / self.cell_size).ceil() as i32 + 1;
        for dx in -cells..=cells {
            for dz in -cells..=cells {
                if let Some(points) = self.cells.get(&(cx + dx, cz + dz)) {
                    out.extend(points.iter().copied());
                }
            }
        }
    }

    pub fn collect_segment_candidates(
        &self,
        start: Vec3,
        end: Vec3,
        padding: f32,
        out: &mut Vec<CorpseBodyPoint>,
    ) {
        out.clear();
        let min_x = start.x.min(end.x) - padding;
        let max_x = start.x.max(end.x) + padding;
        let min_z = start.z.min(end.z) - padding;
        let max_z = start.z.max(end.z) + padding;
        let min_cell_x = (min_x / self.cell_size).floor() as i32;
        let max_cell_x = (max_x / self.cell_size).floor() as i32;
        let min_cell_z = (min_z / self.cell_size).floor() as i32;
        let max_cell_z = (max_z / self.cell_size).floor() as i32;
        let mut seen = HashSet::new();
        for cx in min_cell_x..=max_cell_x {
            for cz in min_cell_z..=max_cell_z {
                if let Some(points) = self.cells.get(&(cx, cz)) {
                    for point in points {
                        if seen.insert(point.body_entity) {
                            out.push(*point);
                        }
                    }
                }
            }
        }
    }
}

fn apply_joint_limits(
    joint: SphericalJointBuilder,
    body_id: RagdollBodyId,
) -> SphericalJointBuilder {
    match body_id {
        RagdollBodyId::SpineLower | RagdollBodyId::SpineUpper => joint
            .limits(JointAxis::AngX, [-0.45, 0.45])
            .limits(JointAxis::AngY, [-0.55, 0.55])
            .limits(JointAxis::AngZ, [-0.35, 0.35]),
        RagdollBodyId::Head => joint
            .limits(JointAxis::AngX, [-0.75, 0.75])
            .limits(JointAxis::AngY, [-1.15, 1.15])
            .limits(JointAxis::AngZ, [-0.65, 0.65]),
        RagdollBodyId::UpperArmL | RagdollBodyId::UpperArmR => joint
            .limits(JointAxis::AngX, [-1.45, 1.20])
            .limits(JointAxis::AngY, [-0.70, 0.70])
            .limits(JointAxis::AngZ, [-0.85, 0.85]),
        RagdollBodyId::ForearmL | RagdollBodyId::ForearmR => joint
            .limits(JointAxis::AngX, [0.0, 1.65])
            .limits(JointAxis::AngY, [-0.20, 0.20])
            .limits(JointAxis::AngZ, [-0.20, 0.20]),
        RagdollBodyId::HandL | RagdollBodyId::HandR => joint
            .limits(JointAxis::AngX, [-0.40, 0.40])
            .limits(JointAxis::AngY, [-0.25, 0.25])
            .limits(JointAxis::AngZ, [-0.25, 0.25]),
        RagdollBodyId::ThighL | RagdollBodyId::ThighR => joint
            .limits(JointAxis::AngX, [-1.10, 0.65])
            .limits(JointAxis::AngY, [-0.35, 0.35])
            .limits(JointAxis::AngZ, [-0.28, 0.28]),
        RagdollBodyId::CalfL | RagdollBodyId::CalfR => joint
            .limits(JointAxis::AngX, [0.0, 1.70])
            .limits(JointAxis::AngY, [-0.15, 0.15])
            .limits(JointAxis::AngZ, [-0.15, 0.15]),
        RagdollBodyId::FootL | RagdollBodyId::FootR => joint
            .limits(JointAxis::AngX, [-0.35, 0.35])
            .limits(JointAxis::AngY, [-0.15, 0.15])
            .limits(JointAxis::AngZ, [-0.15, 0.15]),
        RagdollBodyId::Pelvis => joint,
    }
}

/// Soft-mode joint limits. Wide enough that gravity can pull the corpse out of
/// its spawn T-pose (arms need ~90° of swing to fall to the sides; the spine
/// must fold a little for a believable crumple) while still preventing
/// anatomically impossible poses.
fn apply_soft_joint_limits(
    mut joint: GenericJointBuilder,
    body_id: RagdollBodyId,
) -> GenericJointBuilder {
    match body_id {
        RagdollBodyId::SpineLower => {
            joint = joint
                .limits(JointAxis::AngX, [-0.50, 0.50])
                .limits(JointAxis::AngY, [-0.40, 0.40])
                .limits(JointAxis::AngZ, [-0.40, 0.40]);
        }
        RagdollBodyId::SpineUpper => {
            joint = joint
                .limits(JointAxis::AngX, [-0.42, 0.42])
                .limits(JointAxis::AngY, [-0.35, 0.35])
                .limits(JointAxis::AngZ, [-0.34, 0.34]);
        }
        RagdollBodyId::Head => {
            joint = joint
                .limits(JointAxis::AngX, [-0.75, 0.75])
                .limits(JointAxis::AngY, [-1.05, 1.05])
                .limits(JointAxis::AngZ, [-0.65, 0.65]);
        }
        RagdollBodyId::UpperArmL | RagdollBodyId::UpperArmR => {
            joint = joint
                .limits(JointAxis::AngX, [-1.60, 1.30])
                .limits(JointAxis::AngY, [-0.90, 0.90])
                .limits(JointAxis::AngZ, [-1.20, 1.20]);
        }
        RagdollBodyId::ForearmL | RagdollBodyId::ForearmR => {
            joint = joint
                .limits(JointAxis::AngX, [0.0, 1.80])
                .limits(JointAxis::AngY, [-0.20, 0.20])
                .limits(JointAxis::AngZ, [-0.20, 0.20]);
        }
        RagdollBodyId::ThighL | RagdollBodyId::ThighR => {
            joint = joint
                .limits(JointAxis::AngX, [-1.50, 0.70])
                .limits(JointAxis::AngY, [-0.50, 0.50])
                .limits(JointAxis::AngZ, [-0.45, 0.45]);
        }
        RagdollBodyId::CalfL | RagdollBodyId::CalfR => {
            joint = joint
                .limits(JointAxis::AngX, [0.0, 2.30])
                .limits(JointAxis::AngY, [-0.15, 0.15])
                .limits(JointAxis::AngZ, [-0.15, 0.15]);
        }
        _ => {}
    }
    joint
}

fn collider_for_body(def: &RagdollBodyDef) -> Collider {
    match humanoid_body_shape(def.id) {
        HumanoidBodyShape::Sphere { radius } => Collider::ball(radius),
        HumanoidBodyShape::Capsule {
            half_segment,
            radius,
        } => Collider::capsule_y(half_segment, radius),
        HumanoidBodyShape::Cuboid { half_extents } => {
            Collider::cuboid(half_extents.x, half_extents.y, half_extents.z)
        }
    }
}

fn pose_from_transform(body: RagdollBodyId, transform: &Transform) -> RagdollBodyPose {
    RagdollBodyPose {
        body,
        position: transform.translation,
        rotation: pack_quat_i16(transform.rotation),
    }
}

pub(crate) fn build_started_message(
    npc: &Npc,
    lifecycle: &CorpseLifecycle,
    bindings: &NpcRagdollBodies,
    body_transforms: &Query<&Transform>,
) -> Option<NpcRagdollStarted> {
    let mut bodies = Vec::with_capacity(bindings.bindings.len());
    let mut root_position = None;
    let mut root_rotation = None;

    for binding in &bindings.bindings {
        let tf = body_transforms.get(binding.entity).ok()?;
        let pose = pose_from_transform(binding.id, tf);
        if binding.id == RagdollBodyId::Pelvis {
            root_position = Some(pose.position);
            root_rotation = Some(pose.rotation);
        }
        bodies.push(pose);
    }

    Some(NpcRagdollStarted {
        npc_id: npc.id,
        archetype: npc.archetype,
        started_at: lifecycle.started_at,
        root_position: root_position?,
        root_rotation: root_rotation?,
        bodies,
    })
}

pub fn cleanup_ragdoll_bodies(commands: &mut Commands, bodies: &NpcRagdollBodies) {
    for binding in &bodies.bindings {
        commands.entity(binding.entity).despawn();
    }
}

pub fn despawn_npc_with_bodies(
    commands: &mut Commands,
    npc_entity: Entity,
    bodies: Option<&NpcRagdollBodies>,
) {
    if let Some(bodies) = bodies {
        cleanup_ragdoll_bodies(commands, bodies);
    }
    commands.entity(npc_entity).despawn();
}

/// `CITYSIM_RAGDOLL_TEST_SECS=<n>`: kill every alive NPC `n` seconds after
/// server start with a synthetic side-on shot impulse. Lets ragdolls be
/// exercised end-to-end without aiming/shooting (unattended verification).
pub fn debug_auto_kill_npcs(
    time: Res<Time>,
    mut commands: Commands,
    mut npcs: Query<(Entity, &NpcPosition, &mut Health), (With<Npc>, Without<NpcRagdoll>)>,
    mut config: Local<Option<Option<f32>>>,
    mut fired: Local<bool>,
) {
    let secs = *config.get_or_insert_with(|| {
        std::env::var("CITYSIM_RAGDOLL_TEST_SECS")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
    });
    let Some(secs) = secs else { return };
    if *fired || time.elapsed_secs() < secs {
        return;
    }
    *fired = true;

    let mut killed = 0usize;
    for (entity, pos, mut health) in npcs.iter_mut() {
        health.take_damage(100_000.0);
        // Synthetic rifle-ish kill shot from the side, matching the impulse
        // range produced by hit_characters on a lethal hit (mass-proportional
        // application: 120 N·s ≈ 1.25 m/s whole-body shove).
        let dir = Vec3::new(1.0, 0.35, 0.25).normalize();
        commands.entity(entity).insert(NpcDeathImpact {
            hit_point: pos.0 + Vec3::new(0.0, 0.35, 0.0),
            impulse: dir * 120.0,
            body: None,
        });
        killed += 1;
    }
    if killed > 0 {
        info!("CITYSIM_RAGDOLL_TEST: auto-killed {killed} NPC(s) for ragdoll testing");
    }
}

pub fn activate_npc_ragdolls(
    mut commands: Commands,
    time: Res<Time>,
    mut budget: ResMut<CorpseBudget>,
    mut npcs: Query<
        (
            Entity,
            &Npc,
            &Health,
            &NpcPosition,
            &NpcRotation,
            &NpcVelocity,
            &mut NpcActivity,
            Option<&NpcDeathImpact>,
            Option<&NpcRagdoll>,
            &ReplicationState,
        ),
        With<Npc>,
    >,
    mut clients: Query<
        (Entity, &mut MessageSender<NpcRagdollStarted>),
        (With<ClientOf>, With<Connected>),
    >,
) {
    let now = time.elapsed_secs();
    for (
        npc_entity,
        npc,
        health,
        npc_pos,
        npc_rot,
        npc_vel,
        mut activity,
        death_impact,
        ragdoll,
        replication,
    ) in npcs.iter_mut()
    {
        if !health.is_dead() || ragdoll.is_some() {
            continue;
        }
        if !matches!(
            npc.archetype,
            NpcArchetype::Oilman | NpcArchetype::Dummy | NpcArchetype::CombatDummy
        ) {
            commands.entity(npc_entity).remove::<NpcDeathImpact>();
            continue;
        }

        // Client visual rigs are instantiated with a +PI yaw at model root.
        // Match that frame here so authoritative ragdoll bodies line up with
        // mapped skeleton bones (left/right limbs and knees/elbows).
        let root_rotation = Quat::from_rotation_y(npc_rot.0 + RAGDOLL_VISUAL_YAW_OFFSET);
        let mut spawned = Vec::with_capacity(HUMANOID_RAGDOLL_BODIES.len());
        let mut by_id: HashMap<RagdollBodyId, (Entity, Vec3, Quat)> =
            HashMap::with_capacity(HUMANOID_RAGDOLL_BODIES.len());

        for def in HUMANOID_RAGDOLL_BODIES {
            let world_pos = npc_pos.0 + root_rotation * def.local_offset;
            let local_axis = ragdoll_body_axis(&def);
            let local_rot = Quat::from_rotation_arc(Vec3::Y, local_axis);
            let spawn_tf =
                Transform::from_translation(world_pos).with_rotation(root_rotation * local_rot);
            let entity = commands
                .spawn((
                    RagdollPhysicsBody,
                    RigidBody::Dynamic,
                    collider_for_body(&def),
                    if SOFT_RAGDOLL_MODE {
                        layers::ragdoll_no_self_groups()
                    } else {
                        layers::ragdoll_groups()
                    },
                    AdditionalMassProperties::Mass(def.mass.max(0.2)),
                    spawn_tf,
                    GlobalTransform::from(spawn_tf),
                    Friction::coefficient(1.2),
                    Restitution::coefficient(0.0),
                    Damping {
                        linear_damping: RAGDOLL_LINEAR_DAMPING,
                        angular_damping: RAGDOLL_ANGULAR_DAMPING,
                    },
                    GravityScale(1.0),
                    Velocity::default(),
                    ExternalImpulse::default(),
                    Sleeping::default(),
                    Ccd::disabled(),
                ))
                .id();

            spawned.push((def, entity, world_pos, spawn_tf.rotation));
            by_id.insert(def.id, (entity, world_pos, spawn_tf.rotation));
        }

        for (def, child_entity, child_pos, _child_rot) in &spawned {
            let Some(parent_id) = def.parent else {
                continue;
            };
            let Some((parent_entity, parent_pos, parent_rot)) = by_id.get(&parent_id).copied()
            else {
                continue;
            };
            let Some((_, _, child_rot)) = by_id.get(&def.id).copied() else {
                continue;
            };
            let anchor_world = (parent_pos + *child_pos) * 0.5;
            let parent_anchor_local = parent_rot.inverse() * (anchor_world - parent_pos);
            let child_anchor_local = child_rot.inverse() * (anchor_world - *child_pos);
            if SOFT_RAGDOLL_MODE {
                // Preserve the current parent/child orientation at joint creation.
                // If we leave fixed-joint bases at identity, Rapier injects a large
                // corrective torque on frame 1 to force bodies into a different frame,
                // which looks like immediate ragdoll "breakdancing".
                let parent_basis_local = Quat::IDENTITY;
                let child_basis_local = (child_rot.inverse() * parent_rot).normalize();
                // Every link (torso included) is a limited spherical joint so
                // the corpse can fold/crumple naturally; per-body limits keep
                // it anatomical. The old rigid torso welds made the whole
                // upper body fall as one T-posed plank.
                let mut joint = apply_soft_joint_limits(
                    GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                        .local_anchor1(parent_anchor_local)
                        .local_anchor2(child_anchor_local)
                        .local_basis1(parent_basis_local)
                        .local_basis2(child_basis_local),
                    def.id,
                )
                .build();
                joint.set_contacts_enabled(false);
                commands.entity(*child_entity).insert(ImpulseJoint::new(
                    parent_entity,
                    TypedJoint::GenericJoint(joint),
                ));
            } else {
                let mut joint = apply_joint_limits(
                    SphericalJointBuilder::new()
                        .local_anchor1(parent_anchor_local)
                        .local_anchor2(child_anchor_local),
                    def.id,
                )
                .build();
                joint.set_contacts_enabled(false);
                commands
                    .entity(*child_entity)
                    .insert(ImpulseJoint::new(parent_entity, joint));
            }
        }

        // Death knockback applied as EXPLICIT VELOCITIES, not ExternalImpulse:
        // impulses applied during body initialization use rapier's
        // collider-derived mass (AdditionalMassProperties hasn't been synced
        // yet), which made corpses launch at ~10x the intended speed
        // ("jet engine" kills). We know our intended masses, so we compute
        // the velocity change ourselves — deterministic by construction.
        {
            let (impulse_mag, impulse_dir) = death_impact
                .map(|impact| (impact.impulse.length(), impact.impulse.normalize_or_zero()))
                .unwrap_or((0.0, Vec3::ZERO));
            let total_mass: f32 = spawned.iter().map(|(def, _, _, _)| def.mass.max(0.2)).sum();
            // Whole-body shove (Source-style): uniform velocity change plus
            // the NPC's own movement momentum carried into the corpse.
            let uniform_dv = (impulse_mag * IMPULSE_UNIFORM_SHARE / total_mass.max(1.0)).min(1.6);
            let base_velocity = npc_vel.0 + impulse_dir * uniform_dv;

            // Localized extra kick + spin on the struck body.
            let mut nearest: Option<(Entity, Vec3, f32)> = None;
            if let Some(impact) = death_impact {
                if let Some(body) = impact.body {
                    nearest = spawned
                        .iter()
                        .find(|(def, _, _, _)| def.id == body)
                        .map(|(def, entity, pos, _)| (*entity, *pos, def.mass.max(0.2)));
                }
                if nearest.is_none() {
                    let mut best_dist_sq = f32::INFINITY;
                    for (def, entity, pos, _rot) in &spawned {
                        let dist_sq = pos.distance_squared(impact.hit_point);
                        if dist_sq < best_dist_sq {
                            best_dist_sq = dist_sq;
                            nearest = Some((*entity, *pos, def.mass.max(0.2)));
                        }
                    }
                }
            }

            for (_def, entity, pos, _rot) in &spawned {
                let mut linvel = base_velocity;
                let mut angvel = Vec3::ZERO;
                if let (Some(impact), Some((hit_entity, _hit_pos, hit_mass))) =
                    (death_impact, nearest)
                {
                    if *entity == hit_entity {
                        let local_dv = (impulse_mag * IMPULSE_LOCAL_SHARE / hit_mass)
                            .min(IMPULSE_LOCAL_MAX_DV);
                        linvel += impulse_dir * local_dv;
                        // Spin from the off-center hit (angular impulse ~ r x J),
                        // clamped so limbs twitch rather than helicopter.
                        let lever = impact.hit_point - *pos;
                        angvel = lever.cross(impulse_dir) * 30.0;
                        let spin = angvel.length();
                        if spin > 6.0 {
                            angvel = angvel / spin * 6.0;
                        }
                    }
                }
                commands.entity(*entity).insert(Velocity { linvel, angvel });
            }
        }

        let bindings = NpcRagdollBodies {
            bindings: spawned
                .iter()
                .map(|(def, entity, _, _)| RagdollBodyBinding {
                    id: def.id,
                    entity: *entity,
                    radius: humanoid_body_bounding_radius(humanoid_body_shape(def.id)),
                })
                .collect(),
            next_seq: 0,
            last_pose_sent_at: now,
        };
        let lifecycle = CorpseLifecycle {
            started_at: now,
            despawn_at: now + DEAD_NPC_DESPAWN_TIME,
            age_order: budget.next_age_order,
        };
        budget.next_age_order = budget.next_age_order.wrapping_add(1);

        activity.0 = NpcActivityKind::Dead;
        commands
            .entity(npc_entity)
            .insert((NpcRagdoll, bindings.clone(), lifecycle));
        commands.entity(npc_entity).remove::<NpcDeathImpact>();

        let root_position = by_id
            .get(&RagdollBodyId::Pelvis)
            .map(|(_, pos, _)| *pos - Vec3::Y * PELVIS_LOCAL_Y_FROM_NPC_CENTER)
            .unwrap_or(npc_pos.0);
        let root_rotation_packed = pack_quat_i16(root_rotation);

        let started = NpcRagdollStarted {
            npc_id: npc.id,
            archetype: npc.archetype,
            started_at: now,
            root_position,
            root_rotation: root_rotation_packed,
            bodies: spawned
                .into_iter()
                .map(|(def, _entity, world_pos, world_rot)| RagdollBodyPose {
                    body: def.id,
                    position: world_pos,
                    rotation: pack_quat_i16(world_rot),
                })
                .collect(),
        };

        for (client_entity, mut sender) in clients.iter_mut() {
            if replication.is_visible(client_entity) {
                sender.send::<ReliableChannel>(started.clone());
            }
        }
    }
}

/// Extra velocity clamps for soft-mode ragdolls to prevent runaway spin/energy.
pub fn stabilize_soft_ragdoll_bodies(mut bodies: Query<&mut Velocity, With<RagdollPhysicsBody>>) {
    if !SOFT_RAGDOLL_MODE {
        return;
    }

    for mut velocity in bodies.iter_mut() {
        let lin = velocity.linvel.length();
        if lin > RAGDOLL_MAX_LINEAR_SPEED {
            velocity.linvel = velocity.linvel / lin * RAGDOLL_MAX_LINEAR_SPEED;
        }
        let ang = velocity.angvel.length();
        if ang > RAGDOLL_MAX_ANGULAR_SPEED {
            velocity.angvel = velocity.angvel / ang * RAGDOLL_MAX_ANGULAR_SPEED;
        }
        // Zero near-rest velocities ONCE (guard on > 0.0): writing Velocity
        // every tick would wake the bodies forever and cause endless
        // micro-solving (visible corpse twitching, never sleeping).
        let lin_sq = velocity.linvel.length_squared();
        if lin_sq > 0.0 && lin_sq < 1.0e-4 {
            velocity.linvel = Vec3::ZERO;
        }
        let ang_sq = velocity.angvel.length_squared();
        if ang_sq > 0.0 && ang_sq < 1.0e-4 {
            velocity.angvel = Vec3::ZERO;
        }
    }
}

pub fn sync_npc_roots_from_ragdolls(
    mut corpses: Query<
        (
            &NpcRagdollBodies,
            &mut NpcPosition,
            &mut NpcRotation,
            Option<&mut NpcActivity>,
        ),
        With<NpcRagdoll>,
    >,
    body_transforms: Query<&Transform>,
) {
    for (bindings, mut pos, mut rot, activity) in corpses.iter_mut() {
        let pelvis = bindings
            .bindings
            .iter()
            .find(|binding| binding.id == RagdollBodyId::Pelvis)
            .or_else(|| bindings.bindings.first());
        let Some(pelvis) = pelvis else { continue };
        let Ok(tf) = body_transforms.get(pelvis.entity) else {
            continue;
        };
        pos.set_if_neq(NpcPosition(
            tf.translation - Vec3::Y * PELVIS_LOCAL_Y_FROM_NPC_CENTER,
        ));
        let (yaw, _, _) = tf.rotation.to_euler(EulerRot::YXZ);
        rot.set_if_neq(NpcRotation(yaw - RAGDOLL_VISUAL_YAW_OFFSET));
        if let Some(mut activity) = activity {
            activity.set_if_neq(NpcActivity(NpcActivityKind::Dead));
        }
    }
}

pub fn sync_corpse_collision_index(
    mut index: ResMut<CorpseCollisionIndex>,
    mut telemetry: ResMut<RagdollTelemetry>,
    corpses: Query<(Entity, &NpcRagdollBodies), With<NpcRagdoll>>,
    body_transforms: Query<&Transform>,
) {
    index.cells.clear();
    index.by_npc.clear();
    let mut corpse_count = 0usize;

    for (npc_entity, bindings) in corpses.iter() {
        corpse_count += 1;
        let mut npc_points = Vec::with_capacity(bindings.bindings.len());
        for binding in &bindings.bindings {
            let Ok(tf) = body_transforms.get(binding.entity) else {
                continue;
            };
            let point = CorpseBodyPoint {
                npc_entity,
                body_entity: binding.entity,
                body: binding.id,
                position: tf.translation,
                radius: binding.radius,
            };
            npc_points.push(point);
        }
        for point in npc_points.iter().copied() {
            let key = index.cell_key(point.position);
            index.cells.entry(key).or_default().push(point);
        }
        index.by_npc.insert(npc_entity, npc_points);
    }

    index.version = index.version.wrapping_add(1);
    telemetry.active_corpses = corpse_count;
}

pub fn send_ragdoll_pose_snapshots(
    time: Res<Time>,
    mut stream: ResMut<RagdollPoseStream>,
    mut telemetry: ResMut<RagdollTelemetry>,
    mut corpses: Query<(Entity, &Npc, &mut NpcRagdollBodies, &ReplicationState), With<NpcRagdoll>>,
    body_transforms: Query<&Transform>,
    body_motion: Query<(&Velocity, &Sleeping), With<RagdollPhysicsBody>>,
    mut clients: Query<
        (Entity, &mut MessageSender<NpcRagdollPoseBatch>),
        (With<ClientOf>, With<Connected>),
    >,
) {
    stream.accumulator += time.delta_secs();
    if stream.accumulator < stream.interval_secs {
        return;
    }
    stream.accumulator = 0.0;

    let client_entities: Vec<Entity> = clients.iter_mut().map(|(entity, _)| entity).collect();
    if client_entities.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let mut samples_by_client: HashMap<Entity, Vec<NpcRagdollPoseSample>> = client_entities
        .iter()
        .copied()
        .map(|entity| (entity, Vec::new()))
        .collect();

    for (_npc_entity, npc, mut bindings, replication) in corpses.iter_mut() {
        if !client_entities
            .iter()
            .any(|client_entity| replication.is_visible(*client_entity))
        {
            continue;
        }
        let moving = bindings.bindings.iter().any(|binding| {
            body_motion
                .get(binding.entity)
                .is_ok_and(|(velocity, sleeping)| {
                    !sleeping.sleeping
                        && (velocity.linvel.length_squared() > 2.5e-3
                            || velocity.angvel.length_squared() > 1.0e-2)
                })
        });
        if !moving && now - bindings.last_pose_sent_at < 1.0 {
            continue;
        }

        let mut body_poses = Vec::with_capacity(bindings.bindings.len());
        let mut root_position = None;
        let mut root_rotation = None;
        for binding in &bindings.bindings {
            let Ok(tf) = body_transforms.get(binding.entity) else {
                continue;
            };
            let pose = pose_from_transform(binding.id, tf);
            if binding.id == RagdollBodyId::Pelvis {
                root_position = Some(pose.position);
                root_rotation = Some(pose.rotation);
            }
            body_poses.push(pose);
        }

        let (Some(root_position), Some(root_rotation)) = (root_position, root_rotation) else {
            continue;
        };
        let seq = bindings.next_seq;
        bindings.next_seq = bindings.next_seq.wrapping_add(1);
        bindings.last_pose_sent_at = now;
        let sample = NpcRagdollPoseSample {
            npc_id: npc.id,
            seq,
            root_position,
            root_rotation,
            bodies: body_poses,
        };
        for client_entity in &client_entities {
            if replication.is_visible(*client_entity) {
                samples_by_client
                    .get_mut(client_entity)
                    .expect("connected client pose batch should exist")
                    .push(sample.clone());
            }
        }
    }

    let mut recipients = 0u64;
    let mut bytes_sent = 0u64;
    for (client_entity, mut sender) in clients.iter_mut() {
        let samples = samples_by_client.remove(&client_entity).unwrap_or_default();
        if samples.is_empty() {
            continue;
        }
        let batch = NpcRagdollPoseBatch {
            server_time_ms: (now * 1000.0) as u64,
            samples,
        };
        bytes_sent = bytes_sent.saturating_add(bincode::serialized_size(&batch).unwrap_or(0));
        sender.send::<RagdollPoseChannel>(batch);
        recipients = recipients.saturating_add(1);
    }
    if recipients > 0 {
        telemetry.total_pose_msgs = telemetry.total_pose_msgs.saturating_add(recipients);
        telemetry.total_pose_bytes = telemetry.total_pose_bytes.saturating_add(bytes_sent);
    }
}

pub fn evict_excess_corpses(
    mut commands: Commands,
    budget: Res<CorpseBudget>,
    mut telemetry: ResMut<RagdollTelemetry>,
    corpses: Query<(Entity, &CorpseLifecycle, Option<&NpcRagdollBodies>), With<NpcRagdoll>>,
) {
    let count = corpses.iter().count();
    if count <= budget.max_corpses {
        return;
    }

    let mut ordered: Vec<(Entity, u64)> = corpses
        .iter()
        .map(|(entity, lifecycle, _)| (entity, lifecycle.age_order))
        .collect();
    ordered.sort_by_key(|entry| entry.1);
    let evict_count = count - budget.max_corpses;

    for (entity, _) in ordered.into_iter().take(evict_count) {
        if let Ok((_, _, bodies)) = corpses.get(entity) {
            despawn_npc_with_bodies(&mut commands, entity, bodies);
            telemetry.total_evicted = telemetry.total_evicted.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpse_collision_index_collects_spawned_points() {
        let mut app = App::new();
        app.init_resource::<CorpseCollisionIndex>();
        app.init_resource::<RagdollTelemetry>();
        app.add_systems(Update, sync_corpse_collision_index);

        let body_entity = app
            .world_mut()
            .spawn(Transform::from_translation(Vec3::new(1.0, 0.0, 2.0)))
            .id();
        let npc_entity = app
            .world_mut()
            .spawn((
                NpcRagdoll,
                NpcRagdollBodies {
                    bindings: vec![RagdollBodyBinding {
                        id: RagdollBodyId::Pelvis,
                        entity: body_entity,
                        radius: 0.25,
                    }],
                    next_seq: 0,
                    last_pose_sent_at: 0.0,
                },
            ))
            .id();

        app.update();

        let mut out = Vec::new();
        app.world()
            .resource::<CorpseCollisionIndex>()
            .collect_nearby(Vec3::new(1.0, 0.0, 2.0), 1.0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].npc_entity, npc_entity);
    }

    #[test]
    fn evict_excess_corpses_despawns_oldest() {
        let mut app = App::new();
        app.insert_resource(CorpseBudget {
            max_corpses: 2,
            next_age_order: 0,
        });
        app.init_resource::<RagdollTelemetry>();
        app.add_systems(Update, evict_excess_corpses);

        let oldest = app
            .world_mut()
            .spawn((
                NpcRagdoll,
                CorpseLifecycle {
                    started_at: 0.0,
                    despawn_at: 60.0,
                    age_order: 0,
                },
                NpcRagdollBodies::default(),
            ))
            .id();
        let newer = app
            .world_mut()
            .spawn((
                NpcRagdoll,
                CorpseLifecycle {
                    started_at: 0.0,
                    despawn_at: 60.0,
                    age_order: 1,
                },
                NpcRagdollBodies::default(),
            ))
            .id();
        let newest = app
            .world_mut()
            .spawn((
                NpcRagdoll,
                CorpseLifecycle {
                    started_at: 0.0,
                    despawn_at: 60.0,
                    age_order: 2,
                },
                NpcRagdollBodies::default(),
            ))
            .id();

        app.update();

        assert!(app.world().get_entity(oldest).is_err());
        assert!(app.world().get_entity(newer).is_ok());
        assert!(app.world().get_entity(newest).is_ok());
        assert_eq!(app.world().resource::<RagdollTelemetry>().total_evicted, 1);
    }
}
