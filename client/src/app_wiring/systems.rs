//! Client system wiring.
//!
//! Default: the FistForce sandbox shooter (walk around, shoot, drive, loot).
//! Set `FISTFORCE_RAIL=1` to boot the rail-tycoon prototype shell instead —
//! the two modes share rendering/terrain/props but need different cameras,
//! input, and HUD, so they are wired mutually exclusively.

use super::*;

pub fn setup_systems(app: &mut App) {
    wire_common_systems(app);
    if super::dev::rail_mode_enabled() {
        wire_rail_systems(app);
    } else {
        wire_fps_systems(app);
    }
}

/// Wiring shared by both the shooter and the rail prototype: window setup,
/// rendering, connection flow, hierarchy fixes, sky, and graphics settings.
fn wire_common_systems(app: &mut App) {
    app.add_systems(
        OnEnter(GameState::Connecting),
        apply_connect_window_settings,
    );

    app.add_systems(Startup, game_systems::setup_rendering);

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
            game_systems::update_day_night_cycle,
            game_systems::update_atmosphere,
            game_systems::apply_graphics_settings,
        )
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(
        Update,
        (
            game_systems::apply_cloud_texture_sampler,
            game_systems::update_cloud_cover,
            game_systems::update_cloud_layers,
            game_systems::spawn_cloud_cards,
            game_systems::update_cloud_cards,
        )
            .chain()
            .run_if(in_state(GameState::Playing)),
    );
}

/// The rail-tycoon prototype shell (FISTFORCE_RAIL=1): RTS camera + build UI.
fn wire_rail_systems(app: &mut App) {

    app.add_systems(
        OnEnter(GameState::Playing),
        (
            game_systems::spawn_world,
            camera_rts::release_cursor_for_rts,
        )
            .chain(),
    );


    app.add_systems(
        Update,
        (
            camera_rts::ensure_commander_camera_controller,
            camera_rts::release_cursor_for_rts,
            camera_rts::update_cursor_terrain_hit,
            camera_rts::update_commander_camera,
        )
            .chain()
            .run_if(in_state(GameState::Playing)),
    );

}

/// The FistForce sandbox shooter (default mode).
fn wire_fps_systems(app: &mut App) {
    // Setup systems (run once at startup - rendering only)
    app.add_systems(
        Startup,
        (
            game_systems::setup_debug_physics_box_assets,
            game_systems::setup_particle_assets,
            game_systems::setup_player_character_assets,
            game_systems::setup_npc_assets,
        ),
    );

    // Spawn world visuals and the perf overlay when entering gameplay
    app.add_systems(
        OnEnter(GameState::Playing),
        (
            game_systems::spawn_world,
            perf_overlay::spawn_debug_overlay,
        ),
    );

    // Cleanup overlays when leaving gameplay
    app.add_systems(
        OnExit(GameState::Playing),
        (
            perf_overlay::despawn_debug_overlay,
        ),
    );

    // Send input to server at fixed tick rate (60 Hz)
    app.add_systems(
        FixedUpdate,
        input::handle_send_input_to_server
            .run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))),
    );

    // Replication-driven spawn/setup must NOT be gated solely to `Playing`.
    app.add_systems(
        Update,
        (
            game_systems::handle_player_spawned,
            game_systems::sync_player_character_models,
            game_systems::handle_npc_spawned,
            game_systems::ensure_local_player_tag,
        )
            .chain()
            .run_if(in_state(GameState::Connecting).or(in_state(GameState::Playing))),
    );

    // Gameplay systems (only when playing) - split into groups to avoid tuple limit
    app.add_systems(
        Update,
        (
            input::handle_keyboard_input,
            input::handle_mouse_input,
            game_systems::apply_cursor_grab,
            game_systems::spawn_debug_physics_box_visuals,
            (
                game_systems::sync_player_transforms,
                game_systems::sync_npc_transforms,
                game_systems::sync_debug_physics_box_transforms,
                camera::update_camera,
            )
                .chain(),
            camera::update_camera_fov,
            game_systems::update_sand_particles,
        )
            .run_if(in_state(GameState::Playing)),
    );

    // Player character visuals/animation
    app.add_systems(
        Update,
        (
            game_systems::setup_player_rig,
            game_systems::update_player_animation,
            game_systems::update_local_player_visibility,
            game_systems::update_player_shadow_culling,
            game_systems::apply_player_shadow_state_to_new_meshes,
        )
            .run_if(in_state(GameState::Playing)),
    );

    // NPC visuals/animation + debug hitboxes
    app.add_systems(
        Update,
        (
            game_systems::setup_npc_rig,
            (
                game_systems::receive_ragdoll_started,
                game_systems::receive_ragdoll_pose_batch,
                game_systems::apply_ragdoll_pose,
            )
                .chain(),
            game_systems::update_npc_visibility,
            game_systems::apply_npc_no_frustum_culling_to_new_meshes,
            game_systems::apply_npc_shadow_state_to_new_meshes,
            game_systems::apply_double_sided_npc_materials,
            game_systems::update_npc_animation,
            game_systems::update_npc_hitbox_debug_gizmos,
            game_systems::update_npc_ragdoll_debug_gizmos,
        )
            .run_if(in_state(GameState::Playing)),
    );


    app.add_systems(
        Update,
        (
            perf_overlay::update_client_perf_snapshot,
        )
            .run_if(in_state(GameState::Playing)),
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
        perf_overlay::emit_client_perf_summary.run_if(in_state(GameState::Playing)),
    );

}
