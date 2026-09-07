"""Validate shipped wardrobe nodes, skinning, clip coverage/times and mesh budgets.

python3 asset_creation/character/validate_character_glb.py
This complements Blender pose inspection and real Bevy captures; it is not visual proof.
"""

import itertools
import json
import math
import struct
from pathlib import Path

from wardrobe_items import DEFAULT_OUTFIT, ITEMS, OUTFITS

ROOT = Path(__file__).resolve().parents[2]
path = ROOT / "client/assets/characters/Humanoid.glb"
raw = path.read_bytes()
magic, version, length = struct.unpack_from("<4sII", raw)
assert magic == b"glTF" and version == 2 and length == len(raw)
json_size, _ = struct.unpack_from("<II", raw, 12)
gltf = json.loads(raw[20 : 20 + json_size])
bin_offset = 20 + json_size + 8
binary = raw[bin_offset:]
COMPONENTS = {5121: ("B", 1), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}
WIDTHS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def values(index):
    accessor = gltf["accessors"][index]
    view = gltf["bufferViews"][accessor["bufferView"]]
    code, size = COMPONENTS[accessor["componentType"]]
    width = WIDTHS[accessor["type"]]
    offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    stride = view.get("byteStride", size * width)
    return [
        struct.unpack_from("<" + code * width, binary, offset + i * stride)
        for i in range(accessor["count"])
    ]


nodes = {node["name"]: node for node in gltf["nodes"] if "name" in node}
assert set(ITEMS) <= nodes.keys(), set(ITEMS) - nodes.keys()
assert "Character_Base" in nodes
assert len(gltf["skins"]) == 1 and len(gltf["skins"][0]["joints"]) == 21
metrics = {}
for name, node in nodes.items():
    if "mesh" not in node:
        continue
    primitives = gltf["meshes"][node["mesh"]]["primitives"]
    assert "skin" in node, name
    vertices = triangles = 0
    for primitive in primitives:
        attrs = primitive["attributes"]
        positions = values(attrs["POSITION"])
        vertices += len(positions)
        indices = [i[0] for i in values(primitive["indices"])]
        triangles += len(indices) // 3
        assert len(indices) % 3 == 0
        assert all(0 <= i < len(positions) for i in indices)
        for point in positions:
            assert all(math.isfinite(v) for v in point), name
        for weights in values(attrs["WEIGHTS_0"]):
            assert abs(sum(weights) - 1) < 1e-5, name
        for joints in values(attrs["JOINTS_0"]):
            assert max(joints) < 21, name
        # Exactly one render colour channel; extra exported channels are ignored by Bevy.
        assert "COLOR_1" not in attrs, name
    metrics[name] = {
        "gpu_vertices": vertices,
        "triangles": triangles,
        "primitives": len(primitives),
    }
clips = {}
required = {
    "bow_ready",
    "bow_shoot",
    "idle",
    "walk",
    "run",
    "build",
    "chop",
    "harvest",
    "carry",
    "pull",
    "combat_strike",
    "combat_guard",
    "combat_recoil",
    "combat_fall",
    "combat_fall_back",
    "swim",
    "swim_idle",
    "lie_down",
    "lie_idle",
}
for animation in gltf["animations"]:
    name = animation["name"]
    durations = []
    targets = set()
    for channel in animation["channels"]:
        sampler = animation["samplers"][channel["sampler"]]
        times = [v[0] for v in values(sampler["input"])]
        assert abs(times[0]) < 1e-6, (name, times[0])
        assert all(a < b for a, b in itertools.pairwise(times)), name
        assert all(
            math.isfinite(v) for row in values(sampler["output"]) for v in row
        ), name
        durations.append(times[-1])
        targets.add(channel["target"]["node"])
    assert len(targets) == 21, (name, len(targets))
    clips[name] = max(durations)
assert required <= clips.keys()
assert abs(clips["combat_strike"] - 1) < 1e-6
assert (
    abs(clips["combat_fall"] - 1) < 1e-6 and abs(clips["combat_fall_back"] - 1) < 1e-6
)
assert abs(clips["lie_down"] - 1.5) < 1e-6
outfits = {}
for name, changes in {"default": {}, **OUTFITS}.items():
    outfit = DEFAULT_OUTFIT | changes
    worn = ["Character_Base", *outfit.values()]
    if outfit["headgear"] != "Headgear_None":
        worn.remove(outfit["hair"])
    outfits[name] = {
        key: sum(metrics.get(item, {}).get(key, 0) for item in worn)
        for key in ("gpu_vertices", "triangles", "primitives")
    }
report = {
    "bytes": len(raw),
    "joints": 21,
    "clips": clips,
    "meshes": metrics,
    "outfits": outfits,
}
out = ROOT / "logs/reviews/character-refresh/glb-validation.json"
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(report, indent=2) + "\n")
print(
    json.dumps({"bytes": len(raw), "clips": len(clips), "outfits": outfits}, indent=2)
)
