//! assets systems.

use super::*;

/// Load the custom base model + animations and build an `AnimationGraph`.
pub fn setup_player_character_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut animation_graphs: ResMut<Assets<AnimationGraph>>,
) {
    let mut characters: HashMap<PlayerCharacter, CharacterAssets> = HashMap::new();

    // === Base character ===
    let base_scene: Handle<Scene> = asset_server.load("characters/custom/basemodel.glb#Scene0");

    // Custom base model animations
    // Order is assumed to be: 0 = run, 1 = walk, 2 = fall
    let base_run_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/basemodel.glb#Animation0");
    let base_walk_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/basemodel.glb#Animation1");
    let base_fall_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/basemodel.glb#Animation2");

    // Build graph with idle (walk at 0 speed), walk, run, jump, fall
    // Order: idle, walk, run, jump, fall
    let (base_graph, base_nodes) = AnimationGraph::from_clips([
        base_walk_clip.clone(),
        base_walk_clip,
        base_run_clip,
        base_fall_clip.clone(),
        base_fall_clip,
    ]);
    let base_graph = animation_graphs.add(base_graph);

    characters.insert(
        PlayerCharacter::Base,
        CharacterAssets {
            scene: base_scene,
            animation_graph: base_graph,
            idle_node: base_nodes[0],
            walk_node: base_nodes[1],
            walk_back_node: base_nodes[1],
            strafe_left_node: base_nodes[1],
            strafe_right_node: base_nodes[1],
            run_node: base_nodes[2],
            driving_node: base_nodes[0],
            jump_node: base_nodes[3],
            fall_node: base_nodes[4],
            model_scale: BASE_MODEL_SCALE,
            model_yaw_offset: BASE_MODEL_YAW_OFFSET,
        },
    );

    // === Oilman character ===
    let oilman_scene: Handle<Scene> =
        asset_server.load("characters/custom/oilman_animated.glb#Scene0");
    let oilman_tpose_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation0");
    let oilman_idle_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation1");
    let oilman_jog_forward_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation2");
    let oilman_jog_backward_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation3");
    let oilman_jog_strafe_right_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation4");
    let oilman_jog_strafe_left_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation5");
    let oilman_run_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation6");
    let oilman_jump_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation7");
    let oilman_driving_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation8");
    let oilman_look_behind_run_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation9");

    let (oilman_graph, oilman_nodes) = AnimationGraph::from_clips([
        oilman_tpose_clip,
        oilman_idle_clip,
        oilman_jog_forward_clip,
        oilman_jog_backward_clip,
        oilman_jog_strafe_right_clip,
        oilman_jog_strafe_left_clip,
        oilman_run_clip,
        oilman_jump_clip,
        oilman_driving_clip,
        oilman_look_behind_run_clip,
    ]);
    let oilman_graph = animation_graphs.add(oilman_graph);

    characters.insert(
        PlayerCharacter::Oilman,
        CharacterAssets {
            scene: oilman_scene,
            animation_graph: oilman_graph,
            idle_node: oilman_nodes[1],
            walk_node: oilman_nodes[2],
            walk_back_node: oilman_nodes[3],
            strafe_right_node: oilman_nodes[4],
            strafe_left_node: oilman_nodes[5],
            run_node: oilman_nodes[6],
            driving_node: oilman_nodes[8],
            jump_node: oilman_nodes[7],
            fall_node: oilman_nodes[7],
            model_scale: OILMAN_MODEL_SCALE,
            model_yaw_offset: OILMAN_MODEL_YAW_OFFSET,
        },
    );

    commands.insert_resource(PlayerCharacterAssets { characters });

    info!("Loaded player character assets (base + oilman)");
}
