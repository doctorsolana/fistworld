#[path = "ai/mod.rs"]
mod ai;
#[path = "app/mod.rs"]
mod app;
#[path = "city/mod.rs"]
mod city;
#[path = "collision/mod.rs"]
mod collision;
#[path = "inventory/mod.rs"]
mod inventory;
#[path = "net/mod.rs"]
mod net;
#[path = "persistence/mod.rs"]
mod persistence;
#[path = "physics/mod.rs"]
mod physics;
#[path = "player/mod.rs"]
mod player;
#[path = "rail/mod.rs"]
mod rail;
#[path = "telemetry/mod.rs"]
mod telemetry;
#[path = "vehicle/mod.rs"]
mod vehicle;
#[path = "world/mod.rs"]
mod world;

fn main() {
    app::run();
}
