//! Authoritative NPC ragdoll runtime (Oilman v1).

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::*;
use shared::components::{
    Health, Npc, NpcActivity, NpcActivityKind, NpcArchetype, NpcPosition, NpcRotation,
};
use shared::npc::DEAD_NPC_DESPAWN_TIME;
use shared::protocol::{
    pack_quat_i16, NpcRagdollPoseBatch, NpcRagdollPoseSample, NpcRagdollStarted, RagdollBodyId,
    RagdollBodyPose, RagdollPoseChannel, ReliableChannel,
};
use std::collections::{HashMap, HashSet};

use crate::physics::layers;

const DEFAULT_CORPSE_CAP: usize = 64;
const DEFAULT_RAGDOLL_POSE_HZ: f32 = 15.0;
const CORPSE_CELL_SIZE: f32 = 8.0;
const PELVIS_LOCAL_Y_FROM_NPC_CENTER: f32 = 0.0;
const RAGDOLL_VISUAL_YAW_OFFSET: f32 = std::f32::consts::PI;
const SOFT_RAGDOLL_MODE: bool = true;
const RAGDOLL_LINEAR_DAMPING: f32 = 8.5;
const RAGDOLL_ANGULAR_DAMPING: f32 = 20.0;
const RAGDOLL_MAX_LINEAR_SPEED: f32 = 4.0;
const RAGDOLL_MAX_ANGULAR_SPEED: f32 = 5.5;

#[derive(Component, Clone, Copy, Debug)]
pub struct RagdollPhysicsBody;

#[derive(Component, Clone, Copy, Debug)]
pub struct NpcDeathImpact {
    pub hit_point: Vec3,
    pub impulse: Vec3,
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

#[derive(Clone, Copy)]
struct BodyDefinition {
    id: RagdollBodyId,
    local_offset: Vec3,
    radius: f32,
    mass: f32,
    parent: Option<RagdollBodyId>,
}

const OILMAN_BODY_DEFS: [BodyDefinition; 12] = [
    BodyDefinition {
        id: RagdollBodyId::Pelvis,
        local_offset: Vec3::new(0.0, 0.0, 0.0),
        radius: 0.13,
        mass: 13.0,
        parent: None,
    },
    BodyDefinition {
        id: RagdollBodyId::SpineLower,
        local_offset: Vec3::new(0.0, 0.20, 0.0),
        radius: 0.11,
        mass: 8.0,
        parent: Some(RagdollBodyId::Pelvis),
    },
    BodyDefinition {
        id: RagdollBodyId::SpineUpper,
        local_offset: Vec3::new(0.0, 0.42, 0.0),
        radius: 0.11,
        mass: 7.0,
        parent: Some(RagdollBodyId::SpineLower),
    },
    BodyDefinition {
        id: RagdollBodyId::Head,
        local_offset: Vec3::new(0.0, 0.74, 0.0),
        radius: 0.10,
        mass: 5.0,
        parent: Some(RagdollBodyId::SpineUpper),
    },
    BodyDefinition {
        id: RagdollBodyId::UpperArmL,
        local_offset: Vec3::new(-0.24, 0.44, 0.0),
        radius: 0.07,
        mass: 3.0,
        parent: Some(RagdollBodyId::SpineUpper),
    },
    BodyDefinition {
        id: RagdollBodyId::UpperArmR,
        local_offset: Vec3::new(0.24, 0.44, 0.0),
        radius: 0.07,
        mass: 3.0,
        parent: Some(RagdollBodyId::SpineUpper),
    },
    BodyDefinition {
        id: RagdollBodyId::ForearmL,
        local_offset: Vec3::new(-0.48, 0.40, 0.0),
        radius: 0.06,
        mass: 2.5,
        parent: Some(RagdollBodyId::UpperArmL),
    },
    BodyDefinition {
        id: RagdollBodyId::ForearmR,
        local_offset: Vec3::new(0.48, 0.40, 0.0),
        radius: 0.06,
        mass: 2.5,
        parent: Some(RagdollBodyId::UpperArmR),
    },
    BodyDefinition {
        id: RagdollBodyId::ThighL,
        local_offset: Vec3::new(-0.11, -0.33, 0.0),
        radius: 0.085,
        mass: 5.0,
        parent: Some(RagdollBodyId::Pelvis),
    },
    BodyDefinition {
        id: RagdollBodyId::ThighR,
        local_offset: Vec3::new(0.11, -0.33, 0.0),
        radius: 0.085,
        mass: 5.0,
        parent: Some(RagdollBodyId::Pelvis),
    },
    BodyDefinition {
        id: RagdollBodyId::CalfL,
        local_offset: Vec3::new(-0.11, -0.73, 0.0),
        radius: 0.075,
        mass: 4.0,
        parent: Some(RagdollBodyId::ThighL),
    },
    BodyDefinition {
        id: RagdollBodyId::CalfR,
        local_offset: Vec3::new(0.11, -0.73, 0.0),
        radius: 0.075,
        mass: 4.0,
        parent: Some(RagdollBodyId::ThighR),
    },
];

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
        RagdollBodyId::ThighL | RagdollBodyId::ThighR => joint
            .limits(JointAxis::AngX, [-1.10, 0.65])
            .limits(JointAxis::AngY, [-0.35, 0.35])
            .limits(JointAxis::AngZ, [-0.28, 0.28]),
        RagdollBodyId::CalfL | RagdollBodyId::CalfR => joint
            .limits(JointAxis::AngX, [0.0, 1.70])
            .limits(JointAxis::AngY, [-0.15, 0.15])
            .limits(JointAxis::AngZ, [-0.15, 0.15]),
        RagdollBodyId::Pelvis => joint,
    }
}

fn apply_soft_joint_limits(
    mut joint: GenericJointBuilder,
    body_id: RagdollBodyId,
) -> GenericJointBuilder {
    match body_id {
        RagdollBodyId::Head => {
            joint = joint
                .limits(JointAxis::AngX, [-0.65, 0.65])
                .limits(JointAxis::AngY, [-0.95, 0.95])
                .limits(JointAxis::AngZ, [-0.55, 0.55]);
        }
        RagdollBodyId::UpperArmL | RagdollBodyId::UpperArmR => {
            joint = joint
                .limits(JointAxis::AngX, [-1.05, 0.95])
                .limits(JointAxis::AngY, [-0.60, 0.60])
                .limits(JointAxis::AngZ, [-0.70, 0.70]);
        }
        RagdollBodyId::ForearmL | RagdollBodyId::ForearmR => {
            joint = joint
                .limits(JointAxis::AngX, [0.0, 1.45])
                .limits(JointAxis::AngY, [-0.15, 0.15])
                .limits(JointAxis::AngZ, [-0.15, 0.15]);
        }
        RagdollBodyId::ThighL | RagdollBodyId::ThighR => {
            joint = joint
                .limits(JointAxis::AngX, [-0.85, 0.50])
                .limits(JointAxis::AngY, [-0.28, 0.28])
                .limits(JointAxis::AngZ, [-0.22, 0.22]);
        }
        RagdollBodyId::CalfL | RagdollBodyId::CalfR => {
            joint = joint
                .limits(JointAxis::AngX, [0.0, 1.40])
                .limits(JointAxis::AngY, [-0.12, 0.12])
                .limits(JointAxis::AngZ, [-0.12, 0.12]);
        }
        _ => {}
    }
    joint
}

fn local_offset_for(body_id: RagdollBodyId) -> Option<Vec3> {
    OILMAN_BODY_DEFS
        .iter()
        .find(|def| def.id == body_id)
        .map(|def| def.local_offset)
}

fn local_axis_for(def: &BodyDefinition) -> Vec3 {
    if let Some(parent_id) = def.parent {
        if let Some(parent_offset) = local_offset_for(parent_id) {
            let axis = def.local_offset - parent_offset;
            if axis.length_squared() > 1.0e-6 {
                return axis.normalize();
            }
        }
    }
    Vec3::Y
}

fn collider_for_body(def: &BodyDefinition) -> Collider {
    match def.id {
        RagdollBodyId::Pelvis => Collider::cuboid(0.12, 0.10, 0.10),
        RagdollBodyId::SpineLower | RagdollBodyId::SpineUpper => {
            Collider::capsule_y(0.08, def.radius.max(0.05))
        }
        RagdollBodyId::Head => Collider::ball(def.radius.max(0.05)),
        RagdollBodyId::UpperArmL | RagdollBodyId::UpperArmR => {
            Collider::capsule_y(0.11, (def.radius * 0.85).max(0.04))
        }
        RagdollBodyId::ForearmL | RagdollBodyId::ForearmR => {
            Collider::capsule_y(0.13, (def.radius * 0.8).max(0.04))
        }
        RagdollBodyId::ThighL | RagdollBodyId::ThighR => {
            Collider::capsule_y(0.16, (def.radius * 0.9).max(0.05))
        }
        RagdollBodyId::CalfL | RagdollBodyId::CalfR => {
            Collider::capsule_y(0.15, (def.radius * 0.85).max(0.045))
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

fn build_started_message(
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
            &mut NpcActivity,
            Option<&NpcDeathImpact>,
            Option<&NpcRagdoll>,
        ),
        With<Npc>,
    >,
    mut clients: Query<&mut MessageSender<NpcRagdollStarted>, (With<ClientOf>, With<Connected>)>,
) {
    let now = time.elapsed_secs();
    for (npc_entity, npc, health, npc_pos, npc_rot, mut activity, death_impact, ragdoll) in
        npcs.iter_mut()
    {
        if !health.is_dead() || ragdoll.is_some() {
            continue;
        }
        if npc.archetype != NpcArchetype::Oilman {
            commands.entity(npc_entity).remove::<NpcDeathImpact>();
            continue;
        }

        // Client visual rigs are instantiated with a +PI yaw at model root.
        // Match that frame here so authoritative ragdoll bodies line up with
        // mapped skeleton bones (left/right limbs and knees/elbows).
        let root_rotation = Quat::from_rotation_y(npc_rot.0 + RAGDOLL_VISUAL_YAW_OFFSET);
        let mut spawned = Vec::with_capacity(OILMAN_BODY_DEFS.len());
        let mut by_id: HashMap<RagdollBodyId, (Entity, Vec3, Quat)> =
            HashMap::with_capacity(OILMAN_BODY_DEFS.len());

        for def in OILMAN_BODY_DEFS {
            let world_pos = npc_pos.0 + root_rotation * def.local_offset;
            let local_axis = local_axis_for(&def);
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
                    Ccd::enabled(),
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
                if matches!(
                    def.id,
                    RagdollBodyId::SpineLower | RagdollBodyId::SpineUpper
                ) {
                    // Keep torso rigid to avoid fold/curl artifacts.
                    let mut joint = FixedJointBuilder::new()
                        .local_anchor1(parent_anchor_local)
                        .local_anchor2(child_anchor_local)
                        .local_basis1(parent_basis_local)
                        .local_basis2(child_basis_local)
                        .build();
                    // Disable parent-child contacts across ragdoll links.
                    joint.set_contacts_enabled(false);
                    commands
                        .entity(*child_entity)
                        .insert(ImpulseJoint::new(parent_entity, joint));
                } else {
                    // Limbs/head use limited spherical joints for a natural fall while
                    // still preserving the spawned local joint frames.
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
                }
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

        if let Some(impact) = death_impact {
            let mut nearest = None;
            let mut best_dist_sq = f32::INFINITY;
            for (_def, entity, pos, _rot) in &spawned {
                let dist_sq = pos.distance_squared(impact.hit_point);
                if dist_sq < best_dist_sq {
                    best_dist_sq = dist_sq;
                    nearest = Some((*entity, *pos));
                }
            }
            if let Some((body_entity, body_pos)) = nearest {
                let scaled = if SOFT_RAGDOLL_MODE {
                    impact.impulse * 0.18
                } else {
                    impact.impulse
                };
                let impulse = ExternalImpulse::at_point(scaled, impact.hit_point, body_pos);
                commands.entity(body_entity).insert(impulse);
            }
        }

        let bindings = NpcRagdollBodies {
            bindings: spawned
                .iter()
                .map(|(def, entity, _, _)| RagdollBodyBinding {
                    id: def.id,
                    entity: *entity,
                    radius: def.radius,
                })
                .collect(),
            next_seq: 0,
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

        for mut sender in clients.iter_mut() {
            sender.send::<ReliableChannel>(started.clone());
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
        if velocity.linvel.length_squared() < 1.0e-4 {
            velocity.linvel = Vec3::ZERO;
        }
        if velocity.angvel.length_squared() < 1.0e-4 {
            velocity.angvel = Vec3::ZERO;
        }
    }
}

pub fn replay_active_ragdolls_to_new_clients(
    mut new_clients: Query<
        &mut MessageSender<NpcRagdollStarted>,
        (With<ClientOf>, With<Connected>, Added<Connected>),
    >,
    corpses: Query<(&Npc, &CorpseLifecycle, &NpcRagdollBodies), With<NpcRagdoll>>,
    body_transforms: Query<&Transform>,
) {
    for mut sender in new_clients.iter_mut() {
        for (npc, lifecycle, bindings) in corpses.iter() {
            if let Some(msg) = build_started_message(npc, lifecycle, bindings, &body_transforms) {
                sender.send::<ReliableChannel>(msg);
            }
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
        pos.0 = tf.translation - Vec3::Y * PELVIS_LOCAL_Y_FROM_NPC_CENTER;
        let (yaw, _, _) = tf.rotation.to_euler(EulerRot::YXZ);
        rot.0 = yaw - RAGDOLL_VISUAL_YAW_OFFSET;
        if let Some(mut activity) = activity {
            activity.0 = NpcActivityKind::Dead;
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
    mut corpses: Query<(&Npc, &mut NpcRagdollBodies), With<NpcRagdoll>>,
    body_transforms: Query<&Transform>,
    mut clients: Query<&mut MessageSender<NpcRagdollPoseBatch>, (With<ClientOf>, With<Connected>)>,
) {
    stream.accumulator += time.delta_secs();
    if stream.accumulator < stream.interval_secs {
        return;
    }
    stream.accumulator = 0.0;

    let mut samples = Vec::with_capacity(corpses.iter().len());
    for (npc, mut bindings) in corpses.iter_mut() {
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
        samples.push(NpcRagdollPoseSample {
            npc_id: npc.id,
            seq,
            root_position,
            root_rotation,
            bodies: body_poses,
        });
    }

    if samples.is_empty() {
        return;
    }

    let batch = NpcRagdollPoseBatch {
        server_time_ms: (time.elapsed_secs() * 1000.0) as u64,
        samples,
    };
    let bytes = bincode::serialized_size(&batch).unwrap_or(0) as u64;
    let mut recipients = 0u64;
    for mut sender in clients.iter_mut() {
        sender.send::<RagdollPoseChannel>(batch.clone());
        recipients = recipients.saturating_add(1);
    }
    if recipients > 0 {
        telemetry.total_pose_msgs = telemetry.total_pose_msgs.saturating_add(recipients);
        telemetry.total_pose_bytes = telemetry
            .total_pose_bytes
            .saturating_add(bytes.saturating_mul(recipients));
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
