//! Map editor library.
//!
//! `main.rs` is a thin wrapper around [`app::run`]; the modules live here so other
//! binaries in this crate (notably `src/bin/worldgen.rs`) can drive generation headlessly.

pub mod app;
pub mod camera;
pub mod city;
pub mod lighting;
pub mod picking;
pub mod session;
pub mod terrain_material;
pub mod tools;
pub mod ui;
pub mod worldgen;
