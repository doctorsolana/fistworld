//! Opt-in evidence of embodied people traversing completed civic gateways.
//!
//! This diagnostic never inserts movement targets or changes simulation. With
//! the environment switch absent it returns before iterating either query.

use std::collections::HashMap;

use bevy::prelude::*;
use shared::components::{
    CharacterKind, FortificationKind, FortificationSegment, PersonId, PlayerPosition,
};

use crate::world::simulation_time::SimulationTime;
use crate::world::village::strategic::StrategicPerson;

const SIDE_HYSTERESIS: f32 = 0.3;
const OBSERVATION_DISTANCE: f32 = 12.0;
const MAX_OBSERVATIONS: usize = 4_096;
const MAX_GATES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ActorKey {
    Person(PersonId),
    Entity(Entity),
}

#[derive(Clone, Copy)]
struct Observation {
    body: Entity,
    /// Last point definitely on one side of the gate, outside the jitter band.
    side_position: Vec3,
    side_time: f64,
    last_position: Vec3,
    last_time: f64,
    seen: u64,
}

/// Public because Bevy's system type crosses the module boundary. All mutable
/// trace data is local to this optional diagnostic system, never game state.
#[derive(Default)]
pub struct DefensePassageTrace {
    enabled: Option<bool>,
    tick: u64,
    world_seconds: f64,
    real_since_report: f32,
    observations: HashMap<(ActorKey, Entity), Observation>,
    villager_crossings: u64,
    hero_crossings: u64,
    excluded_jumps: u64,
    capacity_skips: u64,
}

fn gate_side(gate: &FortificationSegment, point: Vec3) -> f32 {
    let axis = (gate.end.xz() - gate.start.xz()).normalize_or_zero();
    (point.xz() - gate.midpoint().xz()).dot(Vec2::new(-axis.y, axis.x))
}

fn plausible_step(previous: Vec3, current: Vec3, elapsed: f32) -> bool {
    previous.is_finite() && current.is_finite() && elapsed.is_finite() && elapsed > 0.0
        // Twelve m/s includes running/riding without allowing a distant spawn,
        // route correction or strategic promotion to masquerade as passage.
        && previous.distance(current) <= (elapsed * 12.0 + 0.15).min(12.0)
}

/// Signed crossing of the open span, including slope-adjusted floor/headroom.
/// Reaching the plane and turning back, travelling along it, crossing a post,
/// and a discontinuous position jump are all excluded.
fn crossed_gate(
    gate: &FortificationSegment,
    previous: Vec3,
    current: Vec3,
    elapsed: f32,
) -> Option<i8> {
    if !gate.complete
        || gate.kind != FortificationKind::Gate
        || !plausible_step(previous, current, elapsed)
    {
        return None;
    }
    let a = gate_side(gate, previous);
    let b = gate_side(gate, current);
    if a.abs() < SIDE_HYSTERESIS || b.abs() < SIDE_HYSTERESIS || a.signum() == b.signum() {
        return None;
    }
    let t = a / (a - b);
    let crossing = previous.lerp(current, t);
    let direction = gate.end.xz() - gate.start.xz();
    let length = direction.length();
    if length <= 2.0 * shared::physics::CHARACTER_NAV_RADIUS {
        return None;
    }
    let along = (crossing.xz() - gate.start.xz()).dot(direction / length);
    // Keep the capsule clear of the gate posts; midpoint-nearness alone would
    // count a diagonal route passing outside the actual opening.
    if along < shared::physics::CHARACTER_NAV_RADIUS
        || along > length - shared::physics::CHARACTER_NAV_RADIUS
    {
        return None;
    }
    let floor = gate.start.y + (gate.end.y - gate.start.y) * along / length;
    if crossing.y < floor - 0.6 || crossing.y > floor + gate.material.gate_clear_height() {
        return None;
    }
    Some(if b > 0.0 { 1 } else { -1 })
}

pub fn trace_defense_passages(
    time: SimulationTime,
    mut trace: Local<DefensePassageTrace>,
    gates: Query<(Entity, &FortificationSegment)>,
    people: Query<
        (Entity, Option<&PersonId>, &CharacterKind, &PlayerPosition),
        Without<StrategicPerson>,
    >,
) {
    let enabled = *trace.enabled.get_or_insert_with(|| {
        std::env::var("FISTWORLD_LAB_DEFENSE_TRACE").is_ok_and(|value| value == "1")
    });
    if !enabled {
        return;
    }
    if !time.world_seconds().is_finite() || time.world_seconds() <= 0.0 {
        return;
    }
    trace.tick = trace.tick.wrapping_add(1);
    trace.world_seconds += f64::from(time.world_seconds());
    trace.real_since_report += time.real_seconds();
    let now = trace.world_seconds;
    let tick = trace.tick;
    let gates: Vec<_> = gates
        .iter()
        .filter(|(_, gate)| gate.complete && gate.kind == FortificationKind::Gate)
        .take(MAX_GATES)
        .collect();
    if gates.is_empty() {
        trace.observations.clear();
    } else {
        for (entity, person, kind, position) in &people {
            let actor = person
                .copied()
                .map_or(ActorKey::Entity(entity), ActorKey::Person);
            for (gate_entity, gate) in &gates {
                if position.0.xz().distance(gate.midpoint().xz())
                    > gate.length() * 0.5 + OBSERVATION_DISTANCE
                {
                    continue;
                }
                let key = (actor, *gate_entity);
                let initial = Observation {
                    body: entity,
                    side_position: position.0,
                    side_time: now,
                    last_position: position.0,
                    last_time: now,
                    seen: tick,
                };
                let Some(previous) = trace.observations.get(&key).copied() else {
                    if trace.observations.len() < MAX_OBSERVATIONS {
                        trace.observations.insert(key, initial);
                    } else {
                        trace.capacity_skips += 1;
                    }
                    continue;
                };
                if previous.body != entity
                    || previous.seen.wrapping_add(1) != tick
                    || !plausible_step(
                        previous.last_position,
                        position.0,
                        (now - previous.last_time) as f32,
                    )
                {
                    trace.excluded_jumps += 1;
                    trace.observations.insert(key, initial);
                    continue;
                }
                if let Some(direction) = crossed_gate(
                    gate,
                    previous.side_position,
                    position.0,
                    (now - previous.side_time) as f32,
                ) {
                    match kind {
                        CharacterKind::Villager => trace.villager_crossings += 1,
                        CharacterKind::Hero => trace.hero_crossings += 1,
                    }
                    let total = trace.villager_crossings + trace.hero_crossings;
                    if total <= 24 || total % 32 == 0 || trace.villager_crossings == 1 {
                        info!("Defense passage: settlement={} circuit={} gate={} actor={:?} kind={:?} direction={} x={:.2} z={:.2} villager_crossings={} hero_crossings={}",
                            gate.settlement_id.0, gate.circuit, gate_entity.to_bits(), actor, kind, direction,
                            position.0.x, position.0.z, trace.villager_crossings, trace.hero_crossings);
                    }
                }
                let mut next = previous;
                next.last_position = position.0;
                next.last_time = now;
                next.seen = tick;
                if gate_side(gate, position.0).abs() >= SIDE_HYSTERESIS {
                    next.side_position = position.0;
                    next.side_time = now;
                }
                trace.observations.insert(key, next);
            }
        }
    }
    if tick % 60 == 0 {
        trace
            .observations
            .retain(|_, observation| tick.wrapping_sub(observation.seen) <= 120);
    }
    if trace.real_since_report >= 5.0 {
        trace.real_since_report = 0.0;
        info!("Defense passage totals: gates={} tracked={} villagers={} heroes={} excluded_jumps={} capacity_skips={}",
            gates.len(), trace.observations.len(), trace.villager_crossings, trace.hero_crossings,
            trace.excluded_jumps, trace.capacity_skips);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{FortificationMaterial, SettlementId};

    fn gate() -> FortificationSegment {
        FortificationSegment {
            settlement_id: SettlementId(1),
            circuit: 0,
            start: Vec3::new(-4.0, 1.0, 0.0),
            end: Vec3::new(4.0, 2.0, 0.0),
            kind: FortificationKind::Gate,
            material: FortificationMaterial::Palisade,
            complete: true,
        }
    }

    #[test]
    fn passage_requires_crossing_the_actual_open_span_in_either_direction() {
        let gate = gate();
        let a = Vec3::new(0.0, 1.5, -1.0);
        let b = Vec3::new(0.0, 1.5, 1.0);
        assert_eq!(crossed_gate(&gate, a, b, 0.25), Some(1));
        assert_eq!(crossed_gate(&gate, b, a, 0.25), Some(-1));
        assert_eq!(crossed_gate(&gate, a, Vec3::new(0.0, 1.5, 0.1), 0.25), None);
        assert_eq!(
            crossed_gate(&gate, a + Vec3::X * 6.0, b + Vec3::X * 6.0, 0.25),
            None
        );
        assert_eq!(
            crossed_gate(&gate, a + Vec3::Y * 6.0, b + Vec3::Y * 6.0, 0.25),
            None
        );
    }

    #[test]
    fn walking_along_a_wall_pending_gates_and_position_jumps_are_not_passages() {
        let mut gate = gate();
        assert_eq!(
            crossed_gate(
                &gate,
                Vec3::new(-1.0, 1.5, 0.0),
                Vec3::new(1.0, 1.5, 0.0),
                0.25
            ),
            None
        );
        assert_eq!(
            crossed_gate(
                &gate,
                Vec3::new(0.0, 1.5, -20.0),
                Vec3::new(0.0, 1.5, 20.0),
                5.0
            ),
            None
        );
        assert_eq!(
            crossed_gate(
                &gate,
                Vec3::new(0.0, 1.5, -1.0),
                Vec3::new(0.0, 1.5, 1.0),
                0.001
            ),
            None
        );
        gate.complete = false;
        assert_eq!(
            crossed_gate(
                &gate,
                Vec3::new(0.0, 1.5, -1.0),
                Vec3::new(0.0, 1.5, 1.0),
                0.25
            ),
            None
        );
        gate.complete = true;
        gate.kind = FortificationKind::Wall;
        assert_eq!(
            crossed_gate(
                &gate,
                Vec3::new(0.0, 1.5, -1.0),
                Vec3::new(0.0, 1.5, 1.0),
                0.25
            ),
            None
        );
    }
}
