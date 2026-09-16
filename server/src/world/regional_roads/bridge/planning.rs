//! A bounded shortcut survey over a route that has actually carried cargo.
//! It may replace one expensive river-head detour; it cannot invent an island
//! connection from a straight line between towns that have never traded.

use super::super::RegionalStep;
use bevy::prelude::*;
use shared::{components::RoadBridge, spatial::SpatialObstacleGrid, terrain::WorldTerrain};
use std::collections::HashMap;

#[derive(Resource)]
struct PendingShortcut {
    signature: u64,
    corridor: Vec<Vec2>,
    candidates: std::collections::VecDeque<(usize, usize, RoadBridge)>,
}

pub(crate) fn plan_shortcuts(world: &mut World, corridor: &[Vec2]) -> Option<Vec<RegionalStep>> {
    use std::hash::{Hash, Hasher};
    let terrain = world.get_resource::<WorldTerrain>()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    terrain.generator.active_map_content_hash().hash(&mut hash);
    terrain.modification_version().hash(&mut hash);
    for p in corridor {
        (p.x.to_bits(), p.y.to_bits()).hash(&mut hash);
    }
    let signature = hash.finish();
    let previous = world.remove_resource::<PendingShortcut>();
    let Some(mut pending) = previous
        .filter(|p| p.signature == signature)
        .or_else(|| prepare(world, corridor, signature))
    else {
        return Some(Vec::new());
    };
    while let Some((a, b, bridge)) = pending.candidates.front().cloned() {
        match crate::world::village_roads::regional_bridge_footprint_clear(
            world,
            &[bridge.start.xz(), bridge.end.xz()],
            bridge.width,
        ) {
            None => {
                world.insert_resource(pending);
                return None;
            }
            Some(false) => {
                pending.candidates.pop_front();
            }
            Some(true) => {
                let mut steps = dirt_sections(&pending.corridor[..=a]);
                steps.push(RegionalStep::Bridge(bridge));
                steps.extend(dirt_sections(&pending.corridor[b..]));
                return Some(steps);
            }
        }
    }
    Some(dirt_sections(&pending.corridor))
}

fn prepare(world: &World, corridor: &[Vec2], signature: u64) -> Option<PendingShortcut> {
    if corridor.len() < 2 || corridor.len() > 8192 || corridor.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let terrain = world.get_resource::<WorldTerrain>()?;
    let grid = world.get_resource::<SpatialObstacleGrid>();
    // A traded service threshold may be inside its building. Road work starts
    // outside that local doorway, never through its wall. The normal road
    // planner still supplies the short final connection to a service counter.
    let safe = |p: Vec2| {
        let dry = [-1.3, 0.0, 1.3].into_iter().all(|x| {
            [-1.3, 0.0, 1.3]
                .into_iter()
                .all(|z| terrain.get_water_height(p.x + x, p.y + z).is_none())
        });
        dry && grid.is_none_or(|g| !g.point_near_obstacle(p, 1.3))
    };
    let first = corridor
        .iter()
        .position(|p| safe(*p))
        .unwrap_or(corridor.len());
    let end = corridor.iter().rposition(|p| safe(*p)).unwrap_or(0);
    if first >= end
        || corridor[first].distance(corridor[0]) > 32.0
        || corridor[end].distance(*corridor.last().unwrap()) > 32.0
    {
        return None;
    }
    let corridor = &corridor[first..=end];
    let mut walked = vec![0.0; corridor.len()];
    for i in 1..corridor.len() {
        walked[i] = walked[i - 1] + corridor[i - 1].distance(corridor[i]);
    }
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let mut candidates = Vec::new();
    // At most sixteen remembered anchors per spatial cell, sampled about
    // twelve metres apart. No all-pairs scan as the regional graph grows.
    for i in (0..corridor.len()).step_by(6) {
        let p = corridor[i];
        let key = ((p.x / 120.0).floor() as i32, (p.y / 120.0).floor() as i32);
        for x in key.0 - 1..=key.0 + 1 {
            for z in key.1 - 1..=key.1 + 1 {
                for &j in cells.get(&(x, z)).into_iter().flatten() {
                    let distance = corridor[j].distance(p);
                    let detour = walked[i] - walked[j];
                    if (16.0..=120.0).contains(&distance) && detour > distance * 1.8 + 60.0 {
                        candidates.push((detour - distance, j, i));
                        if candidates.len() > 32 {
                            candidates.sort_by(|a, b| {
                                b.0.total_cmp(&a.0)
                                    .then_with(|| a.1.cmp(&b.1))
                                    .then_with(|| a.2.cmp(&b.2))
                            });
                            candidates.truncate(32);
                        }
                    }
                }
            }
        }
        let bucket = cells.entry(key).or_default();
        if bucket.len() < 16 {
            bucket.push(i);
        }
    }
    candidates.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let candidates: Vec<_> = candidates
        .into_iter()
        .take(4)
        .filter_map(|(_, a, b)| {
            survey_bridge(corridor[a], corridor[b], |p| {
                (
                    terrain.get_height(p.x, p.y),
                    terrain.get_water_height(p.x, p.y),
                )
            })
            .map(|bridge| (a, b, bridge))
        })
        .collect();
    Some(PendingShortcut {
        signature,
        corridor: corridor.to_vec(),
        candidates: candidates.into(),
    })
}

pub(crate) fn survey_bridge(
    start: Vec2,
    end: Vec2,
    sample: impl Fn(Vec2) -> (f32, Option<f32>),
) -> Option<RoadBridge> {
    let length = start.distance(end);
    if !(16.0..=120.0).contains(&length) {
        return None;
    }
    let direction = (end - start) / length;
    let side = Vec2::new(-direction.y, direction.x);
    let width = 3.6;
    let mut wet_start = None;
    let mut wet_end = 0.0;
    let mut ended = false;
    let mut highest_water = f32::NEG_INFINITY;
    let samples = (length / 0.5).ceil() as usize;
    for i in 0..=samples {
        let d = length * i as f32 / samples as f32;
        let mut wet = false;
        for s in [-width * 0.5, 0.0, width * 0.5] {
            let (ground, water) = sample(start + direction * d + side * s);
            if !ground.is_finite() {
                return None;
            }
            if let Some(water) = water {
                if !water.is_finite() || water - ground > 6.0 {
                    return None;
                }
                highest_water = highest_water.max(water);
                wet = true;
            }
        }
        if wet {
            if ended {
                return None;
            } // separate channels/islands require separate projects
            wet_start.get_or_insert(d);
            wet_end = d;
        } else if wet_start.is_some() {
            ended = true;
        }
    }
    let wet_start = wet_start?;
    if wet_end - wet_start > 48.0 {
        return None;
    }
    let a = sample(start).0;
    let b = sample(end).0;
    let deck_height =
        (highest_water + shared::components::ROAD_BRIDGE_WATER_CLEARANCE).max(a.max(b));
    let ramp_length = ((deck_height - a.min(b)) / 0.4).ceil().max(4.0);
    if wet_start < ramp_length + 0.5 || length - wet_end < ramp_length + 0.5 {
        return None;
    }
    let bridge = RoadBridge {
        start: Vec3::new(start.x, a, start.y),
        end: Vec3::new(end.x, b, end.y),
        deck_height,
        ramp_length,
        width,
        built: false,
    };
    if !bridge.valid() {
        return None;
    }
    for i in 0..=samples {
        let d = length * i as f32 / samples as f32;
        for s in [-width * 0.5, 0.0, width * 0.5] {
            let (h, water) = sample(start + direction * d + side * s);
            if h > bridge.surface_height(d) + 0.2 {
                return None;
            }
            if (d <= ramp_length || d >= length - ramp_length) && water.is_some() {
                return None;
            }
        }
    }
    Some(bridge)
}

fn dirt_sections(points: &[Vec2]) -> Vec<RegionalStep> {
    let mut out = Vec::new();
    let Some(&first) = points.first() else {
        return out;
    };
    let mut section = vec![first];
    let mut distance = 0.0;
    for edge in points.windows(2) {
        let length = edge[0].distance(edge[1]);
        if !length.is_finite() || length > 32.1 {
            return Vec::new();
        }
        if length < 0.01 {
            continue;
        }
        if distance + length > 128.0 && section.len() >= 2 {
            out.push(RegionalStep::Dirt(std::mem::replace(
                &mut section,
                vec![edge[0]],
            )));
            distance = 0.0;
        }
        section.push(edge[1]);
        distance += length;
    }
    if section.len() >= 2 {
        out.push(RegionalStep::Dirt(section));
    }
    out
}
