#!/usr/bin/env python3
"""Generate continuous vegetation close -> map -> close regression scenarios.

Run the generated RON with the real capture binary; inspect PNGs and their
capture.json counters. No settlement fixture replaces the generated terrain.
The Stoneham location uses the seed from the reported vegetation disappearance;
forest uses the maintained Showcase/91 forest-floor reference.
"""

import argparse
from pathlib import Path


CLOSE_ZOOM = 90.0
MAP_ZOOM = 2400.0
PROBE_EVERY = 30


def pose(frame):
    """Two round trips, with steady views before each semantic assertion."""
    if frame <= 60:
        return CLOSE_ZOOM, "close", True
    cycle = (frame - 61) // 300 + 1
    step = (frame - 61) % 300
    if step < 60:
        progress = (step + 1) / 60
        return CLOSE_ZOOM * (MAP_ZOOM / CLOSE_ZOOM) ** progress, f"out-{cycle}", False
    if step < 120:
        # Counters are sampled in Update, before visibility propagation and
        # deferred streaming commands. Never assert on the transition frame.
        return MAP_ZOOM, f"map-{cycle}", step >= 75
    if step < 180:
        progress = (step - 119) / 60
        return MAP_ZOOM * (CLOSE_ZOOM / MAP_ZOOM) ** progress, f"in-{cycle}", False
    return CLOSE_ZOOM, f"recovered-{cycle}", step >= 225


def scenario(name, map_id, x, z, seed=None):
    shots = []
    for frame in range(661):
        zoom, phase, settled = pose(frame)
        assertions = ""
        if settled and frame % PROBE_EVERY == 0:
            relation, minimum = ("at_most", 0) if phase.startswith("map") else ("at_least", 1)
            assertions = (
                " assertions: ["
                f"prop_roots_{relation}(count: {minimum}), "
                f"visible_tree_roots_{relation}(count: {minimum}), "
                f"grass_batches_{relation}(count: {minimum})],"
            )
        shots.append(
            f'        (name: "{frame:04}-{phase}", '
            f"focus: ({x:.1f}, 0.0, {z:.1f}), yaw: -0.45, "
            f"zoom: {zoom:.5f}, time_of_day: 0.5,{assertions}),"
        )
    seed_environment = f'        "FISTWORLD_WORLD_SEED": "{seed}",\n' if seed is not None else ""
    return f'''(
    version: 1,
    name: "{name}",
    map: "{map_id}",
    output_dir: "logs/captures/{name}",
    resolution: (1280, 720),
    fixed_delta_seconds: 0.016666667,
    target: scene,
    show_window: false,
    warmup_frames: 360,
    readiness: (
        minimum_frames: 60,
        maximum_frames: 1800,
        minimum_loaded_chunks: 289,
        stable_loaded_chunk_frames: 30,
    ),
    continuous: true,
    probe_every: {PROBE_EVERY},
    environment: {{
        "FISTFORCE_NO_SETTINGS_FILE": "1",
        "FISTFORCE_CAPTURE_CLOUDS": "clear",
{seed_environment}    }},
    shots: [
{chr(10).join(shots)}
    ],
)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="Ignored output directory for generated RON")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for name, map_id, x, z, seed in [
        ("stoneham-vegetation-roundtrip", "world", -3189, 1289, 11272639609695457076),
        ("forest-vegetation-roundtrip", "big_world", -3000, -260, None),
    ]:
        path = args.output / f"{name}.ron"
        path.write_text(scenario(name, map_id, x, z, seed))
        print(path)


if __name__ == "__main__":
    main()
