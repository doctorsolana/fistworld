//! Shared FistWorld startup presentation; connection and identity stay with their owners.

mod artwork;
mod backdrop;
mod input;
mod motion;
pub(crate) mod widgets;

pub(crate) use artwork::StartupArtwork;
pub(crate) use motion::LoadingDiamond;

use crate::states::GameState;
use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<StartupArtwork>();
    app.add_systems(
        PostUpdate,
        input::sync_visible_controls
            .after(bevy::ui::UiSystems::Layout)
            .run_if(not(in_state(GameState::Playing))),
    );
    backdrop::install(app);
    app.add_systems(
        Update,
        motion::animate_loading.run_if(not(in_state(GameState::Playing))),
    )
    .add_systems(
        Update,
        position_wordmark.run_if(not(in_state(GameState::Playing))),
    )
    .add_systems(
        Update,
        resize_startup.run_if(not(in_state(GameState::Playing))),
    )
    .add_systems(OnEnter(GameState::Connecting), spawn_connecting)
    .add_systems(OnExit(GameState::Connecting), despawn_connecting);
}

/// Keep the whole design canvas readable when the launcher window is resized.
/// Playing restores the separate graphics-settings owner on connection.
fn resize_startup(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut ui_scale: ResMut<bevy::ui::UiScale>,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = capture.as_ref().map_or(
        Vec2::new(
            window.physical_width() as f32,
            window.physical_height() as f32,
        ),
        |config| Vec2::new(config.resolution[0] as f32, config.resolution[1] as f32),
    );
    let scale = (size.x / 1600.0).min(size.y / 900.0).clamp(0.5, 3.0) / window.scale_factor();
    if (ui_scale.0 - scale).abs() > 0.001 {
        ui_scale.0 = scale;
    }
}

fn position_wordmark(
    state: Res<State<GameState>>,
    phase: Res<crate::ui::name_entry::NameEntryPhase>,
    mut marks: Query<&mut Node, With<widgets::CompactWordmark>>,
) {
    let loading = *state.get() == GameState::Connecting || phase.is_busy();
    let top = Val::Vh(if loading { 13.0 } else { 7.5 });
    for mut node in &mut marks {
        if node.top != top {
            node.top = top;
        }
    }
}

#[derive(Component)]
struct ConnectingRoot;

fn spawn_connecting(mut commands: Commands, art: Res<StartupArtwork>) {
    commands
        .spawn((ConnectingRoot, widgets::screen()))
        .with_children(|root| {
            widgets::small_wordmark(root, &art);
            widgets::loading_panel(root, &art, "Connecting…", ());
            root.spawn((Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(50.0),
                margin: UiRect::top(Val::Px(196.0)),
                ..default()
            },))
                .with_children(|below| {
                    widgets::action_button(
                        below,
                        &art,
                        "CANCEL",
                        crate::render::systems::ConnectionCancel,
                        Val::Px(180.0),
                        46.0,
                        false,
                    );
                });
        });
}

fn despawn_connecting(mut commands: Commands, roots: Query<Entity, With<ConnectingRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}
