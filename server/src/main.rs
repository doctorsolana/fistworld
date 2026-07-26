#[path = "app/mod.rs"]
mod app;
#[path = "city/mod.rs"]
mod city;
#[path = "collision/mod.rs"]
mod collision;
#[path = "net/mod.rs"]
mod net;
#[path = "persistence/mod.rs"]
mod persistence;
#[path = "physics/mod.rs"]
mod physics;
#[path = "player/mod.rs"]
mod player;
#[path = "telemetry/mod.rs"]
mod telemetry;
#[path = "world/mod.rs"]
mod world;

fn main() {
    app::run();
}
