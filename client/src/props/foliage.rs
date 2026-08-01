use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;

use crate::render::systems::GraphicsSettings;

use super::wind::{
    bake_foliage_color_ramp, wind_params_for_mesh, WindExtension, WindFoliageMaterial,
};
use super::{
    is_tree_kind, EnvironmentProp, FoliageMaterialCache, NeedsFoliageMaterials, PropKindTag,
    TreeLodMeshHandles,
};

/// Grass additionally gets the root-to-tip color ramp (and gust sheen) and
/// per-instance height jitter; trees/bushes keep their authored colors.
pub(super) fn is_grass_kind(kind: shared::props::PropKind) -> bool {
    use shared::props::PropKind::*;
    matches!(kind, Grass_Patch | Grass_Tall)
}

/// Kinds that get wind sway (trunk-rooted plants; flowers and ground clutter
/// are too small to read).
fn is_swayable(kind: shared::props::PropKind) -> bool {
    use shared::props::PropKind::*;
    is_tree_kind(kind)
        || is_grass_kind(kind)
        || matches!(kind, Bush_01 | Bush_02 | Bush_03 | Bush_04)
}

fn sway_strength(kind: shared::props::PropKind) -> (f32, f32) {
    use shared::props::PropKind::*;
    if is_grass_kind(kind) {
        // Gust crests bend tips ~0.5m on a ~0.9m plant (35-45°, BotW-style
        // waves). Small values here read as "barely sways at all".
        return (0.22, 1.4);
    }
    match kind {
        Bush_01 | Bush_02 | Bush_03 | Bush_04 => (0.030, 1.25),
        // Trees. The first number is METRES AT THE TIP, so 0.055 was 5.5 cm on
        // a canopy 6-8 m up -- physically defensible and visually nothing,
        // especially from an RTS camera where that is a fraction of a pixel.
        //
        // 0.11 m is still conservative: a real canopy in a moderate breeze
        // moves considerably more than a hand's width. It reads as movement at
        // playing zoom without turning the forest into seaweed, which is the
        // failure mode on the other side.
        _ => (0.11, 1.05),
    }
}

/// Matte-flatten a cloned base material.
///
/// The dreamy look has no specular: highlights on a thousand props read as noise from
/// altitude, and glints are exactly what made the old water/foliage look busy. Fully
/// rough + zero reflectance gives flat, readable shapes lit only by diffuse + ambient.
pub(crate) fn flatten_base(base: &mut StandardMaterial) {
    base.reflectance = 0.0;
    base.perceptual_roughness = 1.0;
    base.metallic = 0.0;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_foliage_materials(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    roots: Query<(Entity, Option<&PropKindTag>), With<NeedsFoliageMaterials>>,
    children_q: Query<&Children>,
    materials_q: Query<&MeshMaterial3d<StandardMaterial>>,
    wind_q: Query<(), With<MeshMaterial3d<WindFoliageMaterial>>>,
    mesh_q: Query<&Mesh3d>,
    tree_lods_q: Query<&TreeLodMeshHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut wind_materials: ResMut<Assets<WindFoliageMaterial>>,
    mut cache: ResMut<FoliageMaterialCache>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
) {
    // Climate params for the frost tint (see wind_foliage.wgsl / worldgen.rs).
    let (climate_half, climate_phase) = terrain
        .as_ref()
        .map(|terrain| {
            let bounds = terrain.generator.active_map_bounds();
            let seed = terrain
                .generator
                .loaded_map()
                .definition
                .generated
                .as_ref()
                .map(|g| g.seed)
                .unwrap_or(0);
            (
                (bounds.max[0] - bounds.min[0]) * 0.5,
                shared::worldgen::climate_phase(seed),
            )
        })
        .unwrap_or((4096.0, 0.0));
    let target_alpha = if settings.foliage_cutout_enabled {
        AlphaMode::Mask(0.5)
    } else {
        AlphaMode::Blend
    };

    for (root, kind_tag) in roots.iter() {
        let kind = kind_tag.map(|tag| tag.0);
        let grass = kind.map(is_grass_kind).unwrap_or(false);
        let swayable = kind.map(is_swayable).unwrap_or(false);
        let (strength, speed) = kind.map(sway_strength).unwrap_or((0.04, 1.1));

        let mut stack = vec![root];
        let mut updated_any = false;
        let mut saw_material = false;
        // A subtree member whose material asset has not loaded yet: the whole root
        // must be retried next frame, even if siblings were processed.
        let mut deferred = false;

        while let Some(entity) = stack.pop() {
            // Already wind-converted (e.g. after a cutout-setting refresh):
            // nothing to do, but count it so the marker gets removed.
            if wind_q.contains(entity) {
                saw_material = true;
            }

            if let Ok(handle) = materials_q.get(entity) {
                let id = handle.0.id();
                if !cache.processed.contains(&id) {
                    if let Some(mut material) = materials.get_mut(&handle.0) {
                        saw_material = true;
                        let mut updated_material = false;

                        // Vegetation should be matte to avoid shiny trunks/leaves.
                        if material.metallic != 0.0 {
                            material.metallic = 0.0;
                            updated_material = true;
                        }
                        if material.perceptual_roughness < 0.9 {
                            material.perceptual_roughness = 0.9;
                            updated_material = true;
                        }
                        if material.reflectance > 0.25 {
                            material.reflectance = 0.25;
                            updated_material = true;
                        }

                        // Only make foliage double-sided; keep opaque trunks single-sided.
                        if material.alpha_mode != AlphaMode::Opaque {
                            material.cull_mode = None;
                            if material.alpha_mode != target_alpha {
                                material.alpha_mode = target_alpha;
                                updated_material = true;
                            }
                        }

                        if updated_material {
                            updated_any = true;
                        }

                        cache.processed.insert(id);
                    }
                }

                // Convert swayable foliage to the wind material. Grass also
                // gets the root-to-tip color ramp (drives the gust sheen).
                if swayable {
                    if let Ok(mesh_handle) = mesh_q.get(entity) {
                        // The base material may still be loading — since 0.19 the
                        // renderable `/std` sub-asset lands after the glTF itself.
                        // Converting before it exists would clone a default-white
                        // base into the cached wind material and poison every
                        // later instance of this tree kind. Leave the marker and
                        // retry next frame instead.
                        if !cache.wind_materials.contains_key(&handle.0.id())
                            && materials.get(&handle.0).is_none()
                        {
                            deferred = true;
                            if let Ok(children) = children_q.get(entity) {
                                for child in children.iter() {
                                    stack.push(child);
                                }
                            }
                            continue;
                        }
                        saw_material = true;

                        if grass {
                            // Ramp-bake every mesh this entity can display
                            // (all LODs, so the gradient doesn't pop). Only mark a
                            // mesh ramped once it actually baked — a not-yet-loaded
                            // mesh must be retried, not remembered as done.
                            let mut mesh_handles = vec![mesh_handle.0.clone()];
                            if let Ok(lods) = tree_lods_q.get(entity) {
                                mesh_handles.push(lods.lod0.clone());
                                if let Some(lod1) = &lods.lod1 {
                                    mesh_handles.push(lod1.clone());
                                }
                            }
                            for mh in &mesh_handles {
                                if !cache.ramped_meshes.contains(&mh.id()) {
                                    if let Some(mut mesh) = meshes.get_mut(mh) {
                                        bake_foliage_color_ramp(&mut mesh);
                                        cache.ramped_meshes.insert(mh.id());
                                    }
                                }
                            }
                        }

                        let wind_handle = if let Some(existing) =
                            cache.wind_materials.get(&handle.0.id())
                        {
                            existing.clone()
                        } else {
                            let Some(mut base) = materials.get(&handle.0).cloned() else {
                                continue;
                            };
                            flatten_base(&mut base);
                            let params = meshes
                                .get(&mesh_handle.0)
                                .map(|mesh| wind_params_for_mesh(mesh, strength, speed))
                                .unwrap_or(Vec4::new(strength, speed, 0.0, 1.0));
                            let (height_jitter, height_stretch) =
                                if grass { (0.25, 1.3) } else { (0.0, 1.0) };
                            let new_handle = wind_materials.add(ExtendedMaterial {
                                base,
                                extension: WindExtension {
                                    params,
                                    extra: Vec4::new(
                                        height_jitter,
                                        height_stretch,
                                        climate_half,
                                        climate_phase,
                                    ),
                                },
                            });
                            cache
                                .wind_materials
                                .insert(handle.0.id(), new_handle.clone());
                            new_handle
                        };

                        commands
                            .entity(entity)
                            .remove::<MeshMaterial3d<StandardMaterial>>()
                            .insert(MeshMaterial3d(wind_handle));
                        updated_any = true;
                    }
                }
            }

            if let Ok(children) = children_q.get(entity) {
                for child in children.iter() {
                    stack.push(child);
                }
            }
        }

        if (updated_any || saw_material) && !deferred {
            commands.entity(root).remove::<NeedsFoliageMaterials>();
        }
    }
}

pub(super) fn refresh_foliage_materials_on_setting_change(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    props: Query<(Entity, &PropKindTag), With<EnvironmentProp>>,
    mut cache: ResMut<FoliageMaterialCache>,
) {
    if !settings.is_changed() {
        return;
    }

    let cutout = settings.foliage_cutout_enabled;
    if cache.last_cutout_enabled == Some(cutout) {
        return;
    }

    cache.last_cutout_enabled = Some(cutout);
    cache.processed.clear();

    for (entity, kind) in props.iter() {
        if needs_foliage_materials(kind.0) {
            commands.entity(entity).insert(NeedsFoliageMaterials);
        }
    }
}

pub(super) fn needs_foliage_materials(kind: shared::props::PropKind) -> bool {
    use shared::props::PropKind::*;
    matches!(
        kind,
        // environment/trees
        Tree_01
            | Tree_02
            | Tree_08
            | Tree_09
            | Tree_10
            | Tree_18
            | Tree_29
            // environment/trees_dead
            | Dead_tree_1
            | Dead_tree_2
            | Dead_tree_3
            // environment/trees_pine
            | Pine_Tree_1
            | Pine_Tree_2
            | Pine_Tree_3
            | Pine_Tree_4
            // environment/bushes
            | Bush_01
            | Bush_02
            | Bush_03
            | Bush_04
            // environment/flowers
            | Flower_01
            | Flower_02
            | Flower_03
            | Flower_04
            | Flower_05
            | Spring_Flower_06
            | Spring_Flower_07
            | Spring_Flower_08
            | Spring_Flower_09
            // environment/grass
            | Grass_Patch
            | Grass_Tall
    )
}
