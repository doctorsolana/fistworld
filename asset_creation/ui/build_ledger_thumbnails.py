#!/usr/bin/env python3
"""Capture canonical Bevy building art and deliver compact encyclopedia pictures.

Requires Pillow. Run after building the ordinary capture binary:
  python3 asset_creation/ui/build_ledger_thumbnails.py --capture

The maintained RON recipes stage the same authored runtime GLBs using the existing
asset fixture. It disables vegetation and waits for each scene's dependencies.
Source PNG/JSON/logs stay ignored; only compressed display-size JPEGs ship. No
baked UI text, independently rerendered portraits, or full screenshots ship.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
import os
from pathlib import Path
import subprocess

from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[2]
SCENARIOS = ROOT / "capture/scenarios/ledger-buildings"
OUTPUT = ROOT / "client/assets/ui/ledger/buildings"
DEFAULT_CAPTURES = ROOT / "logs/captures/ledger-buildings"
# Anchor distances are only for the initial asset-fixture shot; the delivered
# camera is fitted from actual mesh vertices. All pictures share morning light.
BUILDINGS = {
    "hall": ("MootHall", 24.0),
    "hall-village": ("VillageHall", 31.0),
    "hall-town": ("TownHall", 39.0),
    "house": ("LogCabin", 19.0),
    "house-l2": ("CabinL2", 22.0),
    "house-long": ("LongCabin", 21.0),
    "house-long-l2": ("LongCabinL2", 24.0),
    "lumberjack": ("LumberjackHut", 21.0),
    "windmill": ("WindMill", 40.0),
    "storage": ("StorageHall", 26.0),
    "bakery": ("Bakery", 26.0),
    "farm": ("Farmstead", 25.0),
    "quarry": ("StoneQuarry", 27.0),
    "fisherman": ("FishermansHut", 23.0),
    "market": ("Market", 38.0),
    "tavern": ("Tavern", 34.0),
    "church": ("Church", 34.0),
    "livestock": ("LivestockFarm", 27.0),
}


def glb_vertices(path: Path) -> list[list[float]]:
    """Read authored world-space vertices for a tight, repeatable camera fit."""
    raw = path.read_bytes()
    size = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20:20 + size])
    binary_start = 20 + size + 8
    identity = [[float(i == j) for j in range(4)] for i in range(4)]
    points: list[list[float]] = []

    def multiply(a, b):
        return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]

    def transform(node):
        if "matrix" in node:
            m = node["matrix"]
            return [[m[j * 4 + i] for j in range(4)] for i in range(4)]
        x, y, z, w = node.get("rotation", [0, 0, 0, 1])
        rotation = [
            [1 - 2*y*y - 2*z*z, 2*x*y - 2*z*w, 2*x*z + 2*y*w],
            [2*x*y + 2*z*w, 1 - 2*x*x - 2*z*z, 2*y*z - 2*x*w],
            [2*x*z - 2*y*w, 2*y*z + 2*x*w, 1 - 2*x*x - 2*y*y],
        ]
        scale = node.get("scale", [1, 1, 1])
        translation = node.get("translation", [0, 0, 0])
        return [[rotation[i][j] * scale[j] for j in range(3)] + [translation[i]] for i in range(3)] + [[0, 0, 0, 1]]

    def visit(index, parent):
        node = doc["nodes"][index]
        world = multiply(parent, transform(node))
        if "mesh" in node:
            for primitive in doc["meshes"][node["mesh"]]["primitives"]:
                accessor = doc["accessors"][primitive["attributes"]["POSITION"]]
                if accessor["componentType"] != 5126 or accessor["type"] != "VEC3":
                    raise ValueError("Thumbnail source requires standard float32 GLB positions")
                view = doc["bufferViews"][accessor["bufferView"]]
                offset = binary_start + view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
                stride = view.get("byteStride", 12)
                for index in range(accessor["count"]):
                    vertex = struct.unpack_from("<3f", raw, offset + index * stride)
                    points.append([sum(world[i][j] * vertex[j] for j in range(3)) + world[i][3] for i in range(3)])
        for child in node.get("children", []):
            visit(child, world)

    for node in doc["scenes"][doc.get("scene", 0)]["nodes"]:
        visit(node, identity)
    if not points:
        raise ValueError(f"No render geometry in {path}")
    return points


def camera_for(model: str) -> tuple[float, float, float, float]:
    points = glb_vertices(ROOT / f"client/assets/game_assets/buildings/village/{model}.glb")
    lower = [min(p[i] for p in points) for i in range(3)]
    upper = [max(p[i] for p in points) for i in range(3)]
    centre = [(a+b)/2 for a, b in zip(lower, upper)]
    yaw, pitch = 2.55, 0.48
    sy, cy, sp, cp = math.sin(yaw), math.cos(yaw), math.sin(pitch), math.cos(pitch)
    right, up, backward = (cy, 0, -sy), (-sy*sp, cp, -cy*sp), (sy*cp, sp, cy*cp)
    tan_half_fov = math.tan(math.pi/8)  # Bevy's default45degree perspective FOV.
    distance = 0.0
    for corner in points:
        point = [a-b for a, b in zip(corner, centre)]
        dot = lambda axis: sum(a*b for a, b in zip(point, axis))
        distance = max(distance, abs(dot(right))/(tan_half_fov*1.6*0.84) + dot(backward), abs(dot(up))/(tan_half_fov*0.84) + dot(backward))
    # village_lab seed3 terrain at the maintained(-15,-15)fixture origin.
    ground = 8.885553
    return (-15+centre[0]+backward[0]*distance, -15+centre[2]+backward[2]*distance,
            ground+centre[1]+backward[1]*distance, distance)


def recipe(name: str, model: str, zoom: float) -> str:
    camera_x, camera_z, camera_eye, distance = camera_for(model)
    return f'''// Generated by asset_creation/ui/build_ledger_thumbnails.py; edit the maintained recipe there.
(
    version: 1,
    name: "ledger-building-{name}",
    map: "village_lab",
    output_dir: "logs/captures/ledger-buildings/{name}",
    resolution: (960, 600),
    fixed_delta_seconds: 0.016666667,
    target: scene,
    show_window: false,
    warmup_frames: 180,
    readiness: (minimum_frames: 60, maximum_frames: 1200, minimum_loaded_chunks: 25, stable_loaded_chunk_frames: 20),
    environment: {{
        "FISTFORCE_CAPTURE_ASSET": "game_assets/buildings/village/{model}.glb#Scene0",
        "FISTFORCE_CAPTURE_CLOUDS": "clear",
    }},
    shots: [
        // The first shot anchors asset creation; only the fitted second image ships.
        (name: "anchor", focus: (-15.0, 0.0, -15.0), yaw: 2.55, zoom: {max(zoom, distance * 1.4):.6f}, time_of_day: 0.43),
        (name: "three-quarter", focus: ({camera_x:.6f}, 0.0, {camera_z:.6f}), yaw: 2.55,
         zoom: {distance:.6f}, pitch: Some(0.48), eye: {camera_eye:.6f}, time_of_day: 0.43,
         assertions: [loaded_chunks_at_least(count: 25), prop_roots_at_most(count: 0), grass_batches_at_most(count: 0)]),
    ],
)
'''


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_capture(directory: Path) -> dict:
    metadata = json.loads((directory / "three-quarter.capture.json").read_text())
    assert metadata["target"] == "scene", "A composed UI screenshot is not building artwork"
    assert (metadata["width"], metadata["height"]) == (960, 600)
    assert metadata["assertions"] and all(a["passed"] for a in metadata["assertions"])
    assert metadata.get("comparison_error") is None
    return metadata


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", action="store_true", help="Render fresh PNG/JSONs before encoding")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/playtest/capture")
    parser.add_argument("--source", type=Path, default=DEFAULT_CAPTURES)
    parser.add_argument("--only", nargs="+", choices=BUILDINGS)
    parser.add_argument("--recipes-only", action="store_true")
    args = parser.parse_args()
    SCENARIOS.mkdir(parents=True, exist_ok=True)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    chosen = args.only or list(BUILDINGS)
    manifest_path = OUTPUT / "sources.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {"schema_version": 1, "images": {}}
    for name in chosen:
        model, zoom = BUILDINGS[name]
        scenario = SCENARIOS / f"{name}.ron"
        scenario.write_text(recipe(name, model, zoom))
        if args.recipes_only:
            continue
        directory = args.source.resolve() / name
        if args.capture:
            directory.mkdir(parents=True, exist_ok=True)
            with (directory / "capture.log").open("w") as log:
                subprocess.run([str(args.binary.resolve()), "--scenario", str(scenario), "--out", str(directory)],
                               cwd=ROOT, env={**os.environ, "BEVY_ASSET_ROOT": str(ROOT / "client/assets")},
                               stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
        metadata = verify_capture(directory)
        source = directory / "three-quarter.png"
        # Fit once at delivery size. Camera fitting leaves all geometry inside
        # the central84percent, so this small aspect-ratio crop keeps every roof.
        image = ImageOps.fit(Image.open(source).convert("RGB"), (384, 224),
                             method=Image.Resampling.LANCZOS, centering=(0.5, 0.5))
        output = OUTPUT / f"{name}.jpg"
        image.save(output, quality=80, optimize=True, progressive=True, subsampling=0)
        asset = ROOT / f"client/assets/game_assets/buildings/village/{model}.glb"
        manifest["images"][name] = {
            "asset": str(asset.relative_to(ROOT)),
            "asset_sha256": sha256(asset),
            "scenario": str(scenario.relative_to(ROOT)),
            "capture_git_commit": metadata["git_commit"],
            "width": 384, "height": 224,
            "encoding": "JPEG quality 80, progressive, 4:4:4",
            "bytes": output.stat().st_size,
        }
        print(f"{name}: 384x224, {output.stat().st_size:,} bytes", flush=True)
    if not args.recipes_only:
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
