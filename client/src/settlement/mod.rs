//! Settlements on the client: draw the current civic hall, and say where places are.
//!
//! A settlement replicates as a NAME, a TIER and a POSITION -- not as a bag of
//! buildings. Its moot hall is drawn here from that position, because buildings
//! are how a settlement's plan gets expressed rather than what constitutes it
//! (WORLD-DESIGN section 1). That is also why the hall is not replicated: it is
//! derivable, so sending it would be sending the same fact twice.
//!
//! This facade owns plugin scheduling. Building scenes, construction, farm grounds,
//! livestock, animation, lighting and stock displays own their presentation state.

mod roads;
mod smoke;

mod animation;
mod buildings;
mod construction;
mod debug;
pub(crate) mod fortifications;
mod grounds;
mod lighting;
mod livestock;
mod stock;

use crate::states::GameState;
use crate::terrain::TerrainUpdateSet;
use animation::{
    drive_building_doors, drive_windmill_motion, recover_stale_building_animation_wiring,
    setup_building_door_animations, trace_replicated_building_door_demands, BuildingDoorAssets,
};
use bevy::prelude::*;
use buildings::{attach_building_visuals, attach_settlement_visuals, claim_building_ground};
use construction::{
    attach_construction_supply_visuals, raise_construction_visuals,
    sync_construction_supply_visuals,
};
use debug::debug_draw_settlement_planning_rings;
use grounds::{attach_farm_field_visuals, attach_fishing_pier_visuals};
use lighting::{
    setup_building_night_lighting, setup_window_lighting, sync_building_night_lighting,
    sync_window_lighting,
};
use livestock::{
    animate_pasture_animals, animate_pasture_sheep_parts, attach_livestock_pasture_visuals,
    tag_pasture_sheep_parts,
};
use stock::{setup_bakery_bread_display, sync_bakery_bread_display};

pub use buildings::{BuildingVisual, SettlementVisual};
pub use grounds::{FarmFieldVisual, FishingPierVisual};
pub use livestock::LivestockPastureVisual;

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(smoke::BakerySmokePlugin);
        app.add_plugins(fortifications::FortificationPlugin);
        app.init_resource::<BuildingDoorAssets>();
        app.init_resource::<roads::VillageRoadPaintState>();
        app.add_systems(
            Update,
            (
                attach_settlement_visuals,
                attach_building_visuals,
                attach_farm_field_visuals,
                attach_fishing_pier_visuals,
                attach_livestock_pasture_visuals,
                animate_pasture_animals,
                tag_pasture_sheep_parts,
                animate_pasture_sheep_parts,
                roads::paint_village_roads_into_terrain.after(TerrainUpdateSet),
                attach_construction_supply_visuals,
                sync_construction_supply_visuals,
                claim_building_ground,
                raise_construction_visuals,
                (setup_window_lighting, sync_window_lighting).chain(),
                (setup_building_night_lighting, sync_building_night_lighting).chain(),
                (setup_bakery_bread_display, sync_bakery_bread_display).chain(),
                (
                    recover_stale_building_animation_wiring,
                    setup_building_door_animations,
                    drive_windmill_motion,
                    trace_replicated_building_door_demands,
                    drive_building_doors,
                )
                    .chain(),
                debug_draw_settlement_planning_rings,
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests;
