"""Validate baked deformation, loop seams, and the exported animation contract.
Run with the canonical animated horse.blend loaded. Never saves the source.
"""

import json
import math
import struct
from pathlib import Path
import bpy

EXPECTED = {
    "horse_idle": 3.0,
    "horse_graze": 4.0,
    "horse_alert": 2.0,
    "horse_walk": 1.0,
    "horse_trot": 0.7,
    "horse_canter": 0.7,
    "horse_gallop": 0.6,
}
root = Path(__file__).resolve().parents[2]
rig = bpy.data.objects["HorseRig"]
body = bpy.data.objects["Horse"]
scene = bpy.context.scene
assert set(EXPECTED) == {action.name for action in bpy.data.actions}
report = {}
for name, duration in EXPECTED.items():
    action = bpy.data.actions[name]
    rig.animation_data.action = action
    assert (
        abs(
            (action.frame_range[1] - action.frame_range[0]) / scene.render.fps
            - duration
        )
        < 1e-6
    )
    lows, ends = [], []
    frame_count = round(duration * scene.render.fps)
    for sample in range(frame_count * 2 + 1):
        scene.frame_set(sample // 2, subframe=(sample % 2) / 2)
        bpy.context.view_layer.update()
        evaluated = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
        mesh = evaluated.to_mesh()
        assert all(math.isfinite(c) for v in mesh.vertices for c in v.co)
        lows.append(min(v.co.z for v in mesh.vertices))
        if sample in (0, frame_count * 2):
            ends.append([v.co.copy() for v in mesh.vertices])
        evaluated.to_mesh_clear()
    seam = max((a - b).length for a, b in zip(*ends))
    assert seam < 1e-5, (name, "loop seam", seam)
    assert min(lows) > -0.001, (name, "ground penetration", min(lows))
    assert min(lows) < 0.005, (name, "never contacts ground", min(lows))
    report[name] = {"duration": duration, "minimum_z": min(lows), "loop_error": seam}
raw = (root / "client/assets/game_assets/environment/animals/Horse.glb").read_bytes()
size = struct.unpack_from("<I", raw, 12)[0]
gltf = json.loads(raw[20 : 20 + size])
binary = raw[28 + size :]
assert not gltf.get("extensionsRequired")
assert {a["name"] for a in gltf["animations"]} == set(EXPECTED)
for animation in gltf["animations"]:
    durations = []
    for sampler in animation["samplers"]:
        accessor = gltf["accessors"][sampler["input"]]
        view = gltf["bufferViews"][accessor["bufferView"]]
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        times = struct.unpack_from("<" + "f" * accessor["count"], binary, offset)
        assert times[0] == 0.0 and all(a < b for a, b in zip(times, times[1:]))
        durations.append(times[-1])
    assert abs(max(durations) - EXPECTED[animation["name"]]) < 1e-5
    targets = {c["target"]["node"] for c in animation["channels"]}
    assert set(gltf["skins"][0]["joints"]) <= targets, animation["name"]
print(json.dumps(report, indent=2), flush=True)
print("Horse motion and exported clips validated", flush=True)
