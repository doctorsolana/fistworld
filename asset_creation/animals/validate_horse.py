"""Validate the canonical horse, including topology and the GLB skin contract."""

import bpy, bmesh, json, struct
from pathlib import Path

root = Path(__file__).resolve().parents[2]
body = bpy.data.objects["Horse"]
rig = bpy.data.objects["HorseRig"]
assert len(body.data.vertices) <= 350, ("Horse vertex budget", len(body.data.vertices))
assert not body.data.validate(verbose=True), "Invalid source mesh required repair"
bm = bmesh.new()
bm.from_mesh(body.data)
assert all(e.is_manifold for e in bm.edges), (
    "Open/non-manifold edges",
    sum(not e.is_manifold for e in bm.edges),
)
assert all(f.calc_area() > 1e-10 for f in bm.faces), "Degenerate faces"
assert all(v.link_faces for v in bm.verts), "Loose source vertices"
assert min(v.co.z for v in bm.verts) > -0.001, "Hoof below ground"
assert abs(min(v.co.z for v in bm.verts)) < 0.001, "Horse floating above ground"
bm.free()
for v in body.data.vertices:
    assert 1 <= len(v.groups) <= 4, ("Skin influence count", v.index, len(v.groups))
    assert abs(sum(g.weight for g in v.groups) - 1) < 1e-5, (
        "Unnormalized skin",
        v.index,
    )
    assert all(body.vertex_groups[g.group].name in rig.data.bones for g in v.groups)
raw = (root / "client/assets/game_assets/environment/animals/Horse.glb").read_bytes()
length = struct.unpack_from("<I", raw, 12)[0]
gltf = json.loads(raw[20 : 20 + length])
assert len(gltf["meshes"]) == 1
assert len(gltf["meshes"][0]["primitives"]) == 1
assert len(gltf["skins"][0]["joints"]) == 21
assert any(n.get("name") == "Anchor_Rider" for n in gltf["nodes"])
primitive = gltf["meshes"][0]["primitives"][0]
print(
    "Horse validated:",
    len(body.data.vertices),
    "source vertices,",
    gltf["accessors"][primitive["attributes"]["POSITION"]]["count"],
    "export vertices,",
    gltf["accessors"][primitive["indices"]]["count"] // 3,
    "triangles",
)
