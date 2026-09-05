//! Screenshot the game world for visual verification.
//!
//! ```text
//! cargo run -p client --bin capture -- --out /tmp/shots --at 0,0,0 --zoom 200
//! cargo run -p client --bin capture -- --preset spawn        # a few useful angles
//! ```
//!
//! Set `BEVY_ASSET_ROOT="$PWD/client/assets"` when running from the repo so the asset
//! server uses the source asset tree rather than looking beside the built executable.

use std::path::PathBuf;

use bevy::math::Vec3;
use client::capture::{run, CaptureConfig, Shot};
use client::capture_artifact::{
    CaptureComparisonConfig, CaptureRecordingConfig, CaptureScenario, CaptureTarget,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let scenario_path = args
        .windows(2)
        .find(|pair| pair[0] == "--scenario")
        .map(|pair| PathBuf::from(&pair[1]));
    let loaded_scenario = scenario_path.as_ref().map(|path| {
        CaptureScenario::load(path).unwrap_or_else(|error| {
            eprintln!("capture: {error}");
            std::process::exit(2);
        })
    });
    if let Some(scenario) = &loaded_scenario {
        scenario.apply_environment();
    }

    let mut out_dir = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.output_dir.clone())
        .unwrap_or_else(|| PathBuf::from("/tmp/citysim-shots"));
    let mut focus = Vec3::ZERO;
    let mut yaw = -0.45_f32;
    let mut zoom = 220.0_f32;
    let mut time_of_day = 0.5_f32;
    let mut warmup_frames = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.warmup_frames)
        .unwrap_or(240);
    let mut settle_frames = if loaded_scenario.is_some() { 0 } else { 60 };
    let mut preset: Option<String> = None;
    let mut name = "shot".to_string();
    let mut pitch: Option<f32> = None;
    let mut eye = 1.7_f32;
    let mut scenario_name = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.name.clone())
        .unwrap_or_else(|| "command-line".to_owned());
    let mut resolution = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.resolution)
        .unwrap_or([1_600, 900]);
    let mut fixed_delta_seconds = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.fixed_delta_seconds)
        .unwrap_or(1.0 / 60.0);
    let mut target = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.target)
        .unwrap_or_default();
    let mut show_window = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.show_window)
        .unwrap_or(true);
    let mut comparison = loaded_scenario
        .as_ref()
        .and_then(|scenario| scenario.comparison.clone());
    let mut recording = loaded_scenario
        .as_ref()
        .and_then(|scenario| scenario.recording.clone());
    let mut diagnostics = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.diagnostics.clone())
        .unwrap_or_default();

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
            "--time" => time_of_day = value().parse().unwrap_or(time_of_day),
            "--pitch" => pitch = value().parse().ok(),
            "--eye" => eye = value().parse().unwrap_or(eye),
            "--warmup" => warmup_frames = value().parse().unwrap_or(warmup_frames),
            "--settle" => settle_frames = value().parse().unwrap_or(settle_frames),
            "--preset" => preset = Some(value()),
            "--scenario" => {
                let _ = value();
            }
            "--scenario-name" => scenario_name = value(),
            "--resolution" => resolution = parse_resolution(&value()).unwrap_or(resolution),
            "--fixed-delta" => fixed_delta_seconds = value().parse().unwrap_or(fixed_delta_seconds),
            "--target" => {
                target = match value().as_str() {
                    "scene" => CaptureTarget::Scene,
                    "window" => CaptureTarget::Window,
                    other => {
                        eprintln!("capture: unknown target '{other}', expected window or scene");
                        target
                    }
                }
            }
            "--hidden" => show_window = false,
            "--compare" => {
                comparison = Some(CaptureComparisonConfig {
                    baseline_dir: PathBuf::from(value()),
                    ..Default::default()
                });
            }
            "--update-baselines" => {
                comparison = Some(CaptureComparisonConfig {
                    baseline_dir: PathBuf::from(value()),
                    update_baselines: true,
                    ..Default::default()
                });
            }
            "--record" => {
                recording = Some(CaptureRecordingConfig {
                    enabled: true,
                    ..Default::default()
                });
            }
            "--perf-overlay" => diagnostics.performance_overlay = true,
            "--gizmos" => diagnostics.gizmos = true,
            "--render-diagnostics" => diagnostics.render_timings = true,
            other => eprintln!("capture: ignoring unknown arg '{other}'"),
        }
        i += 1;
    }

    let mut shots = match (preset.as_deref(), loaded_scenario.as_ref()) {
        (Some(p), _) => preset_shots(p, focus, time_of_day),
        (None, Some(scenario)) => scenario
            .shots
            .clone()
            .into_iter()
            .map(|shot| shot.into_runtime(&scenario.readiness))
            .collect(),
        (None, None) => vec![Shot {
            name,
            focus,
            yaw,
            zoom,
            time_of_day,
            ..Default::default()
        }],
    };
    // Free look composes with presets too: `--preset daycycle --pitch 0.0`
    // photographs the horizon across the whole day.
    if pitch.is_some() {
        for shot in &mut shots {
            shot.pitch = pitch;
            shot.eye = eye;
        }
    }

    // The flight preset only means anything when the camera genuinely moves
    // every rendered frame; settle frames would reintroduce the stationary
    // catch-up gaps the preset exists to eliminate.
    let continuous = matches!(preset.as_deref(), Some("streaming-flight"))
        || loaded_scenario
            .as_ref()
            .is_some_and(|scenario| scenario.continuous);
    let probe_every = loaded_scenario
        .as_ref()
        .map(|scenario| scenario.probe_every)
        .unwrap_or(4);

    println!(
        "capture: {} shot(s) -> {} (warmup {} frames{})",
        shots.len(),
        out_dir.display(),
        warmup_frames,
        if continuous {
            ", continuous flight"
        } else {
            ""
        },
    );

    run(CaptureConfig {
        scenario_name,
        out_dir,
        shots,
        resolution,
        fixed_delta_seconds,
        target,
        show_window,
        comparison,
        recording,
        diagnostics,
        warmup_frames,
        settle_frames: if continuous { 0 } else { settle_frames },
        continuous,
        probe_every,
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
                time_of_day,
                ..Default::default()
            })
            .collect(),
        // Near/far pair plus a low angle: LOD popping, terrain silhouette, horizon.
        "survey" => vec![
            Shot {
                name: "close".into(),
                focus,
                zoom: 90.0,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "mid".into(),
                focus,
                zoom: 260.0,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "far".into(),
                focus,
                zoom: 700.0,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "horizon".into(),
                focus,
                zoom: 200.0,
                pitch: Some(0.18),
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
                pitch: Some(0.12),
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "water_mid".into(),
                focus,
                zoom: 200.0,
                time_of_day,
                ..Default::default()
            },
            Shot {
                name: "water_down".into(),
                focus,
                zoom: 260.0,
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
                time_of_day,
                ..Default::default()
            })
            .collect(),
        // Deliberately jump the middle-zoom camera farther than one streamed
        // chunk between screenshots. With a one-frame settle this catches the
        // real leading-edge handoff while it is incomplete, rather than only
        // photographing the world after streaming has caught up.
        "streaming-pan" => [
            ("pan_warm", Vec3::ZERO),
            ("pan_east", Vec3::new(384.0, 0.0, 0.0)),
            ("pan_north", Vec3::new(384.0, 0.0, -384.0)),
            ("pan_home", Vec3::ZERO),
        ]
        .into_iter()
        .map(|(name, offset)| Shot {
            name: name.into(),
            focus: focus + offset,
            zoom: 1_092.0,
            time_of_day,
            ..Default::default()
        })
        .collect(),
        // Hold the destination for successive one-frame captures after a
        // six-chunk camera jump. This photographs the exact frame where the
        // far-terrain hole commits, rather than only the safe fallback before
        // loading and the settled result afterwards.
        "streaming-handoff" => std::iter::once(Shot {
            name: "handoff_warm".into(),
            focus,
            zoom: 1_092.0,
            time_of_day,
            ..Default::default()
        })
        .chain((0..32).map(|frame| Shot {
            name: format!("handoff_{frame:02}"),
            focus: focus + Vec3::new(384.0, 0.0, 0.0),
            zoom: 1_092.0,
            time_of_day,
            ..Default::default()
        }))
        .collect(),
        // Move the camera every RENDERED frame (continuous mode: no settle,
        // no between-shot file waits), crossing several chunk rows diagonally
        // at a deliberately aggressive ~29 m/frame. Screenshot probes fire
        // asynchronously every 4th frame and are collected after the flight,
        // so streaming never gets a stationary frame to catch up in — this is
        // the preset that actually reproduces fast-pan artifacts.
        "streaming-flight" => (0..48)
            .map(|frame| Shot {
                name: format!("flight_{frame:02}"),
                focus: focus + Vec3::new(frame as f32 * 24.0, 0.0, frame as f32 * -16.0),
                zoom: 1_092.0,
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

fn parse_resolution(value: &str) -> Option<[u32; 2]> {
    let (width, height) = value.split_once('x').or_else(|| value.split_once('X'))?;
    let width = width.trim().parse().ok()?;
    let height = height.trim().parse().ok()?;
    (width > 0 && height > 0).then_some([width, height])
}

fn print_help() {
    println!(
        r#"capture — screenshot the game world (no server needed)

USAGE (from repo root):
    cargo run -p client --bin capture -- [OPTIONS]

OPTIONS:
    --scenario <ron>   Load a checked-in deterministic capture scenario
    --out <dir>        Output directory        [default: /tmp/citysim-shots]
    --name <str>       Filename stem for a single shot
    --at <x,y,z>       Camera focus point ("x,z" also works)
    --yaw <rad>        Camera yaw              [default: -0.45]
    --zoom <m>         Distance from focus     [default: 220]
    --time <0..1>      Time of day, 0.5 = noon [default: 0.5]
    --warmup <frames>  Frames before first shot, for streaming  [default: 240]
    --settle <frames>  Frames after each camera move            [default: 60]
    --pitch <rad>      free-look: pitch below horizon (0 = level, negative = up); bypasses the RTS tilt lock
    --eye <m>          free-look camera height above the water (default 1.7)
    --preset <name>    orbit | survey | daycycle | water | shorecycle | streaming-pan | streaming-handoff | streaming-flight
    --resolution <WxH> Fixed physical capture resolution [default: 1600x900]
    --fixed-delta <s>  Deterministic real-time step       [default: 0.0166667]
    --target <kind>    window (scene + UI) | scene (offscreen 3D only)
    --hidden           Hide the OS window (both capture targets are supported)
    --compare <dir>    Compare PNGs to baselines; write *.diff.png on failure
    --update-baselines <dir>  Create or replace approved baseline PNGs
    --record           Record deterministic raw H.264 (requires --features capture-video)
    --perf-overlay     Include the F3 performance/world overlay
    --gizmos           Include F4 planning/collider gizmos
    --render-diagnostics  Log Bevy render timing diagnostics

EXAMPLES:
    cargo run -p client --bin capture -- --at 0,0 --preset survey
    cargo run -p client --bin capture -- --at -210,-170 --preset water --out /tmp/water
    cargo run -p client --bin capture -- --at 0,0 --preset daycycle
    cargo run -p client --bin capture -- --scenario capture/scenarios/world-survey.ron
"#
    );
}
