#!/usr/bin/env python3
"""Generate repeatable continuous RON flights for capture --benchmark.

Run without --benchmark to inspect PNG/JSON probes of exactly the same path.
Each flight crosses chunk boundaries, returns through the same area, and repeats
that circuit so first-use stalls can be distinguished from repeat traversal.
"""

import argparse
import math
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--frames", type=int, default=1200)
    args = parser.parse_args()
    if args.frames < 120:
        parser.error("use at least 120 frames to exercise streaming")
    args.output.mkdir(parents=True, exist_ok=True)
    # Showcase/91 forest-floor and river-mouth fixtures, also used by the
    # checked-in visual scenarios. Zoom sweeps exercise the map-view handoff.
    routes = {
        "forest-close": (-3000, -260, 52),
        "forest-mid": (-3000, -260, 180),
        "forest-wide": (-3000, -260, 700),
        "forest-map": (-3000, -260, 2000),
        "forest-zoom": (-3000, -260, None),
        "coast": (-2585, 1195, 125),
    }
    for name, (x, z, zoom) in routes.items():
        shots = []
        for frame in range(args.frames):
            phase = frame / (args.frames - 1) * 4.0
            travel = 1.0 - abs(phase % 2.0 - 1.0)
            distance = zoom if zoom is not None else 52 * (2400 / 52) ** travel
            # Time advances deterministically too, exercising day/night material
            # updates alongside wind and ocean animation driven by Bevy globals.
            day_time = 0.5 + frame / 60.0 / 900.0
            shots.append(
                f'        (name: "frame-{frame:04}", '
                f'focus: ({x + travel * 384:.5f}, 0.0, {z + math.sin(phase * math.pi) * 96:.5f}), '
                f'yaw: -0.45, zoom: {distance:.5f}, time_of_day: {day_time:.8f}),'
            )
        scenario = f'''(
    version: 1,
    name: "{name}",
    map: "big_world",
    output_dir: "logs/captures/performance/{name}",
    resolution: (1920, 1080),
    fixed_delta_seconds: 0.016666667,
    target: scene,
    show_window: false,
    warmup_frames: 360,
    readiness: (
        minimum_frames: 60,
        maximum_frames: 1200,
        minimum_loaded_chunks: 289,
        stable_loaded_chunk_frames: 30,
    ),
    continuous: true,
    probe_every: {max(1, args.frames // 12)},
    shots: [
{chr(10).join(shots)}
    ],
)
'''
        (args.output / f"{name}.ron").write_text(scenario)


if __name__ == "__main__":
    main()
