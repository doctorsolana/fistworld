//! systems systems.

use super::*;

pub fn setup_systems(app: &mut App) {
    app.add_systems(
        OnEnter(GameState::Connecting),
        apply_connect_window_settings,
    );

    // Setup systems (run once at startup - rendering only)
    app.add_systems(
        Startup,
        (
            game_systems::setup_rendering,
            game_systems::setup_particle_assets,
            game_systems::setup_vehicle_visual_assets,
            weapons::setup_weapon_visual_assets,
            weapons::setup_weapon_audio_assets,
            weapon_view::setup_weapon_model_assets,
            game_systems::setup_player_character_assets,
            game_systems::setup_npc_assets,
        ),
    );

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

    // Spawn world visuals, HUD, crosshair, and death screen when entering gameplay
    app.add_systems(
        OnEnter(GameState::Playing),
        (
            game_systems::spawn_world,
            crosshair::spawn_crosshair,
            crosshair::spawn_death_screen,
            weapon_view::spawn_weapon_hud,
            weapons::spawn_debug_overlay,
            weapons::cleanup_shoot_input_suppress,
        ),
    );

    // Cleanup HUD, crosshair, and death screen when leaving gameplay
    app.add_systems(
        OnExit(GameState::Playing),
        (
            crosshair::despawn_crosshair,
            crosshair::despawn_death_screen,
            weapon_view::despawn_weapon_hud,
            weapon_view::despawn_third_person_weapon,
            weapon_view::despawn_remote_third_person_weapons,
            weapons::reset_projectile_indices,
            weapons::despawn_debug_overlay,
        ),
    );

    // Send input to server at fixed tick rate (60 Hz)
    app.add_systems(
        FixedUpdate,
        input::handle_send_input_to_server
            .run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))),
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

    // Replication-driven spawn/setup must NOT be gated solely to `Playing`.
    app.add_systems(
        Update,
        (
            game_systems::handle_player_spawned,
            game_systems::sync_player_character_models,
            game_systems::handle_npc_spawned,
            game_systems::handle_vehicle_spawned,
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
            input::update_vehicle_state,
            input::handle_mouse_input,
            input::update_death_state,
            game_systems::apply_cursor_grab,
            (
                game_systems::sync_vehicle_transforms,
                game_systems::sync_player_transforms,
                game_systems::sync_npc_transforms,
                camera::update_camera,
            )
                .chain(),
            game_systems::update_vehicle_hover,
            game_systems::update_vehicle_shadow_culling,
            game_systems::apply_vehicle_shadow_state_to_new_meshes,
            camera::update_camera_fov,
            camera::update_sniper_fisheye,
            game_systems::spawn_sand_particles,
            game_systems::update_sand_particles,
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
            game_systems::update_npc_visibility,
            game_systems::apply_npc_no_frustum_culling_to_new_meshes,
            game_systems::apply_npc_shadow_state_to_new_meshes,
            game_systems::apply_double_sided_npc_materials,
            game_systems::update_npc_animation,
            game_systems::update_npc_hitbox_debug_gizmos,
        )
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        (
            crosshair::update_crosshair_visibility,
            crosshair::update_crosshair_ads,
            crosshair::update_hit_markers,
            crosshair::update_death_screen,
        )
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        (
            weapons::sync_player_owner_index,
            weapons::sync_remote_muzzle_index
                .after(weapon_view::update_remote_third_person_weapons),
            weapons::update_client_perf_snapshot,
            weapons::update_weapon_warmup_queue,
            weapons::spawn_weapon_warmups,
            weapons::cleanup_weapon_warmups,
            weapons::handle_shoot_input,
            weapons::handle_reload_input,
            weapons::handle_weapon_sounds,
            weapons::handle_bullet_spawned,
            weapons::update_recoil_recovery,
        )
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        (
            weapons::update_bullet_visuals,
            weapons::update_local_tracers,
            weapons::handle_bullet_impacts,
        )
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        (
            weapons::update_impact_markers,
            weapons::update_blood_bursts,
            weapons::update_blood_droplets,
            weapons::update_blood_splash_rings,
            weapons::update_blood_ground_splats,
            weapons::update_muzzle_smoke,
            weapons::update_muzzle_flash,
            weapons::handle_hit_confirms,
        )
            .run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        weapons::handle_toggle_perf_overlay.run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        weapons::handle_toggle_debug_mode.run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        weapons::update_trajectory_debug_gizmos.run_if(in_state(GameState::Playing)),
    );

    app.add_systems(
        Update,
        weapons::update_debug_overlay
            .run_if(in_state(GameState::Playing))
            .run_if(bevy::time::common_conditions::on_timer(
                std::time::Duration::from_millis(125),
            )),
    );

    app.add_systems(
        Update,
        weapons::update_perf_drop_monitor
            .run_if(in_state(GameState::Playing))
            .run_if(bevy::time::common_conditions::on_timer(
                std::time::Duration::from_millis(500),
            )),
    );

    app.add_systems(
        Update,
        weapons::emit_client_perf_summary.run_if(in_state(GameState::Playing)),
    );

    // Weapon view systems (3D models and HUD)
    app.add_systems(
        Update,
        (
            weapon_view::handle_weapon_switch,
            weapon_view::update_weapon_hud,
            weapon_view::update_first_person_weapon,
            weapon_view::update_third_person_weapon,
            weapon_view::update_remote_third_person_weapons,
            weapon_view::update_weapon_animation
                .after(weapons::handle_shoot_input)
                .after(weapons::handle_reload_input),
        )
            .after(game_systems::handle_player_spawned)
            .after(game_systems::sync_player_character_models)
            .run_if(in_state(GameState::Playing)),
    );
}
