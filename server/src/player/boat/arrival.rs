//! Spawn-time clearance shared by player and natural immigrant dinghies.
//!
//! Geography supplies deterministic preferred coasts; live occupancy decides
//! which nearby water point can actually receive a new hull. This is admission
//! clearance, not ship steering or a second runtime collision integrator.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::{
    CharacterKind, Horse, PlayerBoat, PlayerPosition, Vessel, WreckedVessel, HORSE_CLEARANCE,
};
use shared::terrain::WorldTerrain;

use super::{segment_is_water, water_at, CoastalVoyage};

// A 4.27m by 1.80m dinghy fits within this conservative circle, with room
// for the seated occupant and swell. Independent of the current hull heading.
pub(crate) const ARRIVAL_HULL_RADIUS: f32 = 3.0;
const BODY_RADIUS: f32 = 0.6;
const BUCKET_SIZE: f32 = ARRIVAL_HULL_RADIUS * 2.0;
const SLOT_SPACING: f32 = 8.0;
const SLOT_RINGS: usize = 4;
const SLOT_DIRECTIONS: usize = 8;

pub(crate) type ArrivalBodies<'w, 's> = Query<
    'w,
    's,
    (&'static PlayerPosition, Has<CharacterKind>, Has<Horse>),
    Or<(
        With<PlayerBoat>,
        With<Vessel>,
        With<WreckedVessel>,
        With<CharacterKind>,
        With<Horse>,
    )>,
>;

#[derive(Default)]
pub(crate) struct ArrivalOccupancy {
    bodies: HashMap<IVec2, Vec<(Vec2, f32)>>,
}

impl ArrivalOccupancy {
    /// Only called once an actual arrival needs admission. Ordinary fixed
    /// ticks do not scan world bodies or rebuild this spawn-only index.
    pub(crate) fn from_bodies(bodies: &ArrivalBodies) -> Self {
        let mut occupied = Self::default();
        for (position, person, horse) in bodies.iter() {
            let radius = if horse {
                HORSE_CLEARANCE
            } else if person {
                BODY_RADIUS
            } else {
                ARRIVAL_HULL_RADIUS
            };
            occupied.insert(position.0.xz(), radius);
        }
        occupied
    }

    fn insert(&mut self, position: Vec2, radius: f32) {
        if position.is_finite() {
            self.bodies
                .entry((position / BUCKET_SIZE).floor().as_ivec2())
                .or_default()
                .push((position, radius));
        }
    }

    pub(crate) fn reserve(&mut self, position: Vec3) {
        self.insert(position.xz(), ARRIVAL_HULL_RADIUS);
    }

    fn clear(&self, position: Vec2) -> bool {
        let cell = (position / BUCKET_SIZE).floor().as_ivec2();
        (-1..=1).all(|x| {
            (-1..=1).all(|y| {
                self.bodies
                    .get(&(cell + IVec2::new(x, y)))
                    .is_none_or(|bodies| {
                        bodies.iter().all(|(other, radius)| {
                            position.distance_squared(*other)
                                >= (ARRIVAL_HULL_RADIUS + radius).powi(2)
                        })
                    })
            })
        })
    }

    /// Keep the preferred coast and its certified landing. Try the original
    /// start, then 32 seed-ordered nearby slots; each offset must still connect
    /// to the original start through water. Never force a crowded fallback.
    pub(crate) fn vacant_voyage(
        &self,
        terrain: &WorldTerrain,
        voyage: CoastalVoyage,
        seed: u64,
    ) -> Option<CoastalVoyage> {
        arrival_slots(voyage.start.xz(), seed).find_map(|point| {
            if !self.clear(point) {
                return None;
            }
            let water = water_at(terrain, point)?;
            if !segment_is_water(terrain, voyage.start.xz(), point) {
                return None;
            }
            Some(CoastalVoyage {
                start: Vec3::new(point.x, water, point.y),
                ..voyage
            })
        })
    }

    /// Reserve immediately, because several names can create heroes before
    /// Commands publishes any of their boats to the live occupancy query.
    pub(crate) fn reserve_player_voyage(
        &mut self,
        terrain: &WorldTerrain,
        voyages: &[CoastalVoyage],
        seed: u64,
    ) -> Option<CoastalVoyage> {
        if voyages.is_empty() {
            return None;
        }
        let preferred = (seed % voyages.len() as u64) as usize;
        let voyage = (0..voyages.len()).find_map(|offset| {
            self.vacant_voyage(terrain, voyages[(preferred + offset) % voyages.len()], seed)
        })?;
        self.reserve(voyage.start);
        Some(voyage)
    }
}

fn arrival_slots(start: Vec2, seed: u64) -> impl Iterator<Item = Vec2> {
    std::iter::once(start).chain((0..SLOT_RINGS * SLOT_DIRECTIONS).map(move |slot| {
        let ring = slot / SLOT_DIRECTIONS + 1;
        let direction = (slot + (seed % SLOT_DIRECTIONS as u64) as usize) % SLOT_DIRECTIONS;
        let angle = direction as f32 * std::f32::consts::TAU / SLOT_DIRECTIONS as f32;
        start + Vec2::new(angle.cos(), angle.sin()) * (ring as f32 * SLOT_SPACING)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_arrival_neighborhood_refuses_instead_of_overlapping() {
        let terrain = WorldTerrain::default();
        let voyage = super::super::coastal_voyages(&terrain, 0)[0];
        let mut occupied = ArrivalOccupancy::default();
        for point in arrival_slots(voyage.start.xz(), 7) {
            occupied.reserve(Vec3::new(point.x, 0.0, point.y));
        }
        assert!(occupied.vacant_voyage(&terrain, voyage, 7).is_none());
        assert!(occupied.reserve_player_voyage(&terrain, &[], 7).is_none());
    }

    #[test]
    fn arrival_clearance_checks_adjacent_negative_coordinate_buckets() {
        let mut occupied = ArrivalOccupancy::default();
        occupied.reserve(Vec3::new(-0.1, 0.0, -0.1));
        assert!(!occupied.clear(Vec2::splat(0.1)));
        assert!(occupied.clear(Vec2::new(6.0, -0.1)));
    }

    #[test]
    fn simultaneous_arrival_reservations_are_distinct_deterministic_water_starts() {
        let terrain = WorldTerrain::default();
        let voyage = super::super::coastal_voyages(&terrain, 0)[0];
        let mut occupied = ArrivalOccupancy::default();
        let mut repeated = ArrivalOccupancy::default();
        let mut starts: Vec<Vec3> = Vec::new();
        for _ in 0..8 {
            let next = occupied
                .reserve_player_voyage(&terrain, &[voyage], 11)
                .expect("eight dinghies must fit along the open arrival water");
            let replay = repeated
                .reserve_player_voyage(&terrain, &[voyage], 11)
                .unwrap();
            assert_eq!(next.start, replay.start);
            assert_eq!(next.landing, voyage.landing);
            assert!(segment_is_water(
                &terrain,
                voyage.start.xz(),
                next.start.xz()
            ));
            for prior in &starts {
                assert!(prior.distance(next.start) >= ARRIVAL_HULL_RADIUS * 2.0);
            }
            starts.push(next.start);
        }
    }

    #[test]
    fn arrival_body_index_includes_unowned_boats_wrecks_people_and_horses() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        // An immigrant boat has no CommandedBy, and a wreck has no Vessel.
        world.spawn((PlayerBoat, PlayerPosition(Vec3::ZERO)));
        world.spawn((WreckedVessel, PlayerPosition(Vec3::new(20.0, 0.0, 0.0))));
        world.spawn((
            CharacterKind::Villager,
            PlayerPosition(Vec3::new(40.0, 0.0, 0.0)),
        ));
        world.spawn((
            Horse { id: 1, rider: None },
            PlayerPosition(Vec3::new(60.0, 0.0, 0.0)),
        ));
        world
            .run_system_once(|bodies: ArrivalBodies| {
                let occupied = ArrivalOccupancy::from_bodies(&bodies);
                for x in [0.0, 20.0, 40.0, 60.0] {
                    assert!(!occupied.clear(Vec2::new(x, 0.0)));
                }
                assert!(!occupied.clear(Vec2::new(5.9, 0.0)));
                assert!(!occupied.clear(Vec2::new(63.0, 0.0)));
            })
            .unwrap();
    }
}
