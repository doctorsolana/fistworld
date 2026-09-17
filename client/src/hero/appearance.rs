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
    /// Merged outfit meshes, shared by every rig wearing the same combination.
    pub(super) merged_meshes: HashMap<MergeKey, Handle<Mesh>>,
    /// The one white, vertex-coloured material every merged outfit uses.
    pub(super) merged_material: Option<Handle<StandardMaterial>>,
    /// Materials already matted, so the GPU upload happens once each.
    pub(super) matted: bevy::platform::collections::HashSet<AssetId<StandardMaterial>>,
}

/// A local, non-replicated character rig (the creator preview). Shares the
/// hero dressing/animation systems but has no server-side existence.
#[derive(Component)]
pub struct HeroPreviewRig;

/// Mesh primitives of a wardrobe item that is not worn, despawned to keep
/// ~18 unused items per rig out of transform propagation, visibility,
/// skinned-bounds updates and extraction. Rebuilt when the item is worn.
#[derive(Component)]
pub(super) struct WardrobeStash(pub(super) Vec<StashedPrimitive>);

#[derive(Clone)]
pub(super) struct StashedPrimitive {
    pub(super) name: Name,
    pub(super) mesh: Handle<Mesh>,
    pub(super) material: Handle<StandardMaterial>,
    pub(super) skin: Option<bevy::mesh::skinning::SkinnedMesh>,
    pub(super) aabb: Option<bevy::camera::primitives::Aabb>,
    pub(super) transform: Transform,
    /// The body's skin primitive: its colour is the outfit's skin tone.
    pub(super) skin_material: bool,
}

/// Test/tooling override: merge outfits regardless of the env switch.
#[derive(Resource, Default)]
pub(super) struct ForceOutfitMerge;

/// Identity of a merged outfit mesh: the merged node names (sorted) and the
/// skin tone. Every rig wearing the same combination shares one mesh asset.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct MergeKey {
    pub(super) parts: Vec<String>,
    pub(super) skin: u8,
}

/// The one skinned entity that draws this rig's body plus every worn
/// untextured wardrobe item (see `merge_outfit_mesh`). Textured items (hair)
/// stay their own entities until they share an atlas.
#[derive(Component)]
pub(super) struct HeroMerged {
    pub(super) entity: Entity,
    pub(super) key: MergeKey,
}

/// Build one skinned mesh from `parts`, each `(mesh, linear rgba colour)`.
///
/// Bevy's `Mesh::merge` silently skips attributes the other mesh lacks, which
/// would misalign vertices, so every part is first normalised to the same
/// attribute set: position, normal, uv (zeros when absent), colour (the
/// material colour, multiplied into baked vertex colours when present) and
/// the shared skin's joint indices/weights. Indices are widened to u32.
pub(super) fn merge_outfit_mesh(parts: &[(&Mesh, [f32; 4])]) -> Result<Mesh, String> {
    use bevy::mesh::{Indices, VertexAttributeValues};
    let mut merged: Option<Mesh> = None;
    for (index, (source, colour)) in parts.iter().enumerate() {
        let count = source.count_vertices();
        let mut part = Mesh::new(
            source.primitive_topology(),
            bevy::asset::RenderAssetUsages::default(),
        );
        for attribute in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_JOINT_INDEX,
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
        ] {
            let Some(values) = source.attribute(attribute) else {
                return Err(format!("part {index} lacks {}", attribute.name));
            };
            part.insert_attribute(attribute, values.clone());
        }
        let uv = match source.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uv)) => uv.clone(),
            Some(_) => return Err(format!("part {index} has a non-f32 uv layout")),
            None => vec![[0.0, 0.0]; count],
        };
        part.insert_attribute(Mesh::ATTRIBUTE_UV_0, VertexAttributeValues::Float32x2(uv));
        let colours = match source.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(baked)) => baked
                .iter()
                .map(|c| {
                    [
                        c[0] * colour[0],
                        c[1] * colour[1],
                        c[2] * colour[2],
                        c[3] * colour[3],
                    ]
                })
                .collect(),
            Some(_) => return Err(format!("part {index} has a non-f32 colour layout")),
            None => vec![*colour; count],
        };
        part.insert_attribute(Mesh::ATTRIBUTE_COLOR, VertexAttributeValues::Float32x4(colours));
        let indices: Vec<u32> = match source.indices() {
            Some(indices) => indices.iter().map(|i| i as u32).collect(),
            None => (0..count as u32).collect(),
        };
        part.insert_indices(Indices::U32(indices));
        match merged.as_mut() {
            None => merged = Some(part),
            Some(target) => target
                .merge(&part)
                .map_err(|error| format!("part {index}: {error:?}"))?,
        }
    }
    let mut merged = merged.ok_or_else(|| "no parts to merge".to_string())?;
    merged
        .generate_skinned_mesh_bounds()
        .map_err(|error| format!("skinned bounds: {error:?}"))?;
    Ok(merged)
}

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

/// Dress a rig to match its replicated outfit.
///
/// Runs until the async scene instantiation exposes every wardrobe node, then
/// stamps [`HeroDressed`]. Outfit changes re-run it via the marker removal
/// below. Per wardrobe node the outcome is one of:
/// - **live**: worn, primitives present (hair, or everything when merging is
///   off);
/// - **stashed**: unworn, or worn but absorbed into the merged outfit mesh;
///   primitives despawned, [`WardrobeStash`] on the node rebuilds them.
/// With merging on, the body and every worn *untextured* item become ONE
/// skinned entity ([`HeroMerged`]), cached per [`MergeKey`].
#[allow(clippy::too_many_arguments)]
pub(super) fn dress_heroes(
    mut commands: Commands,
    manifest: Res<HeroManifest>,
    mut assets: ResMut<HeroAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    heroes: Query<
        (Entity, &HeroOutfit, Option<&HeroMerged>),
        (With<HeroVisual>, With<HeroFullRig>, Without<HeroDressed>),
    >,
    changed: Query<Entity, (With<HeroDressed>, Changed<HeroOutfit>)>,
    children_q: Query<&Children>,
    mut named: Query<(&Name, &mut Visibility)>,
    mut scene_roots: Query<(Entity, &mut Visibility), (With<HeroSceneRoot>, Without<Name>)>,
    primitives: Query<(
        &Name,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
        Option<&bevy::mesh::skinning::SkinnedMesh>,
        Option<&bevy::camera::primitives::Aabb>,
        &Transform,
        Option<&GltfMaterialName>,
    )>,
    stashes: Query<&WardrobeStash>,
    mut switches: Local<Option<(bool, bool, bool)>>,
    force_merge: Option<Res<ForceOutfitMerge>>,
) {
    // `FISTFORCE_WARDROBE_STASH_OFF=1`: keep unworn primitives alive (hidden).
    // `FISTFORCE_MERGE_OUTFITS=1`: merge the body and worn untextured items
    // into one skinned entity per rig. OFF BY DEFAULT: measured 2026-09-17 in
    // the 1000-villager lab it halves mesh entities (5.8k -> 2.8k) but buys
    // 0-1.5 ms, within run noise, so it is not worth its churn yet. See
    // docs/CROWD-MESH-MERGE-PLAN.md for the measurements.
    // `FISTFORCE_RIG_VIS_DIAG=1`: log the dressing outcome per rig.
    let (stash_off, mut merge_off, diag) = *switches.get_or_insert_with(|| {
        (
            std::env::var("FISTFORCE_WARDROBE_STASH_OFF").is_ok(),
            std::env::var("FISTFORCE_MERGE_OUTFITS").is_err(),
            std::env::var("FISTFORCE_RIG_VIS_DIAG").is_ok(),
        )
    });
    if force_merge.is_some() {
        merge_off = false;
    }
    // Every wardrobe item across all slots must be present before dressing,
    // or a half-instantiated scene would show the whole closet for a frame.
    let wardrobe_node_count: usize = manifest.slots.iter().map(|slot| slot.items.len()).sum();
    for entity in changed.iter() {
        commands.entity(entity).remove::<HeroDressed>();
    }

    for (hero, outfit, merged) in heroes.iter() {
        // 1. Find the wardrobe nodes and the body node.
        let mut found = 0usize;
        let mut stack = vec![hero];
        let mut nodes: Vec<(Entity, String, bool)> = Vec::new();
        let mut body_node = None;
        while let Some(node) = stack.pop() {
            if let Ok((name, _)) = named.get(node) {
                let name = name.as_str();
                // EXACT node names only: glTF primitive children carry
                // suffixed names ("Hair_Afro.0"); hiding those explicitly
                // would override their shown parent and bald the hero.
                if manifest.is_wardrobe_node(name) {
                    found += 1;
                    nodes.push((node, name.to_string(), !outfit.hides_node(&manifest, name)));
                } else if name == manifest.body {
                    body_node = Some(node);
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

        // 2. What each node's primitives are, live or stashed.
        let gather = |node: Entity| -> (bool, Vec<StashedPrimitive>) {
            if let Ok(stash) = stashes.get(node) {
                return (false, stash.0.clone());
            }
            let mut parts = Vec::new();
            if let Ok(children) = children_q.get(node) {
                for child in children.iter() {
                    if let Ok((name, mesh, material, skin, aabb, transform, material_name)) =
                        primitives.get(child)
                    {
                        parts.push(StashedPrimitive {
                            name: name.clone(),
                            mesh: mesh.0.clone(),
                            material: material.0.clone(),
                            skin: skin.cloned(),
                            aabb: aabb.copied(),
                            transform: *transform,
                            skin_material: material_name
                                .is_some_and(|name| name.0 == manifest.skin.material),
                        });
                    }
                }
            }
            (true, parts)
        };
        let mut states: Vec<(Entity, bool, bool, Vec<StashedPrimitive>)> = nodes
            .iter()
            .map(|(node, _, worn)| {
                let (live, parts) = gather(*node);
                (*node, *worn, live, parts)
            })
            .collect();
        let body = body_node.map(|node| {
            let (live, parts) = gather(node);
            (node, live, parts)
        });

        // 3. Merge the body and every worn untextured item into one mesh.
        let mut absorbed: Vec<Entity> = Vec::new();
        let mut merged_spawn: Option<(Handle<Mesh>, bevy::mesh::skinning::SkinnedMesh, MergeKey)> =
            None;
        let mut keep_merged = false;
        if !merge_off && !stash_off {
            if let Some((body_entity, _, body_parts)) = &body {
                let textureless = |parts: &[StashedPrimitive]| {
                    !parts.is_empty()
                        && parts.iter().all(|part| {
                            materials
                                .get(&part.material)
                                .is_some_and(|material| material.base_color_texture.is_none())
                        })
                };
                let mut candidates: Vec<(Entity, &str, &Vec<StashedPrimitive>)> = states
                    .iter()
                    .zip(nodes.iter())
                    .filter(|((_, worn, _, parts), _)| *worn && textureless(parts))
                    .map(|((node, _, _, parts), (_, name, _))| (*node, name.as_str(), parts))
                    .collect();
                candidates.sort_by(|a, b| a.1.cmp(b.1));
                if textureless(body_parts) {
                    let key = MergeKey {
                        parts: std::iter::once(manifest.body.clone())
                            .chain(candidates.iter().map(|(_, name, _)| name.to_string()))
                            .collect(),
                        skin: outfit.skin,
                    };
                    let skin = body_parts.iter().find_map(|part| part.skin.clone());
                    if merged.is_some_and(|merged| merged.key == key) {
                        keep_merged = true;
                        absorbed.push(*body_entity);
                        absorbed.extend(candidates.iter().map(|(node, _, _)| *node));
                    } else if let Some(skin) = skin {
                        let handle = match assets.merged_meshes.get(&key) {
                            Some(handle) => Ok(handle.clone()),
                            None => {
                                let tone = manifest.skin_color(outfit.skin);
                                let colour_of = |part: &StashedPrimitive| -> Option<[f32; 4]> {
                                    if part.skin_material {
                                        return Some([tone[0], tone[1], tone[2], 1.0]);
                                    }
                                    let linear = materials.get(&part.material)?.base_color.to_linear();
                                    Some([linear.red, linear.green, linear.blue, linear.alpha])
                                };
                                let mut inputs: Vec<(&Mesh, [f32; 4])> = Vec::new();
                                let mut complete = true;
                                for part in body_parts
                                    .iter()
                                    .chain(candidates.iter().flat_map(|(_, _, parts)| parts.iter()))
                                {
                                    match (meshes.get(&part.mesh), colour_of(part)) {
                                        (Some(mesh), Some(colour)) => inputs.push((mesh, colour)),
                                        _ => {
                                            complete = false;
                                            break;
                                        }
                                    }
                                }
                                if complete {
                                    merge_outfit_mesh(&inputs).map(|mesh| {
                                        let handle = meshes.add(mesh);
                                        assets.merged_meshes.insert(key.clone(), handle.clone());
                                        handle
                                    })
                                } else {
                                    Err("assets not loaded yet".to_string())
                                }
                            }
                        };
                        match handle {
                            Ok(handle) => {
                                absorbed.push(*body_entity);
                                absorbed.extend(candidates.iter().map(|(node, _, _)| *node));
                                merged_spawn = Some((handle, skin, key));
                            }
                            Err(error) => {
                                warn!("Outfit merge skipped for {hero:?}: {error}");
                            }
                        }
                    }
                }
            }
        }
        if diag {
            info!(
                "DressDiag hero={hero:?} body={} worn={} absorbed={} merged_spawn={} keep_merged={}",
                body.is_some(),
                states.iter().filter(|(_, worn, _, _)| *worn).count(),
                absorbed.len(),
                merged_spawn.is_some(),
                keep_merged
            );
        }
        let merged_material = if merged_spawn.is_some() {
            let template = body
                .as_ref()
                .and_then(|(_, _, parts)| parts.first())
                .and_then(|part| materials.get(&part.material).cloned())
                .unwrap_or_default();
            let HeroAssets {
                merged_material,
                matted,
                ..
            } = &mut *assets;
            Some(
                merged_material
                    .get_or_insert_with(|| {
                        let mut material = template;
                        material.base_color = Color::WHITE;
                        material.base_color_texture = None;
                        crate::props::foliage::flatten_base(&mut material);
                        let handle = materials.add(material);
                        matted.insert(handle.id());
                        handle
                    })
                    .clone(),
            )
        } else {
            None
        };

        // 4. Drive every node to its target state.
        if let Some((body_entity, body_live, body_parts)) = body {
            states.push((body_entity, true, body_live, body_parts));
        }
        for (node, worn, live, parts) in states {
            if let Ok((_, mut visibility)) = named.get_mut(node) {
                let target = if worn {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *visibility != target {
                    *visibility = target;
                }
            }
            if stash_off {
                continue;
            }
            let should_be_live = worn && !absorbed.contains(&node);
            if should_be_live && !live {
                for primitive in &parts {
                    let mut child = commands.spawn((
                        primitive.name.clone(),
                        Mesh3d(primitive.mesh.clone()),
                        MeshMaterial3d(primitive.material.clone()),
                        primitive.transform,
                        Visibility::Inherited,
                        ChildOf(node),
                    ));
                    if let Some(skin) = &primitive.skin {
                        child.insert((
                            skin.clone(),
                            bevy::camera::visibility::DynamicSkinnedMeshBounds,
                        ));
                    }
                    if let Some(aabb) = primitive.aabb {
                        child.insert(aabb);
                    }
                }
                commands.entity(node).remove::<WardrobeStash>();
            } else if !should_be_live && live {
                if let Ok(children) = children_q.get(node) {
                    for child in children.iter() {
                        if primitives.contains(child) {
                            commands.entity(child).despawn();
                        }
                    }
                }
                if !parts.is_empty() {
                    commands.entity(node).insert(WardrobeStash(parts));
                }
            }
        }

        // 5. Reveal the (hidden-at-spawn) scene and place the merged entity.
        let mut scene_root = None;
        if let Ok(children) = children_q.get(hero) {
            for child in children.iter() {
                if let Ok((entity, mut visibility)) = scene_roots.get_mut(child) {
                    scene_root = Some(entity);
                    if *visibility != Visibility::Inherited {
                        *visibility = Visibility::Inherited;
                    }
                }
            }
        }
        match (merged_spawn, merged, keep_merged) {
            (Some((mesh, skin, key)), previous, _) => {
                if let Some(previous) = previous {
                    commands.entity(previous.entity).try_despawn();
                }
                let parent = scene_root.unwrap_or(hero);
                let entity = commands
                    .spawn((
                        Name::new("Outfit.Merged"),
                        Mesh3d(mesh),
                        MeshMaterial3d(merged_material.clone().unwrap_or_default()),
                        skin,
                        bevy::camera::visibility::DynamicSkinnedMeshBounds,
                        Transform::default(),
                        Visibility::Inherited,
                        ChildOf(parent),
                    ))
                    .id();
                commands
                    .entity(hero)
                    .insert((HeroMerged { entity, key }, HeroSkinApplied(outfit.skin)));
            }
            (None, Some(previous), false) => {
                commands.entity(previous.entity).try_despawn();
                commands.entity(hero).remove::<HeroMerged>();
            }
            _ => {}
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
        (With<HeroVisual>, With<HeroFullRig>, Without<HeroMerged>),
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
