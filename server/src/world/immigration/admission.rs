//! Physical entry is separate from selecting a destination. Waiting arrivals
//! retain the same body and dinghy through bounded town/route retries.

use super::*;

/// The live hull already exists, but has not committed to a reachable town.
#[derive(Component, Debug, Clone)]
pub(crate) struct ChoosingSettlement {
    pub passenger: Entity,
    pub facts: ImmigrantArrival,
    pub decision_seed: u64,
    pub manual: bool,
    pub retry_at: f64,
    pub rejected_towns: Vec<Entity>,
}

pub(super) fn enter_world(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    seed: &mut crate::world::dev::VillagerSeed,
    entry: CoastalVoyage,
    now: f64,
    decision_seed: u64,
    manual: bool,
) {
    seed.0 = seed.0.wrapping_add(1);
    let position = entry.start + Quat::from_rotation_y(entry.yaw) * HELM_LOCAL;
    let passenger = crate::player::hero::spawn_villager(commands, terrain, seed.0, position);
    let facts = ImmigrantArrival {
        entry: entry.start,
        entered_at: now,
        chosen_settlement: None,
        chosen_score: None,
        chosen_at: None,
    };
    let boat = commands
        .spawn((
            PlayerBoat,
            ImmigrantArrivalBoat,
            Vessel,
            VesselNavigation::DINGHY,
            ChoosingSettlement {
                passenger,
                facts,
                decision_seed,
                manual,
                retry_at: now,
                rejected_towns: Vec::new(),
            },
            PlayerPosition(entry.start),
            PlayerRotation(entry.yaw),
            CharacterMotion::STATIONARY,
            RegionCoord::from_world_pos(entry.start),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    commands.entity(passenger).insert((
        AboardBoat,
        facts,
        NaturalImmigrantVoyage {
            boat,
            settlement: None,
        },
        VillagerIntent::Idle,
        CharacterActivity::Sitting,
        CharacterObjective::ChoosingSettlement,
        PlayerPosition(position),
        PlayerRotation(entry.yaw),
        RegionCoord::from_world_pos(position),
    ));
    info!(
        "Natural immigrant {passenger:?} entered the world aboard {boat:?} at {:?}; destination undecided",
        entry.start
    );
}

pub(super) fn commit_choice(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    boat: Entity,
    arrival: &ChoosingSettlement,
    start: Vec3,
    choice: &SettlementChoice,
    settlement_id: Option<SettlementId>,
    voyage: CoastalVoyage,
    route: Vec<Vec2>,
    water_revision: (u64, u32, u64),
    now: f64,
) {
    let mut facts = arrival.facts;
    facts.chosen_settlement = settlement_id;
    facts.chosen_score = Some(choice.score);
    facts.chosen_at = Some(now);
    let proof = crate::player::boat::VesselRouteCertification::new(
        terrain,
        water_revision,
        crate::player::boat::clearance::WatercraftClearance::DINGHY,
        start.xz(),
        &route,
    );
    commands
        .entity(boat)
        .remove::<ChoosingSettlement>()
        .insert((
            NpcArrivalBoat {
                passenger: arrival.passenger,
                settlement: choice.entity,
                mooring: voyage.mooring,
                landing: voyage.landing,
                retry_at: 0.,
                retry_count: 0,
                route_version: terrain.modification_version(),
                decision_seed: arrival.decision_seed,
                manual: arrival.manual,
            },
            VesselRoute {
                waypoints: route,
                next: 0,
            },
            proof,
        ));
    commands.entity(arrival.passenger).insert((
        facts,
        NaturalImmigrantVoyage {
            boat,
            settlement: Some(choice.entity),
        },
        VillagerIntent::ArrivingBySea {
            settlement: choice.entity,
        },
        CharacterObjective::SailingToSettlement,
    ));
    info!(
        "Natural immigrant {:?} chose '{}' at {:.1} attractiveness after {:.2} world seconds; landing walk {:.0}m",
        arrival.passenger,
        choice.name,
        choice.score,
        (now - facts.entered_at).max(0.),
        voyage.landing.xz().distance(choice.position.xz())
    );
}

#[cfg(test)]
mod tests;
