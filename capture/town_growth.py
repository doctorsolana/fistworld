#!/usr/bin/env python3
"""Photograph a real town-growth snapshot with the normal Bevy renderer."""

import argparse
import json
import math
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def scenario(snapshot, source, output):
    hall = snapshot["settlements"][0]["position"]
    buildings = snapshot["buildings"]
    complete = [entry for entry in buildings if entry["construction"] is None]
    houses = [entry for entry in complete if entry["kind"] == "House"]
    extents = [40.0]
    for entry in snapshot["settlements"] + buildings + snapshot["fields"] + snapshot["pastures"]:
        center = entry["footprint_center"]
        half_diagonal = math.hypot(*entry["footprint"]) * 0.5
        extents.append(math.hypot(center[0] - hall[0], center[1] - hall[2]) + half_diagonal + 8)
    for entry in snapshot["roads"]:
        for x, z in entry["road"]["points"]:
            extents.append(math.hypot(x - hall[0], z - hall[2]) + 8)
    fortifications = snapshot.get("fortifications", [])
    complete_defenses = [entry for entry in fortifications if entry["complete"]]
    for entry in fortifications:
        for x, _, z in [entry["start"], entry["end"]]:
            extents.append(math.hypot(x - hall[0], z - hall[2]) + 8)
    radius = max(extents)
    square = snapshot["settlements"][0].get("civic_square")
    if square:
        radius = max(radius, math.hypot(square["center"][0] - hall[0], square["center"][2] - hall[2])
                     + math.hypot(*square["half_extents"]) + 8)
    # Inspect the closest group of actual homes as well as the entire town.
    def neighbor_distance(home):
        x, _, z = home["position"]
        return min(
            (math.hypot(other["position"][0] - x, other["position"][2] - z)
             for other in houses if other is not home),
            default=0,
        )

    detail = min(houses, key=neighbor_distance)["position"] if houses else hall
    shots = []
    views = [
        ("overview", hall, -0.45, radius * 2.0),
        ("reverse", hall, 2.5, radius * 2.0),
        ("neighborhood", detail, -0.8, 95.0),
    ]
    if square:
        views.append(("civic-square", square["center"], square["rotation"] - 0.4, 120.0))
    districts = snapshot.get("districts", [])
    if districts:
        ward = max(districts, key=lambda ward: sum(
            math.hypot(home["position"][0] - ward["center"][0],
                       home["position"][2] - ward["center"][1]) < 60 for home in houses))
        views.append(("residential-quarter", [ward["center"][0], hall[1], ward["center"][1]], -0.5, 175.0))
    gates = [entry for entry in complete_defenses if entry["kind"] == "Gate"]
    if gates:
        gate = gates[0]
        center = [(a + b) * 0.5 for a, b in zip(gate["start"], gate["end"])]
        delta = [b - a for a, b in zip(gate["start"], gate["end"])]
        views.append(("gateway", center, -math.atan2(delta[2], delta[0]) + 0.35, 38.0))
    for name, focus, yaw, zoom in views:
        shots.append(f"""(
            name: {json.dumps(name)}, focus: ({focus[0]}, {focus[1]}, {focus[2]}),
            yaw: {yaw}, zoom: {zoom}, time_of_day: 0.5,
            assertions: [settlements_at_least(count: {len(snapshot['settlements'])}),
                         settlement_buildings_at_least(count: {len(complete)}),
                         fortification_sections_at_least(count: {len(complete_defenses)})],
        )""")
    if len(houses) >= 2:
        # Stand just outside an actual doorway and look along its local house
        # row. Preserve the exported floor height instead of putting a free-
        # look camera at sea level on an inland hill.
        home = min(houses, key=neighbor_distance)
        neighbor = min((other for other in houses if other is not home),
                       key=lambda other: math.hypot(other["position"][0] - home["position"][0],
                                                    other["position"][2] - home["position"][2]))
        door = home["door"]
        dx, dz = door[0] - home["position"][0], door[2] - home["position"][2]
        length = max(math.hypot(dx, dz), 0.001)
        focus = [door[0] + 2 * dx / length, door[1], door[2] + 2 * dz / length]
        along_x = neighbor["position"][0] - home["position"][0]
        along_z = neighbor["position"][2] - home["position"][2]
        yaw = math.atan2(-along_x, -along_z)
        shots.append(f"""(
            name: "street", focus: ({focus[0]}, {focus[1]}, {focus[2]}),
            yaw: {yaw}, zoom: 50.0, pitch: Some(-0.03), eye: 1.9, time_of_day: 0.5,
            assertions: [settlement_buildings_at_least(count: {len(complete)})],
        )""")
    return f"""(
    version: 1,
    name: {json.dumps(f"town-{snapshot['seed']}-{snapshot['profile']}-day-{snapshot['day']}")},
    map: {json.dumps(snapshot['map_id'])},
    output_dir: {json.dumps(str(output))},
    resolution: (1600, 900), target: scene, show_window: false,
    fixed_delta_seconds: 0.016666667, warmup_frames: 240,
    readiness: (minimum_frames: 90, maximum_frames: 1800,
                minimum_loaded_chunks: 25, stable_loaded_chunk_frames: 20),
    environment: {{
        "FISTFORCE_CAPTURE_TOWN_SNAPSHOT": {json.dumps(str(source))},
        "FISTFORCE_CAPTURE_CLOUDS": "clear",
    }},
    shots: [{','.join(shots)}],
)
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--no-build", action="store_true", help="Use the existing playtest capture binary")
    args = parser.parse_args()
    source = args.snapshot.resolve()
    snapshot = json.loads(source.read_text())
    if snapshot.get("version") != 1 or not snapshot.get("settlements"):
        parser.error("expected a version-1 town snapshot containing a settlement")
    output = (args.output or ROOT / "logs/captures/town-growth" / source.parent.name / source.stem).resolve()
    output.mkdir(parents=True, exist_ok=True)
    scenario_path = output / "scenario.ron"
    scenario_path.write_text(scenario(snapshot, source, output))
    env = {key: value for key, value in os.environ.items() if not key.startswith("FISTFORCE_CAPTURE_")}
    env["BEVY_ASSET_ROOT"] = str(ROOT / "client/assets")
    target = Path(env.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    if not args.no_build:
        with (output / "build.log").open("w") as log:
            subprocess.run(
                ["cargo", "build", "--offline", "--profile", "playtest", "-p", "client", "--bin", "capture"],
                cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True,
            )
    with (output / "capture.log").open("w") as log:
        subprocess.run(
            [str(target / "playtest/capture"), "--scenario", str(scenario_path)],
            cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True,
        )
    print(f"Town views and capture metadata: {output}")


if __name__ == "__main__":
    main()
