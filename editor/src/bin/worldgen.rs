//! Headless world generation.
//!
//! The editor's generator is the only thing that can build a world, but it normally needs
//! a GUI session to drive it. This runs the same code with no window, so worlds can be
//! generated from the command line, in scripts, or by an agent iterating on the result.
//!
//! ```text
//! cargo run -p editor --bin worldgen -- --map big_world --style showcase --size 8192
//! cargo run -p editor --bin worldgen -- --help
//! ```
//!
//! Run from the repo root so map paths resolve against `client/assets/maps`.

use std::path::PathBuf;

use bevy::prelude::*;

use editor::session::{EditorEnvironmentState, EditorSession};
use editor::tools::VisualRefreshFlags;
use editor::worldgen::{generate_world, WorldStyle};

use shared::map::{
    load_map, map_definition_path, map_dir_for_id, save_map_definition_atomic,
    save_map_edits_atomic, MapBounds, MapDefinition, MapTerrain,
};
use shared::terrain::WorldTerrain;
use shared::worldgen::{GeneratedWorld, WORLDGEN_VERSION};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let mut map_id = "big_world".to_string();
    let mut style = WorldStyle::Showcase;
    let mut seed: u64 = 2026;
    let mut size: f32 = 8192.0;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let mut value = || -> String {
            i += 1;
            args.get(i).cloned().unwrap_or_default()
        };
        match arg.as_str() {
            "--map" => map_id = value(),
            "--seed" => seed = value().parse().unwrap_or(seed),
            "--size" => size = value().parse().unwrap_or(size),
            "--style" => {
                style = match value().to_ascii_lowercase().as_str() {
                    "island" => WorldStyle::Island,
                    "mainland" => WorldStyle::Mainland,
                    _ => WorldStyle::Showcase,
                }
            }
            other => eprintln!("worldgen: ignoring unknown arg '{other}'"),
        }
        i += 1;
    }

    if let Err(err) = run(&map_id, style, seed, size) {
        eprintln!("worldgen: {err}");
        std::process::exit(1);
    }
}

fn run(map_id: &str, style: WorldStyle, seed: u64, size: f32) -> Result<(), String> {
    // The generator reads bounds from the active map, so the target map has to exist
    // first. Seed it from the shipped map rather than inventing an empty definition,
    // which keeps terrain/water settings consistent with everything else.
    std::env::set_var("CITYSIM_MAP_ID", map_id);

    // `map_dir_for_id` is relative ("maps/<id>") and only becomes real when the loader
    // resolves it against an asset root, so writing needs the absolute path.
    let asset_root = std::env::var("FISTFORCE_ASSET_PATH")
        .or_else(|_| std::env::var("BEVY_ASSET_ROOT").map(|r| format!("{r}/assets")))
        .unwrap_or_else(|_| "client/assets".to_string());
    std::env::set_var("FISTFORCE_ASSET_PATH", &asset_root);
    let map_dir: PathBuf = PathBuf::from(&asset_root).join(map_dir_for_id(map_id));

    let half = (size * 0.5).clamp(256.0, 65_536.0);

    // A generated world needs no shipped files at all — just its recipe. If
    // the map directory doesn't exist yet, seed it with a bare recipe
    // definition; the loader rebuilds terrain from it and generate_world
    // overwrites everything below with the real content.
    if !map_dir.exists() {
        std::fs::create_dir_all(&map_dir)
            .map_err(|err| format!("Failed to create {}: {err}", map_dir.display()))?;
        let definition = MapDefinition {
            map_id: map_id.to_string(),
            bounds: MapBounds {
                min: [-half, -half],
                max: [half, half],
            },
            terrain: MapTerrain {
                // Unused for generated maps; kept for the schema's sake.
                heightmap: "height.png".to_string(),
                minimap: None,
                water_level: Some(0.0),
                height_min: 0.0,
                height_max: 0.0,
            },
            generated: Some(GeneratedWorld {
                style,
                seed,
                generator_version: WORLDGEN_VERSION,
                half_extent: half,
            }),
            player_spawn: None,
            objects: Vec::new(),
            blockers: Vec::new(),
        };
        save_map_definition_atomic(&map_definition_path(&map_dir), &definition)?;
        println!("worldgen: created new map directory {}", map_dir.display());
    }

    let loaded = load_map(map_id)?;

    let mut definition = loaded.definition.clone();
    definition.bounds = MapBounds {
        min: [-half, -half],
        max: [half, half],
    };

    let mut session = EditorSession::new(
        map_id.to_string(),
        map_dir.clone(),
        map_definition_path(&map_dir),
        definition,
        loaded.edits.clone(),
    );

    // Generation reads bounds off the terrain, so it must see the resized map rather than
    // whatever was on disk.
    let mut resized = load_map(map_id)?;
    resized.definition.bounds = session.map_definition.bounds;
    let mut world = WorldTerrain::default();
    world.reload_from_loaded_map(resized);

    let mut env_state = EditorEnvironmentState::default();
    let mut city_state = Default::default();
    let mut flags = VisualRefreshFlags::default();

    let started = std::time::Instant::now();
    generate_world(
        style,
        seed,
        &mut session,
        &mut world,
        &mut env_state,
        &mut city_state,
        &mut flags,
    )?;
    let elapsed = started.elapsed();

    session.map_definition.validate()?;
    session.map_edits.validate()?;
    save_map_definition_atomic(&session.map_path, &session.map_definition)?;
    save_map_edits_atomic(&session.map_dir, &session.map_edits)?;

    let props = session.map_definition.objects.len();
    let roads = session.map_edits.roads.len();
    println!(
        "worldgen: '{}' {:?} seed {} — {:.0}x{:.0}m, {} props, {} roads, spawn {:?}, {:.1}s",
        map_id,
        style,
        seed,
        size,
        size,
        props,
        roads,
        session.map_definition.player_spawn,
        elapsed.as_secs_f32(),
    );
    Ok(())
}

fn print_help() {
    println!(
        r#"worldgen — generate a world headlessly (no editor window)

USAGE (from repo root):
    cargo run -p editor --bin worldgen -- [OPTIONS]

OPTIONS:
    --map <id>       Target map id; the directory must already exist  [default: big_world]
    --style <s>      showcase | island | mainland                     [default: showcase]
    --seed <n>       Generation seed                                  [default: 2026]
    --size <m>       Map edge length in metres                        [default: 8192]

If the map directory does not exist it is created from a bare seed recipe — a generated
world needs no shipped files. Generated maps store the recipe in map.ron and rebuild
terrain from it at load; height.png is only read by legacy hand-authored maps.
"#
    );
}
