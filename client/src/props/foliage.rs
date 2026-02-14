use bevy::prelude::*;

use crate::render::systems::GraphicsSettings;

use super::{EnvironmentProp, FoliageMaterialCache, NeedsFoliageMaterials, PropKindTag};

pub(super) fn apply_foliage_materials(
    mut commands: Commands,
    roots: Query<Entity, With<NeedsFoliageMaterials>>,
    children_q: Query<&Children>,
    materials_q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<GraphicsSettings>,
    mut cache: ResMut<FoliageMaterialCache>,
) {
    let target_alpha = if settings.foliage_cutout_enabled {
        AlphaMode::Mask(0.5)
    } else {
        AlphaMode::Blend
    };

    for root in roots.iter() {
        let mut stack = vec![root];
        let mut updated_any = false;
        let mut saw_material = false;

        while let Some(entity) = stack.pop() {
            if let Ok(handle) = materials_q.get(entity) {
                let id = handle.0.id();
                if cache.processed.contains(&id) {
                    continue;
                }
                if let Some(material) = materials.get_mut(&handle.0) {
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

            if let Ok(children) = children_q.get(entity) {
                for child in children.iter() {
                    stack.push(child);
                }
            }
        }

        if updated_any || saw_material {
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
            // environment/leaves
            | Env_Leaves_02
            | Env_Leaves_03
    )
}
