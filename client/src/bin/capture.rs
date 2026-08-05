//! Screenshot the game world for visual verification.
//!
//! ```text
//! cargo run -p client --bin capture -- --out /tmp/shots --at 0,0,0 --zoom 200
//! cargo run -p client --bin capture -- --preset spawn        # a few useful angles
//! ```
//!
//! Must be run from the repo root (or with `BEVY_ASSET_ROOT` set) so the asset server
//! resolves `client/assets`.

use std::path::PathBuf;

use bevy::math::Vec3;
use client::capture::{run, CaptureConfig, Shot};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let mut out_dir = PathBuf::from("/tmp/citysim-shots");
    let mut focus = Vec3::ZERO;
    let mut yaw = -0.45_f32;
    let mut zoom = 220.0_f32;
    let mut tilt = 0.75_f32;
    let mut time_of_day = 0.5_f32;
    let mut warmup_frames = 240_u32;
    let mut settle_frames = 60_u32;
    let mut preset: Option<String> = None;
    let mut name = "shot".to_string();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut value = || -> String {
            i += 1;
            args.get(i).cloned().unwrap_or_default()
        };
        match arg {
            "--out" => out_dir = PathBuf::from(value()),
            "--name" => name = value(),
            "--at" => focus = parse_vec3(&value()),
            "--yaw" => yaw = value().parse().unwrap_or(yaw),
            "--zoom" => zoom = value().parse().unwrap_or(zoom),
            "--tilt" => tilt = value().parse().unwrap_or(tilt),
            "--time" => time_of_day = value().parse().unwrap_or(time_of_day),
            "--warmup" => warmup_frames = value().parse().unwrap_or(warmup_frames),
            "--settle" => settle_frames = value().parse().unwrap_or(settle_frames),
            "--preset" => preset = Some(value()),
            other => eprintln!("capture: ignoring unknown arg '{other}'"),
        }
        i += 1;
    }

    let shots = match preset.as_deref() {
        Some(p) => preset_shots(p, focus, time_of_day),
        None => vec![Shot {
            name,
            focus,
            yaw,
            zoom,
            tilt,
            time_of_day,
        }],
    };

    println!(
        "capture: {} shot(s) -> {} (warmup {} frames)",
        shots.len(),
        out_dir.display(),
        warmup_frames
    );

    run(CaptureConfig {
        out_dir,
        shots,
        warmup_frames,
        settle_frames,
    });
}

/// Multi-angle presets, so one run answers "how does it look?" rather than one framing.
fn preset_shots(preset: &str, focus: Vec3, time_of_day: f32) -> Vec<Shot> {
    match preset {
        // Orbit the same point — catches anything that only breaks from one direction
        // (back-face winding, billboards, one-sided foliage).
        "orbit" => (0..4)
            .map(|i| Shot {
                name: format!("orbit_{}", i * 90),
                focus,
                yaw: i as f32 * std::f32::consts::FRAC_PI_2,
                zoom: 220.0,
                tilt: 0.7,
                time_of_day,
            })
            .collect(),
        // Near/far pair plus a low angle: LOD popping, terrain silhouette, horizon.
        "survey" => vec![
            Shot {
                name: "close".into(),
                focus,
                zoom: 90.0,
                tilt: 0.55,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "mid".into(),
                focus,
                zoom: 260.0,
                tilt: 0.75,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "far".into(),
                focus,
                zoom: 700.0,
                tilt: 0.95,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "horizon".into(),
                focus,
                zoom: 200.0,
                tilt: 0.18,
                time_of_day,
                ..Default::default()
            },
        ],
        // Same framing across the day — lighting, shadows, atmosphere, water response.
        "daycycle" => vec![
            Shot {
                name: "dawn".into(),
                focus,
                time_of_day: 0.08,
                ..Default::default()
            },
            Shot {
                name: "noon".into(),
                focus,
                time_of_day: 0.5,
                ..Default::default()
            },
            Shot {
                name: "dusk".into(),
                focus,
                time_of_day: 0.92,
                ..Default::default()
            },
        ],
        // Low angle over water: the class of bug that cost the most time historically
        // (winding order, shore foam, caustics, glints).
        "water" => vec![
            Shot {
                name: "water_low".into(),
                focus,
                zoom: 120.0,
                tilt: 0.12,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "water_mid".into(),
                focus,
                zoom: 200.0,
                tilt: 0.45,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "water_down".into(),
                focus,
                zoom: 260.0,
                tilt: 1.1,
                time_of_day,
                ..Default::default()
            },
        ],
        // Hold one close shoreline framing across several seconds. A single
        // water screenshot can accidentally catch a flattering wave phase;
        // this preset makes phase-dependent terrain gaps and foam teeth show.
        "shorecycle" => (0..6)
            .map(|phase| Shot {
                name: format!("shore_phase_{phase}"),
                focus,
                zoom: 105.0,
                tilt: 0.48,
                time_of_day,
                ..Default::default()
            })
            .collect(),
        "spawn" => preset_shots("survey", focus, time_of_day),
        other => {
            eprintln!("capture: unknown preset '{other}', using a single shot");
            vec![Shot {
                focus,
                time_of_day,
                ..Default::default()
            }]
        }
    }
}

fn parse_vec3(s: &str) -> Vec3 {
    let parts: Vec<f32> = s
        .split(',')
        .map(|p| p.trim().parse().unwrap_or(0.0))
        .collect();
    match parts.len() {
        3 => Vec3::new(parts[0], parts[1], parts[2]),
        // "x,z" is the common case when eyeballing a map position.
        2 => Vec3::new(parts[0], 0.0, parts[1]),
        _ => Vec3::ZERO,
    }
}

fn print_help() {
    println!(
        r#"capture — screenshot the game world (no server needed)

USAGE (from repo root):
    cargo run -p client --bin capture -- [OPTIONS]

OPTIONS:
    --out <dir>        Output directory        [default: /tmp/citysim-shots]
    --name <str>       Filename stem for a single shot
    --at <x,y,z>       Camera focus point ("x,z" also works)
    --yaw <rad>        Camera yaw              [default: -0.45]
    --zoom <m>         Distance from focus     [default: 220]
    --tilt <rad>       Downward tilt; 0.1 = near-horizon, 1.2 = top-down
    --time <0..1>      Time of day, 0.5 = noon [default: 0.5]
    --warmup <frames>  Frames before first shot, for streaming  [default: 240]
    --settle <frames>  Frames after each camera move            [default: 60]
    --preset <name>    orbit | survey | daycycle | water | shorecycle

EXAMPLES:
    cargo run -p client --bin capture -- --at 0,0 --preset survey
    cargo run -p client --bin capture -- --at -210,-170 --preset water --out /tmp/water
    cargo run -p client --bin capture -- --at 0,0 --preset daycycle
"#
    );
}
