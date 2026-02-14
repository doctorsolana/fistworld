//! hud systems.

use super::*;

/// Spawn the crosshair UI
pub fn spawn_crosshair(mut commands: Commands) {
    // Root container (full screen, centered)
    commands
        .spawn((
            Crosshair,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // Ensure it doesn't block mouse input
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            // Center dot
            parent.spawn((
                CrosshairDot,
                Node {
                    width: Val::Px(4.0),
                    height: Val::Px(4.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                Visibility::Visible,
            ));

            // Top line
            parent.spawn((
                CrosshairLine {
                    direction: CrosshairLineDir::Top,
                },
                Node {
                    width: Val::Px(2.0),
                    height: Val::Px(8.0),
                    position_type: PositionType::Absolute,
                    top: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                Visibility::Visible,
            ));

            // Bottom line
            parent.spawn((
                CrosshairLine {
                    direction: CrosshairLineDir::Bottom,
                },
                Node {
                    width: Val::Px(2.0),
                    height: Val::Px(8.0),
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                Visibility::Visible,
            ));

            // Left line
            parent.spawn((
                CrosshairLine {
                    direction: CrosshairLineDir::Left,
                },
                Node {
                    width: Val::Px(8.0),
                    height: Val::Px(2.0),
                    position_type: PositionType::Absolute,
                    left: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                Visibility::Visible,
            ));

            // Right line
            parent.spawn((
                CrosshairLine {
                    direction: CrosshairLineDir::Right,
                },
                Node {
                    width: Val::Px(8.0),
                    height: Val::Px(2.0),
                    position_type: PositionType::Absolute,
                    right: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                Visibility::Visible,
            ));

            // Sniper-only scope overlay: black mask around a small rounded-square viewport.
            parent
                .spawn((
                    SniperScopeOverlay,
                    Node {
                        width: Val::Px(0.0),
                        height: Val::Px(0.0),
                        position_type: PositionType::Absolute,
                        left: Val::Percent(50.0),
                        top: Val::Percent(50.0),
                        ..default()
                    },
                    Visibility::Hidden,
                    Pickable::IGNORE,
                ))
                .with_children(|scope| {
                    let mask_color = Color::srgba(0.0, 0.0, 0.0, 0.94);
                    let reticle_color = Color::srgba(1.0, 1.0, 1.0, 0.85);

                    // Outside mask
                    scope.spawn((
                        Node {
                            width: Val::Px(SNIPER_SCOPE_MASK_SIZE),
                            height: Val::Px(SNIPER_SCOPE_MASK_SIZE),
                            position_type: PositionType::Absolute,
                            left: Val::Px(-SNIPER_SCOPE_MASK_SIZE * 0.5),
                            bottom: Val::Px(SNIPER_SCOPE_HALF),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(SNIPER_SCOPE_MASK_SIZE),
                            height: Val::Px(SNIPER_SCOPE_MASK_SIZE),
                            position_type: PositionType::Absolute,
                            left: Val::Px(-SNIPER_SCOPE_MASK_SIZE * 0.5),
                            top: Val::Px(SNIPER_SCOPE_HALF),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(SNIPER_SCOPE_MASK_SIZE),
                            height: Val::Px(SNIPER_SCOPE_SIZE),
                            position_type: PositionType::Absolute,
                            right: Val::Px(SNIPER_SCOPE_HALF),
                            top: Val::Px(-SNIPER_SCOPE_HALF),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(SNIPER_SCOPE_MASK_SIZE),
                            height: Val::Px(SNIPER_SCOPE_SIZE),
                            position_type: PositionType::Absolute,
                            left: Val::Px(SNIPER_SCOPE_HALF),
                            top: Val::Px(-SNIPER_SCOPE_HALF),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));

                    // Rounded corners for the square scope viewport.
                    let corner_diameter = SNIPER_SCOPE_CORNER_RADIUS * 2.0;
                    scope.spawn((
                        Node {
                            width: Val::Px(corner_diameter),
                            height: Val::Px(corner_diameter),
                            position_type: PositionType::Absolute,
                            left: Val::Px(-SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            top: Val::Px(-SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            border_radius: BorderRadius::all(Val::Px(SNIPER_SCOPE_CORNER_RADIUS)),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(corner_diameter),
                            height: Val::Px(corner_diameter),
                            position_type: PositionType::Absolute,
                            left: Val::Px(SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            top: Val::Px(-SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            border_radius: BorderRadius::all(Val::Px(SNIPER_SCOPE_CORNER_RADIUS)),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(corner_diameter),
                            height: Val::Px(corner_diameter),
                            position_type: PositionType::Absolute,
                            left: Val::Px(-SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            top: Val::Px(SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            border_radius: BorderRadius::all(Val::Px(SNIPER_SCOPE_CORNER_RADIUS)),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(corner_diameter),
                            height: Val::Px(corner_diameter),
                            position_type: PositionType::Absolute,
                            left: Val::Px(SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            top: Val::Px(SNIPER_SCOPE_HALF - SNIPER_SCOPE_CORNER_RADIUS),
                            border_radius: BorderRadius::all(Val::Px(SNIPER_SCOPE_CORNER_RADIUS)),
                            ..default()
                        },
                        BackgroundColor(mask_color),
                    ));

                    // Scope reticle cross.
                    let reticle_length = SNIPER_SCOPE_SIZE - (SNIPER_SCOPE_RETICLE_MARGIN * 2.0);
                    scope.spawn((
                        Node {
                            width: Val::Px(2.0),
                            height: Val::Px(reticle_length),
                            position_type: PositionType::Absolute,
                            left: Val::Px(-1.0),
                            top: Val::Px(-(reticle_length * 0.5)),
                            ..default()
                        },
                        BackgroundColor(reticle_color),
                    ));
                    scope.spawn((
                        Node {
                            width: Val::Px(reticle_length),
                            height: Val::Px(2.0),
                            position_type: PositionType::Absolute,
                            left: Val::Px(-(reticle_length * 0.5)),
                            top: Val::Px(-1.0),
                            ..default()
                        },
                        BackgroundColor(reticle_color),
                    ));
                });
        });
}

/// Update crosshair visibility based on camera mode
pub fn update_crosshair_visibility(
    mut crosshair_query: Query<&mut Visibility, With<Crosshair>>,
    input_state: Res<crate::input::InputState>,
) {
    for mut visibility in crosshair_query.iter_mut() {
        *visibility = match input_state.camera_mode {
            CameraMode::FirstPerson => Visibility::Visible,
            CameraMode::ThirdPerson => Visibility::Hidden,
        };
    }
}

/// Update crosshair appearance when aiming down sights
#[allow(clippy::type_complexity)]
pub fn update_crosshair_ads(
    mut query_set: ParamSet<(
        Query<
            (&mut Node, &mut BackgroundColor, &mut Visibility),
            (With<CrosshairDot>, Without<SniperScopeOverlay>),
        >,
        Query<
            (
                &CrosshairLine,
                &mut Node,
                &mut BackgroundColor,
                &mut Visibility,
            ),
            (Without<CrosshairDot>, Without<SniperScopeOverlay>),
        >,
        Query<
            &mut Visibility,
            (
                With<SniperScopeOverlay>,
                Without<CrosshairDot>,
                Without<CrosshairLine>,
            ),
        >,
    )>,
    input_state: Res<crate::input::InputState>,
    local_player: Query<&shared::components::EquippedWeapon, With<shared::components::LocalPlayer>>,
) {
    let sniper_ads = input_state.aiming
        && input_state.camera_mode == CameraMode::FirstPerson
        && local_player
            .iter()
            .next()
            .map(|weapon| weapon.weapon_type == shared::weapons::WeaponType::Sniper)
            .unwrap_or(false);

    for mut visibility in query_set.p2().iter_mut() {
        *visibility = if sniper_ads {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    let aiming = input_state.aiming && !sniper_ads;

    // Update center dot - smaller and more visible when aiming
    for (mut node, mut bg, mut visibility) in query_set.p0().iter_mut() {
        *visibility = if sniper_ads {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };

        if aiming {
            node.width = Val::Px(3.0);
            node.height = Val::Px(3.0);
            *bg = BackgroundColor(Color::srgba(1.0, 0.3, 0.3, 1.0)); // Red dot when ADS
        } else {
            node.width = Val::Px(4.0);
            node.height = Val::Px(4.0);
            *bg = BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85));
        }
    }

    // Update crosshair lines - move closer to center when aiming
    for (line, mut node, mut bg, mut visibility) in query_set.p1().iter_mut() {
        *visibility = if sniper_ads {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };

        let (hip_offset, ads_offset) = (14.0, 6.0); // Hip fire vs ADS offset from center
        let offset = if aiming { ads_offset } else { hip_offset };

        match line.direction {
            CrosshairLineDir::Top => {
                node.top = Val::Px(-offset);
                node.height = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
            CrosshairLineDir::Bottom => {
                node.bottom = Val::Px(-offset);
                node.height = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
            CrosshairLineDir::Left => {
                node.left = Val::Px(-offset);
                node.width = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
            CrosshairLineDir::Right => {
                node.right = Val::Px(-offset);
                node.width = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
        }

        // Change color when aiming
        if aiming {
            *bg = BackgroundColor(Color::srgba(1.0, 0.4, 0.4, 0.9)); // Reddish when ADS
        } else {
            *bg = BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7));
        }
    }
}

/// Despawn crosshair and hit markers when leaving gameplay
pub fn despawn_crosshair(
    mut commands: Commands,
    crosshairs: Query<Entity, With<Crosshair>>,
    hit_markers: Query<Entity, With<HitMarker>>,
) {
    for entity in crosshairs.iter() {
        commands.entity(entity).despawn();
    }
    for entity in hit_markers.iter() {
        commands.entity(entity).despawn();
    }
}
