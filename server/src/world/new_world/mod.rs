//! The ordinary game's one-time inhabited opening. Terrain and society share
//! a seed; society becomes mutable authoritative state after initialization.
//! Nothing here runs per login, replenishes stores, or directs later growth.

mod connectivity;
mod layout;
mod planning;
mod sites;
mod spawn;
#[cfg(test)]
mod tests;

use crate::collision::library::DerivedColliderLibrary;
use crate::player::boat::CoastalVoyage;
use bevy::prelude::*;
use shared::terrain::WorldTerrain;

const SETTLEMENT_COUNT: usize = 10;
const MIN_SETTLEMENT_COUNT: usize = 8;
const MIN_SETTLEMENT_DISTANCE: f32 = 480.0;

#[derive(Resource, Debug)]
pub(crate) struct WorldOpening {
    pub seed: u64,
    pub arrivals: Vec<CoastalVoyage>,
    pub settlements: usize,
    pub residents: usize,
}

struct Community {
    site: sites::Site,
    layout: layout::Layout,
    name: String,
    arrival: Option<CoastalVoyage>,
    radius: f32,
}

pub(crate) fn populate(
    mut commands: Commands,
    mut terrain: ResMut<WorldTerrain>,
    library: Res<DerivedColliderLibrary>,
    mut ids: ResMut<crate::world::identity::WorldIdAllocator>,
    mut deltas: ResMut<crate::world::village::PublishedTerrainDeltas>,
    existing: Option<Res<WorldOpening>>,
) {
    if existing.is_some() || terrain.generator.active_map_id() != shared::map::SESSION_MAP_ID {
        return;
    }
    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .expect("session recipe")
        .seed;
    let start = std::time::Instant::now();
    let communities = planning::plan_world(&terrain, &library, seed)
        .unwrap_or_else(|error| panic!("World generation failed: {error}"));
    let mut opening = WorldOpening {
        seed,
        arrivals: Vec::new(),
        settlements: communities.len(),
        residents: 0,
    };
    for community in communities {
        opening.residents += community.layout.population;
        if let Some(arrival) = community.arrival {
            opening.arrivals.push(arrival);
        }
        spawn::community(
            &mut commands,
            &mut terrain,
            &mut ids,
            &mut deltas,
            &community,
        );
    }
    assert!(
        !opening.arrivals.is_empty(),
        "A populated world must have a reachable player arrival"
    );
    println!("World opening ready: seed={} settlements={} residents={} arrivals={} elapsed={:.2}s; ordinary simulation takes over",
        opening.seed, opening.settlements, opening.residents, opening.arrivals.len(), start.elapsed().as_secs_f32());
    commands.insert_resource(opening);
}
