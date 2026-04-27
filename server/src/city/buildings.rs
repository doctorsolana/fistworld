use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use shared::{
    building::{BuildingPosition, PlacedBuilding},
    city::{
        plot_building_ground_position, plot_building_rect, AuthoredCityBuilding, AuthoredCityLayout,
    },
    terrain::WorldTerrain,
};

pub fn sync_authored_plot_buildings(
    mut commands: Commands,
    world: Res<WorldTerrain>,
    city_layout: Res<AuthoredCityLayout>,
    existing: Query<(Entity, &AuthoredCityBuilding)>,
) {
    let mut existing_by_plot = HashMap::with_capacity(existing.iter().len());
    for (entity, authored) in existing.iter() {
        existing_by_plot.insert(authored.plot_id, entity);
    }

    let mut live_plot_ids = HashSet::new();

    for plot in city_layout.layout.plots() {
        let Some(kind) = plot.building_kind else {
            continue;
        };

        live_plot_ids.insert(plot.id);
        let spec = kind.spec();
        let footprint = plot_building_rect(plot, kind);
        let ground_y = world.get_height(footprint.center.x, footprint.center.y);
        let position = BuildingPosition(plot_building_ground_position(plot, kind, ground_y));
        let placed = PlacedBuilding {
            building_type: spec.building_type,
            rotation: footprint.rotation_y,
        };

        if let Some(entity) = existing_by_plot.get(&plot.id).copied() {
            commands.entity(entity).insert((placed, position));
            continue;
        }

        commands.spawn((
            Name::new(format!(
                "AuthoredPlotBuilding({}:{})",
                plot.id, spec.display_name
            )),
            AuthoredCityBuilding { plot_id: plot.id },
            placed,
            position,
        ));
    }

    for (entity, authored) in existing.iter() {
        if !live_plot_ids.contains(&authored.plot_id) {
            commands.entity(entity).despawn();
        }
    }
}
