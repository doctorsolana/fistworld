//! Founding layouts use ordinary plot, field, doorway and road validators.
//! Failed sites are discarded before any people or buildings are committed.

use super::sites::Site;
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::{village, village_roads};
use bevy::prelude::*;
use shared::components::{RoadClass, RoadSurface, SettlementBuildingKind as Kind, VillageRoad};
use shared::terrain::WorldTerrain;

mod economy;
use economy::{opportunity_roll, FoodCapacity};

/// Read-only audit of the same initial full-staffed business roster and
/// investment standards used by the planner. The Hall's public jobs count too.
#[derive(Debug)]
#[cfg(test)]
pub(super) struct EconomyAudit {
    pub rated_rations: u32,
    pub funded_positions: usize,
    pub unsuitable_sites: usize,
}

#[cfg(test)]
impl EconomyAudit {
    pub fn supports(&self, population: usize) -> bool {
        self.unsuitable_sites == 0
            && self.funded_positions <= population
            && u64::from(self.rated_rations) * 5 >= population as u64 * 6
    }
}

#[cfg(test)]
pub(super) fn audit_economy(sites: &[(Kind, f32)]) -> EconomyAudit {
    EconomyAudit {
        rated_rations: FoodCapacity::from_sites(sites.iter().copied()).rations(),
        funded_positions: usize::from(Kind::Hall.positions())
            + sites
                .iter()
                .map(|(kind, _)| usize::from(kind.positions()))
                .sum::<usize>(),
        unsuitable_sites: sites
            .iter()
            .filter(|(kind, quality)| !economy::qualifies(*kind, *quality))
            .count(),
    }
}

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
                plot.kind
                    .intended_field_positions(plot.position, plot.rotation),
                plot.kind.intended_field_half_extents(),
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
    population: usize,
    square: Option<shared::components::SettlementCivicSquare>,
}

impl Planner<'_> {
    fn workers(&self) -> usize {
        usize::from(Kind::Hall.positions())
            + self
                .plots
                .iter()
                .map(|plot| usize::from(plot.kind.positions()))
                .sum::<usize>()
    }

    fn capacity(&self) -> FoodCapacity {
        FoodCapacity::from_sites(self.plots.iter().map(|plot| (plot.kind, plot.quality)))
    }

    fn optional(&mut self, kind: Kind) -> bool {
        // Food and the Hall's actual funded jobs take priority. Keep a small
        // uncommitted workforce for portering and later autonomous ventures.
        let budget = self.population.saturating_sub(self.population.div_ceil(8));
        self.workers() + usize::from(kind.positions()) <= budget && self.place(kind)
    }

    fn place(&mut self, kind: Kind) -> bool {
        if self.workers() + usize::from(kind.positions()) > self.population {
            return false;
        }
        let roads: Vec<_> = self.plots.iter().map(|p| &p.road).collect();
        let squares: Vec<_> = self.square.iter().collect();
        let mut rng = shared::rng::XorShift64::new(
            self.site.salt ^ self.sequence.wrapping_add(1).wrapping_mul(0x9E37_79B9),
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
        let fixed = if kind == Kind::Market {
            self.square
                .as_ref()
                .map(|square| (square.market_position, square.market_rotation))
        } else {
            fishing.map(|(position, rotation, _)| (position, rotation))
        };
        let terrain = self.terrain;
        let hall = self.site.hall;
        let candidates = (0..if fixed_site { 1 } else { 384 }).map(move |attempt| {
            if let Some(fixed) = fixed {
                fixed
            } else {
                // Small settlements favour short walks around a loose civic
                // core. Later candidates open the fringe instead of forcing a
                // fixed street grid through slopes, fields or permanent props.
                let random = rng.next_u64();
                let fraction = (random & 65535) as f32 / 65535.0;
                let reach = (attempt as f32 / 96.0).clamp(0.25, 1.0);
                let radius = inner + (outer - inner) * fraction.sqrt() * reach;
                let angle = ((random >> 16) & 65535) as f32 / 65535.0 * std::f32::consts::TAU;
                let at = hall.xz() + Vec2::new(angle.cos(), angle.sin()) * radius;
                let toward = (hall.xz() - at).normalize_or_zero();
                (
                    Vec3::new(at.x, terrain.get_height(at.x, at.y), at.y),
                    (-toward.x).atan2(-toward.y),
                )
            }
        });
        // Inspect the available land around this actual plot, not the Hall's
        // biome. Cheap resource ranking runs before expensive route/earthwork
        // certification and remains bounded by the old candidate budget.
        let ranked = matches!(
            kind,
            Kind::Farmstead
                | Kind::LivestockFarm
                | Kind::LumberjackHut
                | Kind::StoneQuarry
                | Kind::Windmill
        );
        let candidates: Box<dyn Iterator<Item = (Vec3, f32)> + '_> = if ranked {
            let mut ranked: Vec<_> = candidates
                .filter_map(|(position, rotation)| {
                    let quality = village::site_quality(terrain, kind, position);
                    if !economy::qualifies(kind, quality) {
                        return None;
                    }
                    let suitability = if kind == Kind::Windmill {
                        terrain
                            .generator
                            .loaded_map()
                            .biome_field
                            .as_deref()
                            .map_or(0.5, |field| {
                                kind.placement_suitability(
                                    &field.resources(position.x, position.z, position.y, 0.0),
                                )
                            })
                    } else {
                        quality
                    };
                    let distance = position.xz().distance(hall.xz()) / outer;
                    Some((position, rotation, suitability - distance * 0.15))
                })
                .collect();
            ranked.sort_by(|a, b| b.2.total_cmp(&a.2));
            Box::new(
                ranked
                    .into_iter()
                    .map(|(position, rotation, _)| (position, rotation)),
            )
        } else {
            // Houses and services keep the cheap first-valid search. Do not
            // sample hundreds of unused candidates after one already fits.
            Box::new(candidates)
        };
        for (position, rotation) in candidates {
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
            if !economy::qualifies(kind, approval.quality)
                || approval.road_access.len() < 2
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
                kind.intended_field_positions(position, rotation),
                kind.intended_field_half_extents(),
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
            self.blockers
                .extend(village::road_access_blockers_for_new_plot(
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
            population: target,
            square: square.clone(),
        };
        // Shores and good pasture can contribute directly edible food. They
        // still need real work geometry and an ordinary dry road to the Hall.
        let water_nearby = [40.0, 80.0, 120.0].into_iter().any(|radius| {
            (0..24).any(|i| {
                let angle = i as f32 / 24.0 * std::f32::consts::TAU;
                let point = site.hall.xz() + Vec2::new(angle.cos(), angle.sin()) * radius;
                terrain.get_water_height(point.x, point.y).is_some()
            })
        });
        if water_nearby {
            planner.place(Kind::FishermansHut);
        }
        if target >= 16 && opportunity_roll(site.salt, Kind::LivestockFarm) < 0.55 {
            planner.place(Kind::LivestockFarm);
        }
        // Size productive land and processors from actual approved yields.
        // A poor farm cannot feed 28 people merely because its shell exists;
        // extra farms can share a mill/bakery instead of repeating a full kit.
        for _ in 0..18 {
            let capacity = planner.capacity();
            if capacity.sustains(target) {
                break;
            }
            if !planner.place(capacity.next_grain_workplace()) {
                debug!(
                    "Founding layout food chain failed at {:?}, target={}, placed={}",
                    site.hall,
                    target,
                    planner.plots.len()
                );
                break;
            }
        }
        if !planner.capacity().sustains(target) {
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
        if opportunity_roll(site.salt, Kind::LumberjackHut) < 0.85 {
            planner.optional(Kind::LumberjackHut);
        }
        if opportunity_roll(site.salt, Kind::StoneQuarry) < 0.8 {
            planner.optional(Kind::StoneQuarry);
        }
        if target >= 28 {
            if opportunity_roll(site.salt, Kind::StorageHall) < 0.8 {
                planner.optional(Kind::StorageHall);
            }
            if opportunity_roll(site.salt, Kind::Tavern) < 0.75 {
                planner.optional(Kind::Tavern);
            }
        }
        if target >= 48 {
            planner.optional(Kind::Church);
        }
        return Some(Layout {
            plots: planner.plots,
            population: target,
            square: planner.square,
        });
    }
    None
}
