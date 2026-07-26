//! Chest/storage system - detect nearby chests, show prompt, handle visuals
//!
//! Press E near a chest to open it. The chest UI is integrated into the inventory.

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use lightyear::prelude::client::Connected;
use lightyear::prelude::*;
use shared::components::{LocalPlayer, PlayerPosition};
use shared::items::{
    ChestPosition, ChestStorage, CloseChestRequest, OpenChestRequest, CHEST_RANGE,
};
use shared::protocol::ReliableChannel;

use crate::input::InputState;
use crate::states::GameState;
use crate::ui::inventory::InventoryOpen;
use crate::ui::modal::sync_modal_cursor;

/// Plugin for the chest system
pub struct ChestPlugin;

impl Plugin for ChestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NearbyChest>();
        app.init_resource::<OpenChest>();
        app.add_systems(Startup, setup_chest_visual_assets);

        // Visual systems run in both Playing and Paused states
        app.add_systems(
            Update,
            (spawn_chest_visuals, despawn_chest_visuals)
                .chain()
                .run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))),
        );

        // Chest interaction only in Playing state
        app.add_systems(
            Update,
            (
                detect_nearby_chests,
                show_chest_prompt,
                handle_chest_open_input,
                auto_close_chest_on_distance,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );

        // Cleanup when leaving Playing
        app.add_systems(OnExit(GameState::Playing), cleanup_chest_ui);
        app.add_systems(OnEnter(GameState::MainMenu), cleanup_chest_visuals);
    }
}

/// Resource tracking the nearest chest to the player
#[derive(Resource, Default)]
pub struct NearbyChest {
    pub entity: Option<Entity>,
    pub position: Option<Vec3>,
}

/// Resource tracking which chest is currently open (client-side view)
#[derive(Resource, Default)]
pub struct OpenChest {
    pub entity: Option<Entity>,
}

/// Marker for the chest prompt UI
#[derive(Component)]
pub struct ChestPrompt;

/// Marker for client-side chest 3D visual
#[derive(Component)]
pub struct ChestVisual {
    pub server_entity: Entity,
}

/// Shared mesh/material for chest visuals.
#[derive(Resource)]
pub struct ChestVisualAssets {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

fn setup_chest_visual_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Cuboid::new(0.8, 0.5, 0.5));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.35, 0.2), // Wood brown
        ..default()
    });

    commands.insert_resource(ChestVisualAssets { mesh, material });
}

/// Detect the nearest chest to the local player
fn detect_nearby_chests(
    mut nearby: ResMut<NearbyChest>,
    local_player: Query<&PlayerPosition, With<LocalPlayer>>,
    chests: Query<(Entity, &ChestPosition)>,
    input_state: Res<InputState>,
) {
    // Don't detect while in vehicle or dead
    if input_state.in_vehicle {
        *nearby = NearbyChest::default();
        return;
    }

    let Ok(player_pos) = local_player.single() else {
        *nearby = NearbyChest::default();
        return;
    };

    // Find the nearest chest within range
    let mut closest: Option<(Entity, Vec3, f32)> = None;

    for (entity, pos) in chests.iter() {
        let distance = player_pos.0.distance(pos.0);
        if distance <= CHEST_RANGE && (closest.is_none() || distance < closest.unwrap().2) {
            closest = Some((entity, pos.0, distance));
        }
    }

    if let Some((entity, pos, _)) = closest {
        nearby.entity = Some(entity);
        nearby.position = Some(pos);
    } else {
        *nearby = NearbyChest::default();
    }
}

/// Show "Press E to open chest" prompt when near a chest
fn show_chest_prompt(
    mut commands: Commands,
    nearby: Res<NearbyChest>,
    open_chest: Res<OpenChest>,
    inventory_open: Res<InventoryOpen>,
    existing_prompt: Query<Entity, With<ChestPrompt>>,
) {
    // Remove existing prompt
    for entity in existing_prompt.iter() {
        commands.entity(entity).despawn();
    }

    // Don't show prompt if chest is already open or inventory is open
    if open_chest.entity.is_some() || inventory_open.0 {
        return;
    }

    // Show prompt if near a chest
    if nearby.entity.is_some() {
        commands.spawn((
            ChestPrompt,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(150.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-100.0)),
                ..default()
            },
            Text::new("Press [E] to open chest"),
            TextFont {
                font_size: 20.0,
                ..default()
            },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
        ));
    }
}

/// Handle E key to open/close chest
fn handle_chest_open_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    nearby: Res<NearbyChest>,
    mut open_chest: ResMut<OpenChest>,
    mut inventory_open: ResMut<InventoryOpen>,
    mut input_state: ResMut<InputState>,
    mut client_query: Query<
        &mut MessageSender<OpenChestRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut close_query: Query<
        &mut MessageSender<CloseChestRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) {
        return;
    }

    // If chest is open, close it
    if open_chest.entity.is_some() {
        open_chest.entity = None;
        inventory_open.0 = false;
        input_state.inventory_open = false;
        sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);

        // Send close request to server
        if let Ok(mut sender) = close_query.single_mut() {
            sender.send::<ReliableChannel>(CloseChestRequest);
        }
        return;
    }

    // If near a chest and not currently open, open it
    if let Some(entity) = nearby.entity {
        open_chest.entity = Some(entity);
        inventory_open.0 = true; // Open inventory UI too (shows both)
        input_state.inventory_open = true;
        sync_modal_cursor(true, &input_state, &windows, &mut cursor_opts);

        // Send open request to server
        if let Ok(mut sender) = client_query.single_mut() {
            sender.send::<ReliableChannel>(OpenChestRequest);
        }
    }
}

/// Auto-close chest when player walks away
fn auto_close_chest_on_distance(
    nearby: Res<NearbyChest>,
    mut open_chest: ResMut<OpenChest>,
    mut inventory_open: ResMut<InventoryOpen>,
    mut input_state: ResMut<InputState>,
    mut close_query: Query<
        &mut MessageSender<CloseChestRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    // If chest is open but we're no longer near it
    if let Some(open_entity) = open_chest.entity {
        let still_near = nearby.entity == Some(open_entity);

        if !still_near {
            open_chest.entity = None;
            inventory_open.0 = false;
            input_state.inventory_open = false;
            sync_modal_cursor(false, &input_state, &windows, &mut cursor_opts);

            // Send close request to server
            if let Ok(mut sender) = close_query.single_mut() {
                sender.send::<ReliableChannel>(CloseChestRequest);
            }
        }
    }
}

/// Spawn 3D visuals for chests
fn spawn_chest_visuals(
    mut commands: Commands,
    assets: Option<Res<ChestVisualAssets>>,
    chests: Query<(Entity, &ChestPosition), Without<ChestVisual>>,
    existing_visuals: Query<&ChestVisual>,
) {
    let Some(assets) = assets else { return };
    // Track which server entities already have visuals
    let existing: std::collections::HashSet<Entity> =
        existing_visuals.iter().map(|v| v.server_entity).collect();

    for (server_entity, pos) in chests.iter() {
        if existing.contains(&server_entity) {
            continue;
        }

        // Spawn a box mesh for the chest
        commands.spawn((
            ChestVisual { server_entity },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(pos.0),
        ));
    }
}

/// Despawn visuals for removed chests
fn despawn_chest_visuals(
    mut commands: Commands,
    visuals: Query<(Entity, &ChestVisual)>,
    chests: Query<Entity, With<ChestStorage>>,
) {
    let active_chests: std::collections::HashSet<Entity> = chests.iter().collect();

    for (visual_entity, visual) in visuals.iter() {
        if !active_chests.contains(&visual.server_entity) {
            commands.entity(visual_entity).despawn();
        }
    }
}

/// Cleanup chest UI
fn cleanup_chest_ui(
    mut commands: Commands,
    prompts: Query<Entity, With<ChestPrompt>>,
    mut open_chest: ResMut<OpenChest>,
) {
    for entity in prompts.iter() {
        commands.entity(entity).despawn();
    }
    open_chest.entity = None;
}

/// Cleanup chest visuals
fn cleanup_chest_visuals(mut commands: Commands, visuals: Query<Entity, With<ChestVisual>>) {
    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }
}
