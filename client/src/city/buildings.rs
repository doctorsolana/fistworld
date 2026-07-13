use bevy::asset::AssetId;
use bevy::prelude::*;
use std::collections::HashSet;

use shared::{
    building::{BuildingPosition, PlacedBuilding},
    city::{
        plot_building_ground_position, plot_building_rect, plot_building_scene_transform,
        AuthoredCityBuilding, AuthoredCityLayout,
    },
    terrain::WorldTerrain,
};

use crate::render::systems::ClientWorldRoot;

#[derive(Component)]
pub(crate) struct CityPlotBuildingVisual;

#[derive(Component)]
pub(crate) struct CityBuildingMaterialsWarmed;

#[derive(Resource, Default)]
pub(crate) struct CityMaterialCache {
    processed: HashSet<AssetId<StandardMaterial>>,
}

pub fn spawn_city_plot_buildings(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    world: Res<WorldTerrain>,
    city_layout: Res<AuthoredCityLayout>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    existing_visuals: Query<Entity, With<CityPlotBuildingVisual>>,
) {
    if !existing_visuals.is_empty() {
        return;
    }

    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    for plot in city_layout.layout.plots() {
        let Some(kind) = plot.building_kind else {
            continue;
        };
        let spec = kind.spec();
        let footprint = plot_building_rect(plot, kind);
        let ground_y = world.get_height(footprint.center.x, footprint.center.y);
        let building_entity = commands
            .spawn((
                Name::new(format!("CityBuilding({}:{})", plot.id, spec.display_name)),
                SceneRoot(asset_server.load(spec.scene_path)),
                plot_building_scene_transform(plot, kind, ground_y),
                GlobalTransform::default(),
                Visibility::Inherited,
                InheritedVisibility::default(),
                AuthoredCityBuilding { plot_id: plot.id },
                PlacedBuilding {
                    building_type: spec.building_type,
                    rotation: footprint.rotation_y,
                },
                BuildingPosition(plot_building_ground_position(plot, kind, ground_y)),
                CityPlotBuildingVisual,
            ))
            .id();
        commands.entity(world_root).add_child(building_entity);
    }
}

pub fn warm_city_building_materials(
    mut commands: Commands,
    mut cache: ResMut<CityMaterialCache>,
    roots: Query<
        Entity,
        (
            With<CityPlotBuildingVisual>,
            Without<CityBuildingMaterialsWarmed>,
        ),
    >,
    children_query: Query<&Children>,
    material_query: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for root in roots.iter() {
        let mut stack = vec![root];
        let mut found_loaded_material = false;

        while let Some(entity) = stack.pop() {
            if let Ok(children) = children_query.get(entity) {
                stack.extend(children.iter());
            }

            let Ok(mesh_material) = material_query.get(entity) else {
                continue;
            };

            found_loaded_material = true;
            let material_id = mesh_material.0.id();
            if !cache.processed.insert(material_id) {
                continue;
            }

            if let Some(material) = materials.get_mut(&mesh_material.0) {
                warm_city_material(material);
            }
        }

        if found_loaded_material {
            commands.entity(root).insert(CityBuildingMaterialsWarmed);
        }
    }
}

fn warm_city_material(material: &mut StandardMaterial) {
    let tint = if material.base_color_texture.is_some() {
        (1.0, 0.99, 0.96)
    } else {
        (0.96, 0.96, 0.93)
    };
    let rgba = material.base_color.to_srgba();
    material.base_color = Color::srgba(
        (rgba.red * tint.0).clamp(0.0, 1.0),
        (rgba.green * tint.1).clamp(0.0, 1.0),
        (rgba.blue * tint.2).clamp(0.0, 1.0),
        rgba.alpha,
    );
    material.perceptual_roughness = material.perceptual_roughness.max(0.76);
    material.reflectance = material.reflectance.min(0.18);
    if material.metallic < 0.2 {
        material.metallic = 0.0;
    }
}

pub fn cleanup_city_plot_buildings(
    mut commands: Commands,
    visuals: Query<Entity, With<CityPlotBuildingVisual>>,
) {
    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }
}
