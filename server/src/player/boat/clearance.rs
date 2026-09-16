//! Hull clearance and change-driven water obstacles shared by surveys and sailors.
use super::{water_at, VesselNavigationQueue};
use bevy::{platform::collections::HashMap, prelude::*};
use shared::{
    components::{PortGeometry, RoadBridge, SettlementPort, ShipKind},
    terrain::WorldTerrain,
};
use std::sync::{Arc, OnceLock};

const CELL: f32 = 32.0;
/// Deep side trusses extend 1.01 m beneath the walking surface. Reserving this
/// across the span is conservative even where only deck boards are overhead.
const BRIDGE_UNDERSIDE: f32 = 1.02;
pub(super) const SAMPLE_STEP: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WatercraftClearance {
    pub radius: f32,
    pub draft: f32,
    pub air_draft: f32,
}
impl WatercraftClearance {
    pub(crate) const DINGHY: Self = Self {
        radius: 2.35,
        draft: 0.20,
        air_draft: 4.5,
    };
    pub(crate) const fn for_ship(kind: ShipKind) -> Self {
        match kind {
            ShipKind::Coaster => Self {
                radius: 3.20,
                draft: 0.65,
                air_draft: 4.5,
            },
            ShipKind::Cog => Self {
                radius: 4.80,
                draft: 1.0,
                air_draft: 6.0,
            },
        }
    }
    pub(super) fn key(self) -> [u32; 3] {
        [
            self.radius.to_bits(),
            self.draft.to_bits(),
            self.air_draft.to_bits(),
        ]
    }
    pub(super) fn samples(self) -> Arc<Vec<Vec2>> {
        static DINGHY: OnceLock<Arc<Vec<Vec2>>> = OnceLock::new();
        static COASTER: OnceLock<Arc<Vec<Vec2>>> = OnceLock::new();
        static COG: OnceLock<Arc<Vec<Vec2>>> = OnceLock::new();
        let cached = if self.radius == Self::DINGHY.radius {
            &DINGHY
        } else if self.radius == Self::for_ship(ShipKind::Coaster).radius {
            &COASTER
        } else {
            &COG
        };
        cached.get_or_init(|| self.build_samples()).clone()
    }
    fn build_samples(self) -> Arc<Vec<Vec2>> {
        // A half-diagonal radius permits arbitrary waypoint turns. An additional
        // half-cell covers the gaps between longitudinal and transverse samples.
        let radius = self.radius + SAMPLE_STEP * std::f32::consts::FRAC_1_SQRT_2;
        let n = (radius / SAMPLE_STEP).ceil() as i32;
        let mut offsets: Vec<_> = (-n..=n)
            .flat_map(|x| (-n..=n).map(move |z| Vec2::new(x as f32, z as f32) * SAMPLE_STEP))
            .filter(|p| p.length_squared() <= radius * radius)
            .collect();
        let ring = (std::f32::consts::TAU * radius / SAMPLE_STEP).ceil() as usize;
        for i in 0..ring {
            let angle = std::f32::consts::TAU * i as f32 / ring as f32;
            offsets.push(Vec2::new(angle.cos(), angle.sin()) * radius);
        }
        Arc::new(offsets)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Obstacle {
    Bridge(RoadBridge),
    Pier(PortGeometry),
}
#[derive(Resource, Clone, Default, Debug)]
pub(crate) struct WaterNavigationGeometry {
    pub(crate) version: u64,
    obstacles: Arc<Vec<Obstacle>>,
    cells: Arc<HashMap<(i32, i32), Vec<usize>>>,
}
fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// The same conservative mast/width test used by the indexed runtime query.
/// A proposed bridge can be checked before staging without installing fake
/// completed geometry in the authoritative world.
pub(crate) fn bridge_passage_clear(
    bridge: &RoadBridge,
    point: Vec2,
    water: f32,
    hull: WatercraftClearance,
) -> bool {
    let delta = bridge.end.xz() - bridge.start.xz();
    let length = delta.length();
    let axis = delta / length;
    let relative = point - bridge.start.xz();
    let along = relative.dot(axis);
    let across = relative.perp_dot(axis).abs();
    let dx = (0.0 - along).max(along - length).max(0.0);
    let dy = (across - bridge.width * 0.5).max(0.0);
    if dx * dx + dy * dy > hull.radius * hull.radius {
        return true;
    }
    let low = (along - hull.radius).clamp(0., length);
    let high = (along + hull.radius).clamp(0., length);
    let underside = bridge.surface_height(low).min(bridge.surface_height(high)) - BRIDGE_UNDERSIDE;
    water + hull.air_draft <= underside
}

impl WaterNavigationGeometry {
    /// Fixture/planning proof against prospective solid piers, without
    /// publishing them or granting actual boats any navigation permission.
    pub(crate) fn with_proposed_ports(
        &self,
        ports: impl IntoIterator<Item = PortGeometry>,
    ) -> Self {
        let mut next = self.clone();
        let mut obstacles = self.obstacles.as_ref().clone();
        for port in ports {
            let obstacle = Obstacle::Pier(port);
            if port.valid() && !obstacles.contains(&obstacle) {
                obstacles.push(obstacle);
            }
        }
        next.replace(obstacles);
        next
    }

    pub(crate) fn revision(&self, terrain: &WorldTerrain) -> (u64, u32, u64) {
        (
            terrain.generator.active_map_content_hash(),
            terrain.modification_version(),
            self.version,
        )
    }
    fn replace(&mut self, obstacles: Vec<Obstacle>) {
        if *self.obstacles == obstacles {
            return;
        }
        let mut cells: HashMap<(i32, i32), Vec<usize>> = default();
        for (i, obstacle) in obstacles.iter().enumerate() {
            let (minimum, maximum) = match obstacle {
                Obstacle::Bridge(b) => (
                    b.start.xz().min(b.end.xz()) - Vec2::splat(b.width * 0.5),
                    b.start.xz().max(b.end.xz()) + Vec2::splat(b.width * 0.5),
                ),
                Obstacle::Pier(p) => p
                    .footprints()
                    .into_iter()
                    .flat_map(|rect| rect.corners())
                    .fold(
                        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                        |(low, high), point| (low.min(point), high.max(point)),
                    ),
            };
            let low = cell(minimum);
            let high = cell(maximum);
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    cells.entry((x, z)).or_default().push(i);
                }
            }
        }
        self.obstacles = Arc::new(obstacles);
        self.cells = Arc::new(cells);
        self.version = self.version.wrapping_add(1).max(1);
    }
    /// Exact radius/structure overlap before the sampled depth proof. Piers are
    /// solid obstacles; completed bridges allow passage only below the trusses.
    pub(super) fn structure_clear(
        &self,
        point: Vec2,
        water: f32,
        hull: WatercraftClearance,
    ) -> bool {
        let low = cell(point - Vec2::splat(hull.radius));
        let high = cell(point + Vec2::splat(hull.radius));
        (low.0..=high.0).all(|x| {
            (low.1..=high.1).all(|z| {
                self.cells
                    .get(&(x, z))
                    .into_iter()
                    .flatten()
                    .all(|i| match &self.obstacles[*i] {
                        Obstacle::Pier(port) => !port.water_obstructs(point, hull.radius),
                        Obstacle::Bridge(bridge) => {
                            bridge_passage_clear(bridge, point, water, hull)
                        }
                    })
            })
        })
    }
    /// A proposed landing/T-head cannot overlap another pier or a bridge.
    /// The identical completed port may revalidate itself before launching.
    pub(crate) fn port_footprint_clear(&self, proposed: &PortGeometry) -> bool {
        let overlaps = |a: shared::components::PortFootprint,
                        b: shared::components::PortFootprint| {
            shared::components::oriented_rects_overlap(
                a.center,
                a.half_extents,
                a.yaw,
                b.center,
                b.half_extents,
                b.yaw,
            )
        };
        let proposed_rects = proposed.footprints();
        let mut candidates = std::collections::BTreeSet::new();
        for rect in proposed_rects {
            let corners = rect.corners();
            let low = cell(corners.into_iter().reduce(Vec2::min).unwrap());
            let high = cell(corners.into_iter().reduce(Vec2::max).unwrap());
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    candidates.extend(self.cells.get(&(x, z)).into_iter().flatten().copied());
                }
            }
        }
        candidates
            .into_iter()
            .all(|index| match &self.obstacles[index] {
                Obstacle::Pier(existing) if existing == proposed => true,
                Obstacle::Pier(existing) => !existing
                    .footprints()
                    .into_iter()
                    .any(|b| proposed_rects.into_iter().any(|a| overlaps(a, b))),
                Obstacle::Bridge(bridge) => {
                    let delta = bridge.end.xz() - bridge.start.xz();
                    let rect = shared::components::PortFootprint {
                        center: (bridge.start.xz() + bridge.end.xz()) * 0.5,
                        half_extents: Vec2::new(bridge.width, delta.length()) * 0.5,
                        yaw: (-delta.x).atan2(-delta.y),
                    };
                    !proposed_rects.into_iter().any(|a| overlaps(a, rect))
                }
            })
    }
    pub(crate) fn point_clear(
        &self,
        terrain: &WorldTerrain,
        point: Vec2,
        hull: WatercraftClearance,
    ) -> bool {
        let Some(water) = water_at(terrain, point) else {
            return false;
        };
        self.structure_clear(point, water, hull)
            && hull
                .samples()
                .iter()
                .all(|offset| depth_clear(terrain, point + *offset, hull.draft))
    }
    pub(crate) fn segment_clear(
        &self,
        terrain: &WorldTerrain,
        start: Vec2,
        end: Vec2,
        hull: WatercraftClearance,
    ) -> bool {
        if !start.is_finite() || !end.is_finite() {
            return false;
        }
        let samples = hull.samples();
        let steps = (start.distance(end) / SAMPLE_STEP).ceil().max(1.) as usize;
        (0..=steps).all(|i| {
            let p = start.lerp(end, i as f32 / steps as f32);
            water_at(terrain, p).is_some_and(|water| self.structure_clear(p, water, hull))
                && samples
                    .iter()
                    .all(|offset| depth_clear(terrain, p + *offset, hull.draft))
        })
    }
}
pub(super) fn depth_clear(terrain: &WorldTerrain, point: Vec2, draft: f32) -> bool {
    if !terrain
        .generator
        .active_map_bounds()
        .contains_xz(point.x, point.y)
    {
        return false;
    }
    let Some(water) = terrain.water_surface_height(point.x, point.y) else {
        return false;
    };
    // get_water_height already samples the edited ground to decide wetness.
    // Reuse that same sample for draft instead of asking for the height twice;
    // the strict wetness comparison also preserves zero/negative-draft behavior.
    let ground = terrain.get_height(point.x, point.y);
    ground < water && water - ground >= draft
}

/// Shared snapshot for immigration preflight as well as already spawned vessels.
/// Nothing rescans all bridges or ports per hull or per route node.
pub(crate) fn rebuild_water_navigation_geometry(
    mut geometry: ResMut<WaterNavigationGeometry>,
    bridges: Query<(Entity, &RoadBridge)>,
    ports: Query<(Entity, &SettlementPort)>,
    changed_bridges: Query<(), Changed<RoadBridge>>,
    changed_ports: Query<(), Changed<SettlementPort>>,
    mut removed_bridges: RemovedComponents<RoadBridge>,
    mut removed_ports: RemovedComponents<SettlementPort>,
    queue: Option<ResMut<VesselNavigationQueue>>,
) {
    let removed = removed_bridges.read().count() + removed_ports.read().count() > 0;
    if !changed_bridges.is_empty() || !changed_ports.is_empty() || removed {
        let mut obstacles: Vec<_> = bridges
            .iter()
            .filter(|(_, b)| b.built && b.valid())
            .map(|(e, b)| (e.to_bits(), Obstacle::Bridge(b.clone())))
            .chain(
                ports
                    .iter()
                    .filter(|(_, p)| p.built && p.geometry.valid())
                    .map(|(e, p)| (e.to_bits(), Obstacle::Pier(p.geometry))),
            )
            .collect();
        obstacles.sort_unstable_by_key(|(id, _)| *id);
        geometry.replace(
            obstacles
                .into_iter()
                .map(|(_, obstacle)| obstacle)
                .collect(),
        );
    }
    if let Some(mut queue) = queue {
        queue.cache.geometry = geometry.clone();
    }
}

#[cfg(test)]
#[path = "clearance_tests.rs"]
mod tests;
