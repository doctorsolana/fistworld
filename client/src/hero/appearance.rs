//! Character assets, rig spawning, wardrobe, skin and indoor visibility.

use super::animation::{HeroGraph, RigMeshParts};
use super::motion::{HeroMotionSnapshot, HeroVisual};
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::character::CharacterManifest;
use shared::components::{
    CharacterActivity, CharacterKind, HeroOutfit, PlayerPosition, PlayerRotation,
};

/// The art build's character contract: wardrobe slots, skin palette and clip
/// names. Nothing here hardcodes node names — see shared/src/character.rs.
#[derive(Resource, Deref)]
pub struct HeroManifest(pub CharacterManifest);

/// Lazily-loaded character handles, shared by every hero instance.
#[derive(Resource, Default)]
pub struct HeroAssets {
    pub(super) scene: Option<Handle<WorldAsset>>,
    pub(super) gltf: Option<Handle<Gltf>>,
    /// Built once the glTF is loaded; every hero shares the same graph.
    pub(super) graph: Option<HeroGraph>,
    /// One recoloured skin material per tone index, SHARED by every hero
    /// wearing that tone (per-hero clones would cost a draw call each).
    pub(super) skin_materials: HashMap<u8, Handle<StandardMaterial>>,
    /// Materials already matted, so the GPU upload happens once each.
    pub(super) matted: bevy::platform::collections::HashSet<AssetId<StandardMaterial>>,
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
pub(super) struct HeroSceneRoot;

/// Root marker for an instantiated authored character rig.
#[derive(Component)]
pub(crate) struct HeroFullRig;

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
pub(super) fn attach_hero_visuals(
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
pub(super) fn dress_heroes(
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
pub(super) struct HeroSkinApplied(pub(super) u8);

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
pub(super) fn apply_hero_skin(
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

/// Matte every character material once.
///
/// MANDATORY, not cosmetic: the glb ships glTF-default specular and carries no
/// roughness factor at all, so Bevy's 0.5 defaults put a broad specular lobe
/// over near-black albedo — CHARACTER_PIPELINE.md §13 records dark hair
/// rendering mid-grey and auburn hair rendering skin-pink. The asset build
/// deliberately leaves this to the client (same contract as
/// `props::foliage::flatten_base`).
pub(super) fn matte_character_materials(
    mut commands: Commands,
    mut assets: ResMut<HeroAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    primitives: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        Added<MeshMaterial3d<StandardMaterial>>,
    >,
    parents: Query<&ChildOf>,
    rigs: Query<(), Or<(With<CharacterKind>, With<HeroPreviewRig>)>>,
    mut parts: Query<&mut RigMeshParts>,
) {
    // Roots seen for the first time this frame: several primitives of one
    // rig land in the same frame, so their list is assembled here and inserted
    // once instead of racing insert commands against each other.
    let mut fresh_parts: HashMap<Entity, Vec<Entity>> = HashMap::new();
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
        // Every primitive is a mesh part the visibility test consults, whether
        // or not its material still needs matting.
        // Deduplicated: a re-inserted material component re-triggers `Added`
        // for a primitive already on the list.
        match parts.get_mut(ancestor) {
            Ok(mut parts) => {
                if !parts.0.contains(&entity) {
                    parts.0.push(entity);
                }
            }
            Err(_) => {
                let list = fresh_parts.entry(ancestor).or_default();
                if !list.contains(&entity) {
                    list.push(entity);
                }
            }
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
    for (root, list) in fresh_parts {
        commands.entity(root).try_insert(RigMeshParts(list));
    }
}

/// A building is still an exterior shell today, so crossing its authored door
/// hides the character until they come back out. The simulation keeps them at
/// a shallow point beyond the threshold; visible interiors can later replace
/// this with an interior scene without changing the worker state machine.
pub(super) fn sync_indoor_visibility(
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
