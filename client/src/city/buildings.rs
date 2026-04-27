use bevy::prelude::*;

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

pub fn cleanup_city_plot_buildings(
    mut commands: Commands,
    visuals: Query<Entity, With<CityPlotBuildingVisual>>,
) {
    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }
}
