//! layout systems.

use super::*;

pub(super) fn toggle_map(
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<State<GameState>>,
    mut map_open: ResMut<MapOpen>,
    input_state: Res<InputState>,
) {
    if game_state.get() != &GameState::Playing {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyM) && (map_open.0 || !input_state.ui_blocking()) {
        map_open.0 = !map_open.0;
    }
}

pub(super) fn close_map_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut map_open: ResMut<MapOpen>,
) {
    if map_open.0 && keyboard.just_pressed(KeyCode::Escape) {
        map_open.0 = false;
    }
}

pub(super) fn handle_backdrop_click(
    backdrop: Query<&Interaction, (With<MapBackdrop>, Changed<Interaction>)>,
    mut map_open: ResMut<MapOpen>,
) {
    if !map_open.0 {
        return;
    }
    for interaction in backdrop.iter() {
        if *interaction == Interaction::Pressed {
            map_open.0 = false;
        }
    }
}

pub(super) fn sync_map_open_state(
    map_open: Res<MapOpen>,
    mut input_state: ResMut<InputState>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if !map_open.is_changed() {
        return;
    }
    input_state.map_open = map_open.0;
    sync_modal_cursor(map_open.0, &input_state, &windows, &mut cursor_opts);
}

pub(super) fn ensure_map_bounds(
    mut map_config: ResMut<MapUiConfig>,
    terrain: Res<shared::terrain::WorldTerrain>,
) {
    if map_config.bounds.is_some() {
        return;
    }
    map_config.bounds = Some(terrain.generator.active_map_bounds());
}

pub(super) fn spawn_map_ui(
    mut commands: Commands,
    map_open: Res<MapOpen>,
    map_texture: Res<MapTexture>,
    marker_assets: Res<MapMarkerAssets>,
    existing: Query<Entity, With<MapRoot>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !map_open.0 || !existing.is_empty() {
        return;
    }

    let map_handle = map_texture.handle.clone().unwrap_or_else(|| {
        let fallback_image = Image::new(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        images.add(fallback_image)
    });
    let arrow_handle = marker_assets.player_arrow.clone().unwrap_or_else(|| {
        let fallback_image = Image::new(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![255, 255, 255, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        images.add(fallback_image)
    });

    commands
        .spawn((MapRoot, ModalRoot, modal_root_chrome()))
        .with_children(|root| {
            // Backdrop button (click outside map to close)
            root.spawn((MapBackdrop, modal_backdrop_chrome(MODAL_BACKDROP)));

            // Map panel
            root.spawn((
                MapPanel,
                Node {
                    width: Val::Px(MAP_PANEL_SIZE + 24.0),
                    height: Val::Px(MAP_PANEL_SIZE + 64.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(Val::Px(2.0)),
                    padding: UiRect::all(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(MENU_BACKGROUND),
                BorderColor::from(PLATE_RULE),
                bevy::ui::FocusPolicy::Block,
                Pickable::default(),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("WORLD MAP"),
                    TextFont {
                        font_size: FontSize::Px(18.0),
                        ..default()
                    },
                    TextColor(INK_INVERSE),
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                ));

                panel
                    .spawn((
                        Node {
                            width: Val::Px(MAP_PANEL_SIZE),
                            height: Val::Px(MAP_PANEL_SIZE),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::from(PLATE_RULE),
                    ))
                    .with_children(|map_container| {
                        map_container.spawn((
                            MapImage,
                            Node {
                                width: Val::Px(MAP_PANEL_SIZE),
                                height: Val::Px(MAP_PANEL_SIZE),
                                ..default()
                            },
                            ImageNode::new(map_handle),
                        ));

                        map_container
                            .spawn((
                                MapMarkerLayer,
                                Node {
                                    width: Val::Px(MAP_PANEL_SIZE),
                                    height: Val::Px(MAP_PANEL_SIZE),
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(0.0),
                                    top: Val::Px(0.0),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                            ))
                            .with_children(|markers| {
                                // Four tiny UI strips connect the actual
                                // screen-corner projections. They move without
                                // rebuilding the map image or a mesh.
                                for edge in 0..4 {
                                    markers.spawn((
                                        MapViewportEdge(edge),
                                        Node {
                                            display: Display::None,
                                            width: Val::Px(1.0),
                                            height: Val::Px(VIEWPORT_EDGE_THICKNESS),
                                            position_type: PositionType::Absolute,
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgba(0.965, 0.957, 0.933, 0.88)),
                                        UiTransform::default(),
                                    ));
                                }
                                markers.spawn((
                                    MapPlayerMarker,
                                    Node {
                                        display: Display::None,
                                        width: Val::Px(PLAYER_ARROW_SIZE),
                                        height: Val::Px(PLAYER_ARROW_SIZE),
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(
                                            MAP_PANEL_SIZE * 0.5 - PLAYER_ARROW_SIZE * 0.5,
                                        ),
                                        top: Val::Px(
                                            MAP_PANEL_SIZE * 0.5 - PLAYER_ARROW_SIZE * 0.5,
                                        ),
                                        ..default()
                                    },
                                    ImageNode::new(arrow_handle),
                                    UiTransform::default(),
                                ));
                            });
                    });
            });
        });
}

pub(super) fn despawn_map_ui(
    mut commands: Commands,
    map_open: Res<MapOpen>,
    roots: Query<Entity, With<MapRoot>>,
) {
    if map_open.0 {
        return;
    }
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}
