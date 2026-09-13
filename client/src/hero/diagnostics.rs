//! Optional connected movement/animation samples; never installed in normal play.

use super::{animation::HeroAnim, motion::HeroVisual};
use bevy::{animation::AnimatedBy, prelude::*};
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

pub(super) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("FISTWORLD_MOVEMENT_TRACE_DIR") else {
        return;
    };
    std::fs::create_dir_all(&directory).expect("movement trace directory");
    let file = File::create(std::path::Path::new(&directory).join("client-movement.jsonl"))
        .expect("movement trace file");
    app.insert_resource(MovementTrace {
        output: BufWriter::new(file),
        started: Instant::now(),
        sampled: Instant::now(),
    });
    app.add_systems(PostUpdate, sample.after(bevy::app::AnimationSystems));
}

fn sample(
    mut trace: ResMut<MovementTrace>,
    actors: Query<(
        &PersonId,
        &PlayerPosition,
        &Transform,
        &HeroVisual,
        Option<&CharacterMotion>,
        Option<&CharacterActivity>,
        Has<PorterCartState>,
        &HeroAnim,
    )>,
    players: Query<&AnimationPlayer>,
    bones: Query<(&Name, &AnimatedBy, &Transform)>,
    names: Query<&Name>,
) {
    if trace.sampled.elapsed() < Duration::from_millis(100) {
        return;
    }
    trace.sampled = Instant::now();
    let actors: Vec<_> = actors.iter().take(2048).map(|(id, position, visual, speed, motion, activity, cart, anim)| {
        let active = players.get(anim.player).ok().and_then(|p| anim.current_body.and_then(|clip|p.animation(clip)));
        let legs: Vec<_> = if cart { bones.iter().filter(|(name, by, _)| by.0 == anim.player && name.as_str().starts_with("leg.")).map(|(name, _, t)|json!({"name":name.as_str(),"rotation":t.rotation.to_array()})).collect() } else { Vec::new() };
        json!({"id":id.0,"player":names.get(anim.player).ok().map(Name::as_str),"position":position.0.to_array(),"visual":visual.translation.to_array(),"visual_speed":speed.speed(),
            "velocity":motion.map(|m|m.velocity.to_array()),"activity":format!("{activity:?}"),"cart":cart,"culled":anim.paused,
            "pull":anim.pull.is_some() && anim.current_body==anim.pull,
            "animation":active.map(|a|json!({"seek":a.seek_time(),"speed":a.speed(),"weight":a.weight(),"paused":a.is_paused()})),"legs":legs})
    }).collect();
    let record = json!({"elapsed":trace.started.elapsed().as_secs_f64(),"actors":actors});
    writeln!(trace.output, "{record}").expect("movement trace write");
    trace.output.flush().expect("movement trace flush");
}
