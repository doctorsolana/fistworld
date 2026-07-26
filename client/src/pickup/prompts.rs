//! prompts systems.

use super::*;

/// Detect the nearest vehicle to the local player (for mounting)
pub(super) fn detect_nearby_vehicles(
    mut nearby: ResMut<NearbyVehicle>,
    time: Res<Time>,
    local_player: Query<&PlayerPosition, With<LocalPlayer>>,
    vehicles: Query<(Entity, &Vehicle, &VehicleState, &VehicleDriver)>,
    input_state: Res<InputState>,
    mut elapsed: Local<f32>,
    mut initialized: Local<bool>,
) {
    // Don't show prompt if already in vehicle or dead
    if input_state.in_vehicle {
        *nearby = NearbyVehicle::default();
        return;
    }

    let Ok(player_pos) = local_player.single() else {
        *nearby = NearbyVehicle::default();
        return;
    };

    const NEARBY_SCAN_INTERVAL_SECS: f32 = 0.1;
    if *initialized {
        *elapsed += time.delta_secs();
        if *elapsed < NEARBY_SCAN_INTERVAL_SECS {
            return;
        }
    } else {
        *initialized = true;
    }
    *elapsed = 0.0;

    // Find the nearest unoccupied vehicle within range
    let mut closest: Option<(Entity, VehicleType, f32)> = None;
    let interaction_range_sq = VEHICLE_INTERACTION_RANGE * VEHICLE_INTERACTION_RANGE;

    for (entity, vehicle, state, driver) in vehicles.iter() {
        // Skip if vehicle already has a driver
        if driver.driver_id.is_some() {
            continue;
        }

        let distance_sq = player_pos.0.distance_squared(state.position);
        if distance_sq <= interaction_range_sq
            && (closest.is_none() || distance_sq < closest.as_ref().unwrap().2)
        {
            closest = Some((entity, vehicle.vehicle_type, distance_sq));
        }
    }

    if let Some((_entity, vehicle_type, _distance)) = closest {
        *nearby = NearbyVehicle {
            vehicle_type: Some(vehicle_type),
        };
    } else {
        *nearby = NearbyVehicle::default();
    }
}

/// Show vehicle interaction prompt when near an unoccupied vehicle
pub(super) fn show_vehicle_prompt(
    mut commands: Commands,
    nearby: Res<NearbyVehicle>,
    existing_prompt: Query<Entity, With<VehiclePrompt>>,
    mut text_query: Query<&mut Text, With<VehiclePrompt>>,
) {
    // If we have a nearby vehicle, show/update prompt
    if let Some(vehicle_type) = nearby.vehicle_type {
        let vehicle_name = match vehicle_type {
            VehicleType::Motorbike => "bike",
            VehicleType::Car => "steam car",
            VehicleType::CarV2 => "steam car v2",
        };
        let prompt_text = format!("Press E to get on {}", vehicle_name);

        // Update existing prompt or spawn new one
        if let Ok(mut text) = text_query.single_mut() {
            if **text != prompt_text {
                **text = prompt_text;
            }
        } else if existing_prompt.is_empty() {
            // Spawn new prompt (positioned above the pickup prompt location)
            commands.spawn((
                VehiclePrompt,
                Text::new(prompt_text),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 0.9, 0.5, 0.95)), // Slightly yellow tint
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Percent(28.0), // Slightly above pickup prompt
                    left: Val::Percent(50.0),
                    ..default()
                },
                TextLayout::new_with_justify(Justify::Center),
            ));
        }
    } else {
        // No vehicle nearby, despawn prompt
        for entity in existing_prompt.iter() {
            commands.entity(entity).despawn();
        }
    }
}

/// Detect the nearest ground item to the local player
pub(super) fn detect_nearby_items(
    mut nearby: ResMut<NearbyItem>,
    time: Res<Time>,
    local_player: Query<&PlayerPosition, With<LocalPlayer>>,
    ground_items: Query<(Entity, &GroundItem, &GroundItemPosition)>,
    input_state: Res<InputState>,
    mut elapsed: Local<f32>,
    mut initialized: Local<bool>,
) {
    // Don't detect while in vehicle or dead
    if input_state.in_vehicle {
        *nearby = NearbyItem::default();
        return;
    }

    let Ok(player_pos) = local_player.single() else {
        *nearby = NearbyItem::default();
        return;
    };

    const NEARBY_SCAN_INTERVAL_SECS: f32 = 0.1;
    if *initialized {
        *elapsed += time.delta_secs();
        if *elapsed < NEARBY_SCAN_INTERVAL_SECS {
            return;
        }
    } else {
        *initialized = true;
    }
    *elapsed = 0.0;

    // Find the nearest item within pickup range
    let mut closest: Option<(Entity, &GroundItem, f32)> = None;
    let pickup_range_sq = PICKUP_RANGE * PICKUP_RANGE;

    for (entity, item, pos) in ground_items.iter() {
        let distance_sq = player_pos.0.distance_squared(pos.0);
        if distance_sq <= pickup_range_sq
            && (closest.is_none() || distance_sq < closest.as_ref().unwrap().2)
        {
            closest = Some((entity, item, distance_sq));
        }
    }

    if let Some((entity, item, _distance)) = closest {
        *nearby = NearbyItem {
            entity: Some(entity),
            item_type: Some(item.item_type),
            quantity: Some(item.quantity),
        };
    } else {
        *nearby = NearbyItem::default();
    }
}

/// Show pickup prompt when near an item
pub(super) fn show_pickup_prompt(
    mut commands: Commands,
    nearby: Res<NearbyItem>,
    existing_prompt: Query<Entity, With<PickupPrompt>>,
    mut text_query: Query<&mut Text, With<PickupPrompt>>,
) {
    // If we have a nearby item, show/update prompt
    if let (Some(item_type), Some(quantity)) = (nearby.item_type, nearby.quantity) {
        let prompt_text = format!(
            "Press E to pick up {}x {}",
            quantity,
            item_type.display_name()
        );

        // Update existing prompt or spawn new one
        if let Ok(mut text) = text_query.single_mut() {
            if **text != prompt_text {
                **text = prompt_text;
            }
        } else if existing_prompt.is_empty() {
            // Spawn new prompt
            commands.spawn((
                PickupPrompt,
                Text::new(prompt_text),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Percent(25.0),
                    left: Val::Percent(50.0),
                    ..default()
                },
                // Center the text
                TextLayout::new_with_justify(Justify::Center),
            ));
        }
    } else {
        // No item nearby, despawn prompt
        for entity in existing_prompt.iter() {
            commands.entity(entity).despawn();
        }
    }
}

/// Handle E key to pick up items
pub(super) fn handle_pickup_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    nearby: Res<NearbyItem>,
    mut client_query: Query<
        &mut MessageSender<PickupRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) {
        return;
    }

    // Only request pickup if there's a nearby item
    if nearby.entity.is_none() {
        return;
    };

    // Send pickup request to server - server will find nearest item
    if let Ok(mut sender) = client_query.single_mut() {
        sender.send::<ReliableChannel>(PickupRequest);
        info!("Requesting pickup of nearby item");
    }
}

/// Cleanup pickup and vehicle prompt UI when leaving playing state
pub(super) fn cleanup_pickup_ui(
    mut commands: Commands,
    pickup_prompts: Query<Entity, With<PickupPrompt>>,
    vehicle_prompts: Query<Entity, With<VehiclePrompt>>,
) {
    for entity in pickup_prompts.iter() {
        commands.entity(entity).despawn();
    }
    for entity in vehicle_prompts.iter() {
        commands.entity(entity).despawn();
    }
}
