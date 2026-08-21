//! Hero rendering + control: the client half of the embodied character.
//!
//! The server owns hero position/rotation (see server/src/player/hero.rs);
//! this module attaches the canonical humanoid glb to replicated Hero entities,
//! dresses them from their replicated [`HeroOutfit`], drives the walk
//! animation from observed velocity, smooths the streamed transform, and
//! turns clicks into spawn/move commands.

pub mod control;
pub mod footprints;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use shared::character::CharacterManifest;
use shared::components::{
    CharacterActivity, CharacterKind, CharacterMotion, HeroOutfit, PlayerPosition, PlayerRotation,
    TimeWarp,
};
use shared::economy::{CarriedAppearance, CarriedLoad, PorterCartState};
use shared::player::HERO_MOVE_SPEED;

use crate::states::GameState;

/// The art build's character contract: wardrobe slots, skin palette and clip
/// names. Nothing here hardcodes node names — see shared/src/character.rs.
#[derive(Resource, Deref)]
pub struct HeroManifest(pub CharacterManifest);

pub struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        // The manifest ships beside the glb; a missing or malformed one means
        // a broken asset build, so fail loudly at startup rather than
        // rendering bald, naked heroes.
        let manifest = CharacterManifest::load()
            .unwrap_or_else(|e| panic!("character manifest could not be loaded: {e}"));
        info!(
            "Character manifest: {} slots, {} skin tones, {} body + {} face clips",
            manifest.slots.len(),
            manifest.skin.tones.len(),
            manifest.body_clips.len(),
            manifest.face_clips.len()
        );
        // The creator starts on the look the art build declares as default.
        app.insert_resource(control::SelectedOutfit(HeroOutfit::from_manifest(
            &manifest,
        )));
        app.insert_resource(HeroManifest(manifest));
        app.init_resource::<HeroAssets>();
        app.init_resource::<CarriedLoadAssets>();
        app.init_resource::<PorterCartAssets>();
        app.init_resource::<ToolAssets>();
        app.init_resource::<control::WorldPlacementMode>();
        app.init_resource::<footprints::FootprintPool>();
        app.init_resource::<footprints::StrideTrackers>();
        app.add_systems(Startup, footprints::setup_footprint_assets);
        app.add_systems(
            Update,
            (footprints::stamp_footprints, footprints::fade_footprints)
                .run_if(in_state(crate::states::GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                (
                    attach_hero_visuals,
                    dress_heroes,
                    matte_character_materials,
                    apply_hero_skin,
                    setup_hero_animation,
                    sync_hero_transforms,
                ),
                (
                    tag_carry_attachments,
                    tag_tool_attachments,
                    tag_character_heads,
                    sync_indoor_visibility,
                    sync_porter_cart_visuals,
                    sync_carried_load_visuals,
                    sync_tool_visuals,
                    (
                        tag_porter_cart_load_attachments,
                        sync_porter_cart_load_visuals,
                    )
                        .chain(),
                    (
                        recover_stale_porter_cart_animation,
                        setup_porter_cart_animation,
                        drive_hero_locomotion,
                        drive_porter_cart_motion,
                    )
                        .chain(),
                ),
                (
                    control::handle_world_clicks,
                    control::auto_spawn_hero,
                    control::auto_set_time_warp_after,
                ),
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Lazily-loaded character handles, shared by every hero instance.
#[derive(Resource, Default)]
pub struct HeroAssets {
    scene: Option<Handle<WorldAsset>>,
    gltf: Option<Handle<Gltf>>,
    /// Built once the glTF is loaded; every hero shares the same graph.
    graph: Option<HeroGraph>,
    /// One recoloured skin material per tone index, SHARED by every hero
    /// wearing that tone (per-hero clones would cost a draw call each).
    skin_materials: HashMap<u8, Handle<StandardMaterial>>,
    /// Materials already matted, so the GPU upload happens once each.
    matted: bevy::platform::collections::HashSet<AssetId<StandardMaterial>>,
}

/// Authored carried-goods scenes, loaded once and shared by every character.
#[derive(Resource, Default)]
struct CarriedLoadAssets {
    scenes: HashMap<CarriedAppearance, Handle<WorldAsset>>,
}

const PORTER_CART_GLTF_PATH: &str = "game_assets/props/HandCart.glb";
const PORTER_CART_SCENE_PATH: &str = "game_assets/props/HandCart.glb#Scene0";
const PORTER_CART_WHEEL_RADIUS: f32 = 0.35;

fn advanced_cart_wheel_angle(current: f32, ground_distance: f32) -> f32 {
    (current - ground_distance / PORTER_CART_WHEEL_RADIUS).rem_euclid(std::f32::consts::TAU)
}

/// Shared scene, source glTF and graph for every active porter cart.
#[derive(Resource, Default)]
struct PorterCartAssets {
    scene: Option<Handle<WorldAsset>>,
    gltf: Option<Handle<Gltf>>,
    graph: Option<PorterCartGraph>,
}

#[derive(Clone)]
struct PorterCartGraph {
    handle: Handle<AnimationGraph>,
    pull: AnimationNodeIndex,
}

/// Authored hand-tool scenes, loaded once and shared by every character.
#[derive(Resource, Default)]
struct ToolAssets {
    scenes: HashMap<ToolKind, Handle<WorldAsset>>,
}

/// Animation graph handles, shared by every hero.
#[derive(Clone)]
pub struct HeroGraph {
    handle: Handle<AnimationGraph>,
    /// Body clip nodes by manifest clip name.
    body: HashMap<String, AnimationNodeIndex>,
    /// Face clip nodes by manifest clip name.
    face: HashMap<String, AnimationNodeIndex>,
}

/// Client-side smoothing/animation state on the hero root.
#[derive(Component)]
pub struct HeroVisual {
    /// Smoothed world-space speed (m/s) of the VISUAL transform — drives the
    /// walk animation, so feet track what the eye actually sees.
    speed: f32,
}

/// Last authoritative position snapshot received by this client. Rendering
/// extrapolates it for a tightly bounded interval using replicated velocity;
/// decisions and route truth remain entirely server-authoritative.
#[derive(Component)]
pub(crate) struct HeroMotionSnapshot {
    position: Vec3,
    received_at: f64,
}

impl HeroVisual {
    /// Smoothed visual speed, m/s.
    ///
    /// The honest source for "is this character moving?". The replicated
    /// position is a staircase at network rate, so comparing it frame to frame
    /// answers "did a packet land this frame?" rather than "is it walking?".
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// A rig that should always animate at full walk speed (the character
    /// creator preview walks in place on its turntable).
    pub fn walking_in_place() -> Self {
        Self {
            speed: HERO_MOVE_SPEED,
        }
    }
}

/// A local, non-replicated character rig (the creator preview). Shares the
/// hero dressing/animation systems but has no server-side existence.
#[derive(Component)]
pub struct HeroPreviewRig;

/// Marks a hero whose wardrobe matches its replicated outfit.
#[derive(Component)]
pub(crate) struct HeroDressed;

/// The direct child carrying the character scene. Spawned hidden; revealed
/// only once the wardrobe is applied, so the full closet never flashes for
/// a frame (scene instantiation runs after Update, beating the dresser).
#[derive(Component)]
struct HeroSceneRoot;

/// Root marker for an instantiated authored character rig.
#[derive(Component)]
pub(crate) struct HeroFullRig;
/// One packet can be late without making a walking crowd pause. Beyond this
/// horizon we hold the latest truth instead of inventing a long prediction.
const MAX_MOTION_EXTRAPOLATION_SECONDS: f64 = 0.08;

/// Rendering follows accelerated simulation in world time. Dividing the
/// extrapolation horizon by the same factor that multiplied server velocity
/// keeps its maximum spatial guess constant: 10x may move a villager ten
/// times faster, but it must not draw them ten times farther beyond a queue
/// place while waiting for the next snapshot.
fn visual_time_factor(warp: f32) -> f32 {
    if warp.is_finite() {
        warp.max(1.0)
    } else {
        1.0
    }
}

fn extrapolated_motion_target(
    position: Vec3,
    velocity: Vec3,
    snapshot_age: f64,
    time_factor: f32,
) -> Vec3 {
    let horizon = MAX_MOTION_EXTRAPOLATION_SECONDS / f64::from(time_factor.max(1.0));
    position + velocity * snapshot_age.clamp(0.0, horizon) as f32
}

fn visual_position_blend(real_seconds: f32, time_factor: f32) -> f32 {
    // ~12/world-second: at every warp, the visual body trails the
    // authoritative body by the same WORLD distance instead of the same real
    // time. This is particularly visible in tightly spaced Moot queues.
    1.0 - (-12.0 * time_factor.max(1.0) * real_seconds).exp()
}

/// Link from the hero root to the AnimationPlayer entity inside its scene.
#[derive(Component)]
struct HeroAnim {
    player: Entity,
    idle: Option<AnimationNodeIndex>,
    walk: Option<AnimationNodeIndex>,
    build: Option<AnimationNodeIndex>,
    chop: Option<AnimationNodeIndex>,
    harvest: Option<AnimationNodeIndex>,
    carry: Option<AnimationNodeIndex>,
    pull: Option<AnimationNodeIndex>,
    sit_idle: Option<AnimationNodeIndex>,
    current_body: Option<AnimationNodeIndex>,
    fading_body: Option<AnimationNodeIndex>,
    body_fade_seconds: f32,
    paused: bool,
}

const BODY_ANIMATION_FADE_SECONDS: f32 = 0.14;

/// The authored resource scene parented to the rig's `attach.carry` joint.
#[derive(Component)]
struct CarriedLoadVisual(CarriedAppearance);

/// Link from a replicated character to its world-space handcart child.
#[derive(Component)]
struct PorterCartLink(Entity);

/// Root of the instantiated handcart scene. Parenting this directly beneath
/// the character root copies locomotion without inheriting skeleton bob.
#[derive(Component)]
struct PorterCartVisual {
    owner: Entity,
}

#[derive(Component)]
struct PorterCartLoadAttachment {
    owner: Entity,
    slot: u8,
}

#[derive(Component)]
struct PorterCartLoadVisual {
    appearance: CarriedAppearance,
    slot: u8,
}

#[derive(Component)]
struct PorterCartAnimationTarget;

#[derive(Component)]
struct PorterCartAnimation {
    player: Entity,
    pull: AnimationNodeIndex,
    wheels: [Entity; 2],
    wheel_rest: [Quat; 2],
    wheel_angle: f32,
    last_position: Vec3,
}

/// An authored rig node that accepts the current carried-goods visual.
#[derive(Component)]
struct CarryAttachment;

/// Direct link from an authored attachment joint to its replicated character
/// root. Resolving this once avoids walking the glTF hierarchy for every tool
/// and carried-load check on every frame.
#[derive(Component)]
struct CharacterAttachmentOwner(Entity);

/// Exact authored head joint and the character root that owns it.
///
/// Camera presentation must follow the dressed/animated rig rather than guess
/// a face height from the replicated character root. The latter is especially
/// visible in seated poses, where a standing-height estimate can put a close
/// camera inside hair, clothing, or the boat.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CharacterHead {
    pub(crate) owner: Entity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ToolKind {
    Axe,
    Hammer,
    Scythe,
}

impl ToolKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Axe => "Felling axe",
            Self::Hammer => "Framing hammer",
            Self::Scythe => "Mowing scythe",
        }
    }

    const fn scene_path(self) -> &'static str {
        match self {
            Self::Axe => "game_assets/tools/AxeFelling.glb#Scene0",
            Self::Hammer => "game_assets/tools/HammerFraming.glb#Scene0",
            Self::Scythe => "game_assets/tools/ScytheMowing.glb#Scene0",
        }
    }
}

/// The authored tool scene parented to the rig's `attach.tool.R` joint.
#[derive(Component)]
struct ToolVisual(ToolKind);

/// The authored right-hand joint that accepts the active work tool.
#[derive(Component)]
struct ToolAttachment;

/// Spawn the (hidden-until-dressed) character scene under a rig root.
/// Shared by replicated heroes and the creator's preview rig.
pub(crate) fn spawn_character_scene_child(
    root: &mut ChildSpawnerCommands<'_>,
    asset_server: &AssetServer,
    assets: &mut HeroAssets,
    manifest: &CharacterManifest,
) {
    let scene = assets
        .scene
        .get_or_insert_with(|| asset_server.load(manifest.scene.clone()))
        .clone();
    assets.gltf.get_or_insert_with(|| {
        // The Gltf asset (for clips + material lookup) is the path without
        // the "#Scene0" label the scene handle uses.
        let path = manifest
            .scene
            .split('#')
            .next()
            .unwrap_or(&manifest.scene)
            .to_string();
        asset_server.load(path)
    });
    root.spawn((
        HeroSceneRoot,
        Transform::IDENTITY,
        GlobalTransform::default(),
        Visibility::Hidden,
        InheritedVisibility::default(),
        WorldAssetRoot(scene),
    ));
}

/// Give newly replicated heroes a transform + the character scene.
///
/// Polls `Without<HeroVisual>` instead of `Added<Hero>`: replication may
/// deliver `Hero` and `PlayerPosition` in different batches, and a one-shot
/// `Added` trigger that missed the position would leave the hero invisible
/// forever.
fn attach_hero_visuals(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<HeroAssets>,
    manifest: Res<HeroManifest>,
    heroes: Query<
        (Entity, &PlayerPosition, Option<&PlayerRotation>),
        // Gated on CharacterKind, not Hero: villagers are people too and use
        // the same body. `Hero` means "owned by a player", which is a question
        // about ownership, not about having a body to draw.
        (With<CharacterKind>, Without<HeroVisual>),
    >,
) {
    for (entity, pos, rot) in heroes.iter() {
        let yaw = rot.map(|r| r.0).unwrap_or(0.0);
        commands
            .entity(entity)
            .insert((
                Transform::from_translation(pos.0).with_rotation(Quat::from_rotation_y(yaw)),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                HeroVisual { speed: 0.0 },
                HeroMotionSnapshot {
                    position: pos.0,
                    received_at: time.elapsed_secs_f64(),
                },
                HeroFullRig,
            ))
            .with_children(|root| {
                spawn_character_scene_child(root, &asset_server, &mut assets, &manifest);
            });
        debug!("Hero character rig attached for {entity:?}");
    }
}

/// Toggle wardrobe node visibility to match the replicated outfit.
///
/// Runs until the async scene instantiation exposes every wardrobe node,
/// then stamps [`HeroDressed`]. Outfit changes re-run it via the marker
/// removal below.
fn dress_heroes(
    mut commands: Commands,
    manifest: Res<HeroManifest>,
    heroes: Query<
        (Entity, &HeroOutfit),
        (With<HeroVisual>, With<HeroFullRig>, Without<HeroDressed>),
    >,
    changed: Query<Entity, (With<HeroDressed>, Changed<HeroOutfit>)>,
    children_q: Query<&Children>,
    mut named: Query<(&Name, &mut Visibility)>,
    mut scene_roots: Query<&mut Visibility, (With<HeroSceneRoot>, Without<Name>)>,
) {
    // Every wardrobe item across all slots must be present before dressing,
    // or a half-instantiated scene would show the whole closet for a frame.
    let wardrobe_node_count: usize = manifest.slots.iter().map(|slot| slot.items.len()).sum();
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
                if manifest.is_wardrobe_node(name) {
                    found += 1;
                    edits.push((node, outfit.hides_node(&manifest, name)));
                }
            }
            if let Ok(children) = children_q.get(node) {
                stack.extend(children.iter());
            }
        }
        // Scene still instantiating: retry next frame rather than dressing half
        // a closet.
        if found < wardrobe_node_count {
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

/// Records which skin tone index is currently on the body primitive.
#[derive(Component)]
struct HeroSkinApplied(u8);

/// Recolour the body's skin material to the hero's chosen tone.
///
/// Only the default tone's material reaches the glb (glTF drops unused
/// materials), so tones are a RECOLOUR of that one material. The recoloured
/// materials are cached per tone and shared by every hero wearing it —
/// per-hero clones would each cost a draw call.
///
/// The body primitive is found by `GltfMaterialName` (a component Bevy puts on
/// every glTF primitive) matched against the manifest's material name, so this
/// never depends on Bevy's node-naming convention.
fn apply_hero_skin(
    mut commands: Commands,
    manifest: Res<HeroManifest>,
    mut assets: ResMut<HeroAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    heroes: Query<
        (Entity, &HeroOutfit, Option<&HeroSkinApplied>),
        (With<HeroVisual>, With<HeroFullRig>),
    >,
    children_q: Query<&Children>,
    mut primitives: Query<(&GltfMaterialName, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (hero, outfit, applied) in heroes.iter() {
        if applied.is_some_and(|applied| applied.0 == outfit.skin) {
            continue;
        }

        let mut stack = vec![hero];
        let mut target = None;
        while let Some(node) = stack.pop() {
            if let Ok((material_name, mesh_material)) = primitives.get_mut(node) {
                if material_name.0 == manifest.skin.material {
                    target = Some((node, mesh_material.0.clone()));
                    break;
                }
            }
            if let Ok(children) = children_q.get(node) {
                stack.extend(children.iter());
            }
        }
        // Scene still instantiating — retry next frame.
        let Some((primitive, current)) = target else {
            continue;
        };

        let handle = if let Some(cached) = assets.skin_materials.get(&outfit.skin) {
            cached.clone()
        } else {
            // Clone the live material so every other property (roughness,
            // cull mode, the build's matte settings) is preserved; only the
            // base colour changes. The manifest stores LINEAR rgb, matching
            // glTF baseColorFactor.
            let Some(mut tinted) = materials.get(&current).cloned() else {
                continue;
            };
            let rgb = manifest.skin_color(outfit.skin);
            tinted.base_color = Color::linear_rgb(rgb[0], rgb[1], rgb[2]);
            let handle = materials.add(tinted);
            assets.skin_materials.insert(outfit.skin, handle.clone());
            handle
        };

        // Mutate the live primitive immediately. Character LOD may have queued
        // this glTF scene for despawn earlier in the same update; queuing an
        // ordinary `insert` here used to run after that despawn and panic on the
        // stale child entity during large crowd transitions.
        if let Ok((_, mut mesh_material)) = primitives.get_mut(primitive) {
            *mesh_material = MeshMaterial3d(handle);
        }
        // The replicated character root normally outlives its visual rig, but
        // make the marker fallible as well so disconnect/despawn races cannot
        // turn a cosmetic operation into a client crash.
        commands
            .entity(hero)
            .try_insert(HeroSkinApplied(outfit.skin));
    }
}

/// Mask group ids. In Bevy a SET bit means "this node may not animate that
/// group", so the body layer masks FACE out and the face layer masks BODY out.
const MASK_GROUP_FACE: u32 = 0;
const MASK_GROUP_BODY: u32 = 1;
/// Bones whose name starts with this belong to the face layer. Everything else
/// is body — so a new bone can never silently escape a mask group.
const FACE_BONE_PREFIX: &str = "eye.";

/// Clip names the locomotion blend expects to find in `body_clips`.
const CLIP_IDLE: &str = "idle";
const CLIP_WALK: &str = "walk";
/// Played while a villager works on a construction site.
const CLIP_BUILD: &str = "build";
/// Played while working a tree and while walking with a physical load.
const CLIP_CHOP: &str = "chop";
/// Played while cutting and gathering a wheat field.
const CLIP_HARVEST: &str = "harvest";
const CLIP_CARRY: &str = "carry";
/// Played while an active porter trip owns the world-space handcart.
const CLIP_PULL: &str = "pull";
const CLIP_SIT_IDLE: &str = "sit_idle";
/// Resting expression; face clips play on their own masked layer.
const CLIP_FACE_IDLE: &str = "face_idle";

/// Matte every character material once.
///
/// MANDATORY, not cosmetic: the glb ships glTF-default specular and carries no
/// roughness factor at all, so Bevy's 0.5 defaults put a broad specular lobe
/// over near-black albedo — CHARACTER_PIPELINE.md §13 records dark hair
/// rendering mid-grey and auburn hair rendering skin-pink. The asset build
/// deliberately leaves this to the client (same contract as
/// `props::foliage::flatten_base`).
fn matte_character_materials(
    mut assets: ResMut<HeroAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    primitives: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        Added<MeshMaterial3d<StandardMaterial>>,
    >,
    parents: Query<&ChildOf>,
    rigs: Query<(), Or<(With<CharacterKind>, With<HeroPreviewRig>)>>,
) {
    // Scene primitives arrive with their material assets already registered.
    // Inspect only newly instantiated primitives and walk upward once to prove
    // they belong to a character. The previous implementation traversed every
    // descendant of all 160 rigs every rendered frame to rediscover the same
    // dozen shared handles.
    for (entity, mesh_material) in primitives.iter() {
        let mut ancestor = entity;
        let belongs_to_character = loop {
            if rigs.get(ancestor).is_ok() {
                break true;
            }
            let Ok(parent) = parents.get(ancestor) else {
                break false;
            };
            ancestor = parent.parent();
        };
        if !belongs_to_character {
            continue;
        }

        let id = mesh_material.0.id();
        if assets.matted.contains(&id) {
            continue;
        }
        if let Some(mut material) = materials.get_mut(&mesh_material.0) {
            crate::props::foliage::flatten_base(&mut material);
            assets.matted.insert(id);
        }
    }
}

/// Wire the scene's [`AnimationPlayer`] to the shared masked graph.
///
/// Polls instead of `Added<AnimationPlayer>`: the graph can only be built once
/// the glTF asset AND an instantiated rig exist, and a one-shot trigger would
/// miss players that appear before that.
///
/// Masks are MANDATORY here. The exporter bakes every action over all 16
/// bones, so a face clip played unmasked drives the whole body to rest pose,
/// and a body clip played unmasked stamps `face_surprised`'s wide eyes on
/// permanently (the rig was left in that pose when the body actions baked).
#[allow(clippy::too_many_arguments)]
fn setup_hero_animation(
    mut commands: Commands,
    manifest: Res<HeroManifest>,
    mut assets: ResMut<HeroAssets>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<(Entity, &mut AnimationPlayer), Without<AnimationGraphHandle>>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    children_q: Query<&Children>,
    rig_roots: Query<
        Entity,
        (
            Or<(With<CharacterKind>, With<HeroPreviewRig>)>,
            Without<HeroAnim>,
        ),
    >,
) {
    if players.is_empty() {
        return;
    }

    if assets.graph.is_none() {
        let Some(gltf) = assets.gltf.as_ref().and_then(|handle| gltfs.get(handle)) else {
            return;
        };
        // Mask groups are derived from the LIVE rig rather than a hardcoded
        // bone table: a renamed or added bone lands in the body group
        // automatically instead of silently escaping every mask.
        let Some((player_entity, _)) = players.iter().next() else {
            return;
        };
        let mut graph = AnimationGraph::new();
        let mut targets = 0usize;
        let mut face_targets = 0usize;
        let mut stack = vec![(player_entity, Vec::<String>::new())];
        while let Some((node, prefix)) = stack.pop() {
            let Ok(name) = names.get(node) else {
                continue;
            };
            // AnimationTargetId hashes the name path from the animation root
            // INCLUSIVE of the root's own name.
            let mut path = prefix;
            path.push(name.as_str().to_string());
            let leaf = name.as_str();
            let group = if leaf.starts_with(FACE_BONE_PREFIX) {
                face_targets += 1;
                MASK_GROUP_FACE
            } else {
                MASK_GROUP_BODY
            };
            graph.add_target_to_mask_group(AnimationTargetId::from_iter(path.iter()), group);
            targets += 1;
            if let Ok(children) = children_q.get(node) {
                for child in children.iter() {
                    stack.push((child, path.clone()));
                }
            }
        }
        if face_targets == 0 {
            // No eyes yet means the scene is still instantiating; a graph
            // built now would let face clips drive the body.
            return;
        }

        // Two layers off the root, each masking the other's group out.
        let body_layer = graph.add_blend_with_mask(1 << MASK_GROUP_FACE, 1.0, graph.root);
        let face_layer = graph.add_blend_with_mask(1 << MASK_GROUP_BODY, 1.0, graph.root);

        let mut body = HashMap::new();
        for clip_name in &manifest.body_clips {
            if let Some(clip) = gltf.named_animations.get(clip_name.as_str()) {
                body.insert(
                    clip_name.clone(),
                    graph.add_clip(clip.clone(), 1.0, body_layer),
                );
            } else {
                warn!("manifest body clip '{clip_name}' missing from the glb");
            }
        }
        let mut face = HashMap::new();
        for clip_name in &manifest.face_clips {
            if let Some(clip) = gltf.named_animations.get(clip_name.as_str()) {
                face.insert(
                    clip_name.clone(),
                    graph.add_clip(clip.clone(), 1.0, face_layer),
                );
            } else {
                warn!("manifest face clip '{clip_name}' missing from the glb");
            }
        }

        info!(
            "Hero animation graph: {targets} targets ({face_targets} face), \
             {} body + {} face clips",
            body.len(),
            face.len()
        );
        assets.graph = Some(HeroGraph {
            handle: graphs.add(graph),
            body,
            face,
        });
    }
    let hero_graph = assets.graph.clone().expect("graph built above");

    for (player_entity, mut player) in players.iter_mut() {
        // Only adopt players that live under a hero/preview root.
        let mut ancestor = player_entity;
        let mut rig_root = None;
        while let Ok(child_of) = parents.get(ancestor) {
            ancestor = child_of.parent();
            if let Ok(root) = rig_roots.get(ancestor) {
                rig_root = Some(root);
                break;
            }
        }
        let Some(rig_root) = rig_root else {
            continue;
        };

        commands
            .entity(player_entity)
            .try_insert(AnimationGraphHandle(hero_graph.handle.clone()));

        // Keep one steady body clip active. `drive_hero_locomotion` starts a
        // second only for its short crossfade and then removes the old clip.
        // Zero-weight clips are still traversed by Bevy's animation graph, so
        // pre-starting every possible job animation multiplied the cost of a
        // 160-person neighbourhood by seven.
        let idle = hero_graph.body.get(CLIP_IDLE).copied();
        let walk = hero_graph.body.get(CLIP_WALK).copied();
        let build = hero_graph.body.get(CLIP_BUILD).copied();
        let chop = hero_graph.body.get(CLIP_CHOP).copied();
        let harvest = hero_graph.body.get(CLIP_HARVEST).copied();
        let carry = hero_graph.body.get(CLIP_CARRY).copied();
        let pull = hero_graph.body.get(CLIP_PULL).copied();
        let sit_idle = hero_graph.body.get(CLIP_SIT_IDLE).copied();
        if let Some(idle) = idle {
            player.play(idle).repeat().set_weight(1.0);
        }
        // Resting expression on the face layer; body clips cannot touch it.
        if let Some(face_idle) = hero_graph.face.get(CLIP_FACE_IDLE).copied() {
            player.play(face_idle).repeat().set_weight(1.0);
        }

        // Both the glTF child and its replicated root may disappear between
        // query collection and deferred command application (disconnect or
        // world teardown). Cosmetic animation setup must never crash the
        // client on either stale entity.
        commands.entity(rig_root).try_insert(HeroAnim {
            player: player_entity,
            idle,
            walk,
            build,
            chop,
            harvest,
            carry,
            pull,
            sit_idle,
            current_body: idle,
            fading_body: None,
            body_fade_seconds: BODY_ANIMATION_FADE_SECONDS,
            paused: false,
        });
        debug!("Hero animation configured for {rig_root:?}");
    }
}

/// Exponentially smooth the visual transform toward the replicated state.
///
/// Replication arrives at ~30Hz in steps; the visual lerp hides the steps.
/// The observed visual speed feeds the walk animation, so the feet always
/// match the motion on screen, whatever the network does.
pub(crate) fn sync_hero_transforms(
    time: Res<Time>,
    warp: Query<&TimeWarp>,
    mut heroes: Query<(
        Ref<PlayerPosition>,
        &PlayerRotation,
        Option<&CharacterMotion>,
        &mut Transform,
        &mut HeroVisual,
        &mut HeroMotionSnapshot,
    )>,
) {
    let dt = time.delta_secs().max(1e-4);
    let now = time.elapsed_secs_f64();
    let time_factor = visual_time_factor(warp.iter().next().map_or(1.0, |warp| warp.0));
    let blend = visual_position_blend(dt, time_factor);

    for (pos, rot, motion, mut transform, mut visual, mut snapshot) in heroes.iter_mut() {
        if pos.is_changed() {
            snapshot.position = pos.0;
            snapshot.received_at = now;
        }
        let velocity = motion.map_or(Vec3::ZERO, |motion| motion.velocity);
        let before = transform.translation;
        let target = extrapolated_motion_target(
            snapshot.position,
            velocity,
            now - snapshot.received_at,
            time_factor,
        );
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

/// A building is still an exterior shell today, so crossing its authored door
/// hides the character until they come back out. The simulation keeps them at
/// a shallow point beyond the threshold; visible interiors can later replace
/// this with an interior scene without changing the worker state machine.
fn sync_indoor_visibility(
    characters: Query<
        (&Children, Option<&CharacterActivity>),
        (
            With<HeroVisual>,
            With<HeroDressed>,
            Or<(Changed<CharacterActivity>, Added<HeroDressed>)>,
        ),
    >,
    mut scene_roots: Query<&mut Visibility, With<HeroSceneRoot>>,
) {
    for (children, activity) in characters.iter() {
        let target = if activity.is_some_and(|activity| *activity == CharacterActivity::Indoors) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for child in children.iter() {
            if let Ok(mut visibility) = scene_roots.get_mut(child) {
                if *visibility != target {
                    *visibility = target;
                }
            }
        }
    }
}

/// Parent the matching authored resource bundle to the rig's carry attachment.
fn tag_carry_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<CarryAttachment>)>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() == "attach.carry" {
            let mut ancestor = entity;
            while let Ok(parent) = parents.get(ancestor) {
                ancestor = parent.parent();
                if characters.get(ancestor).is_ok() {
                    commands
                        .entity(entity)
                        .insert((CarryAttachment, CharacterAttachmentOwner(ancestor)));
                    break;
                }
            }
        }
    }
}

/// Tag the authored right-hand tool joint once its glTF node is instantiated.
fn tag_tool_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<ToolAttachment>)>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() == "attach.tool.R" {
            let mut ancestor = entity;
            while let Ok(parent) = parents.get(ancestor) {
                ancestor = parent.parent();
                if characters.get(ancestor).is_ok() {
                    commands
                        .entity(entity)
                        .insert((ToolAttachment, CharacterAttachmentOwner(ancestor)));
                    break;
                }
            }
        }
    }
}

/// Resolve the canonical `head` bone after the asynchronous glTF hierarchy is
/// instantiated. Like tools and carried loads, this pays the ancestry walk
/// once and leaves a direct owner link for cheap per-frame camera queries.
fn tag_character_heads(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<CharacterHead>)>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() != "head" {
            continue;
        }
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if characters.get(ancestor).is_ok() {
                commands
                    .entity(entity)
                    .insert(CharacterHead { owner: ancestor });
                break;
            }
        }
    }
}

/// Add and remove the cart as a direct child of the replicated character.
/// Its authored root is already in the character's final game-space frame, so
/// no corrective offset, rotation or scale belongs here.
fn sync_porter_cart_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<PorterCartAssets>,
    active: Query<(Entity, Option<&PorterCartLink>), (With<CharacterKind>, With<PorterCartState>)>,
    inactive: Query<(Entity, &PorterCartLink), (With<CharacterKind>, Without<PorterCartState>)>,
    cart_visuals: Query<(), With<PorterCartVisual>>,
) {
    for (owner, link) in active.iter() {
        if link.is_some_and(|link| cart_visuals.get(link.0).is_ok()) {
            continue;
        }

        let scene = assets
            .scene
            .get_or_insert_with(|| asset_server.load(PORTER_CART_SCENE_PATH))
            .clone();
        assets
            .gltf
            .get_or_insert_with(|| asset_server.load(PORTER_CART_GLTF_PATH));
        let cart = commands
            .spawn((
                Name::new("Porter Handcart"),
                PorterCartVisual { owner },
                Transform::IDENTITY,
                Visibility::default(),
                WorldAssetRoot(scene),
            ))
            .id();
        commands
            .entity(owner)
            .add_child(cart)
            .insert(PorterCartLink(cart));
    }

    for (owner, link) in inactive.iter() {
        if cart_visuals.get(link.0).is_ok() {
            commands.entity(link.0).despawn();
        }
        commands.entity(owner).remove::<PorterCartLink>();
    }
}

/// Resolve the two authored bed anchors to their replicated character owner.
/// This is a one-time hierarchy walk per instantiated cart, not a per-frame
/// scene-name search.
fn tag_porter_cart_load_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<PorterCartLoadAttachment>)>,
    parents: Query<&ChildOf>,
    carts: Query<&PorterCartVisual>,
) {
    for (entity, name) in named.iter() {
        let slot = match name.as_str() {
            "Anchor_Load.1" => 1,
            "Anchor_Load.2" => 2,
            _ => continue,
        };
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if let Ok(cart) = carts.get(ancestor) {
                commands.entity(entity).insert(PorterCartLoadAttachment {
                    owner: cart.owner,
                    slot,
                });
                break;
            }
        }
    }
}

/// Rebind if scene streaming ever replaces the animated descendants while
/// retaining the cart root.
fn recover_stale_porter_cart_animation(
    mut commands: Commands,
    carts: Query<(Entity, &PorterCartAnimation)>,
    players: Query<(), With<AnimationPlayer>>,
    transforms: Query<(), With<Transform>>,
    children: Query<&Children>,
    wired: Query<(), With<PorterCartAnimationTarget>>,
) {
    for (root, animation) in carts.iter() {
        let stale = players.get(animation.player).is_err()
            || animation
                .wheels
                .iter()
                .any(|wheel| transforms.get(*wheel).is_err());
        if !stale {
            continue;
        }
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if wired.get(entity).is_ok() {
                commands
                    .entity(entity)
                    .remove::<PorterCartAnimationTarget>();
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        commands.entity(root).remove::<PorterCartAnimation>();
    }
}

/// Bind the authored cart-body nod. Wheel rotation is intentionally excluded
/// from the graph and driven from measured world distance below.
#[allow(clippy::too_many_arguments)]
fn setup_porter_cart_animation(
    mut commands: Commands,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut assets: ResMut<PorterCartAssets>,
    targets: Query<(Entity, &Name, &AnimatedBy), Without<PorterCartAnimationTarget>>,
    mut players: Query<&mut AnimationPlayer>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    carts: Query<&PorterCartVisual, Without<PorterCartAnimation>>,
    owners: Query<&Transform, With<CharacterKind>>,
) {
    for (target, name, animated_by) in targets.iter() {
        if name.as_str() != "HandCartBody" {
            continue;
        }

        let mut ancestor = target;
        let mut cart_root = None;
        loop {
            if let Ok(cart) = carts.get(ancestor) {
                cart_root = Some((ancestor, cart));
                break;
            }
            let Ok(parent) = parents.get(ancestor) else {
                break;
            };
            ancestor = parent.parent();
        }
        let Some((root, cart)) = cart_root else {
            continue;
        };

        let mut wheels = [None, None];
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(node_name) = names.get(entity) {
                match node_name.as_str() {
                    "HandCartWheelL" => wheels[0] = Some(entity),
                    "HandCartWheelR" => wheels[1] = Some(entity),
                    _ => {}
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        let [Some(wheel_l), Some(wheel_r)] = wheels else {
            continue;
        };
        let Ok([wheel_l_transform, wheel_r_transform]) = transforms.get_many([wheel_l, wheel_r])
        else {
            continue;
        };
        let Ok(owner_transform) = owners.get(cart.owner) else {
            continue;
        };
        let Ok(mut player) = players.get_mut(animated_by.0) else {
            continue;
        };

        let cart_graph = if let Some(graph) = &assets.graph {
            graph.clone()
        } else {
            let Some(gltf) = assets.gltf.as_ref().and_then(|handle| gltfs.get(handle)) else {
                continue;
            };
            let Some(clip) = gltf.named_animations.get("cart_pull") else {
                warn!("HandCart.glb has no cart_pull clip");
                continue;
            };
            let mut graph = AnimationGraph::new();
            let pull = graph.add_clip(clip.clone(), 1.0, graph.root);
            let graph = PorterCartGraph {
                handle: graphs.add(graph),
                pull,
            };
            assets.graph = Some(graph.clone());
            graph
        };

        player.play(cart_graph.pull).repeat().set_speed(0.0);
        commands
            .entity(animated_by.0)
            .insert(AnimationGraphHandle(cart_graph.handle.clone()));
        commands.entity(target).insert(PorterCartAnimationTarget);
        commands.entity(root).insert(PorterCartAnimation {
            player: animated_by.0,
            pull: cart_graph.pull,
            wheels: [wheel_l, wheel_r],
            wheel_rest: [wheel_l_transform.rotation, wheel_r_transform.rotation],
            wheel_angle: 0.0,
            last_position: owner_transform.translation,
        });
    }
}

/// Parent one or two copies of the authoritative carried-good appearance to
/// the cart bed. Cart slots use the resource GLBs at authored scale; the 1.35
/// hand-held compensation belongs only to the character's carry joint.
fn sync_porter_cart_load_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<CarriedLoadAssets>,
    attachments: Query<(Entity, Ref<PorterCartLoadAttachment>)>,
    children: Query<&Children>,
    loads: Query<(Ref<CarriedLoad>, Ref<PorterCartState>), With<CharacterKind>>,
    existing_visuals: Query<&PorterCartLoadVisual>,
) {
    for (attachment, marker) in attachments.iter() {
        let Ok((load, cart)) = loads.get(marker.owner) else {
            continue;
        };
        if !marker.is_added() && !load.is_changed() && !cart.is_changed() {
            continue;
        }
        let desired = (marker.slot <= cart.load_slots)
            .then(|| load.visible_appearance())
            .flatten();
        let existing = children.get(attachment).ok().and_then(|children| {
            children.iter().find_map(|child| {
                existing_visuals
                    .get(child)
                    .ok()
                    .map(|visual| (child, visual.appearance, visual.slot))
            })
        });
        if existing
            .is_some_and(|(_, appearance, slot)| Some(appearance) == desired && slot == marker.slot)
        {
            continue;
        }
        if let Some((entity, _, _)) = existing {
            commands.entity(entity).despawn();
        }
        let Some(appearance) = desired else {
            continue;
        };
        let spec = carried_asset_spec(appearance);
        let scene = assets
            .scenes
            .entry(appearance)
            .or_insert_with(|| asset_server.load(spec.scene_path))
            .clone();
        commands.entity(attachment).with_children(|anchor| {
            anchor.spawn((
                Name::new(format!("Cart load {}", marker.slot)),
                PorterCartLoadVisual {
                    appearance,
                    slot: marker.slot,
                },
                WorldAssetRoot(scene),
                Transform::IDENTITY,
            ));
        });
    }
}

/// Keep the cart-body nod phase-identical to the character's pull clip and
/// rotate both wheels by actual ground distance (`theta = -distance / r`).
fn drive_porter_cart_motion(
    mut carts: Query<(&PorterCartVisual, &mut PorterCartAnimation)>,
    heroes: Query<(&Transform, &HeroAnim), With<CharacterKind>>,
    mut animation_players: ParamSet<(Query<&AnimationPlayer>, Query<&mut AnimationPlayer>)>,
    mut wheel_transforms: Query<&mut Transform, Without<CharacterKind>>,
) {
    for (cart, mut animation) in carts.iter_mut() {
        let Ok((owner_transform, hero_anim)) = heroes.get(cart.owner) else {
            continue;
        };

        let hero_pose = {
            let players = animation_players.p0();
            let Some(pull) = hero_anim.pull else {
                continue;
            };
            let Ok(player) = players.get(hero_anim.player) else {
                continue;
            };
            let Some(active) = player.animation(pull) else {
                continue;
            };
            (active.seek_time(), active.speed(), active.weight())
        };
        {
            let mut players = animation_players.p1();
            let Ok(mut player) = players.get_mut(animation.player) else {
                continue;
            };
            if !player.is_playing_animation(animation.pull) {
                player.play(animation.pull).repeat();
            }
            if let Some(active) = player.animation_mut(animation.pull) {
                active.set_seek_time(hero_pose.0);
                active.set_speed(hero_pose.1);
                active.set_weight(hero_pose.2);
            }
        }

        let position = owner_transform.translation;
        let delta = Vec2::new(
            position.x - animation.last_position.x,
            position.z - animation.last_position.z,
        );
        let travelled = delta.length();
        animation.last_position = position;
        // Network correction/teleport: relocate the cart without presenting a
        // wildly spinning wheel for one frame. Ordinary 100x lab movement is
        // far below this threshold and remains distance-exact.
        if travelled <= 32.0 {
            animation.wheel_angle = advanced_cart_wheel_angle(animation.wheel_angle, travelled);
        }
        let wheel_rotation = Quat::from_rotation_x(animation.wheel_angle);
        for (index, wheel) in animation.wheels.into_iter().enumerate() {
            if let Ok(mut transform) = wheel_transforms.get_mut(wheel) {
                transform.rotation = animation.wheel_rest[index] * wheel_rotation;
            }
        }
    }
}

fn sync_carried_load_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<CarriedLoadAssets>,
    attachments: Query<(Entity, Ref<CarryAttachment>, &CharacterAttachmentOwner)>,
    children: Query<&Children>,
    loads: Query<(Ref<CarriedLoad>, Option<Ref<PorterCartState>>), With<CharacterKind>>,
    mut removed_carts: RemovedComponents<PorterCartState>,
    existing_visuals: Query<&CarriedLoadVisual>,
) {
    let removed_carts: HashSet<_> = removed_carts.read().collect();
    for (attachment, marker, owner) in attachments.iter() {
        let Ok((load, cart)) = loads.get(owner.0) else {
            continue;
        };
        if !marker.is_added()
            && !load.is_changed()
            && !cart.as_ref().is_some_and(|cart| cart.is_changed())
            && !removed_carts.contains(&owner.0)
        {
            continue;
        }
        // A cart load belongs on its authored bed anchors, never duplicated in
        // the porter's arms. Removing the cart re-evaluates this attachment so
        // an abnormal in-flight personal load remains visible.
        let desired = cart.is_none().then(|| load.visible_appearance()).flatten();
        let existing = children.get(attachment).ok().and_then(|children| {
            children.iter().find_map(|child| {
                existing_visuals
                    .get(child)
                    .ok()
                    .map(|visual| (child, visual.0))
            })
        });

        if existing.is_some_and(|(_, appearance)| Some(appearance) == desired) {
            continue;
        }
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        let Some(appearance) = desired else {
            continue;
        };

        let spec = carried_asset_spec(appearance);
        let scene = assets
            .scenes
            .entry(appearance)
            .or_insert_with(|| asset_server.load(spec.scene_path))
            .clone();

        commands.entity(attachment).with_children(|bone| {
            bone.spawn((
                Name::new(format!("Carried {}", appearance.label())),
                CarriedLoadVisual(appearance),
                WorldAssetRoot(scene),
                // Every authored bundle has its origin on its base, and the
                // joint marks that same base. No height correction belongs here.
                carried_bundle_transform(),
            ));
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CarriedAssetSpec {
    scene_path: &'static str,
}

/// The authored measurements were intentionally conservative. In the actual
/// isometric game view they read as parcels rather than work loads, so carried
/// presentation is enlarged without changing inventory bulk or the source GLBs.
const CARRIED_BUNDLE_SCALE: f32 = 1.35;
/// Bevy/character forward is -Z. Pull the load slightly away from the torso so
/// its centre sits in the cupped hands instead of clipping into the chest.
const CARRIED_BUNDLE_FORWARD_OFFSET: f32 = -0.08;

fn carried_bundle_transform() -> Transform {
    Transform::from_xyz(0.0, 0.0, CARRIED_BUNDLE_FORWARD_OFFSET)
        .with_scale(Vec3::splat(CARRIED_BUNDLE_SCALE))
}

fn carried_asset_spec(appearance: CarriedAppearance) -> CarriedAssetSpec {
    match appearance {
        CarriedAppearance::WoodBundle => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/WoodBundle.glb#Scene0",
        },
        CarriedAppearance::WheatSheaf => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/WheatSheaf.glb#Scene0",
        },
        CarriedAppearance::FishBasket => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/FishBasket.glb#Scene0",
        },
        CarriedAppearance::StoneBundle => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/StoneBundle.glb#Scene0",
        },
        CarriedAppearance::IronBundle => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/IronBundle.glb#Scene0",
        },
        CarriedAppearance::FlourSack => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/FlourSack.glb#Scene0",
        },
        CarriedAppearance::BreadBasket => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/BreadBasket.glb#Scene0",
        },
    }
}

fn desired_tool(activity: Option<CharacterActivity>, carrying: bool) -> Option<ToolKind> {
    if carrying {
        return None;
    }
    match activity {
        Some(CharacterActivity::Chopping) => Some(ToolKind::Axe),
        Some(CharacterActivity::Farming) => Some(ToolKind::Scythe),
        Some(CharacterActivity::Building | CharacterActivity::Mining) => Some(ToolKind::Hammer),
        _ => None,
    }
}

/// Attach only the tool required by the character's current visible work.
///
/// A physical load always wins: a villager carrying wood cannot also hold a
/// tool. `CharacterActivity` is authoritative, avoiding an N characters × M
/// construction-sites proximity join on the client.
fn sync_tool_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<ToolAssets>,
    attachments: Query<(Entity, Ref<ToolAttachment>, &CharacterAttachmentOwner)>,
    children: Query<&Children>,
    characters: Query<
        (
            Option<Ref<CharacterActivity>>,
            Option<Ref<CarriedLoad>>,
            Option<Ref<PorterCartState>>,
        ),
        With<CharacterKind>,
    >,
    mut removed_carts: RemovedComponents<PorterCartState>,
    existing_visuals: Query<&ToolVisual>,
) {
    let removed_carts: HashSet<_> = removed_carts.read().collect();
    for (attachment, marker, owner) in attachments.iter() {
        let Ok((activity, carried, cart)) = characters.get(owner.0) else {
            continue;
        };
        if !marker.is_added()
            && !activity
                .as_ref()
                .is_some_and(|activity| activity.is_changed())
            && !carried.as_ref().is_some_and(|carried| carried.is_changed())
            && !cart.as_ref().is_some_and(|cart| cart.is_changed())
            && !removed_carts.contains(&owner.0)
        {
            continue;
        }
        let desired = desired_tool(
            activity.as_deref().copied(),
            cart.is_some() || carried.is_some_and(|load| !load.is_empty()),
        );
        let existing = children.get(attachment).ok().and_then(|children| {
            children.iter().find_map(|child| {
                existing_visuals
                    .get(child)
                    .ok()
                    .map(|visual| (child, visual.0))
            })
        });

        if existing.is_some_and(|(_, tool)| Some(tool) == desired) {
            continue;
        }
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        let Some(tool) = desired else {
            continue;
        };

        let scene = assets
            .scenes
            .entry(tool)
            .or_insert_with(|| asset_server.load(tool.scene_path()))
            .clone();
        commands.entity(attachment).with_children(|joint| {
            joint.spawn((
                Name::new(tool.label()),
                ToolVisual(tool),
                WorldAssetRoot(scene),
                // Tools are authored grip-at-origin in the attachment joint's
                // basis. Any correction here would conceal an asset contract bug.
                Transform::IDENTITY,
            ));
        });
    }
}

fn desired_body_animation(
    visual: &HeroVisual,
    anim: &HeroAnim,
    activity: Option<CharacterActivity>,
    carrying: bool,
    carting: bool,
    motion: Option<CharacterMotion>,
) -> (Option<AnimationNodeIndex>, f32, bool) {
    // A seated character can be translated by a moving parent such as a
    // vessel. That world-space speed is not locomotion: the feet must remain
    // in the authored seated pose while the boat moves beneath them.
    if activity == Some(CharacterActivity::Sitting) {
        return (anim.sit_idle.or(anim.idle), 1.0, false);
    }

    // Different start/stop thresholds keep small replicated speed noise from
    // continually restarting idle and walk.
    let current = anim.current_body;
    let was_locomoting = current.is_some()
        && (current == anim.walk || current == anim.carry || current == anim.pull);
    let observed_speed = visual
        .speed
        .max(motion.map_or(0.0, |motion| motion.velocity.length()));
    // Replicated motion is authoritative while a route is active. Visual
    // speed remains an important fallback for one-tick warp arrivals and
    // interpolation catch-up, where the server may already be stationary even
    // though the body is still visibly covering ground.
    let moving = motion.is_some_and(CharacterMotion::is_moving)
        || visual.speed > if was_locomoting { 0.10 } else { 0.24 };
    let stride_speed = (observed_speed / HERO_MOVE_SPEED).clamp(0.4, 1.6);

    if carting {
        return (
            anim.pull.or(anim.carry).or(anim.idle),
            if moving { stride_speed } else { 0.0 },
            !moving,
        );
    }
    if carrying {
        return (
            anim.carry.or(anim.idle),
            if moving { stride_speed } else { 0.0 },
            !moving,
        );
    }
    if moving {
        return (anim.walk.or(anim.idle), stride_speed, false);
    }

    let clip = match activity {
        Some(CharacterActivity::Chopping) => anim.chop,
        Some(CharacterActivity::Farming) => anim.harvest,
        Some(
            CharacterActivity::Fishing | CharacterActivity::Building | CharacterActivity::Mining,
        ) => anim.build,
        _ => anim.idle,
    }
    .or(anim.idle);
    (clip, 1.0, false)
}

/// Keep exactly one body animation active in steady state and at most two for
/// a short crossfade. The face loop remains a separate masked layer.
fn drive_hero_locomotion(
    time: Res<Time>,
    mut heroes: Query<(
        &HeroVisual,
        &mut HeroAnim,
        Option<&InheritedVisibility>,
        Option<&CharacterActivity>,
        Option<&CarriedLoad>,
        Option<&PorterCartState>,
        Option<&CharacterMotion>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (visual, mut anim, inherited, activity, carried, cart, motion) in heroes.iter_mut() {
        let hidden = inherited.is_some_and(|visibility| !visibility.get())
            || activity.is_some_and(|activity| *activity == CharacterActivity::Indoors);
        let Ok(mut player) = players.get_mut(anim.player) else {
            continue;
        };

        if hidden {
            if !anim.paused {
                player.pause_all();
                anim.paused = true;
            }
            continue;
        }
        if anim.paused {
            player.resume_all();
            anim.paused = false;
        }

        let carting = cart.is_some();
        let carrying = !carting && carried.is_some_and(|load| !load.is_empty());
        let (desired, speed, freeze_at_contact) = desired_body_animation(
            visual,
            &anim,
            activity.copied(),
            carrying,
            carting,
            motion.copied(),
        );
        let Some(desired) = desired else {
            continue;
        };

        if anim.current_body != Some(desired) || !player.is_playing_animation(desired) {
            if let Some(previous_fade) = anim.fading_body.take() {
                player.stop(previous_fade);
            }
            let previous = anim
                .current_body
                .filter(|previous| *previous != desired && player.is_playing_animation(*previous));
            // `start` deliberately restarts job clips at a readable contact
            // pose. Only the old and new body clips coexist during this fade.
            player
                .start(desired)
                .repeat()
                .set_weight(if previous.is_some() { 0.0 } else { 1.0 });
            anim.current_body = Some(desired);
            anim.fading_body = previous;
            anim.body_fade_seconds = 0.0;
        }

        if let Some(active) = player.animation_mut(desired) {
            if freeze_at_contact && active.seek_time() != 0.0 {
                active.set_seek_time(0.0);
            }
            if (active.speed() - speed).abs() > 0.01 {
                active.set_speed(speed);
            }
        }

        if let Some(fading) = anim.fading_body {
            anim.body_fade_seconds += time.delta_secs();
            let weight = (anim.body_fade_seconds / BODY_ANIMATION_FADE_SECONDS).clamp(0.0, 1.0);
            if let Some(active) = player.animation_mut(desired) {
                active.set_weight(weight);
            }
            if let Some(active) = player.animation_mut(fading) {
                active.set_weight(1.0 - weight);
            }
            if weight >= 1.0 {
                player.stop(fading);
                anim.fading_body = None;
            }
        } else if let Some(active) = player.animation_mut(desired) {
            if (active.weight() - 1.0).abs() > 0.01 {
                active.set_weight(1.0);
            }
        }
    }
}

#[cfg(test)]
mod carried_tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    fn queue_skin_primitive_despawn(
        mut commands: Commands,
        primitives: Query<Entity, With<GltfMaterialName>>,
    ) {
        for primitive in primitives.iter() {
            commands.entity(primitive).despawn();
        }
    }

    fn queue_animation_rig_despawn(
        mut commands: Commands,
        roots: Query<Entity, (With<CharacterKind>, With<HeroFullRig>)>,
    ) {
        for root in roots.iter() {
            commands.entity(root).despawn();
        }
    }

    fn animation_setup_test_app() -> App {
        let manifest = CharacterManifest::load().expect("shipped character manifest");
        let mut app = App::new();
        app.insert_resource(HeroManifest(manifest));
        app.init_resource::<Assets<Gltf>>();
        app.init_resource::<Assets<AnimationGraph>>();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<AnimationGraph>>()
            .add(AnimationGraph::new());
        app.insert_resource(HeroAssets {
            graph: Some(HeroGraph {
                handle,
                body: HashMap::new(),
                face: HashMap::new(),
            }),
            ..default()
        });
        app
    }

    #[test]
    fn skin_application_survives_a_same_frame_primitive_despawn() {
        let manifest = CharacterManifest::load().expect("shipped character manifest");
        let skin_material_name = manifest.skin.material.clone();
        let mut app = App::new();
        app.insert_resource(HeroManifest(manifest));
        app.init_resource::<HeroAssets>();
        app.init_resource::<Assets<StandardMaterial>>();

        let base = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let primitive = app
            .world_mut()
            .spawn((GltfMaterialName(skin_material_name), MeshMaterial3d(base)))
            .id();
        let hero = app
            .world_mut()
            .spawn((
                HeroVisual { speed: 0.0 },
                HeroFullRig,
                HeroOutfit::default(),
            ))
            .add_child(primitive)
            .id();

        // A world teardown can queue the glTF child for despawn before deferred
        // commands are flushed. Material mutation must not leave a stale
        // entity command.
        app.add_systems(
            Update,
            (queue_skin_primitive_despawn, apply_hero_skin).chain_ignore_deferred(),
        );
        app.update();

        assert!(app.world().get_entity(primitive).is_err());
        assert!(app.world().entity(hero).contains::<HeroSkinApplied>());
    }

    #[test]
    fn animation_setup_survives_a_same_frame_rig_despawn() {
        let mut app = animation_setup_test_app();
        let player = app.world_mut().spawn(AnimationPlayer::default()).id();
        let root = app
            .world_mut()
            .spawn((CharacterKind::Villager, HeroFullRig))
            .add_child(player)
            .id();

        // A disconnect/world teardown can invalidate both deferred insertion
        // targets after setup has already queried them. Fallible inserts must
        // make that cosmetic race a no-op rather than an EntityMut panic.
        app.add_systems(
            Update,
            (queue_animation_rig_despawn, setup_hero_animation).chain_ignore_deferred(),
        );
        app.update();

        assert!(app.world().get_entity(root).is_err());
        assert!(app.world().get_entity(player).is_err());
    }

    #[test]
    fn motion_extrapolation_bridges_one_packet_but_never_runs_away() {
        let position = Vec3::new(5.0, 2.0, -3.0);
        let velocity = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(
            extrapolated_motion_target(position, velocity, 0.03, 1.0),
            Vec3::new(5.3, 2.0, -3.0)
        );
        assert_eq!(
            extrapolated_motion_target(position, velocity, 2.0, 1.0),
            Vec3::new(5.8, 2.0, -3.0),
            "a missing packet must not turn into long-running client authority"
        );
        assert_eq!(
            extrapolated_motion_target(position, Vec3::ZERO, 2.0, 1.0),
            position
        );
    }

    #[test]
    fn accelerated_motion_keeps_the_same_visual_spatial_error() {
        let position = Vec3::new(5.0, 2.0, -3.0);
        let base_velocity = Vec3::new(HERO_MOVE_SPEED, 0.0, 0.0);
        let base_target = extrapolated_motion_target(position, base_velocity, 1.0, 1.0);
        let base_offset = base_target - position;

        for warp in [10.0_f32, 25.0, 100.0] {
            let target = extrapolated_motion_target(position, base_velocity * warp, 1.0, warp);
            assert!(
                target.distance(base_target) < 1.0e-5,
                "{warp}x extrapolated by {:?}, expected {:?}",
                target - position,
                base_offset,
            );

            let base_blend = visual_position_blend(1.0 / 60.0, 1.0);
            let warped_blend = visual_position_blend(1.0 / 60.0, warp);
            assert!(
                warped_blend > base_blend,
                "accelerated bodies must settle faster in real time"
            );
        }
    }

    #[test]
    fn every_carried_appearance_has_an_authored_scene() {
        for appearance in [
            CarriedAppearance::WoodBundle,
            CarriedAppearance::WheatSheaf,
            CarriedAppearance::FishBasket,
            CarriedAppearance::StoneBundle,
            CarriedAppearance::IronBundle,
            CarriedAppearance::FlourSack,
            CarriedAppearance::BreadBasket,
        ] {
            let spec = carried_asset_spec(appearance);
            assert!(spec.scene_path.ends_with(".glb#Scene0"));
        }
        assert_eq!(
            carried_asset_spec(CarriedAppearance::FlourSack).scene_path,
            "game_assets/resources/carried/FlourSack.glb#Scene0"
        );
        assert_eq!(
            carried_asset_spec(CarriedAppearance::BreadBasket).scene_path,
            "game_assets/resources/carried/BreadBasket.glb#Scene0"
        );
        let wood = carried_bundle_transform();
        assert_eq!(wood.scale, Vec3::splat(1.35));
        assert_eq!(wood.translation.y, 0.0);
        assert_eq!(wood.translation.z, -0.08);
    }

    #[test]
    fn porter_cart_asset_matches_the_runtime_node_and_clip_contract() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/game_assets/props/HandCart.glb");
        let bytes = std::fs::read(&path).expect("shipped handcart GLB exists");
        assert_eq!(&bytes[0..4], b"glTF");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("handcart GLB JSON parses");
        let nodes = document["nodes"].as_array().expect("handcart nodes");
        let node_index = |wanted: &str| {
            nodes
                .iter()
                .position(|node| node["name"].as_str() == Some(wanted))
                .unwrap_or_else(|| panic!("handcart node '{wanted}' is missing"))
        };
        let body = node_index("HandCartBody");
        let wheels = [node_index("HandCartWheelL"), node_index("HandCartWheelR")];
        for name in [
            "HandCart",
            "Anchor_Load.1",
            "Anchor_Load.2",
            "Anchor_GripL",
            "Anchor_GripR",
        ] {
            node_index(name);
        }

        let animations = document["animations"]
            .as_array()
            .expect("handcart animations");
        let animation = |wanted: &str| {
            animations
                .iter()
                .find(|animation| animation["name"].as_str() == Some(wanted))
                .unwrap_or_else(|| panic!("handcart clip '{wanted}' is missing"))
        };
        let pull_channels = animation("cart_pull")["channels"]
            .as_array()
            .expect("cart_pull channels");
        assert_eq!(pull_channels.len(), 1);
        assert_eq!(
            pull_channels[0]["target"]["node"].as_u64(),
            Some(body as u64)
        );
        assert_eq!(
            pull_channels[0]["target"]["path"].as_str(),
            Some("rotation")
        );

        let wheel_channels = animation("wheels_roll")["channels"]
            .as_array()
            .expect("wheels_roll channels");
        assert_eq!(wheel_channels.len(), 2);
        let mut wheel_targets: Vec<_> = wheel_channels
            .iter()
            .filter_map(|channel| channel["target"]["node"].as_u64())
            .map(|node| node as usize)
            .collect();
        wheel_targets.sort_unstable();
        let mut expected = wheels;
        expected.sort_unstable();
        assert_eq!(wheel_targets, expected);
    }

    #[test]
    fn wheel_roll_uses_distance_and_the_authored_negative_direction() {
        let angle = advanced_cart_wheel_angle(0.0, PORTER_CART_WHEEL_RADIUS);
        assert!((angle - (std::f32::consts::TAU - 1.0)).abs() < 1e-5);
        let circumference = std::f32::consts::TAU * PORTER_CART_WHEEL_RADIUS;
        let revolution = advanced_cart_wheel_angle(0.0, circumference);
        assert!(revolution < 1e-5 || (std::f32::consts::TAU - revolution) < 1e-5);
    }

    #[test]
    fn work_activity_selects_one_tool_and_carrying_selects_none() {
        assert_eq!(
            desired_tool(Some(CharacterActivity::Chopping), false),
            Some(ToolKind::Axe)
        );
        assert_eq!(
            desired_tool(Some(CharacterActivity::Farming), false),
            Some(ToolKind::Scythe)
        );
        assert_eq!(
            desired_tool(Some(CharacterActivity::Building), false),
            Some(ToolKind::Hammer)
        );
        assert_eq!(desired_tool(None, false), None);
        assert_eq!(desired_tool(Some(CharacterActivity::Chopping), true), None);
        assert_eq!(desired_tool(Some(CharacterActivity::Fishing), false), None);
    }

    #[test]
    fn a_seated_passenger_does_not_walk_when_the_vessel_moves() {
        let idle = AnimationNodeIndex::new(0);
        let walk = AnimationNodeIndex::new(1);
        let sit = AnimationNodeIndex::new(2);
        let anim = HeroAnim {
            player: Entity::from_bits(1),
            idle: Some(idle),
            walk: Some(walk),
            build: None,
            chop: None,
            harvest: None,
            carry: None,
            pull: None,
            sit_idle: Some(sit),
            current_body: Some(walk),
            fading_body: None,
            body_fade_seconds: 0.0,
            paused: false,
        };

        let (clip, speed, frozen) = desired_body_animation(
            &HeroVisual {
                speed: HERO_MOVE_SPEED,
            },
            &anim,
            Some(CharacterActivity::Sitting),
            false,
            false,
            None,
        );

        assert_eq!(clip, Some(sit));
        assert_eq!(speed, 1.0);
        assert!(!frozen);
    }

    #[test]
    fn farming_uses_harvest_without_leaking_into_the_build_clip() {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let idle = AnimationNodeIndex::new(0);
        let build = AnimationNodeIndex::new(1);
        let harvest = AnimationNodeIndex::new(2);
        let mut player = AnimationPlayer::default();
        player.play(idle).repeat().set_weight(1.0);
        let player_entity = world.spawn(player).id();
        world.spawn((
            HeroVisual { speed: 0.0 },
            HeroAnim {
                player: player_entity,
                idle: Some(idle),
                walk: None,
                build: Some(build),
                chop: None,
                harvest: Some(harvest),
                carry: None,
                pull: None,
                sit_idle: None,
                current_body: Some(idle),
                fading_body: None,
                body_fade_seconds: 0.0,
                paused: false,
            },
            CharacterActivity::Farming,
        ));

        world.run_system_once(drive_hero_locomotion).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(std::time::Duration::from_secs_f32(
                BODY_ANIMATION_FADE_SECONDS,
            ));
        world.run_system_once(drive_hero_locomotion).unwrap();

        let player = world.get::<AnimationPlayer>(player_entity).unwrap();
        assert_eq!(player.animation(harvest).unwrap().weight(), 1.0);
        assert!(player.animation(build).is_none());
        assert!(player.animation(idle).is_none());
        assert_eq!(player.playing_animations().count(), 1);
    }

    #[test]
    fn a_loaded_stationary_villager_freezes_in_the_carry_pose() {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let carry = AnimationNodeIndex::new(0);
        let idle = AnimationNodeIndex::new(1);
        let mut player = AnimationPlayer::default();
        player.play(idle).repeat().set_weight(1.0);
        let player_entity = world.spawn(player).id();
        world.spawn((
            HeroVisual { speed: 0.0 },
            HeroAnim {
                player: player_entity,
                idle: Some(idle),
                walk: None,
                build: None,
                chop: None,
                harvest: None,
                carry: Some(carry),
                pull: None,
                sit_idle: None,
                current_body: Some(idle),
                fading_body: None,
                body_fade_seconds: 0.0,
                paused: false,
            },
            CarriedLoad {
                good: Some(shared::economy::Good::Wood),
                amount: 1,
                appearance: Some(CarriedAppearance::WoodBundle),
            },
        ));

        world.run_system_once(drive_hero_locomotion).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(std::time::Duration::from_secs_f32(
                BODY_ANIMATION_FADE_SECONDS,
            ));
        world.run_system_once(drive_hero_locomotion).unwrap();

        let player = world.get::<AnimationPlayer>(player_entity).unwrap();
        let active_carry = player.animation(carry).unwrap();
        assert_eq!(active_carry.weight(), 1.0);
        assert_eq!(active_carry.speed(), 0.0);
        assert_eq!(active_carry.seek_time(), 0.0);
        assert!(player.animation(idle).is_none());
        assert_eq!(player.playing_animations().count(), 1);
    }

    #[test]
    fn an_active_porter_cart_walks_even_before_visual_interpolation_reports_speed() {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let idle = AnimationNodeIndex::new(0);
        let carry = AnimationNodeIndex::new(1);
        let pull = AnimationNodeIndex::new(2);
        let mut player = AnimationPlayer::default();
        player.play(idle).repeat().set_weight(1.0);
        let player_entity = world.spawn(player).id();
        world.spawn((
            HeroVisual { speed: 0.0 },
            HeroAnim {
                player: player_entity,
                idle: Some(idle),
                walk: None,
                build: None,
                chop: None,
                harvest: None,
                carry: Some(carry),
                pull: Some(pull),
                sit_idle: None,
                current_body: Some(idle),
                fading_body: None,
                body_fade_seconds: 0.0,
                paused: false,
            },
            CarriedLoad {
                good: Some(shared::economy::Good::Wood),
                amount: 24,
                appearance: Some(CarriedAppearance::WoodBundle),
            },
            PorterCartState { load_slots: 2 },
            CharacterMotion::new(Vec3::new(0.0, 0.0, -HERO_MOVE_SPEED)),
        ));

        world.run_system_once(drive_hero_locomotion).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(std::time::Duration::from_secs_f32(
                BODY_ANIMATION_FADE_SECONDS,
            ));
        world.run_system_once(drive_hero_locomotion).unwrap();

        let player = world.get::<AnimationPlayer>(player_entity).unwrap();
        let active_pull = player.animation(pull).unwrap();
        assert_eq!(active_pull.weight(), 1.0);
        assert_eq!(active_pull.speed(), 1.0);
        assert!(player.animation(carry).is_none());
        assert!(player.animation(idle).is_none());
        assert_eq!(player.playing_animations().count(), 1);
    }
}
