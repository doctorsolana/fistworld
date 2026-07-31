//! Villages that run themselves.
//!
//! God mode introduces people and founds halls. Everything after that is the
//! villagers' own doing: they choose where to live, they decide what the place
//! needs next, and they site their own buildings. No player assigns a resident,
//! an occupation or a plot.
//!
//! Deliberately NOT here yet, and deferred on purpose so the autonomy can be
//! judged on its own: immigration, births, boats, markets, goods, production,
//! employment and worker slots. A building here is a decision that happened,
//! not an economy that runs.
//!
//! Everything in this module is server truth. Clients receive settlements and
//! buildings and draw them; they never decide anything.

use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{
    CharacterKind, CharacterName, PlayerPosition, PlayerRotation, Residence, Settlement,
    SettlementBuilding, SettlementBuildingKind,
};
use shared::terrain::WorldTerrain;

use crate::player::hero::MoveTarget;

/// How often an uncommitted villager looks for somewhere to live.
///
/// Seconds, not frames: this is a decision, and decisions should not get more
/// frequent because the server is running well.
const SEEK_INTERVAL: f32 = 3.0;

/// How often a settlement considers what it needs next.
const PERMIT_INTERVAL: f32 = 4.0;

/// How close a villager must get to the hall to have arrived.
///
/// Generous, because arrival is the point rather than the precision: a villager
/// who stops a metre short and stands there forever is a bug the player will
/// read as the whole system being broken.
const ARRIVAL_RADIUS: f32 = 6.0;

/// How long a permitted building takes to appear, in seconds.
///
/// A fixed timer stands in for real construction. Materials, builders and
/// hauling are deliberately deferred; what this preserves is that a decision
/// and its result are separate events, so the panel can honestly show something
/// as "under construction".
const BUILD_SECONDS: f32 = 6.0;

/// Where a villager is in the business of joining somewhere.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum VillagerIntent {
    /// Knows of nowhere to go. Re-checks on the seek tick.
    Idle,
    /// Walking to a settlement's hall.
    Travelling { settlement: Entity },
    /// Lives somewhere. The hall is their lodging until houses exist.
    Resident { settlement: Entity },
}

/// A building a settlement has approved and is waiting on.
#[derive(Component, Debug, Clone)]
pub struct UnderConstruction {
    pub kind: SettlementBuildingKind,
    pub position: Vec3,
    pub rotation: f32,
    pub owner: Option<String>,
    pub settlement: Entity,
    pub seconds_left: f32,
}

/// Paces the two decision ticks.
#[derive(Resource)]
pub struct VillageClock {
    seek: f32,
    permit: f32,
}

impl Default for VillageClock {
    fn default() -> Self {
        Self {
            seek: 0.0,
            permit: 0.0,
        }
    }
}

/// Give every villager an intent, so the rest of the module can assume one.
///
/// Polls rather than reacting to `Added`, for the reason this codebase has now
/// been bitten by repeatedly: components arrive in separate batches and a
/// one-shot on `Added` misses whoever was assembled late.
pub fn tag_villager_intent(
    mut commands: Commands,
    villagers: Query<
        Entity,
        (
            With<CharacterName>,
            With<PlayerPosition>,
            Without<VillagerIntent>,
        ),
    >,
    kinds: Query<&CharacterKind>,
) {
    for entity in villagers.iter() {
        // Heroes are players' bodies and join nothing on their own.
        if kinds.get(entity) != Ok(&CharacterKind::Villager) {
            continue;
        }
        commands.entity(entity).insert(VillagerIntent::Idle);
    }
}

/// Uncommitted villagers pick somewhere to live and start walking.
pub fn seek_settlement(
    time: Res<Time>,
    mut clock: ResMut<VillageClock>,
    mut commands: Commands,
    settlements: Query<(Entity, &Settlement, &PlayerPosition)>,
    mut villagers: Query<(Entity, &PlayerPosition, &mut VillagerIntent)>,
) {
    clock.seek += time.delta_secs();
    if clock.seek < SEEK_INTERVAL {
        return;
    }
    clock.seek = 0.0;

    for (entity, position, mut intent) in villagers.iter_mut() {
        if !matches!(*intent, VillagerIntent::Idle) {
            continue;
        }
        // Nearest non-ruined settlement. Ruins have no hall to walk to and
        // nobody to join.
        let nearest = settlements
            .iter()
            .filter(|(_, settlement, _)| {
                settlement.tier != shared::components::SettlementTier::Ruins
            })
            .min_by(|a, b| {
                a.2 .0
                    .distance_squared(position.0)
                    .total_cmp(&b.2 .0.distance_squared(position.0))
            });
        // No settlement anywhere: stay idle and look again next tick. A villager
        // with nowhere to go is a real state, not an error.
        let Some((settlement_entity, _, hall)) = nearest else {
            continue;
        };
        *intent = VillagerIntent::Travelling {
            settlement: settlement_entity,
        };
        commands.entity(entity).insert(MoveTarget(hall.0));
    }
}

/// Villagers who reached their hall become residents of that settlement.
pub fn arrive_at_settlement(
    mut commands: Commands,
    halls: Query<(&PlayerPosition, &Settlement)>,
    mut villagers: Query<(Entity, &PlayerPosition, &mut VillagerIntent)>,
) {
    for (entity, position, mut intent) in villagers.iter_mut() {
        let VillagerIntent::Travelling { settlement } = *intent else {
            continue;
        };
        // The settlement went away mid-journey: go back to looking.
        let Ok((hall, place)) = halls.get(settlement) else {
            *intent = VillagerIntent::Idle;
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<Residence>();
            continue;
        };
        if position.0.distance(hall.0) > ARRIVAL_RADIUS {
            continue;
        }
        *intent = VillagerIntent::Resident { settlement };
        // Residence is the replicated half of the same fact, so a client can
        // name who lives where without knowing anything about intents.
        commands
            .entity(entity)
            .remove::<MoveTarget>()
            .insert(Residence(place.name.clone()));
    }
}

/// Recount every settlement's residents from the villagers who live there.
///
/// DERIVED every tick rather than incremented on arrival, because a counter
/// that is nudged by events drifts the first time an event is missed -- and a
/// resident count that disagrees with the people standing there is exactly the
/// kind of lie the encyclopedia must never tell.
pub fn recount_residents(
    mut settlements: Query<(Entity, &mut Settlement)>,
    villagers: Query<&VillagerIntent>,
) {
    for (entity, mut settlement) in settlements.iter_mut() {
        let count = villagers
            .iter()
            .filter(|intent| matches!(intent, VillagerIntent::Resident { settlement } if *settlement == entity))
            .count() as u32;
        // Change detection drives replication; an idle village must not
        // re-send its count every tick.
        if settlement.residents != count {
            info!(
                "Village '{}': {} resident(s), was {}",
                settlement.name, count, settlement.residents
            );
            settlement.residents = count;
        }
    }
}

/// What a settlement wants next, given what it has and what it has already
/// approved.
///
/// Strict order, and the "already planned" half is what stops three residents
/// all deciding the village needs a farm at the same instant.
pub fn next_need(existing: &HashSet<SettlementBuildingKind>) -> Option<SettlementBuildingKind> {
    for kind in [
        SettlementBuildingKind::Farmstead,
        SettlementBuildingKind::LumberjackHut,
        SettlementBuildingKind::House,
    ] {
        if !existing.contains(&kind) {
            return Some(kind);
        }
    }
    None
}

/// A resident applies for a permit, and it is approved if it is valid.
///
/// One decision in flight per settlement. Permits are free at a new foundation,
/// so nothing is charged and the treasury does not move -- but the fee path is
/// where paid permits will land, including for independent settlements, whose
/// income is their own.
#[allow(clippy::too_many_arguments)]
pub fn consider_permits(
    time: Res<Time>,
    mut clock: ResMut<VillageClock>,
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    settlements: Query<(Entity, &Settlement, &PlayerPosition)>,
    buildings: Query<&SettlementBuilding>,
    pending: Query<&UnderConstruction>,
    placed: Query<(&SettlementBuilding, &PlayerPosition)>,
    villagers: Query<(&CharacterName, &VillagerIntent)>,
) {
    clock.permit += time.delta_secs();
    if clock.permit < PERMIT_INTERVAL {
        return;
    }
    clock.permit = 0.0;

    let Some(terrain) = terrain else {
        return;
    };

    for (settlement_entity, settlement, hall) in settlements.iter() {
        // ONE permit in progress at a time, per settlement.
        if pending
            .iter()
            .any(|under| under.settlement == settlement_entity)
        {
            continue;
        }

        // Count what stands AND what is already approved, or three residents
        // deciding at once all build the same thing.
        let mut have: HashSet<SettlementBuildingKind> = buildings
            .iter()
            .filter(|building| building.settlement == settlement.name)
            .map(|building| building.kind)
            .collect();
        for under in pending.iter() {
            if under.settlement == settlement_entity {
                have.insert(under.kind);
            }
        }
        // The hall is always there; it is the founding act, not a need.
        have.insert(SettlementBuildingKind::Hall);

        let Some(kind) = next_need(&have) else {
            continue;
        };

        // A resident applies. No residents, no decisions -- an empty foundation
        // does not build itself, which is the point of residents mattering.
        //
        // The applicant is whoever holds the FEWEST buildings already, not
        // whoever the query happens to return first. That ordering matters more
        // than it looks: taking the first resident gave one villager every
        // building in the village and left the other two owning nothing, which
        // makes the roster decorative. Spreading it means each person has a
        // stake, which is the whole premise of section 1a -- a farm stops when
        // ITS farmer dies, and that is only meaningful if they are different
        // people. Ties break on name so the choice stays deterministic.
        let holdings = |who: &str| -> usize {
            buildings
                .iter()
                .filter(|building| {
                    building.settlement == settlement.name
                        && building.owner.as_deref() == Some(who)
                })
                .count()
                + pending
                    .iter()
                    .filter(|under| {
                        under.settlement == settlement_entity
                            && under.owner.as_deref() == Some(who)
                    })
                    .count()
        };
        let applicant = villagers
            .iter()
            .filter(|(_, intent)| {
                matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
            })
            .map(|(name, _)| name.0.clone())
            .min_by(|a, b| holdings(a).cmp(&holdings(b)).then_with(|| a.cmp(b)));
        let Some(applicant) = applicant else {
            continue;
        };

        // Occupied ground, so a new building does not land on an old one.
        let occupied: Vec<(Vec3, f32)> = placed
            .iter()
            .filter(|(building, _)| building.settlement == settlement.name)
            .map(|(building, position)| (position.0, building.kind.clearance()))
            .chain(std::iter::once((
                hall.0,
                SettlementBuildingKind::Hall.clearance(),
            )))
            .chain(pending.iter().filter_map(|under| {
                (under.settlement == settlement_entity)
                    .then_some((under.position, under.kind.clearance()))
            }))
            .collect();

        let Some((position, rotation)) = find_site(&terrain, hall.0, kind, &occupied) else {
            info!(
                "Village '{}': nowhere to put a {} yet",
                settlement.name,
                kind.label()
            );
            continue;
        };

        // Auto-approved: a valid application from a resident is granted. The
        // fee is zero today, so the treasury is untouched rather than
        // pretending to move.
        commands.spawn((
            UnderConstruction {
                kind,
                position,
                rotation,
                owner: Some(applicant.clone()),
                settlement: settlement_entity,
                seconds_left: BUILD_SECONDS,
            },
            // The replicated half, so the panel can show it as approved.
            shared::components::ConstructionSite {
                kind,
                settlement: settlement.name.clone(),
            },
            PlayerPosition(position),
            Replicate::to_clients(NetworkTarget::All),
        ));
        info!(
            "Village '{}': {applicant} permitted a {} at {:.0},{:.0}",
            settlement.name,
            kind.label(),
            position.x,
            position.z
        );
    }
}

/// Finished buildings become real.
pub fn finish_construction(
    time: Res<Time>,
    mut commands: Commands,
    settlements: Query<&Settlement>,
    mut pending: Query<(Entity, &mut UnderConstruction)>,
) {
    for (entity, mut under) in pending.iter_mut() {
        under.seconds_left -= time.delta_secs();
        if under.seconds_left > 0.0 {
            continue;
        }
        let Ok(settlement) = settlements.get(under.settlement) else {
            // Its settlement vanished; drop the site rather than leaving a
            // building belonging to nowhere.
            commands.entity(entity).despawn();
            continue;
        };
        commands.spawn((
            SettlementBuilding {
                kind: under.kind,
                settlement: settlement.name.clone(),
                owner: under.owner.clone(),
            },
            PlayerPosition(under.position),
            PlayerRotation(under.rotation),
            // No RegionCoord: buildings are part of the map screen, like the
            // settlements they belong to.
            Replicate::to_clients(NetworkTarget::All),
        ));
        info!(
            "Village '{}': {} completed",
            settlement.name,
            under.kind.label()
        );
        commands.entity(entity).despawn();
    }
}

/// Steepness at a point, as a rise over the sampling distance.
fn slope_at(terrain: &WorldTerrain, x: f32, z: f32) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(x, z);
    let dx = (terrain.get_height(x + STEP, z) - here).abs();
    let dz = (terrain.get_height(x, z + STEP) - here).abs();
    dx.max(dz) / STEP
}

/// Steepest ground a building will accept.
const MAX_BUILD_SLOPE: f32 = 0.30;

/// How far above the waterline anything a settlement builds must stand, in metres.
///
/// Not zero: ground exactly at the waterline is shoreline, and a farmstead with
/// its doorstep in the lake reads as a bug even though the maths permitted it.
pub const FREEBOARD: f32 = 1.5;

/// Deterministic ring search for somewhere to put a building.
///
/// Walks outward in rings from the hall, sampling a fixed number of bearings
/// per ring, and takes the first spot that is flat enough and clear of what is
/// already there. Deterministic on purpose: the same village in the same state
/// makes the same choice, so a bug is reproducible rather than a story about
/// what happened once.
///
/// This is emphatically NOT the settlement planner. It has no notion of roads,
/// frontage, farmland quality or forest proximity; it gets buildings onto
/// sensible ground in roughly the right relationship to the hall, and the real
/// planner replaces it wholesale.
pub fn find_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
) -> Option<(Vec3, f32)> {
    const BEARINGS: usize = 12;
    const RING_STEP: f32 = 6.0;

    let (min_radius, max_radius) = kind.preferred_ring();
    let clearance = kind.clearance();

    let mut radius = min_radius;
    while radius <= max_radius {
        for i in 0..BEARINGS {
            // Offset each ring's bearings so successive rings do not line every
            // building up on the same spokes.
            let turn = (i as f32 + (radius / RING_STEP) * 0.5) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let x = hall.x + angle.cos() * radius;
            let z = hall.z + angle.sin() * radius;

            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            // Dry land only. Slope alone lets a village walk straight into a
            // lake, because a lake bed is beautifully flat -- so the flattest
            // ground in reach is exactly the ground that must be refused.
            if terrain
                .water_level()
                .is_some_and(|level| ground < level + FREEBOARD)
            {
                continue;
            }
            let candidate = Vec3::new(x, ground, z);
            let clashes = occupied.iter().any(|(other, other_clearance)| {
                let flat = Vec2::new(candidate.x - other.x, candidate.z - other.z).length();
                flat < clearance + other_clearance
            });
            if clashes {
                continue;
            }
            // Face the hall: a village reads as a village when its buildings
            // acknowledge its centre.
            let rotation = f32::atan2(hall.x - x, hall.z - z);
            return Some((candidate, rotation));
        }
        radius += RING_STEP;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_are_taken_in_order_and_stop_when_met() {
        let mut have = HashSet::new();
        have.insert(SettlementBuildingKind::Hall);
        assert_eq!(next_need(&have), Some(SettlementBuildingKind::Farmstead));

        have.insert(SettlementBuildingKind::Farmstead);
        assert_eq!(next_need(&have), Some(SettlementBuildingKind::LumberjackHut));

        have.insert(SettlementBuildingKind::LumberjackHut);
        assert_eq!(next_need(&have), Some(SettlementBuildingKind::House));

        have.insert(SettlementBuildingKind::House);
        assert_eq!(next_need(&have), None, "a fed, timbered, housed village wants nothing more yet");
    }

    /// The "already planned" half is what stops three residents all deciding
    /// the village needs a farm at the same instant.
    #[test]
    fn a_planned_building_counts_as_had() {
        let mut have = HashSet::new();
        have.insert(SettlementBuildingKind::Hall);
        // Nothing built, but a farm already approved.
        have.insert(SettlementBuildingKind::Farmstead);
        assert_eq!(
            next_need(&have),
            Some(SettlementBuildingKind::LumberjackHut),
            "a planned farm must not be requested twice"
        );
    }

    /// The whole loop, driven by the real systems.
    ///
    /// This is the acceptance test the design was written against, run as code
    /// rather than as a person watching: found a settlement, put three people
    /// on the map some way off, and let the server do everything else. Nothing
    /// here assigns a resident, an occupation or a plot.
    ///
    /// It runs the ACTUAL scheduled systems, including `step_units`, so the
    /// walking, the arrival radius and the permit clock are all under test. A
    /// test that called the decision functions directly would pass while the
    /// villagers stood still forever.
    #[test]
    fn three_villagers_settle_and_build_a_village_unaided() {
        use crate::player::hero::step_units;
        use shared::components::CharacterKind;
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                tag_villager_intent,
                seek_settlement,
                step_units,
                arrive_at_settlement,
                recount_residents,
                consider_permits,
                finish_construction,
            )
                .chain(),
        );

        // A hall on DRY, buildable land. Searched for rather than hardcoded,
        // because the origin of this map happens to be underwater -- and a test
        // that founded there would fail for a reason that has nothing to do
        // with villager autonomy.
        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            let water = terrain.water_level().unwrap_or(f32::NEG_INFINITY);
            (0..400)
                .find_map(|step| {
                    let x = step as f32 * 40.0;
                    let height = terrain.get_height(x, 0.0);
                    (height > water + FREEBOARD + 4.0 && slope_at(terrain, x, 0.0) < 0.1)
                        .then_some(Vec3::new(x, height, 0.0))
                })
                .expect("the map has dry, flat ground somewhere along the x axis")
        };
        app.world_mut().spawn((
            Settlement {
                name: "Yewcrag".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
        ));

        // Three people, dropped well clear of the hall so they have to walk.
        for index in 0..3 {
            let offset = Vec3::new(40.0 + index as f32 * 5.0, 0.0, 25.0);
            let spot = hall_position + offset;
            app.world_mut().spawn((
                CharacterName(format!("Villager{index}")),
                CharacterKind::Villager,
                PlayerPosition(spot),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(spot),
            ));
        }

        // Forty seconds of simulated time at the fixed rate.
        let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
        for _ in 0..(60 * 40) {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
        }

        let mut world = std::mem::take(&mut *app.world_mut());

        let settlement = world
            .query::<&Settlement>()
            .iter(&world)
            .next()
            .cloned()
            .expect("the settlement still exists");
        assert_eq!(
            settlement.residents, 3,
            "all three walked in and joined of their own accord"
        );

        let homes: Vec<String> = world
            .query::<&Residence>()
            .iter(&world)
            .map(|home| home.0.clone())
            .collect();
        assert_eq!(homes.len(), 3, "each of them is on record as living there");
        assert!(homes.iter().all(|home| home == "Yewcrag"));

        let built: HashSet<SettlementBuildingKind> = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .map(|building| building.kind)
            .collect();
        assert!(
            built.contains(&SettlementBuildingKind::Farmstead),
            "a farm went up first, because food comes first: {built:?}"
        );
        assert!(
            built.contains(&SettlementBuildingKind::LumberjackHut),
            "then timber: {built:?}"
        );
        assert!(
            built.contains(&SettlementBuildingKind::House),
            "then somewhere to live: {built:?}"
        );

        // Every building belongs to a named person. A village of anonymous
        // structures is exactly what this design refuses.
        let ownerless = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .filter(|building| building.owner.is_none())
            .count();
        assert_eq!(ownerless, 0, "somebody applied for every one of them");

        // THREE buildings, THREE owners. Not incidental: if one villager ends
        // up holding the whole village the other two are decoration, and
        // "the wheat farm stopped because the farmer died" stops meaning
        // anything -- one death would take everything with it.
        let owners: HashSet<String> = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .filter_map(|building| building.owner.clone())
            .collect();
        assert_eq!(
            owners.len(),
            3,
            "each resident has a stake of their own: {owners:?}"
        );

        // Nothing was built in the water. Flat ground is where a village
        // wants to build and a lake bed is the flattest ground there is, so
        // this is the failure the siting rule exists to prevent.
        let water = world
            .resource::<WorldTerrain>()
            .water_level()
            .unwrap_or(f32::NEG_INFINITY);
        let drowned: Vec<_> = world
            .query::<(&SettlementBuilding, &PlayerPosition)>()
            .iter(&world)
            .filter(|(_, at)| at.0.y < water)
            .map(|(building, at)| (building.kind, at.0))
            .collect();
        assert!(drowned.is_empty(), "nothing was built in the lake: {drowned:?}");

        // No player spent anything and nobody was charged.
        assert_eq!(settlement.treasury, 0, "first permits are free");

        // Where the test actually founded, so a failure elsewhere is diagnosable.
        println!("founded at {hall_position:?}, waterline {water}");
    }

    #[test]
    fn houses_sit_closer_to_the_hall_than_workplaces() {
        let (house_min, house_max) = SettlementBuildingKind::House.preferred_ring();
        let (farm_min, _) = SettlementBuildingKind::Farmstead.preferred_ring();
        let (wood_min, _) = SettlementBuildingKind::LumberjackHut.preferred_ring();
        assert!(house_max <= farm_min, "houses must not reach past the farms");
        assert!(house_min < wood_min);
    }
}
