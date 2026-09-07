"""Check the shipped bow/arrow topology, node contract and release visibility.
Pure Python; run from any working directory after build_bow.py.
"""

import itertools
import json
import math
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path):
    data = path.read_bytes()
    length = struct.unpack_from("<I", data, 12)[0]
    gltf = json.loads(data[20 : 20 + length])
    binary = data[28 + length :]

    def values(index):
        a = gltf["accessors"][index]
        view = gltf["bufferViews"][a["bufferView"]]
        fmt, size = {5126: ("f", 4), 5125: ("I", 4), 5123: ("H", 2), 5121: ("B", 1)}[
            a["componentType"]
        ]
        n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
        offset = view.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = view.get("byteStride", n * size)
        return [
            struct.unpack_from("<" + fmt * n, binary, offset + i * stride)
            for i in range(a["count"])
        ]

    return gltf, values


report = {}
for name in ("Bow", "Arrow"):
    path = ROOT / f"client/assets/game_assets/tools/{name}.glb"
    g, values = read(path)
    assert not g.get("skins") and not g.get("extensionsUsed"), name
    vertices = triangles = primitives = 0
    for mesh in g["meshes"]:
        for p in mesh["primitives"]:
            primitives += 1
            positions = values(p["attributes"]["POSITION"])
            indices = [i[0] for i in values(p["indices"])]
            assert len(indices) % 3 == 0 and all(
                0 <= i < len(positions) for i in indices
            )
            assert all(math.isfinite(v) for row in positions for v in row)
            assert "COLOR_0" in p["attributes"] and "COLOR_1" not in p["attributes"]
            for a, b, c in zip(indices[::3], indices[1::3], indices[2::3]):
                u = [positions[b][i] - positions[a][i] for i in range(3)]
                v = [positions[c][i] - positions[a][i] for i in range(3)]
                cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ]
                assert sum(x * x for x in cross) > 1e-16, (name, "degenerate triangle")
            vertices += len(positions)
            triangles += len(indices) // 3
    nodes = {n.get("name"): i for i, n in enumerate(g["nodes"])}
    if name == "Bow":
        assert {
            "Bow",
            "BowUpper",
            "BowLower",
            "BowGrip",
            "BowStringUpper",
            "BowStringLower",
            "NockedArrow",
            "ArrowRelease",
            "BowTipUpper",
            "BowTipLower",
        } <= nodes.keys()
        clips = {a["name"]: a for a in g["animations"]}
        assert clips.keys() == {"bow_ready", "bow_shoot"}
        for clip, animation in clips.items():
            for channel in animation["channels"]:
                sampler = animation["samplers"][channel["sampler"]]
                times = [v[0] for v in values(sampler["input"])]
                assert abs(times[0]) < 1e-6 and abs(times[-1] - 2) < 1e-6, (
                    clip,
                    times[0],
                    times[-1],
                )
                assert all(a < b for a, b in itertools.pairwise(times))
                assert all(
                    math.isfinite(v) for row in values(sampler["output"]) for v in row
                )
                if (
                    channel["target"] == {"node": nodes["NockedArrow"], "path": "scale"}
                    and clip == "bow_shoot"
                ):
                    keys = list(zip(times, values(sampler["output"])))
                    before = max((t, v) for t, v in keys if t < 1.0)[1]
                    release = max((t, v) for t, v in keys if t <= 1.0)[1]
                    assert before == (1.0, 1.0, 1.0) and release == (0.0, 0.0, 0.0), (
                        keys
                    )
        assert primitives == 6 and triangles < 300
    else:
        assert triangles < 100 and primitives == 1
    report[name] = {
        "bytes": path.stat().st_size,
        "gpu_vertices": vertices,
        "triangles": triangles,
        "primitives": primitives,
    }
print(json.dumps(report, indent=2))
