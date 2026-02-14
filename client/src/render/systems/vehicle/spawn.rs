//! spawn systems.

use super::*;

/// Handle vehicle spawn visuals - hoverbike and test car.
pub fn handle_vehicle_spawned(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    car_assets: Option<Res<CarVisualAssets>>,
    new_vehicles: Query<(Entity, &Vehicle, &VehicleState), Added<Vehicle>>,
) {
    for (entity, vehicle, state) in new_vehicles.iter() {
        info!(
            "Vehicle spawned ({:?}) at {:?}",
            vehicle.vehicle_type, state.position
        );

        // Set up the parent entity with transform and visibility
        let initial_rotation =
            Quat::from_euler(EulerRot::YXZ, state.heading, state.pitch, -state.roll);
        commands.entity(entity).insert((
            Transform::from_translation(state.position).with_rotation(initial_rotation),
            GlobalTransform::from_translation(state.position),
            Visibility::Inherited,
            InheritedVisibility::default(),
            VehicleVisual,
            VehicleRenderSmoothing {
                initialized: true,
                position: state.position,
                heading: state.heading,
                pitch: state.pitch,
                roll: state.roll,
                last_server_position: state.position,
                last_server_heading: state.heading,
                last_server_pitch: state.pitch,
                last_server_roll: state.roll,
                smoothed_velocity: state.velocity,
                smoothed_angular_yaw: state.angular_velocity_yaw,
            },
        ));

        if vehicle.vehicle_type == VehicleType::Car {
            // Simple procedural car for physics testing
            // Using basic shapes so we can clearly see orientation and ground alignment

            let def = shared::vehicle::vehicle_def(VehicleType::Car);
            let Some(car_assets) = car_assets.as_ref() else {
                warn!("CarVisualAssets not initialized; skipping car visuals");
                continue;
            };

            // Dimensions from physics def
            let wheel_radius = def.wheel_radius;
            let half_wb = def.wheel_base * 0.5;
            let half_track = def.track_width * 0.5;

            commands.entity(entity).with_children(|parent| {
                // === MAIN BODY - a box ===
                parent.spawn((
                    Mesh3d(car_assets.body_mesh.clone()),
                    MeshMaterial3d(car_assets.body_material.clone()),
                    // Position body so bottom is near wheel centers
                    Transform::from_xyz(0.0, wheel_radius + 0.1, 0.0),
                ));

                // === FRONT INDICATOR - wedge/box at front ===
                parent.spawn((
                    Mesh3d(car_assets.front_mesh.clone()),
                    MeshMaterial3d(car_assets.front_material.clone()),
                    // Front is negative Z in our coordinate system
                    Transform::from_xyz(0.0, wheel_radius + 0.35, -half_wb - 0.2),
                ));

                // === REAR INDICATOR - red box at back ===
                parent.spawn((
                    Mesh3d(car_assets.rear_mesh.clone()),
                    MeshMaterial3d(car_assets.rear_material.clone()),
                    Transform::from_xyz(0.0, wheel_radius + 0.3, half_wb + 0.15),
                ));

                // === WHEELS - 4 cylinders ===
                // Front-left wheel
                parent.spawn((
                    Mesh3d(car_assets.wheel_mesh.clone()),
                    MeshMaterial3d(car_assets.wheel_material.clone()),
                    Transform::from_xyz(-half_track - 0.1, wheel_radius, -half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));

                // Front-right wheel
                parent.spawn((
                    Mesh3d(car_assets.wheel_mesh.clone()),
                    MeshMaterial3d(car_assets.wheel_material.clone()),
                    Transform::from_xyz(half_track + 0.1, wheel_radius, -half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));

                // Rear-left wheel
                parent.spawn((
                    Mesh3d(car_assets.wheel_mesh.clone()),
                    MeshMaterial3d(car_assets.wheel_material.clone()),
                    Transform::from_xyz(-half_track - 0.1, wheel_radius, half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));

                // Rear-right wheel
                parent.spawn((
                    Mesh3d(car_assets.wheel_mesh.clone()),
                    MeshMaterial3d(car_assets.wheel_material.clone()),
                    Transform::from_xyz(half_track + 0.1, wheel_radius, half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));
            });
            continue;
        }

        let scene: Handle<Scene> = asset_server.load(HOVERBIKE_SCENE);
        let phase = (entity.index_u32() as f32 * 0.73) % std::f32::consts::TAU;

        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                SceneRoot(scene),
                VehicleHoverBob {
                    base_offset: Vec3::new(0.0, HOVERBIKE_VISUAL_Y_OFFSET, 0.0),
                    amplitude: HOVERBIKE_HOVER_AMPLITUDE,
                    frequency: HOVERBIKE_HOVER_FREQUENCY,
                    phase,
                },
                GlobalTransform::default(),
                Transform::from_translation(Vec3::new(0.0, HOVERBIKE_VISUAL_Y_OFFSET, 0.0))
                    .with_rotation(Quat::from_rotation_y(HOVERBIKE_YAW_OFFSET)),
            ));
        });
    }
}
