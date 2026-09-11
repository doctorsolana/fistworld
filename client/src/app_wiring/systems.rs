//! Client system wiring.

use super::*;

pub fn setup_systems(app: &mut App) {
    crate::capture::install_session_capture(app);
    wire_common_systems(app);
    wire_game_systems(app);
}

/// Wiring shared by both the shooter and the rail prototype: window setup,
/// rendering, connection flow, hierarchy fixes, sky, and graphics settings.
fn wire_common_systems(app: &mut App) {
    app.add_systems(
        OnEnter(GameState::Connecting),
        apply_connect_window_settings,
    );

    app.add_systems(Startup, game_systems::setup_rendering);
    if std::env::var_os("FISTWORLD_ARMY_SCENARIO").is_some()
        || std::env::var_os("FISTWORLD_SESSION_CAPTURE_DIR").is_some()
    {
        app.add_systems(PostStartup, crate::capture::setup_capture_presentation);
        app.add_systems(
            PostUpdate,
            crate::capture::sync_capture_presentation.before(bevy::camera::CameraUpdateSystems),
        );
    }

    // Keep the offscreen scene target sized to window * render_scale in every
    // state (resizes happen in menus and on fullscreen transitions too).
    app.add_systems(Update, game_systems::sync_scene_render_target);

    // FISTFORCE_AUTOCONNECT: unattended connect + name submission for perf
    // runs and automated verification.
    if super::dev::autoconnect_name().is_some() {
        app.add_systems(
            Update,
            super::dev::autoconnect_from_main_menu.run_if(in_state(GameState::MainMenu)),
        );
        app.add_systems(
            Update,
            super::dev::autoconnect_submit_name.run_if(in_state(GameState::Connected)),
        );
    }

    // Ensure we clean up visuals when entering menu
    app.add_systems(
        OnEnter(GameState::MainMenu),
        game_systems::cleanup_enter_main_menu,
    );

    app.add_systems(
        Update,
        (
            crate::capture::drive_army_input,
            crate::capture::drive_management_input
                .before(crate::ui::encyclopedia::army::handle_army_buttons),
            crate::capture::drive_battle_input,
            crate::capture::drive_siege_input.before(crate::siege::SiegeInputSet),
        )
            .before(crate::camera_rts::update_cursor_terrain_hit)
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(
        Update,
        (
            crate::capture::drive_army_capture,
            crate::capture::drive_tavern_review,
            crate::capture::drive_management_capture
                .after(crate::ui::encyclopedia::army::sync_army_panel),
            crate::capture::drive_battle_capture
                .after(crate::animation_clock::AnimationClockUpdate),
            crate::capture::drive_siege_capture,
        )
            .after(crate::selection::SelectionGestureSet)
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        (
            crate::capture::drive_battle_ray,
            crate::capture::drive_siege_ray,
        )
            .after(crate::camera_rts::update_cursor_terrain_hit)
            .before(crate::selection::SelectionGestureSet)
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        crate::capture::frame_siege_flight
            .after(crate::camera_rts::update_commander_camera)
            .before(crate::capture::drive_siege_capture)
            .run_if(in_state(GameState::Playing)),
    );

    // Connection systems
    app.add_systems(
        OnEnter(GameState::Connecting),
        game_systems::handle_start_connection,
    );
    app.add_systems(
        Update,
        game_systems::update_connection_status.run_if(in_state(GameState::Connecting)),
    );

    // Keep hierarchy transform/visibility parents consistent to avoid B0004 warning spam.
    app.add_systems(
        PostUpdate,
        (
            render::hierarchy_fix::ensure_hierarchy_parent_audit,
            render::hierarchy_fix::ensure_hierarchy_visibility_parents,
        )
            .chain()
            .before(bevy::transform::TransformSystems::Propagate)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
    app.add_observer(render::hierarchy_fix::ensure_parent_components_on_child_add);
    app.add_observer(render::hierarchy_fix::update_b0004_global_trace);

    // Sky, day/night, and graphics settings application.
    app.add_systems(
        Update,
        (
            // Sole DistanceFog writer; folds in the map-view fade, so it must
            // see this frame's MapViewBlend.
            game_systems::update_day_night_cycle.after(terrain::map_view::update_map_view_state),
            game_systems::update_atmosphere,
            (
                game_systems::tick_display_change_confirmation,
                game_systems::apply_graphics_settings,
                game_systems::save_graphics_settings,
            )
                .chain(),
            game_systems::sync_atmosphere_enabled,
            game_systems::sync_shadow_cascades_to_zoom,
        )
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        (
            crate::capture::drive_live_lab_capture,
            crate::capture::drive_wildlife_capture,
        )
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(
        Update,
        crate::capture::drive_live_voyage_capture
            .after(crate::boat::drive_opening_cinematic)
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(
        Update,
        (
            game_systems::update_cloud_cover,
            game_systems::spawn_cloud_plane,
            game_systems::update_cloud_plane,
        )
            .chain()
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(
        Update,
        // After the day/night cycle so the sun transform the shadow
        // projection reads is current-frame.
        game_systems::sync_cloud_shadow_params
            .after(game_systems::update_day_night_cycle)
            .run_if(in_state(GameState::Playing)),
    );
}

/// Gameplay wiring: top-down commander camera + world visuals.
fn wire_game_systems(app: &mut App) {
    app.init_resource::<camera_rts::PendingCommanderView>();
    // Spawn world visuals and the perf overlay when entering gameplay
    app.add_systems(
        OnEnter(GameState::Playing),
        (
            game_systems::spawn_world,
            perf_overlay::spawn_debug_overlay,
            camera_rts::release_cursor_for_rts,
        ),
    );

    // Top-down commander camera: WASD pan, RMB orbit, wheel zoom, cursor->terrain pick.
    app.add_systems(
        Update,
        (
            camera_rts::ensure_commander_camera_controller,
            camera_rts::update_commander_camera,
            // Pick from the camera transform that will actually be rendered
            // this frame. Computing the ray before smoothing the camera made
            // clicks lag a frame behind during pan, orbit and zoom.
            camera_rts::update_cursor_terrain_hit,
            // Map view reads camera zoom, so it must follow the camera update.
            terrain::map_view::update_map_view_state,
        )
            .chain()
            // Every terrain streaming decision in a frame must read ONE camera
            // pose. Left unordered, this chain could interleave into the middle
            // of the terrain set, moving the anchor between the desired-set
            // computation and the far-hole commit — which stamped the hole one
            // chunk row ahead of the validated square during fast pans.
            .before(crate::terrain::TerrainUpdateSet)
            .run_if(in_state(GameState::Playing)),
    );

    // Stream the commander view to the server; it anchors collider streaming on this.
    app.add_systems(
        FixedUpdate,
        camera_rts::send_commander_view.run_if(in_state(GameState::Playing)),
    );

    // Cleanup overlays when leaving gameplay
    app.add_systems(
        OnExit(GameState::Playing),
        (perf_overlay::despawn_debug_overlay,),
    );

    // Replication-driven spawn/setup must NOT be gated solely to `Playing`.

    // Player character visuals/animation

    // NPC visuals/animation + debug hitboxes

    app.add_systems(
        Update,
        (perf_overlay::update_client_perf_snapshot,).run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        perf_overlay::handle_toggle_perf_overlay.run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        perf_overlay::handle_toggle_debug_mode.run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        perf_overlay::update_debug_overlay
            .run_if(in_state(GameState::Playing))
            .run_if(bevy::time::common_conditions::on_timer(
                std::time::Duration::from_millis(125),
            )),
    );

    app.add_systems(
        Update,
        perf_overlay::update_perf_drop_monitor
            .run_if(in_state(GameState::Playing))
            .run_if(bevy::time::common_conditions::on_timer(
                std::time::Duration::from_millis(500),
            )),
    );

    app.add_systems(
        Update,
        (
            perf_overlay::emit_client_perf_summary.run_if(in_state(GameState::Playing)),
            perf_overlay::exit_after_deadline,
            perf_overlay::log_mesh_census.after(crate::terrain::TerrainUpdateSet),
            perf_overlay::log_changed_mesh_archetypes,
        ),
    );
    app.add_systems(Last, perf_overlay::limit_frame_rate);
}
