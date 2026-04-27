//! Server-side access to authored city layout.

use bevy::prelude::*;

pub mod buildings;

use shared::city::AuthoredCityLayout;

pub fn log_city_layout_summary(layout: Res<AuthoredCityLayout>) {
    let authored_buildings = layout
        .layout
        .plots()
        .iter()
        .filter(|plot| plot.building_kind.is_some())
        .count();
    info!(
        "Loaded city layout '{}' (roads: {}, plots: {}, segments: {}, building plots: {})",
        layout.map_id,
        layout.layout.roads().len(),
        layout.layout.plots().len(),
        layout.layout.road_segments().len(),
        authored_buildings,
    );
}
