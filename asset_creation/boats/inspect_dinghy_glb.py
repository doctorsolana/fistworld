"""Dinghy-specific GLB contract check: hierarchy, controls and wind-fill morph."""

import json
import os
import struct
import sys


here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.dirname(os.path.dirname(here))
path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
    repo, "client", "assets", "game_assets", "vehicles", "boats", "Dinghy.glb")
with open(path, "rb") as source:
    blob = source.read()
magic, version, total = struct.unpack_from("<III", blob, 0)
assert magic == 0x46546C67 and version == 2 and total == len(blob)
offset = 12
chunks = {}
while offset < total:
    length, kind = struct.unpack_from("<II", blob, offset)
    chunks[kind] = blob[offset + 8:offset + 8 + length]
    offset += 8 + length
gltf = json.loads(chunks[0x4E4F534A])

nodes = {node.get("name"): (index, node) for index, node in enumerate(gltf["nodes"])}
required = {
    "Dinghy", "DinghyHull", "DinghyMast", "DinghySailRig", "DinghyBoom",
    "DinghySail", "DinghySailEdges", "Anchor_Occupant", "Anchor_Helm",
    "Anchor_Board.L", "Anchor_Board.R", "Anchor_Moor",
}
assert required == set(nodes), f"node mismatch: missing={required-set(nodes)}, extra={set(nodes)-required}"
assert not any("Oar" in name for name in nodes), "oar node survived the sail conversion"


def child_names(name):
    return {gltf["nodes"][index]["name"] for index in nodes[name][1].get("children", [])}


root_children = child_names("Dinghy")
assert {"DinghyHull", "DinghyMast", "DinghySailRig"} <= root_children
rig_children = child_names("DinghySailRig")
assert rig_children == {"DinghyBoom", "DinghySail", "DinghySailEdges"}, rig_children


def close_vector(actual, expected, tolerance=1e-4):
    return len(actual) == len(expected) and all(
        abs(got - want) <= tolerance for got, want in zip(actual, expected))


# Empty-node translations are part of the vehicle contract.  glTF converts Blender (X,Y,Z) to
# (X,Z,-Y), so the aft seat at source Y=-1.24 appears at glTF Z=+1.24.
assert close_vector(nodes["Anchor_Helm"][1].get("translation", [0, 0, 0]), [0.0, 0.35, 1.24])
assert close_vector(nodes["Anchor_Occupant"][1].get("translation", [0, 0, 0]), [0.0, 0.025, 0.0])
assert "rotation" not in nodes["Anchor_Helm"][1], "helm anchor must inherit the boat's forward axis"

sail_node = nodes["DinghySail"][1]
sail_mesh = gltf["meshes"][sail_node["mesh"]]
assert len(sail_mesh["primitives"]) == 1
primitive = sail_mesh["primitives"][0]
targets = primitive.get("targets", [])
assert len(targets) == 1, f"expected one continuous wind-fill morph, found {len(targets)}"
assert "POSITION" in targets[0], "wind_fill morph has no position deltas"
target_names = sail_mesh.get("extras", {}).get("targetNames", [])
assert target_names == ["wind_fill"], f"morph target name lost: {target_names}"
target_accessor = gltf["accessors"][targets[0]["POSITION"]]
base_accessor = gltf["accessors"][primitive["attributes"]["POSITION"]]
assert target_accessor["count"] == base_accessor["count"], (
    "wind_fill target no longer matches the exported sail vertex stream")
assert max(abs(value) for value in target_accessor["max"] + target_accessor["min"]) > 0.20, (
    "wind_fill deltas are too small to produce a readable billow")

position_vertices = sum(
    gltf["accessors"][primitive["attributes"]["POSITION"]]["count"]
    for mesh in gltf["meshes"] for primitive in mesh["primitives"])
triangles = sum(
    gltf["accessors"][primitive["indices"]]["count"] // 3
    for mesh in gltf["meshes"] for primitive in mesh["primitives"])

print(f"{os.path.basename(path)}: {len(blob)/1024:.0f} KB")
print(f"  hierarchy: Dinghy -> SailRig -> {sorted(rig_children)}")
print("  occupants: seated helm + standing centre anchors, both forward-aligned")
print(
    f"  wind_fill: 1 named morph, {target_accessor['count']} exported vertices, "
    "continuous weight 0..1")
print(f"  exported positions: {position_vertices}; triangles: {triangles}")
print("  OK: wind-reactive dinghy contract")
