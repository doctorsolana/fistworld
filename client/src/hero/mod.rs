//! Hero rendering + control: the client half of the embodied character.
//!
//! The server owns hero position/rotation (see server/src/player/hero.rs);
//! this module attaches the canonical humanoid glb to replicated Hero entities,
//! dresses them from their replicated [`HeroOutfit`], drives the walk
//! animation from observed velocity, smooths the streamed transform, and
//! turns clicks into spawn/move commands.

pub mod control;

use bevy::animation::AnimationTargetId;
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::character::CharacterManifest;
use shared::components::{
    CharacterActivity, CharacterKind, HeroOutfit, PlayerPosition, PlayerRotation,
};
use shared::economy::{CarriedAppearance, CarriedLoad};
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
        app.init_resource::<ToolAssets>();
        app.init_resource::<control::HeroSpawnArm>();
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
                    tag_builders,
                    tag_carry_attachments,
                    tag_tool_attachments,
                    sync_indoor_visibility,
                    sync_carried_load_visuals,
                    sync_tool_visuals,
                    drive_hero_locomotion,
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
    idle: Option<AnimationNodeIndex>,
    walk: Option<AnimationNodeIndex>,
    build: Option<AnimationNodeIndex>,
    chop: Option<AnimationNodeIndex>,
    harvest: Option<AnimationNodeIndex>,
    carry: Option<AnimationNodeIndex>,
    sit_idle: Option<AnimationNodeIndex>,
}

/// The authored resource scene parented to the rig's `attach.carry` joint.
#[derive(Component)]
struct CarriedLoadVisual(CarriedAppearance);

/// An authored rig node that accepts the current carried-goods visual.
#[derive(Component)]
struct CarryAttachment;

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

/// Marks a character the client believes is working on a building.
///
/// DERIVED from a replicated construction site standing next to them, not sent.
/// The server already tells us where every site is and where every villager is;
/// "that villager is building" is the two facts read together, and sending it
/// as a third would be sending the same thing twice — the same reason the moot
/// hall is drawn from the settlement's position rather than replicated.
#[derive(Component)]
struct Building;

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
            ))
            .with_children(|root| {
                spawn_character_scene_child(root, &asset_server, &mut assets, &manifest);
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
    manifest: Res<HeroManifest>,
    heroes: Query<(Entity, &HeroOutfit), (With<HeroVisual>, Without<HeroDressed>)>,
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
    heroes: Query<(Entity, &HeroOutfit, Option<&HeroSkinApplied>), With<HeroVisual>>,
    children_q: Query<&Children>,
    primitives: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (hero, outfit, applied) in heroes.iter() {
        if applied.is_some_and(|applied| applied.0 == outfit.skin) {
            continue;
        }

        let mut stack = vec![hero];
        let mut target = None;
        while let Some(node) = stack.pop() {
            if let Ok((material_name, mesh_material)) = primitives.get(node) {
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

        commands.entity(primitive).insert(MeshMaterial3d(handle));
        commands.entity(hero).insert(HeroSkinApplied(outfit.skin));
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
    rigs: Query<Entity, Or<(With<CharacterKind>, With<HeroPreviewRig>)>>,
    children_q: Query<&Children>,
    primitives: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    for rig in rigs.iter() {
        let mut stack = vec![rig];
        while let Some(node) = stack.pop() {
            if let Ok(mesh_material) = primitives.get(node) {
                let id = mesh_material.0.id();
                // Materials are shared across every hero, so this runs ~12
                // times total. Re-mutating would re-upload to the GPU.
                if assets.matted.insert(id) {
                    if let Some(mut material) = materials.get_mut(&mesh_material.0) {
                        crate::props::foliage::flatten_base(&mut material);
                    } else {
                        // Not loaded yet — retry next frame.
                        assets.matted.remove(&id);
                    }
                }
            }
            if let Ok(children) = children_q.get(node) {
                stack.extend(children.iter());
            }
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
    rigs: Query<(), Or<(With<CharacterKind>, With<HeroPreviewRig>)>>,
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
            if rigs.get(ancestor).is_ok() {
                rig_root = Some(ancestor);
                break;
            }
        }
        let Some(rig_root) = rig_root else {
            continue;
        };

        commands
            .entity(player_entity)
            .insert(AnimationGraphHandle(hero_graph.handle.clone()));

        // Locomotion: both loops always active, cross-faded by weight (see
        // drive_hero_locomotion). RepeatAnimation::Never is the default, so
        // the explicit repeat() matters.
        let idle = hero_graph.body.get(CLIP_IDLE).copied();
        let walk = hero_graph.body.get(CLIP_WALK).copied();
        let build = hero_graph.body.get(CLIP_BUILD).copied();
        let chop = hero_graph.body.get(CLIP_CHOP).copied();
        let harvest = hero_graph.body.get(CLIP_HARVEST).copied();
        let carry = hero_graph.body.get(CLIP_CARRY).copied();
        let sit_idle = hero_graph.body.get(CLIP_SIT_IDLE).copied();
        if let Some(idle) = idle {
            player.play(idle).repeat().set_weight(1.0);
        }
        if let Some(walk) = walk {
            player.play(walk).repeat().set_weight(0.0);
        }
        // Started at zero like the others and cross-faded in, rather than
        // played on demand: every locomotion clip is always resident, so the
        // blend never has to wait for a clip to start.
        if let Some(build) = build {
            player.play(build).repeat().set_weight(0.0);
        }
        if let Some(chop) = chop {
            player.play(chop).repeat().set_weight(0.0);
        }
        if let Some(harvest) = harvest {
            player.play(harvest).repeat().set_weight(0.0);
        }
        if let Some(carry) = carry {
            player.play(carry).repeat().set_weight(0.0);
        }
        if let Some(sit_idle) = sit_idle {
            player.play(sit_idle).repeat().set_weight(0.0);
        }
        // Resting expression on the face layer; body clips cannot touch it.
        if let Some(face_idle) = hero_graph.face.get(CLIP_FACE_IDLE).copied() {
            player.play(face_idle).repeat().set_weight(1.0);
        }

        if rig_roots.get(rig_root).is_ok() {
            commands.entity(rig_root).insert(HeroAnim {
                player: player_entity,
                idle,
                walk,
                build,
                chop,
                harvest,
                carry,
                sit_idle,
            });
        }
        info!("Hero animation configured for {rig_root:?}");
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

/// Cross-fade idle<->walk by observed visual speed, and scale the walk cycle
/// so the feet track what the eye actually sees.
///
/// Hidden rigs (the parked creator preview) freeze outright: animation
/// evaluation runs regardless of visibility, and a character nobody can see
/// must not sample 16 bones per frame forever.
/// Decide who is swinging a hammer.
///
/// A character standing on a live construction site is building. That is the
/// whole rule, and it is derived from two things the server already sends:
/// where the sites are and where the people are. The server's own
/// `VillagerIntent::Building` never leaves the server, and it does not need to.
///
/// Radius rather than exact position because the builder stops within
/// `BUILD_REACH` of the plot centre, not on it, and because a villager who
/// wanders a metre while working should not flicker between clips.
fn tag_builders(
    mut commands: Commands,
    sites: Query<&PlayerPosition, With<shared::components::ConstructionSite>>,
    characters: Query<
        (Entity, &PlayerPosition, Option<&Building>),
        With<shared::components::CharacterKind>,
    >,
) {
    /// Slightly wider than the server's own BUILD_REACH, so the clip is playing
    /// by the time the walk stops rather than a frame later.
    const BUILD_ANIM_RADIUS: f32 = 6.0;

    if sites.is_empty() {
        // Nothing under construction anywhere: clear every tag in one pass
        // rather than distance-checking against an empty set.
        for (entity, _, building) in characters.iter() {
            if building.is_some() {
                commands.entity(entity).remove::<Building>();
            }
        }
        return;
    }

    for (entity, at, building) in characters.iter() {
        let on_site = sites
            .iter()
            .any(|site| site.0.distance(at.0) <= BUILD_ANIM_RADIUS);
        match (on_site, building.is_some()) {
            (true, false) => {
                commands.entity(entity).insert(Building);
            }
            (false, true) => {
                commands.entity(entity).remove::<Building>();
            }
            _ => {}
        }
    }
}

/// A building is still an exterior shell today, so crossing its authored door
/// hides the character until they come back out. The simulation keeps them at
/// a shallow point beyond the threshold; visible interiors can later replace
/// this with an interior scene without changing the worker state machine.
fn sync_indoor_visibility(
    characters: Query<(Option<&CharacterActivity>, Has<HeroDressed>), With<HeroVisual>>,
    mut scene_roots: Query<(&ChildOf, &mut Visibility), With<HeroSceneRoot>>,
) {
    for (parent, mut visibility) in scene_roots.iter_mut() {
        let Ok((activity, dressed)) = characters.get(parent.parent()) else {
            continue;
        };
        if !dressed {
            continue;
        }
        let target = if activity.is_some_and(|activity| *activity == CharacterActivity::Indoors) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != target {
            *visibility = target;
        }
    }
}

/// Parent the matching authored resource bundle to the rig's carry attachment.
fn tag_carry_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<CarryAttachment>)>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() == "attach.carry" {
            commands.entity(entity).insert(CarryAttachment);
        }
    }
}

/// Tag the authored right-hand tool joint once its glTF node is instantiated.
fn tag_tool_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<ToolAttachment>)>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() == "attach.tool.R" {
            commands.entity(entity).insert(ToolAttachment);
        }
    }
}

fn sync_carried_load_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<CarriedLoadAssets>,
    attachments: Query<Entity, With<CarryAttachment>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    loads: Query<&CarriedLoad, With<CharacterKind>>,
    existing_visuals: Query<&CarriedLoadVisual>,
) {
    for attachment in attachments.iter() {
        let mut ancestor = attachment;
        let desired = loop {
            let Ok(parent) = parents.get(ancestor) else {
                break None;
            };
            ancestor = parent.parent();
            if let Ok(load) = loads.get(ancestor) {
                break load.visible_appearance();
            }
        };
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
    }
}

fn desired_tool(
    activity: Option<CharacterActivity>,
    building: bool,
    carrying: bool,
) -> Option<ToolKind> {
    if carrying {
        return None;
    }
    match activity {
        Some(CharacterActivity::Chopping) => Some(ToolKind::Axe),
        Some(CharacterActivity::Farming) => Some(ToolKind::Scythe),
        Some(CharacterActivity::Building) => Some(ToolKind::Hammer),
        _ if building => Some(ToolKind::Hammer),
        _ => None,
    }
}

/// Attach only the tool required by the character's current visible work.
///
/// Construction-site building is still derived client-side from proximity, so
/// the `Building` marker participates alongside the replicated activity. A
/// physical load always wins: a villager carrying wood cannot also hold a tool.
fn sync_tool_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<ToolAssets>,
    attachments: Query<Entity, With<ToolAttachment>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    characters: Query<
        (
            Option<&CharacterActivity>,
            Has<Building>,
            Option<&CarriedLoad>,
        ),
        With<CharacterKind>,
    >,
    existing_visuals: Query<&ToolVisual>,
) {
    for attachment in attachments.iter() {
        let mut ancestor = attachment;
        let desired = loop {
            let Ok(parent) = parents.get(ancestor) else {
                break None;
            };
            ancestor = parent.parent();
            if let Ok((activity, building, carried)) = characters.get(ancestor) {
                break desired_tool(
                    activity.copied(),
                    building,
                    carried.is_some_and(|load| !load.is_empty()),
                );
            }
        };
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

fn drive_hero_locomotion(
    heroes: Query<(
        &HeroVisual,
        &HeroAnim,
        Option<&InheritedVisibility>,
        Option<&Building>,
        Option<&CharacterActivity>,
        Option<&CarriedLoad>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (visual, anim, inherited, building, activity, carried) in heroes.iter() {
        let hidden = inherited.is_some_and(|visibility| !visibility.get())
            || activity.is_some_and(|activity| *activity == CharacterActivity::Indoors);
        let Ok(mut player) = players.get_mut(anim.player) else {
            continue;
        };

        // 0 standing .. 1 full stride, with a small dead zone so replication
        // jitter cannot flutter the blend.
        let motion_blend = if hidden {
            0.0
        } else {
            ((visual.speed - 0.15) / (HERO_MOVE_SPEED * 0.6)).clamp(0.0, 1.0)
        };
        let carrying = carried.is_some_and(|load| !load.is_empty()) && !hidden;
        let walk_blend = if carrying { 0.0 } else { motion_blend };
        // A loaded character owns the whole body layer even while standing.
        // Otherwise the old idle clip pulls both arms away while the bundle
        // remains attached at chest height.
        let carry_blend = if carrying { 1.0 } else { 0.0 };
        let sit_blend = if !carrying
            && !hidden
            && activity.is_some_and(|activity| *activity == CharacterActivity::Sitting)
        {
            1.0 - motion_blend
        } else {
            0.0
        };
        let chop_blend = if !carrying
            && !hidden
            && activity.is_some_and(|activity| *activity == CharacterActivity::Chopping)
        {
            1.0 - motion_blend
        } else {
            0.0
        };
        let harvest_blend = if !carrying
            && !hidden
            && activity.is_some_and(|activity| *activity == CharacterActivity::Farming)
        {
            1.0 - motion_blend
        } else {
            0.0
        };
        // Building wins over standing but NOT over walking: someone still
        // crossing the plot should be seen walking, and the hammer starts when
        // they stop. Falls out of the blend for free rather than needing a
        // state machine.
        let field_work = activity.is_some_and(|activity| *activity == CharacterActivity::Fishing);
        let road_building =
            activity.is_some_and(|activity| *activity == CharacterActivity::Building);
        let build_blend = if !carrying
            && (building.is_some() || field_work || road_building)
            && !hidden
            && chop_blend == 0.0
            && harvest_blend == 0.0
        {
            1.0 - motion_blend
        } else {
            0.0
        };
        let stride_speed = if hidden {
            0.0
        } else {
            (visual.speed / HERO_MOVE_SPEED).clamp(0.4, 1.6)
        };

        if let Some(walk) = anim.walk {
            if let Some(active) = player.animation_mut(walk) {
                if (active.weight() - walk_blend).abs() > 0.01 {
                    active.set_weight(walk_blend);
                }
                if (active.speed() - stride_speed).abs() > 0.01 {
                    active.set_speed(stride_speed);
                }
            }
        }
        if let Some(build) = anim.build {
            if let Some(active) = player.animation_mut(build) {
                if (active.weight() - build_blend).abs() > 0.01 {
                    active.set_weight(build_blend);
                }
                let build_speed = if hidden { 0.0 } else { 1.0 };
                if (active.speed() - build_speed).abs() > 0.01 {
                    active.set_speed(build_speed);
                }
            }
        }
        if let Some(chop) = anim.chop {
            if let Some(active) = player.animation_mut(chop) {
                if (active.weight() - chop_blend).abs() > 0.01 {
                    active.set_weight(chop_blend);
                }
                let chop_speed = if hidden { 0.0 } else { 1.0 };
                if (active.speed() - chop_speed).abs() > 0.01 {
                    active.set_speed(chop_speed);
                }
            }
        }
        if let Some(harvest) = anim.harvest {
            if let Some(active) = player.animation_mut(harvest) {
                if (active.weight() - harvest_blend).abs() > 0.01 {
                    active.set_weight(harvest_blend);
                }
                let harvest_speed = if hidden { 0.0 } else { 1.0 };
                if (active.speed() - harvest_speed).abs() > 0.01 {
                    active.set_speed(harvest_speed);
                }
            }
        }
        if let Some(carry) = anim.carry {
            if let Some(active) = player.animation_mut(carry) {
                if (active.weight() - carry_blend).abs() > 0.01 {
                    active.set_weight(carry_blend);
                }
                let carry_speed = if carrying && motion_blend > 0.0 {
                    stride_speed
                } else {
                    0.0
                };
                if carrying && motion_blend == 0.0 && active.seek_time() != 0.0 {
                    // Frame one is the neutral contact pose: legs centred,
                    // both arms closed around the load. Freeze there until the
                    // villager actually moves again.
                    active.set_seek_time(0.0);
                }
                if (active.speed() - carry_speed).abs() > 0.01 {
                    active.set_speed(carry_speed);
                }
            }
        }
        if let Some(sit_idle) = anim.sit_idle {
            if let Some(active) = player.animation_mut(sit_idle) {
                if (active.weight() - sit_blend).abs() > 0.01 {
                    active.set_weight(sit_blend);
                }
                let sit_speed = if hidden { 0.0 } else { 1.0 };
                if (active.speed() - sit_speed).abs() > 0.01 {
                    active.set_speed(sit_speed);
                }
            }
        }
        if let Some(idle) = anim.idle {
            if let Some(active) = player.animation_mut(idle) {
                let idle_weight = 1.0
                    - walk_blend
                    - carry_blend
                    - build_blend
                    - chop_blend
                    - harvest_blend
                    - sit_blend;
                if (active.weight() - idle_weight).abs() > 0.01 {
                    active.set_weight(idle_weight);
                }
                // Freeze rather than idle-breathe an invisible preview.
                let idle_speed = if hidden { 0.0 } else { 1.0 };
                if (active.speed() - idle_speed).abs() > 0.01 {
                    active.set_speed(idle_speed);
                }
            }
        }
    }
}

#[cfg(test)]
mod carried_tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    #[test]
    fn every_carried_appearance_has_an_authored_scene() {
        for appearance in [
            CarriedAppearance::WoodBundle,
            CarriedAppearance::WheatSheaf,
            CarriedAppearance::FishBasket,
            CarriedAppearance::StoneBundle,
            CarriedAppearance::IronBundle,
        ] {
            let spec = carried_asset_spec(appearance);
            assert!(spec.scene_path.ends_with(".glb#Scene0"));
        }
        let wood = carried_bundle_transform();
        assert_eq!(wood.scale, Vec3::splat(1.35));
        assert_eq!(wood.translation.y, 0.0);
        assert_eq!(wood.translation.z, -0.08);
    }

    #[test]
    fn work_activity_selects_one_tool_and_carrying_selects_none() {
        assert_eq!(
            desired_tool(Some(CharacterActivity::Chopping), false, false),
            Some(ToolKind::Axe)
        );
        assert_eq!(
            desired_tool(Some(CharacterActivity::Farming), false, false),
            Some(ToolKind::Scythe)
        );
        assert_eq!(
            desired_tool(Some(CharacterActivity::Building), false, false),
            Some(ToolKind::Hammer)
        );
        assert_eq!(desired_tool(None, true, false), Some(ToolKind::Hammer));
        assert_eq!(
            desired_tool(Some(CharacterActivity::Chopping), false, true),
            None
        );
        assert_eq!(
            desired_tool(Some(CharacterActivity::Fishing), false, false),
            None
        );
    }

    #[test]
    fn farming_uses_harvest_without_leaking_into_the_build_clip() {
        let mut world = World::new();
        let idle = AnimationNodeIndex::new(0);
        let build = AnimationNodeIndex::new(1);
        let harvest = AnimationNodeIndex::new(2);
        let mut player = AnimationPlayer::default();
        player.play(idle).repeat().set_weight(1.0);
        player.play(build).repeat().set_weight(0.0);
        player.play(harvest).repeat().set_weight(0.0);
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
                sit_idle: None,
            },
            CharacterActivity::Farming,
        ));

        world.run_system_once(drive_hero_locomotion).unwrap();

        let player = world.get::<AnimationPlayer>(player_entity).unwrap();
        assert_eq!(player.animation(harvest).unwrap().weight(), 1.0);
        assert_eq!(player.animation(build).unwrap().weight(), 0.0);
        assert_eq!(player.animation(idle).unwrap().weight(), 0.0);
    }

    #[test]
    fn a_loaded_stationary_villager_freezes_in_the_carry_pose() {
        let mut world = World::new();
        let carry = AnimationNodeIndex::new(0);
        let idle = AnimationNodeIndex::new(1);
        let mut player = AnimationPlayer::default();
        player
            .play(carry)
            .repeat()
            .set_weight(0.0)
            .set_seek_time(0.5);
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
                sit_idle: None,
            },
            CarriedLoad {
                good: Some(shared::economy::Good::Wood),
                amount: 1,
                appearance: Some(CarriedAppearance::WoodBundle),
            },
        ));

        world.run_system_once(drive_hero_locomotion).unwrap();

        let player = world.get::<AnimationPlayer>(player_entity).unwrap();
        let active_carry = player.animation(carry).unwrap();
        assert_eq!(active_carry.weight(), 1.0);
        assert_eq!(active_carry.speed(), 0.0);
        assert_eq!(active_carry.seek_time(), 0.0);
        assert_eq!(player.animation(idle).unwrap().weight(), 0.0);
    }
}
