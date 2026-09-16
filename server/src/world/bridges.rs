//! Completed walking decks, indexed once when authoritative crossings change.
//! A reservation never supplies ground; terrain and obstacle checks stay separate.

use bevy::{platform::collections::HashMap, prelude::*};
use shared::{
    components::{PortGeometry, RoadBridge, SettlementPort},
    terrain::WorldTerrain,
};
use std::sync::Arc;

const CELL: f32 = 32.0;

#[derive(Resource, Default, Clone, Debug)]
pub struct BridgeDecks {
    pub(crate) version: u64,
    bridges: Arc<Vec<RoadBridge>>,
    cells: Arc<HashMap<(i32, i32), Vec<usize>>>,
    ports: Arc<Vec<PortGeometry>>,
    port_cells: Arc<HashMap<(i32, i32), Vec<usize>>>,
}

impl BridgeDecks {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &RoadBridge> {
        self.bridges.iter()
    }

    pub(crate) fn height_at(&self, point: Vec2, clearance: f32) -> Option<f32> {
        self.cells
            .get(&cell(point))
            .into_iter()
            .flatten()
            .filter_map(|index| self.bridges[*index].height_at(point, clearance))
            .chain(
                self.port_cells
                    .get(&cell(point))
                    .into_iter()
                    .flatten()
                    .filter_map(|index| self.ports[*index].walk_height_at(point, clearance)),
            )
            .max_by(f32::total_cmp)
    }

    /// Refine the coarse regional lattice before its next step passes a ramp.
    pub(crate) fn near(&self, point: Vec2, radius: f32) -> bool {
        let low = cell(point - Vec2::splat(radius));
        let high = cell(point + Vec2::splat(radius));
        (low.0..=high.0).any(|x| {
            (low.1..=high.1).any(|z| {
                self.port_cells.get(&(x, z)).into_iter().flatten().any(|i| {
                    self.ports[*i]
                        .footprints()
                        .into_iter()
                        .any(|rect| rect.distance_squared(point) <= radius.max(0.).powi(2))
                }) || self.cells.get(&(x, z)).into_iter().flatten().any(|i| {
                    let bridge = &self.bridges[*i];
                    let delta = bridge.end.xz() - bridge.start.xz();
                    let t = ((point - bridge.start.xz()).dot(delta) / delta.length_squared())
                        .clamp(0., 1.);
                    point.distance_squared(bridge.start.xz() + delta * t)
                        <= (radius + bridge.width * 0.5).powi(2)
                })
            })
        })
    }

    fn replace(&mut self, bridges: Vec<RoadBridge>, ports: Vec<PortGeometry>) {
        if *self.bridges == bridges && *self.ports == ports {
            return;
        }
        let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::default();
        for (i, bridge) in bridges.iter().enumerate() {
            let margin = Vec2::splat(bridge.width * 0.5);
            let low = cell(bridge.start.xz().min(bridge.end.xz()) - margin);
            let high = cell(bridge.start.xz().max(bridge.end.xz()) + margin);
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    cells.entry((x, z)).or_default().push(i);
                }
            }
        }
        let mut port_cells: HashMap<(i32, i32), Vec<usize>> = HashMap::default();
        for (i, port) in ports.iter().enumerate() {
            // Index the three actual rectangles rather than the broad port AABB.
            for footprint in port.footprints() {
                let corners = footprint.corners();
                let low = cell(
                    corners
                        .into_iter()
                        .fold(Vec2::splat(f32::INFINITY), Vec2::min),
                );
                let high = cell(
                    corners
                        .into_iter()
                        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max),
                );
                for x in low.0..=high.0 {
                    for z in low.1..=high.1 {
                        let bucket = port_cells.entry((x, z)).or_default();
                        // Each port contributes at most three adjacent rectangles.
                        if bucket.last() != Some(&i) {
                            bucket.push(i);
                        }
                    }
                }
            }
        }
        self.ports = Arc::new(ports);
        self.port_cells = Arc::new(port_cells);
        self.bridges = Arc::new(bridges);
        self.cells = Arc::new(cells);
        self.version = self.version.wrapping_add(1).max(1);
    }
}

fn cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / CELL).floor() as i32,
        (point.y / CELL).floor() as i32,
    )
}

pub fn rebuild_bridge_decks(
    mut decks: ResMut<BridgeDecks>,
    bridges: Query<(Entity, &RoadBridge)>,
    changed: Query<(), Changed<RoadBridge>>,
    mut removed: RemovedComponents<RoadBridge>,
    ports: Query<(Entity, &SettlementPort)>,
    changed_ports: Query<(), Changed<SettlementPort>>,
    mut removed_ports: RemovedComponents<SettlementPort>,
) {
    let removed = removed.read().count() > 0;
    let removed_ports = removed_ports.read().count() > 0;
    if changed.is_empty() && changed_ports.is_empty() && !removed && !removed_ports {
        return;
    }
    let mut complete: Vec<_> = bridges
        .iter()
        .filter(|(_, b)| b.built && b.valid())
        .map(|(e, b)| (e.to_bits(), b.clone()))
        .collect();
    complete.sort_unstable_by_key(|(id, _)| *id);
    let mut complete_ports: Vec<_> = ports
        .iter()
        .filter(|(_, p)| p.built && p.geometry.valid())
        .map(|(e, p)| (e.to_bits(), p.geometry))
        .collect();
    complete_ports.sort_unstable_by_key(|(id, _)| *id);
    decks.replace(
        complete.into_iter().map(|(_, b)| b).collect(),
        complete_ports.into_iter().map(|(_, p)| p).collect(),
    );
}

pub(crate) fn ground_height(
    terrain: &WorldTerrain,
    decks: Option<&BridgeDecks>,
    point: Vec2,
    clearance: f32,
) -> f32 {
    let terrain_height = terrain.get_height(point.x, point.y);
    decks
        .and_then(|decks| decks.height_at(point, clearance))
        .map_or(terrain_height, |deck| deck.max(terrain_height))
}

/// Same dense sweep as tactical planning: a large time-warp step cannot jump
/// between two supported endpoints across an unfinished or missing deck.
pub(crate) fn segment_walkable(
    terrain: &WorldTerrain,
    decks: Option<&BridgeDecks>,
    start: Vec2,
    end: Vec2,
    clearance: f32,
) -> bool {
    let steps = (start.distance(end) / crate::world::navgrid::NAVIGATION_SAMPLE_STEP)
        .ceil()
        .max(1.) as usize;
    let mut previous = None;
    (0..=steps).all(|i| {
        let p = start.lerp(end, i as f32 / steps as f32);
        let support = decks.and_then(|d| d.height_at(p, clearance));
        if terrain.get_water_height(p.x, p.y).is_some() && support.is_none() {
            return false;
        }
        let height = ground_height(terrain, decks, p, clearance);
        let clear = previous.is_none_or(|h: f32| (height - h).abs() <= 0.47);
        previous = Some(height);
        clear
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bridge() -> RoadBridge {
        RoadBridge {
            start: Vec3::ZERO,
            end: Vec3::new(40., 0., 0.),
            deck_height: 3.,
            ramp_length: 8.,
            width: 3.6,
            built: true,
        }
    }
    #[test]
    fn index_tracks_completion_removal_and_exact_footprint_without_idle_rebuilds() {
        let mut app = App::new();
        app.init_resource::<BridgeDecks>()
            .add_systems(Update, rebuild_bridge_decks);
        let mut b = bridge();
        b.built = false;
        let entity = app.world_mut().spawn(b).id();
        app.update();
        assert_eq!(
            app.world()
                .resource::<BridgeDecks>()
                .height_at(Vec2::new(20., 0.), 0.4),
            None
        );
        app.world_mut().get_mut::<RoadBridge>(entity).unwrap().built = true;
        app.update();
        let decks = app.world().resource::<BridgeDecks>();
        assert_eq!(decks.height_at(Vec2::new(20., 0.), 0.4), Some(3.));
        assert_eq!(decks.height_at(Vec2::new(20., 1.5), 0.4), None);
        let version = decks.version;
        app.update();
        assert_eq!(app.world().resource::<BridgeDecks>().version, version);
        app.world_mut().despawn(entity);
        app.update();
        assert_eq!(
            app.world()
                .resource::<BridgeDecks>()
                .height_at(Vec2::new(20., 0.), 0.4),
            None
        );
        assert!(app.world().resource::<BridgeDecks>().version > version);
    }
    #[test]
    fn completed_port_support_is_indexed_across_seams_and_removed_before_water_steps() {
        use shared::components::{SettlementId, ShipKind};
        let mut terrain = WorldTerrain::default();
        let water = terrain
            .water_surface_height(1700., 0.)
            .expect("water-capable fixture");
        terrain.apply_flatten_rect(
            Vec3::new(1700., water + 0.8, 0.),
            Vec2::new(60., 40.),
            0.,
            0.,
        );
        terrain.apply_flatten_rect(
            Vec3::new(1720., water - 6., 0.),
            Vec2::new(12., 30.),
            0.,
            0.,
        );
        let geometry = PortGeometry {
            shore: Vec3::new(1700., water + 1., 0.),
            pier_end: Vec3::new(1726., water + 1.4, 0.),
            berth: Vec3::new(1730., water, 0.),
            departure: Vec3::new(1730., water, -12.),
            yaw: 0.,
            maximum_ship: ShipKind::Coaster,
        };
        let start = geometry.deck_point(-6.).xz();
        let end = geometry.deck_point(25.).xz();
        assert!(terrain.get_water_height(end.x, end.y).is_some());
        let mut app = App::new();
        app.insert_resource(terrain)
            .init_resource::<BridgeDecks>()
            .add_systems(Update, rebuild_bridge_decks);
        let entity = app
            .world_mut()
            .spawn(SettlementPort {
                settlement: SettlementId(1),
                geometry,
                built: false,
            })
            .id();
        app.update();
        let reserved = app.world().resource::<BridgeDecks>();
        assert!(
            reserved
                .height_at(geometry.deck_point(12.).xz(), 0.35)
                .is_none()
        );
        assert!(!segment_walkable(
            app.world().resource::<WorldTerrain>(),
            Some(reserved),
            start,
            end,
            0.35
        ));
        app.world_mut()
            .get_mut::<SettlementPort>(entity)
            .unwrap()
            .built = true;
        app.update();
        let supported = app.world().resource::<BridgeDecks>().clone();
        assert_eq!(
            supported.iter().count(),
            0,
            "ports must not become road-graph bridge connectors"
        );
        assert_eq!(
            supported.height_at(geometry.shore.xz(), 0.35),
            Some(geometry.shore.y)
        );
        let terrain = app.world().resource::<WorldTerrain>();
        assert!((terrain.get_height(1700., 0.) - (water + 0.8)).abs() < 0.001);
        assert_eq!(
            ground_height(terrain, Some(&supported), geometry.shore.xz(), 0.35),
            geometry.shore.y
        );
        for along in [1.9, 2., 2.1, 12., 21.9, 22., 22.1, 25.] {
            assert!(
                (supported
                    .height_at(geometry.deck_point(along).xz(), 0.35)
                    .unwrap()
                    - geometry.deck_height(along))
                .abs()
                    < 0.0001
            );
        }
        assert!(segment_walkable(
            terrain,
            Some(&supported),
            start,
            end,
            0.35
        ));
        assert!(supported.near(geometry.deck_point(12.).xz() + Vec2::Y * 2.5, 0.4));
        assert!(!supported.near(Vec2::new(-300., -300.), 4.));
        let version = supported.version;
        app.update();
        assert_eq!(app.world().resource::<BridgeDecks>().version, version);
        app.world_mut().despawn(entity);
        app.update();
        let removed = app.world().resource::<BridgeDecks>();
        assert!(removed.version > version);
        assert!(removed.height_at(end, 0.35).is_none());
        assert!(!segment_walkable(
            app.world().resource::<WorldTerrain>(),
            Some(removed),
            start,
            end,
            0.35
        ));
    }
}
