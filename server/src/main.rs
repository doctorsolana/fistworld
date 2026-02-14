#[path = "ai/mod.rs"]
mod ai;
#[path = "app/mod.rs"]
mod app;
#[path = "collision/mod.rs"]
mod collision;
#[path = "combat/mod.rs"]
mod combat;
#[path = "inventory/mod.rs"]
mod inventory;
#[path = "net/mod.rs"]
mod net;
#[path = "persistence/mod.rs"]
mod persistence;
#[path = "player/mod.rs"]
mod player;
#[path = "telemetry/mod.rs"]
mod telemetry;
#[path = "vehicle/mod.rs"]
mod vehicle;
#[path = "world/mod.rs"]
mod world;

fn main() {
    app::run();
}
