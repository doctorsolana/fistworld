use super::*;
use bevy::camera::primitives::MeshAabb;
use bevy::math::Mat4;

const FRONT_AXLE_NAME: &str = "Axel_Front_01";
const STEERING_WHEEL_NAME: &str = "Steering_Wheel_01";
const FRONT_LEFT_WHEEL_NAME: &str = "Wheel_Front_01";
const FRONT_RIGHT_WHEEL_NAME: &str = "Wheel_Front_02";
const REAR_LEFT_WHEEL_NAME: &str = "Wheel_Rear_01";
const REAR_RIGHT_WHEEL_NAME: &str = "Wheel_Rear_02";
const STEER_RESPONSE: f32 = 10.0;
const STEERING_WHEEL_RATIO: f32 = 2.8;

pub fn setup_steam_car_visual_rigs(
    mut commands: Commands,
    cars: Query<(Entity, &Vehicle), (With<VehicleVisual>, With<NeedsSteamCarRigSetup>)>,
    scene_roots: Query<(Entity, &ChildOf), With<SteamCarSceneRoot>>,
    children_q: Query<&Children>,
    parents_q: Query<&ChildOf>,
    names_q: Query<&Name>,
    transforms_q: Query<&Transform>,
    mesh_q: Query<&Mesh3d>,
    meshes: Res<Assets<Mesh>>,
) {
    for (entity, vehicle) in cars.iter() {
        if !matches!(vehicle.vehicle_type, VehicleType::Car | VehicleType::CarV2) {
            commands.entity(entity).remove::<NeedsSteamCarRigSetup>();
            continue;
        }

        let Some(scene_root_entity) = scene_roots
            .iter()
            .find_map(|(scene_root, parent)| (parent.parent() == entity).then_some(scene_root))
        else {
            continue;
        };

        let Some(scene_root) = part_ref(scene_root_entity, &transforms_q) else {
            continue;
        };

        let mut front_axle = None;
        let mut steering_wheel = None;
        let mut front_left_wheel = None;
        let mut front_right_wheel = None;
        let mut rear_left_wheel = None;
        let mut rear_right_wheel = None;

        let mut stack = vec![scene_root_entity];
        while let Some(current) = stack.pop() {
            if let Ok(name) = names_q.get(current) {
                let name = name.as_str();
                if front_axle.is_none() && name.contains(FRONT_AXLE_NAME) {
                    front_axle = part_ref(current, &transforms_q);
                } else if steering_wheel.is_none() && name.contains(STEERING_WHEEL_NAME) {
                    steering_wheel = part_ref(current, &transforms_q);
                } else if front_left_wheel.is_none() && name.contains(FRONT_LEFT_WHEEL_NAME) {
                    front_left_wheel = wheel_ref(
                        entity,
                        current,
                        &children_q,
                        &parents_q,
                        &transforms_q,
                        &mesh_q,
                        &meshes,
                    );
                } else if front_right_wheel.is_none() && name.contains(FRONT_RIGHT_WHEEL_NAME) {
                    front_right_wheel = wheel_ref(
                        entity,
                        current,
                        &children_q,
                        &parents_q,
                        &transforms_q,
                        &mesh_q,
                        &meshes,
                    );
                } else if rear_left_wheel.is_none() && name.contains(REAR_LEFT_WHEEL_NAME) {
                    rear_left_wheel = wheel_ref(
                        entity,
                        current,
                        &children_q,
                        &parents_q,
                        &transforms_q,
                        &mesh_q,
                        &meshes,
                    );
                } else if rear_right_wheel.is_none() && name.contains(REAR_RIGHT_WHEEL_NAME) {
                    rear_right_wheel = wheel_ref(
                        entity,
                        current,
                        &children_q,
                        &parents_q,
                        &transforms_q,
                        &mesh_q,
                        &meshes,
                    );
                }
            }

            if let Ok(children) = children_q.get(current) {
                for child in children.iter() {
                    stack.push(child);
                }
            }
        }

        let (
            Some(front_axle),
            Some(front_left_wheel),
            Some(front_right_wheel),
            Some(rear_left_wheel),
            Some(rear_right_wheel),
        ) = (
            front_axle,
            front_left_wheel,
            front_right_wheel,
            rear_left_wheel,
            rear_right_wheel,
        )
        else {
            continue;
        };

        let def = shared::vehicle::vehicle_def(VehicleType::Car);
        let desired_bottom_y = -static_ride_height(def);
        let wheel_spin_radius = (front_left_wheel.spin_radius
            + front_right_wheel.spin_radius
            + rear_left_wheel.spin_radius
            + rear_right_wheel.spin_radius)
            * 0.25;

        let wheel_meshes = [
            &front_left_wheel,
            &front_right_wheel,
            &rear_left_wheel,
            &rear_right_wheel,
        ];
        let mut current_bottom_y = f32::INFINITY;
        for wheel in wheel_meshes {
            let Some(bottom_y) = mesh_bottom_relative_to_ancestor(
                entity,
                wheel.mesh.entity,
                &parents_q,
                &transforms_q,
                &mesh_q,
                &meshes,
            ) else {
                current_bottom_y = f32::INFINITY;
                break;
            };
            current_bottom_y = current_bottom_y.min(bottom_y);
        }
        if !current_bottom_y.is_finite() {
            continue;
        }

        commands.entity(entity).insert(SteamCarVisualRig {
            scene_root,
            ride_height_offset: desired_bottom_y - current_bottom_y,
            front_axle,
            steering_wheel,
            front_left_wheel,
            front_right_wheel,
            rear_left_wheel,
            rear_right_wheel,
            wheel_spin_radius,
            wheel_spin: 0.0,
            steer_angle: 0.0,
        });
        commands.entity(entity).remove::<NeedsSteamCarRigSetup>();
    }
}

pub fn update_steam_car_visuals(
    time: Res<Time>,
    mut cars: Query<
        (&Vehicle, &VehicleState, &Transform, &mut SteamCarVisualRig),
        With<VehicleVisual>,
    >,
    mut node_transforms: Query<&mut Transform, Without<VehicleVisual>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (vehicle, state, vehicle_transform, mut rig) in cars.iter_mut() {
        if !matches!(vehicle.vehicle_type, VehicleType::Car | VehicleType::CarV2) {
            continue;
        }

        let def = shared::vehicle::vehicle_def(VehicleType::Car);
        let forward = vehicle_transform.rotation * Vec3::NEG_Z;
        let forward_speed = state.velocity.dot(forward);
        let speed_abs = forward_speed.abs();

        let target_steer = if speed_abs > 0.75 {
            ((def.wheel_base * state.angular_velocity_yaw) / speed_abs.max(2.0))
                .atan()
                .clamp(-def.max_steer_angle, def.max_steer_angle)
        } else {
            0.0
        };
        let steer_t = 1.0 - (-STEER_RESPONSE * dt).exp();
        rig.steer_angle += (target_steer - rig.steer_angle) * steer_t;

        rig.wheel_spin =
            wrap_angle(rig.wheel_spin + forward_speed * dt / rig.wheel_spin_radius.max(0.01));

        apply_root_offset(
            &mut node_transforms,
            &rig.scene_root,
            rig.ride_height_offset,
        );
        apply_node_rotation(
            &mut node_transforms,
            &rig.front_axle,
            Quat::from_rotation_z(-rig.steer_angle),
        );
        apply_wheel_spin(
            &mut node_transforms,
            &rig.front_left_wheel,
            Quat::from_rotation_x(rig.wheel_spin),
        );
        apply_wheel_spin(
            &mut node_transforms,
            &rig.front_right_wheel,
            Quat::from_rotation_x(rig.wheel_spin),
        );
        apply_wheel_spin(
            &mut node_transforms,
            &rig.rear_left_wheel,
            Quat::from_rotation_x(rig.wheel_spin),
        );
        apply_wheel_spin(
            &mut node_transforms,
            &rig.rear_right_wheel,
            Quat::from_rotation_x(rig.wheel_spin),
        );

        if let Some(steering_wheel) = rig.steering_wheel.as_ref() {
            apply_node_rotation(
                &mut node_transforms,
                steering_wheel,
                Quat::from_rotation_z(-rig.steer_angle * STEERING_WHEEL_RATIO),
            );
        }
    }
}

fn part_ref(entity: Entity, transforms_q: &Query<&Transform>) -> Option<SteamCarNodeRef> {
    transforms_q
        .get(entity)
        .ok()
        .map(|transform| SteamCarNodeRef {
            entity,
            base: transform.clone(),
        })
}

fn wheel_ref(
    ancestor: Entity,
    node_entity: Entity,
    children_q: &Query<&Children>,
    parents_q: &Query<&ChildOf>,
    transforms_q: &Query<&Transform>,
    mesh_q: &Query<&Mesh3d>,
    meshes: &Assets<Mesh>,
) -> Option<SteamCarWheelRef> {
    let mesh_entity = first_mesh_descendant(node_entity, children_q, mesh_q)?;
    let mesh_handle = mesh_q.get(mesh_entity).ok()?;
    let mesh_asset = meshes.get(&mesh_handle.0)?;
    let aabb = mesh_asset.compute_aabb()?;
    let center: Vec3 = aabb.center.into();

    Some(SteamCarWheelRef {
        mesh: part_ref(mesh_entity, transforms_q)?,
        pivot_local: center,
        spin_radius: wheel_spin_radius_relative_to_ancestor(
            ancestor,
            mesh_entity,
            parents_q,
            transforms_q,
            mesh_q,
            meshes,
        )?,
    })
}

fn first_mesh_descendant(
    root: Entity,
    children_q: &Query<&Children>,
    mesh_q: &Query<&Mesh3d>,
) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(current) = stack.pop() {
        if mesh_q.get(current).is_ok() {
            return Some(current);
        }
        if let Ok(children) = children_q.get(current) {
            for child in children.iter() {
                stack.push(child);
            }
        }
    }
    None
}

fn static_ride_height(def: shared::vehicle::VehicleDef) -> f32 {
    let static_compression = (def.mass * -def.gravity) / (4.0 * def.suspension_stiffness.max(1.0));
    (def.wheel_radius + def.suspension_rest - static_compression).clamp(
        def.wheel_radius * 0.95,
        def.wheel_radius + def.suspension_rest,
    )
}

fn mesh_bottom_relative_to_ancestor(
    ancestor: Entity,
    mesh_entity: Entity,
    parents_q: &Query<&ChildOf>,
    transforms_q: &Query<&Transform>,
    mesh_q: &Query<&Mesh3d>,
    meshes: &Assets<Mesh>,
) -> Option<f32> {
    let mesh_handle = mesh_q.get(mesh_entity).ok()?;
    let mesh_asset = meshes.get(&mesh_handle.0)?;
    let aabb = mesh_asset.compute_aabb()?;
    let center: Vec3 = aabb.center.into();
    let half_extents: Vec3 = aabb.half_extents.into();
    let relative = relative_transform_to_ancestor(mesh_entity, ancestor, parents_q, transforms_q)?;

    let mut min_y = f32::INFINITY;
    for x in [-half_extents.x, half_extents.x] {
        for y in [-half_extents.y, half_extents.y] {
            for z in [-half_extents.z, half_extents.z] {
                let point = relative.transform_point3(center + Vec3::new(x, y, z));
                min_y = min_y.min(point.y);
            }
        }
    }
    Some(min_y)
}

fn wheel_spin_radius_relative_to_ancestor(
    ancestor: Entity,
    mesh_entity: Entity,
    parents_q: &Query<&ChildOf>,
    transforms_q: &Query<&Transform>,
    mesh_q: &Query<&Mesh3d>,
    meshes: &Assets<Mesh>,
) -> Option<f32> {
    let mesh_handle = mesh_q.get(mesh_entity).ok()?;
    let mesh_asset = meshes.get(&mesh_handle.0)?;
    let aabb = mesh_asset.compute_aabb()?;
    let half_extents: Vec3 = aabb.half_extents.into();
    let relative = relative_transform_to_ancestor(mesh_entity, ancestor, parents_q, transforms_q)?;
    let scale_y = relative.transform_vector3(Vec3::Y).length();
    let scale_z = relative.transform_vector3(Vec3::Z).length();
    Some((half_extents.y * scale_y).max(half_extents.z * scale_z))
}

fn relative_transform_to_ancestor(
    entity: Entity,
    ancestor: Entity,
    parents_q: &Query<&ChildOf>,
    transforms_q: &Query<&Transform>,
) -> Option<Mat4> {
    let mut current = entity;
    let mut matrix = Mat4::IDENTITY;

    while current != ancestor {
        let local = transforms_q.get(current).ok()?;
        matrix = local.to_matrix() * matrix;
        current = parents_q.get(current).ok()?.parent();
    }

    Some(matrix)
}

fn apply_root_offset(
    node_transforms: &mut Query<&mut Transform, Without<VehicleVisual>>,
    part: &SteamCarNodeRef,
    offset_y: f32,
) {
    if let Ok(mut transform) = node_transforms.get_mut(part.entity) {
        *transform = part.base.clone();
        transform.translation.y += offset_y;
    }
}

fn apply_node_rotation(
    node_transforms: &mut Query<&mut Transform, Without<VehicleVisual>>,
    part: &SteamCarNodeRef,
    delta: Quat,
) {
    if let Ok(mut transform) = node_transforms.get_mut(part.entity) {
        *transform = part.base.clone();
        transform.rotation = part.base.rotation * delta;
    }
}

fn apply_wheel_spin(
    node_transforms: &mut Query<&mut Transform, Without<VehicleVisual>>,
    wheel: &SteamCarWheelRef,
    delta: Quat,
) {
    if let Ok(mut transform) = node_transforms.get_mut(wheel.mesh.entity) {
        *transform = wheel.mesh.base.clone();
        transform.rotation = wheel.mesh.base.rotation * delta;
        transform.translation =
            wheel.mesh.base.translation + wheel.pivot_local - delta.mul_vec3(wheel.pivot_local);
    }
}

fn wrap_angle(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
