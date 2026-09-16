//! Bounded per-port holding positions and exclusive near-pier movement.
//! Reservations authorize a destination, never teleport a hull or replace the
//! retained water navigator. An inbound voyage never reserves the far berth.

use super::routes::PortBerthReservation;
use crate::player::boat::clearance::{WaterNavigationGeometry, WatercraftClearance};
use bevy::{platform::collections::HashMap, prelude::*};
use shared::{components::*, terrain::WorldTerrain};

const MAX_PORTS: usize = 128;
const MAX_HOLDINGS: usize = 12;
const GAP: f32 = 3.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelDirection {
    Arrival,
    Departure,
}

#[derive(Clone, Copy)]
struct Holder {
    entity: Entity,
    id: ShipId,
    kind: ShipKind,
}
impl Holder {
    fn read(world: &World, entity: Entity) -> Option<Self> {
        Some(Self {
            entity,
            id: *world.get::<ShipId>(entity)?,
            kind: world.get::<CompanyShip>(entity)?.kind,
        })
    }
    fn valid(self, world: &World) -> bool {
        world.get::<ShipId>(self.entity) == Some(&self.id)
            && world
                .get::<CompanyShip>(self.entity)
                .is_some_and(|ship| ship.kind == self.kind)
            && world.get::<Vessel>(self.entity).is_some()
    }
    fn radius(self) -> f32 {
        WatercraftClearance::for_ship(self.kind).radius
    }
}
#[derive(Clone, Copy)]
struct Holding {
    holder: Holder,
    point: Vec3,
    revision: (u64, u32, u64),
}
#[derive(Default)]
struct PortState {
    holds: Vec<Holding>,
    channel: Option<(Holder, ChannelDirection)>,
}
#[derive(Resource, Default)]
struct Traffic {
    ports: HashMap<Entity, PortState>,
}

fn prune(world: &World, traffic: &mut Traffic) {
    // Only retained reservations are visited, capped at128 ports ×12 holders.
    traffic.ports.retain(|port, state| {
        if world
            .get::<SettlementPort>(*port)
            .is_none_or(|port| !port.built)
        {
            return false;
        }
        state.holds.retain(|hold| hold.holder.valid(world));
        if state.channel.is_some_and(|(owner, _)| !owner.valid(world)) {
            state.channel = None;
        }
        !state.holds.is_empty() || state.channel.is_some()
    });
}
fn distance_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let delta = end - start;
    let t = ((point - start).dot(delta) / delta.length_squared().max(0.0001)).clamp(0., 1.);
    point.distance(start + delta * t)
}
fn exit_for(port: &SettlementPort, kind: ShipKind) -> Vec2 {
    let forward = (port.geometry.departure.xz() - port.geometry.berth.xz()).normalize_or_zero();
    port.geometry.departure.xz() + forward * (2. * WatercraftClearance::for_ship(kind).radius + GAP)
}

/// Keep the channel until the departing hull reaches this point, not merely
/// the shared departure centre where an arriving hull would collide with it.
pub(crate) fn departure_clear_point(world: &World, port: Entity, ship: Entity) -> Option<Vec3> {
    let holder = Holder::read(world, ship)?;
    let port = world.get::<SettlementPort>(port)?;
    if !port.accepts(holder.kind) {
        return None;
    }
    let terrain = world.get_resource::<WorldTerrain>()?;
    let geometry = world.get_resource::<WaterNavigationGeometry>()?;
    let point = exit_for(port, holder.kind);
    geometry
        .segment_clear(
            terrain,
            port.geometry.departure.xz(),
            point,
            WatercraftClearance::for_ship(holder.kind),
        )
        .then(|| {
            terrain
                .get_water_height(point.x, point.y)
                .map(|y| Vec3::new(point.x, y, point.y))
        })?
}

/// Reserve/reuse one of12 separated anchorage points off the shared channel.
/// None means keep the current safe location and retry later; no fallback uses
/// the crowded departure point. Actual travel still needs the water planner.
pub(crate) fn holding_point(world: &mut World, port_entity: Entity, ship: Entity) -> Option<Vec3> {
    let holder = Holder::read(world, ship)?;
    if !holder.valid(world) {
        return None;
    }
    let port = *world.get::<SettlementPort>(port_entity)?;
    if !port.accepts(holder.kind) {
        return None;
    }
    let mut traffic = world.remove_resource::<Traffic>().unwrap_or_default();
    prune(world, &mut traffic);
    let result = (|| {
        let terrain = world.get_resource::<WorldTerrain>()?;
        let geometry = world.get_resource::<WaterNavigationGeometry>()?;
        let revision = geometry.revision(terrain);
        if let Some(state) = traffic.ports.get_mut(&port_entity) {
            if let Some(existing) = state
                .holds
                .iter_mut()
                .find(|hold| hold.holder.id == holder.id)
            {
                if existing.revision == revision
                    || geometry.point_clear(
                        terrain,
                        existing.point.xz(),
                        WatercraftClearance::for_ship(holder.kind),
                    )
                {
                    existing.revision = revision;
                    return Some(existing.point);
                }
                // Keep invalidated custody until an explicit route cancellation
                // releases it; never move another boat into its current berth.
                return None;
            }
            if state.holds.len() >= MAX_HOLDINGS {
                return None;
            }
        } else if traffic.ports.len() >= MAX_PORTS {
            return None;
        }
        let forward = (port.geometry.departure.xz() - port.geometry.berth.xz()).normalize_or_zero();
        let mut side = Vec2::new(-forward.y, forward.x);
        // Alongside T-head berths depart parallel to the shore. Mirror the
        // holding field toward open water, not equally onto dry land. All
        // candidates still need the unchanged full-hull terrain/obstacle proof.
        if side.dot(port.geometry.seaward()) < 0. {
            side = -side;
        }
        let spacing = 2. * WatercraftClearance::for_ship(ShipKind::Cog).radius + GAP;
        let channel_end = exit_for(&port, ShipKind::Cog);
        for index in 0..MAX_HOLDINGS {
            let column = 1 + index % 3;
            let row = index / 3;
            let point =
                channel_end + side * (column as f32 * spacing) + forward * (row as f32 * spacing);
            if distance_to_segment(point, port.geometry.berth.xz(), channel_end) < spacing - 0.01
                || point.distance(port.geometry.berth.xz()) <= spacing
                || point.distance(port.geometry.departure.xz()) <= spacing
                || traffic
                    .ports
                    .values()
                    .flat_map(|state| &state.holds)
                    .any(|hold| {
                        point.distance(hold.point.xz())
                            < holder.radius() + hold.holder.radius() + GAP - 0.01
                    })
                || !geometry.point_clear(terrain, point, WatercraftClearance::for_ship(holder.kind))
            {
                continue;
            }
            let y = terrain.get_water_height(point.x, point.y)?;
            let point = Vec3::new(point.x, y, point.y);
            traffic
                .ports
                .entry(port_entity)
                .or_default()
                .holds
                .push(Holding {
                    holder,
                    point,
                    revision,
                });
            return Some(point);
        }
        None
    })();
    world.insert_resource(traffic);
    result
}

pub(crate) fn release_holding(world: &mut World, port: Entity, ship: ShipId) {
    if let Some(mut traffic) = world.get_resource_mut::<Traffic>() {
        if let Some(state) = traffic.ports.get_mut(&port) {
            state.holds.retain(|hold| hold.holder.id != ship);
        }
    }
}

/// Call only at the reserved holding point (arrival) or berth (departure).
/// A departure never needs the far port's berth/channel, so occupied ports can
/// exchange ships instead of waiting on each other's reservations.
pub(crate) fn acquire_channel(
    world: &mut World,
    port_entity: Entity,
    ship: Entity,
    direction: ChannelDirection,
) -> bool {
    let Some(holder) = Holder::read(world, ship).filter(|holder| holder.valid(world)) else {
        return false;
    };
    let Some(port) = world
        .get::<SettlementPort>(port_entity)
        .copied()
        .filter(|port| port.accepts(holder.kind))
    else {
        return false;
    };
    let Some(position) = world.get::<PlayerPosition>(ship).map(|p| p.0) else {
        return false;
    };
    let mut traffic = world.remove_resource::<Traffic>().unwrap_or_default();
    prune(world, &mut traffic);
    let acquired = (|| {
        if let Some((owner, retained_direction)) = traffic
            .ports
            .get(&port_entity)
            .and_then(|state| state.channel)
        {
            return owner.id == holder.id && retained_direction == direction;
        }
        match direction {
            ChannelDirection::Arrival => {
                if world
                    .get::<PortBerthReservation>(port_entity)
                    .is_some_and(|owner| owner.0 != holder.id)
                {
                    return false;
                }
                if !traffic.ports.get(&port_entity).is_some_and(|state| {
                    state.holds.iter().any(|hold| {
                        hold.holder.id == holder.id && position.distance(hold.point) <= 0.7
                    })
                }) {
                    return false;
                }
            }
            ChannelDirection::Departure => {
                if position.distance(port.geometry.berth) > 0.7
                    || world
                        .get::<PortBerthReservation>(port_entity)
                        .is_none_or(|owner| owner.0 != holder.id)
                {
                    return false;
                }
            }
        }
        let end = exit_for(&port, ShipKind::Cog);
        if traffic.ports.get(&port_entity).is_some_and(|state| {
            state.holds.iter().any(|hold| {
                hold.holder.id != holder.id
                    && world
                        .get::<PlayerPosition>(hold.holder.entity)
                        .is_some_and(|p| {
                            distance_to_segment(p.0.xz(), port.geometry.berth.xz(), end)
                                < holder.radius() + hold.holder.radius() + GAP - 0.01
                        })
            })
        }) {
            return false;
        }
        if !traffic.ports.contains_key(&port_entity) && traffic.ports.len() >= MAX_PORTS {
            return false;
        }
        traffic.ports.entry(port_entity).or_default().channel = Some((holder, direction));
        true
    })();
    world.insert_resource(traffic);
    acquired
}

pub(crate) fn release_channel(world: &mut World, port: Entity, ship: ShipId) {
    if let Some(mut traffic) = world.get_resource_mut::<Traffic>() {
        if let Some(state) = traffic.ports.get_mut(&port) {
            if state.channel.is_some_and(|(owner, _)| owner.id == ship) {
                state.channel = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{
        map::{HeightmapData, MapBounds},
        terrain::TerrainGenerator,
    };

    fn world(depth: f32) -> World {
        let mut terrain = WorldTerrain::default();
        let mut map = terrain.generator.loaded_map().clone();
        let bounds = MapBounds {
            min: [-128.; 2],
            max: [128.; 2],
        };
        let heights = (0..257)
            .flat_map(|z| (0..257).map(move |_| if z < 132 { 2. } else { -depth }))
            .collect();
        map.definition.bounds = bounds;
        map.definition.generated = None;
        map.heightmap = HeightmapData::new(bounds, 257, 257, heights, Some(0.));
        map.rivers = default();
        map.river_segments_by_chunk.clear();
        map.terrain_deltas_by_chunk.clear();
        terrain.generator = TerrainGenerator::from_loaded_map(map);
        let mut world = World::new();
        world.insert_resource(terrain);
        world.init_resource::<WaterNavigationGeometry>();
        world
    }
    fn port(world: &mut World, x: f32) -> Entity {
        world
            .spawn(SettlementPort {
                settlement: SettlementId(1),
                built: true,
                geometry: PortGeometry {
                    shore: Vec3::new(x, 2., 0.),
                    pier_end: Vec3::new(x, 1., 20.),
                    berth: Vec3::new(x, 0., 26.),
                    departure: Vec3::new(x + 15., 0., 26.),
                    yaw: -std::f32::consts::FRAC_PI_2,
                    maximum_ship: ShipKind::Cog,
                },
            })
            .id()
    }
    fn ship(world: &mut World, id: u64, kind: ShipKind, point: Vec3) -> Entity {
        world
            .spawn((
                ShipId(id),
                CompanyShip {
                    company: CompanyId(1),
                    kind,
                    home_port: BuildingId(1),
                    assigned_route: None,
                    status: ShipStatus::Moored,
                },
                Vessel,
                PlayerPosition(point),
            ))
            .id()
    }
    #[test]
    fn distinct_bounded_holding_slots_keep_the_departure_lane_clear() {
        let mut world = world(4.);
        let port = port(&mut world, 0.);
        let mut positions = Vec::new();
        let mut actors = Vec::new();
        for id in 1..=12 {
            let actor = ship(&mut world, id, ShipKind::Cog, Vec3::new(-90., 0., 90.));
            let position =
                holding_point(&mut world, port, actor).expect("twelve deep-water anchorages");
            assert_eq!(holding_point(&mut world, port, actor), Some(position));
            assert!(
                positions
                    .iter()
                    .all(|p: &Vec3| p.distance(position) >= 12.59)
            );
            let geometry = world.get::<SettlementPort>(port).unwrap();
            assert!(
                (position.xz() - geometry.geometry.berth.xz()).dot(geometry.geometry.seaward())
                    >= 12.59
            );
            assert!(
                distance_to_segment(
                    position.xz(),
                    geometry.geometry.berth.xz(),
                    exit_for(geometry, ShipKind::Cog)
                ) >= 12.59
            );
            positions.push(position);
            actors.push(actor);
        }
        let extra = ship(&mut world, 13, ShipKind::Cog, Vec3::new(-90., 0., 90.));
        assert!(holding_point(&mut world, port, extra).is_none());
        release_holding(&mut world, port, ShipId(1));
        assert!(holding_point(&mut world, port, extra).is_some());
        // Imported/replaced identity cannot retain a dead reservation forever.
        world.entity_mut(actors[1]).insert(ShipId(99));
        let replacement = ship(&mut world, 14, ShipKind::Cog, Vec3::new(-90., 0., 90.));
        assert!(holding_point(&mut world, port, replacement).is_some());
    }
    #[test]
    fn reversed_alongside_departure_keeps_anchorages_seaward() {
        let mut world = world(4.);
        let port = port(&mut world, 0.);
        {
            let mut port = world.get_mut::<SettlementPort>(port).unwrap();
            port.geometry.departure.x = -15.;
            port.geometry.yaw = std::f32::consts::FRAC_PI_2;
        }
        let actor = ship(&mut world, 1, ShipKind::Cog, Vec3::new(-90., 0., 90.));
        let point = holding_point(&mut world, port, actor).expect("deep seaward anchorage");
        let geometry = world.get::<SettlementPort>(port).unwrap().geometry;
        assert!(point.x < geometry.departure.x);
        assert!((point - geometry.berth).xz().dot(geometry.seaward()) >= 12.59);
    }

    #[test]
    fn shallow_anchorages_are_not_granted_to_large_hulls() {
        let mut world = world(0.8);
        let port = port(&mut world, 0.);
        let small = ship(&mut world, 1, ShipKind::Coaster, Vec3::ZERO);
        let large = ship(&mut world, 2, ShipKind::Cog, Vec3::ZERO);
        assert!(holding_point(&mut world, port, small).is_some());
        assert!(departure_clear_point(&world, port, small).is_some());
        assert!(holding_point(&mut world, port, large).is_none());
        assert!(departure_clear_point(&world, port, large).is_none());
    }
    #[test]
    fn occupied_ports_can_depart_without_reserving_each_others_berths() {
        let mut world = world(4.);
        let a = port(&mut world, -48.);
        let b = port(&mut world, 48.);
        let ap = world.get::<SettlementPort>(a).unwrap().geometry.berth;
        let bp = world.get::<SettlementPort>(b).unwrap().geometry.berth;
        let first = ship(&mut world, 1, ShipKind::Coaster, ap);
        let second = ship(&mut world, 2, ShipKind::Coaster, bp);
        world.entity_mut(a).insert(PortBerthReservation(ShipId(1)));
        world.entity_mut(b).insert(PortBerthReservation(ShipId(2)));
        let first_wait = holding_point(&mut world, b, first).unwrap();
        let second_wait = holding_point(&mut world, a, second).unwrap();
        assert!(acquire_channel(
            &mut world,
            a,
            first,
            ChannelDirection::Departure
        ));
        assert!(acquire_channel(
            &mut world,
            b,
            second,
            ChannelDirection::Departure
        ));
        assert!(!acquire_channel(
            &mut world,
            b,
            first,
            ChannelDirection::Arrival
        ));
        // This test supplies physical arrival; full sailing remains navigator/lab coverage.
        world.get_mut::<PlayerPosition>(first).unwrap().0 = first_wait;
        world.get_mut::<PlayerPosition>(second).unwrap().0 = second_wait;
        world.entity_mut(a).remove::<PortBerthReservation>();
        world.entity_mut(b).remove::<PortBerthReservation>();
        release_channel(&mut world, a, ShipId(1));
        release_channel(&mut world, b, ShipId(2));
        assert!(acquire_channel(
            &mut world,
            b,
            first,
            ChannelDirection::Arrival
        ));
        assert!(acquire_channel(
            &mut world,
            a,
            second,
            ChannelDirection::Arrival
        ));
        assert!(!acquire_channel(
            &mut world,
            b,
            second,
            ChannelDirection::Departure
        ));
    }
}
