//! Founding layouts use ordinary plot, field, doorway and road validators.
//! Failed sites are discarded before any people or buildings are committed.

use super::sites::Site;
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::{village, village_roads};
use bevy::prelude::*;
use shared::components::{RoadClass, RoadSurface, SettlementBuildingKind as Kind, VillageRoad};
use shared::terrain::WorldTerrain;

#[derive(Clone, Debug)]
pub(super) struct Plot {
    pub kind: Kind,
    pub position: Vec3,
    pub rotation: f32,
    pub quality: f32,
    pub road: VillageRoad,
}

#[derive(Debug)]
pub(super) struct Layout {
    pub plots: Vec<Plot>,
    pub population: usize,
    pub square: Option<shared::components::SettlementCivicSquare>,
}

impl Layout {
    /// Reserve the whole accepted settlement, including its fields, pastures
    /// and access lanes. Small hamlets need less separation than large towns,
    /// but no two founding plans may overlap each other's occupied ground.
    pub fn radius(&self, hall: Vec3) -> f32 {
        let mut radius = 20.0_f32;
        let mut include = |point: Vec3, extent: f32| {
            radius = radius.max(point.xz().distance(hall.xz()) + extent);
        };
        if let Some(square) = &self.square {
            include(square.center, square.half_extents.length());
        }
        for plot in &self.plots {
            include(plot.position, plot.kind.clearance());
            if let (Some(fields), Some(half)) = (
                plot.kind.field_positions(plot.position, plot.rotation),
                plot.kind.field_half_extents(),
            ) {
                for field in fields {
                    include(
                        field,
                        half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                    );
                }
            }
            if let (Some(pasture), Some(half)) = (
                plot.kind.pasture_position(plot.position, plot.rotation),
                plot.kind.pasture_half_extents(),
            ) {
                include(pasture, half.length() + 2.0);
            }
            for point in &plot.road.points {
                include(
                    Vec3::new(point.x, 0.0, point.y),
                    plot.road.reserved_width * 0.5,
                );
            }
        }
        radius
    }
}

struct Planner<'a> {
    terrain: &'a WorldTerrain,
    site: &'a Site,
    colliders: &'a StaticColliders,
    library: &'a DerivedColliderLibrary,
    plots: Vec<Plot>,
    occupied: Vec<(Vec3, f32)>,
    blockers: Vec<village::RoadAccessBlocker>,
    sequence: u64,
    square: Option<shared::components::SettlementCivicSquare>,
}

impl Planner<'_> {
    fn place(&mut self, kind: Kind) -> bool {
        let roads: Vec<_> = self.plots.iter().map(|p| &p.road).collect();
        let squares: Vec<_> = self.square.iter().collect();
        let mut rng = shared::rng::XorShift64::new(
            self.site.salt ^ (self.sequence + 1).wrapping_mul(0x9E37_79B9),
        );
        self.sequence += 1;
        let (inner, outer) = match kind {
            Kind::Farmstead | Kind::LivestockFarm => (75.0, 280.0),
            Kind::LumberjackHut | Kind::StoneQuarry => (90.0, 280.0),
            Kind::House => (26.0, 150.0),
            Kind::Market | Kind::Church => (24.0, 90.0),
            _ => (45.0, 185.0),
        };
        // Test the shoreline geometry once, instead of rotating hundreds of
        // inland plots in a futile attempt to manufacture fishing water.
        let fishing = (kind == Kind::FishermansHut)
            .then(|| {
                village::find_fishing_site(self.terrain, self.site.hall, &self.occupied, &roads)
            })
            .flatten();
        if kind == Kind::FishermansHut && fishing.is_none() {
            return false;
        }
        let fixed_site = fishing.is_some() || (kind == Kind::Market && self.square.is_some());
        for attempt in 0..384 {
            let (position, rotation) = if kind == Kind::Market && self.square.is_some() {
                let square = self.square.as_ref().unwrap();
                (square.market_position, square.market_rotation)
            } else if let Some((position, rotation, _)) = fishing {
                (position, rotation)
            } else {
                // Small settlements favour short walks around a loose civic
                // core. Later candidates open the fringe instead of forcing a
                // fixed street grid through slopes, fields or permanent props.
                let random = rng.next_u64();
                let fraction = (random & 65535) as f32 / 65535.0;
                let reach = (attempt as f32 / 96.0).clamp(0.25, 1.0);
                let radius = inner + (outer - inner) * fraction.sqrt() * reach;
                let angle = ((random >> 16) & 65535) as f32 / 65535.0 * std::f32::consts::TAU;
                let at = self.site.hall.xz() + Vec2::new(angle.cos(), angle.sin()) * radius;
                let toward = (self.site.hall.xz() - at).normalize_or_zero();
                (
                    Vec3::new(at.x, self.terrain.get_height(at.x, at.y), at.y),
                    (-toward.x).atan2(-toward.y),
                )
            };
            let Ok(approval) = village::validate_manual_plot(
                self.terrain,
                self.site.hall,
                kind,
                position,
                rotation,
                &self.occupied,
                &roads,
                &[],
                &self.blockers,
                Some(self.colliders),
                Some(self.library),
                None,
                &squares,
            ) else {
                if fixed_site {
                    break;
                }
                continue;
            };
            if approval.road_access.len() < 2
                || !village_roads::road_access_is_clear_of_permanent_props(
                    &approval.road_access,
                    RoadClass::Lane.initial_reserved_width(),
                    self.colliders,
                    self.library,
                )
            {
                if fixed_site {
                    break;
                }
                continue;
            }
            let position = approval.position;
            let rotation = approval.rotation;
            self.occupied.push((position, kind.clearance()));
            if let (Some(fields), Some(half)) = (
                kind.field_positions(position, rotation),
                kind.field_half_extents(),
            ) {
                self.occupied.extend(fields.into_iter().map(|p| {
                    (
                        p,
                        half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                    )
                }));
            }
            if let (Some(pasture), Some(half)) = (
                kind.pasture_position(position, rotation),
                kind.pasture_half_extents(),
            ) {
                self.occupied.push((pasture, half.length() + 2.0));
            }
            self.blockers.extend(village::road_access_blockers_for_plot(
                kind, position, rotation,
            ));
            self.plots.push(Plot {
                kind,
                position,
                rotation,
                quality: approval.quality,
                road: VillageRoad {
                    settlement: String::new(),
                    builder: String::new(),
                    built_through: approval
                        .road_access
                        .len()
                        .try_into()
                        .expect("bounded founding road"),
                    points: approval.road_access,
                    width: 2.0,
                    reserved_width: RoadClass::Lane.initial_reserved_width(),
                    surface: RoadSurface::Dirt,
                    class: RoadClass::Lane,
                    stone_committed: 0,
                },
            });
            return true;
        }
        false
    }
}

pub(super) fn plan(
    terrain: &WorldTerrain,
    site: &Site,
    colliders: &StaticColliders,
    library: &DerivedColliderLibrary,
) -> Option<Layout> {
    let square = village::founding_civic_square(terrain, site.hall, colliders, library);
    // A proposed town may turn out to have room only for a village. Reduce
    // residents and associated demand together; never leave a failed half-town.
    for target in [
        site.potential_population,
        site.potential_population.min(24),
        12,
    ] {
        if target >= 24 && square.is_none() {
            continue;
        }
        let mut planner = Planner {
            terrain,
            site,
            colliders,
            library,
            plots: Vec::new(),
            occupied: vec![(site.hall, 15.0)],
            // The access planner owns the Hall shell and its doorway escape.
            // Adding it as an ordinary plot blocker would seal that escape.
            blockers: Vec::new(),
            sequence: 0,
            square: square.clone(),
        };
        // Secure land for complete food chains before filling it with homes.
        let chains = target.div_ceil(28);
        let mut fed = true;
        for _ in 0..chains {
            if !planner.place(Kind::Farmstead)
                || !planner.place(Kind::Windmill)
                || !planner.place(Kind::Bakery)
            {
                debug!(
                    "Founding layout food chain failed at {:?}, target={}, placed={}",
                    site.hall,
                    target,
                    planner.plots.len()
                );
                fed = false;
                break;
            }
        }
        if !fed {
            continue;
        }
        if target >= 24 && !planner.place(Kind::Market) {
            continue;
        }
        let houses = target.div_ceil(4);
        if !(0..houses).all(|_| planner.place(Kind::House)) {
            continue;
        }
        // Site-specific opportunities. Missing optional businesses remain
        // actual openings for later NPC or player investment.
        if site.resources.wood >= 0.20 {
            planner.place(Kind::LumberjackHut);
        }
        if site.resources.stone >= 0.25 {
            planner.place(Kind::StoneQuarry);
        }
        if target >= 28 {
            planner.place(Kind::StorageHall);
            planner.place(Kind::Tavern);
        }
        if target >= 48 {
            planner.place(Kind::Church);
        }
        if site.resources.farmland >= 0.60 && target >= 32 {
            planner.place(Kind::LivestockFarm);
        }
        // A coastal site can add fishing only if its real shore admits the
        // authored hut/pier and a complete dry connection to the Hall.
        let water_nearby = (0..16).any(|i| {
            let angle = i as f32 / 16.0 * std::f32::consts::TAU;
            let point = site.hall.xz() + Vec2::new(angle.cos(), angle.sin()) * 125.0;
            terrain.get_water_height(point.x, point.y).is_some()
        });
        if water_nearby {
            planner.place(Kind::FishermansHut);
        }
        return Some(Layout {
            plots: planner.plots,
            population: target,
            square: planner.square,
        });
    }
    None
}
