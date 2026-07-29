//! Hero rendering + control: the client half of the embodied character.
//!
//! The server owns hero position/rotation (see server/src/player/hero.rs);
//! this module attaches the voxel_boy glb to replicated Hero entities,
//! dresses them from their replicated [`HeroOutfit`], drives the walk
//! animation from observed velocity, smooths the streamed transform, and
//! turns clicks into spawn/move commands.

pub mod control;

use bevy::gltf::Gltf;
use bevy::prelude::*;
use shared::components::{Hero, HeroOutfit, PlayerPosition, PlayerRotation};
use shared::player::HERO_MOVE_SPEED;

use crate::states::GameState;

/// Wardrobe node count in voxel_boy.glb: 6 hair + 3 shorts + 1 tshirt. The
/// dresser waits until the instantiated scene exposes all of them before
/// applying the outfit, so a half-loaded hero never flashes the full closet.
const WARDROBE_NODE_COUNT: usize = 10;

const HERO_GLB: &str = "characters/voxel_boy.glb";

pub struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeroAssets>();
        app.init_resource::<control::SelectedOutfit>();
        app.init_resource::<control::HeroSpawnArm>();
        app.add_systems(
            Update,
            (
                attach_hero_visuals,
                dress_heroes,
                setup_hero_animation,
                sync_hero_transforms,
                drive_hero_walk_animation,
                control::handle_world_clicks,
                control::auto_spawn_hero,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Lazily-loaded voxel_boy handles, shared by every hero instance.
#[derive(Resource, Default)]
pub struct HeroAssets {
    scene: Option<Handle<WorldAsset>>,
    gltf: Option<Handle<Gltf>>,
    /// Built once the glTF is loaded; every hero shares the same graph.
    graph: Option<(Handle<AnimationGraph>, AnimationNodeIndex)>,
}

/// Client-side smoothing/animation state on the hero root.
#[derive(Component)]
pub struct HeroVisual {
    /// Smoothed world-space speed (m/s) of the VISUAL transform — drives the
    /// walk animation, so feet track what the eye actually sees.
    speed: f32,
}

/// Marks a hero whose wardrobe matches its replicated outfit.
#[derive(Component)]
struct HeroDressed;

/// The direct child carrying the character scene. Spawned hidden; revealed
/// only once the wardrobe is applied, so the full closet never flashes for
/// a frame (scene instantiation runs after Update, beating the dresser).
#[derive(Component)]
struct HeroSceneRoot;

/// Link from the hero root to the AnimationPlayer entity inside its scene.
#[derive(Component)]
struct HeroAnim {
    player: Entity,
    walk: AnimationNodeIndex,
}

/// Give newly replicated heroes a transform + the character scene.
///
/// Polls `Without<HeroVisual>` instead of `Added<Hero>`: replication may
/// deliver `Hero` and `PlayerPosition` in different batches, and a one-shot
/// `Added` trigger that missed the position would leave the hero invisible
/// forever.
fn attach_hero_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<HeroAssets>,
    heroes: Query<
        (Entity, &PlayerPosition, Option<&PlayerRotation>),
        (With<Hero>, Without<HeroVisual>),
    >,
) {
    for (entity, pos, rot) in heroes.iter() {
        let scene = assets
            .scene
            .get_or_insert_with(|| asset_server.load(format!("{HERO_GLB}#Scene0")))
            .clone();
        assets
            .gltf
            .get_or_insert_with(|| asset_server.load(HERO_GLB));

        let yaw = rot.map(|r| r.0).unwrap_or(0.0);
        commands
            .entity(entity)
            .insert((
                Transform::from_translation(pos.0).with_rotation(Quat::from_rotation_y(yaw)),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                HeroVisual { speed: 0.0 },
            ))
            .with_children(|root| {
                root.spawn((
                    HeroSceneRoot,
                    Transform::IDENTITY,
                    GlobalTransform::default(),
                    Visibility::Hidden,
                    InheritedVisibility::default(),
                    WorldAssetRoot(scene),
                ));
            });
        info!("Hero visuals attached for {entity:?}");
    }
}

/// Toggle wardrobe node visibility to match the replicated outfit.
///
/// Runs until the async scene instantiation exposes every wardrobe node,
/// then stamps [`HeroDressed`]. Outfit changes re-run it via the marker
/// removal below.
fn dress_heroes(
    mut commands: Commands,
    heroes: Query<(Entity, &HeroOutfit), (With<HeroVisual>, Without<HeroDressed>)>,
    changed: Query<Entity, (With<HeroDressed>, Changed<HeroOutfit>)>,
    children_q: Query<&Children>,
    mut named: Query<(&Name, &mut Visibility)>,
    mut scene_roots: Query<&mut Visibility, (With<HeroSceneRoot>, Without<Name>)>,
) {
    for entity in changed.iter() {
        commands.entity(entity).remove::<HeroDressed>();
    }

    for (hero, outfit) in heroes.iter() {
        let mut found = 0usize;
        let mut stack = vec![hero];
        let mut edits: Vec<(Entity, bool)> = Vec::new();
        while let Some(node) = stack.pop() {
            if let Ok((name, _)) = named.get(node) {
                let name = name.as_str();
                // EXACT node names only: glTF primitive children carry
                // suffixed names ("Hair_Afro.0"); hiding those explicitly
                // would override their shown parent and bald the hero.
                let is_wardrobe = name == "Tshirt"
                    || shared::components::HERO_HAIR_NODES.contains(&name)
                    || shared::components::HERO_SHORTS_NODES.contains(&name);
                if is_wardrobe {
                    found += 1;
                    edits.push((node, outfit.hides_node(name)));
                }
            }
            if let Ok(children) = children_q.get(node) {
                stack.extend(children.iter());
            }
        }
        // Scene still instantiating: retry next frame rather than dressing half
        // a closet.
        if found < WARDROBE_NODE_COUNT {
            continue;
        }
        for (node, hide) in edits {
            if let Ok((_, mut visibility)) = named.get_mut(node) {
                let target = if hide {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
                if *visibility != target {
                    *visibility = target;
                }
            }
        }
        // Dressed: reveal the (hidden-at-spawn) scene.
        if let Ok(children) = children_q.get(hero) {
            for child in children.iter() {
                if let Ok(mut visibility) = scene_roots.get_mut(child) {
                    if *visibility != Visibility::Inherited {
                        *visibility = Visibility::Inherited;
                    }
                }
            }
        }
        commands.entity(hero).insert(HeroDressed);
    }
}

/// Wire the scene's [`AnimationPlayer`] to the shared walk graph.
///
/// Polls instead of `Added<AnimationPlayer>`: the graph can only be built
/// once the whole glTF asset (not just the scene) has loaded, and a one-shot
/// trigger would miss players that appear before that.
fn setup_hero_animation(
    mut commands: Commands,
    mut assets: ResMut<HeroAssets>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<(Entity, &mut AnimationPlayer), Without<AnimationGraphHandle>>,
    parents: Query<&ChildOf>,
    heroes: Query<(), With<Hero>>,
    hero_roots: Query<Entity, (With<Hero>, Without<HeroAnim>)>,
) {
    if players.is_empty() {
        return;
    }
    // Build the shared graph once the glTF exposes the walk clip.
    if assets.graph.is_none() {
        let Some(gltf) = assets.gltf.as_ref().and_then(|handle| gltfs.get(handle)) else {
            return;
        };
        let Some(walk_clip) = gltf.named_animations.get("walk") else {
            warn!("voxel_boy.glb has no 'walk' animation");
            return;
        };
        let (graph, node) = AnimationGraph::from_clip(walk_clip.clone());
        assets.graph = Some((graphs.add(graph), node));
    }
    let (graph, walk) = assets.graph.clone().expect("graph built above");

    for (player_entity, mut player) in players.iter_mut() {
        // Only adopt players that live under a hero root.
        let mut ancestor = player_entity;
        let mut hero_root = None;
        while let Ok(child_of) = parents.get(ancestor) {
            ancestor = child_of.parent();
            if heroes.get(ancestor).is_ok() {
                hero_root = Some(ancestor);
                break;
            }
        }
        let Some(hero_root) = hero_root else {
            continue;
        };

        commands
            .entity(player_entity)
            .insert(AnimationGraphHandle(graph.clone()));
        // Start paused-at-standing: the drive system sets speed from motion.
        player.play(walk).repeat().set_speed(0.0);

        if hero_roots.get(hero_root).is_ok() {
            commands.entity(hero_root).insert(HeroAnim {
                player: player_entity,
                walk,
            });
        }
        info!("Hero walk animation configured for {hero_root:?}");
    }
}

/// Exponentially smooth the visual transform toward the replicated state.
///
/// Replication arrives at ~30Hz in steps; the visual lerp hides the steps.
/// The observed visual speed feeds the walk animation, so the feet always
/// match the motion on screen, whatever the network does.
fn sync_hero_transforms(
    time: Res<Time>,
    mut heroes: Query<(
        &PlayerPosition,
        &PlayerRotation,
        &mut Transform,
        &mut HeroVisual,
    )>,
) {
    let dt = time.delta_secs().max(1e-4);
    // ~12/s: settles within ~2 replication intervals without floatiness.
    let blend = 1.0 - (-12.0 * dt).exp();

    for (pos, rot, mut transform, mut visual) in heroes.iter_mut() {
        let before = transform.translation;
        let target = pos.0;
        let next = if before.distance_squared(target) > 20.0 * 20.0 {
            target // Teleport-scale jumps snap instead of gliding.
        } else {
            before.lerp(target, blend)
        };
        if next != before {
            transform.translation = next;
        }

        let target_rot = Quat::from_rotation_y(rot.0);
        if transform.rotation != target_rot {
            transform.rotation = transform.rotation.slerp(target_rot, blend);
        }

        let frame_speed = next.distance(before) / dt;
        visual.speed = visual.speed + (frame_speed - visual.speed) * blend;
    }
}

/// Scale the walk cycle from visual speed; pause it when standing.
fn drive_hero_walk_animation(
    heroes: Query<(&HeroVisual, &HeroAnim)>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (visual, anim) in heroes.iter() {
        let Ok(mut player) = players.get_mut(anim.player) else {
            continue;
        };
        let Some(active) = player.animation_mut(anim.walk) else {
            continue;
        };
        let speed = if visual.speed > 0.2 {
            (visual.speed / HERO_MOVE_SPEED).clamp(0.4, 1.6)
        } else {
            0.0
        };
        if (active.speed() - speed).abs() > 0.01 {
            active.set_speed(speed);
        }
    }
}
