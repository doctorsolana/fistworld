//! assets systems.

use super::*;

pub fn setup_npc_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut animation_graphs: ResMut<Assets<AnimationGraph>>,
) {
    let scene: Handle<Scene> = asset_server.load("characters/custom/oilman_animated.glb#Scene0");

    // Oilman animations (custom rig).
    // Order:
    // 0=TPose, 1=Idle, 2=Jog_Forward, 3=Jog_Backward, 4=Jog_Strafe_Right,
    // 5=Jog_Strafe_Left, 6=Running, 7=Jumping, 8=Driving, 9=Look_Behind_Run
    let tpose_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation0");
    let idle_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation1");
    let jog_forward_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation2");
    let running_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation6");
    let look_behind_run_clip: Handle<AnimationClip> =
        asset_server.load("characters/custom/oilman_animated.glb#Animation9");

    let (graph, nodes) = AnimationGraph::from_clips([
        tpose_clip,
        idle_clip,
        jog_forward_clip,
        running_clip,
        look_behind_run_clip,
    ]);
    let animation_graph = animation_graphs.add(graph);

    commands.insert_resource(NpcAssets {
        scene,
        animation_graph,
        tpose_node: nodes[0],
        idle_node: nodes[1],
        jog_forward_node: nodes[2],
        running_node: nodes[3],
        look_behind_run_node: nodes[4],
    });

    info!("Loaded Oilman NPC assets (scene + 5 animation clips)");
}
