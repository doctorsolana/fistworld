//! spawn systems.

use super::*;

/// Handle vehicle spawn visuals - hoverbike and test car.
pub fn handle_vehicle_spawned(
    mut commands: Commands,
    visual_assets: Option<Res<VehicleVisualAssets>>,
    new_vehicles: Query<(Entity, &Vehicle, &VehicleState), Added<Vehicle>>,
) {
    let Some(visual_assets) = visual_assets else {
        return;
    };

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

        if matches!(vehicle.vehicle_type, VehicleType::Car | VehicleType::CarV2) {
            commands.entity(entity).insert(NeedsSteamCarRigSetup);
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    SceneRoot(visual_assets.steam_car_scene.clone()),
                    SteamCarSceneRoot,
                    Transform::from_rotation(Quat::from_rotation_y(STEAM_CAR_YAW_OFFSET)),
                    GlobalTransform::default(),
                    Visibility::Inherited,
                    InheritedVisibility::default(),
                ));
            });
            continue;
        }

        let phase = (entity.index_u32() as f32 * 0.73) % std::f32::consts::TAU;

        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                SceneRoot(visual_assets.hoverbike_scene.clone()),
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
