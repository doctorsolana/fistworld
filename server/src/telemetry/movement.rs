//! Opt-in, read-only movement evidence for connected NPC regression runs.

use crate::{
    player::hero::MoveTarget,
    world::{
        village::{MootQueueTicket, VillagerIntent},
        village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute},
    },
};
use bevy::prelude::*;
use serde_json::json;
use shared::{components::*, economy::PorterCartState};
use std::{
    fs::File,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

#[derive(Resource)]
struct MovementTrace {
    output: BufWriter<File>,
    started: Instant,
    sampled: Instant,
}

pub(crate) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("FISTWORLD_MOVEMENT_TRACE_DIR") else {
        return;
    };
    std::fs::create_dir_all(&directory).expect("movement trace directory");
    let file = File::create(std::path::Path::new(&directory).join("server-movement.jsonl"))
        .expect("movement trace file");
    app.insert_resource(MovementTrace {
        output: BufWriter::new(file),
        started: Instant::now(),
        sampled: Instant::now(),
    });
    app.add_systems(PostUpdate, sample);
}

fn sample(
    mut trace: ResMut<MovementTrace>,
    actors: Query<(
        Entity,
        &PersonId,
        &PlayerPosition,
        Option<&CharacterMotion>,
        Option<&MoveTarget>,
        Option<&TravelRoute>,
        Option<&VillagerIntent>,
        Option<&CharacterObjective>,
        Option<&CharacterActivity>,
        Has<PorterCartState>,
        Has<BuildingDoorUse>,
        Has<NavigationRoutePending>,
        Has<NavigationRouteFailed>,
        Option<&MootQueueTicket>,
    )>,
) {
    if trace.sampled.elapsed() < Duration::from_millis(200) {
        return;
    }
    trace.sampled = Instant::now();
    let actors: Vec<_> = actors.iter().take(2048).map(|(entity, id, position, motion, target, route, intent, objective, activity, cart, door, pending, failed, queue)| json!({
        "entity": format!("{entity:?}"), "id": id.0, "position": position.0.to_array(),
        "velocity": motion.map(|m| m.velocity.to_array()), "target": target.map(|t| t.0.to_array()),
        "route": route.map(|r| json!({"next":r.next,"len":r.waypoints.len(),"waypoint":r.waypoints.get(r.next).map(|p|p.position.to_array()),"goal":r.goal.to_array()})),
        "intent": format!("{intent:?}"), "objective": format!("{objective:?}"), "activity": format!("{activity:?}"),
        "cart": cart, "door": door, "pending":pending, "failed":failed, "queue":format!("{queue:?}")
    })).collect();
    let record = json!({"elapsed":trace.started.elapsed().as_secs_f64(),"actors":actors});
    writeln!(trace.output, "{record}").expect("movement trace write");
    trace.output.flush().expect("movement trace flush");
}
