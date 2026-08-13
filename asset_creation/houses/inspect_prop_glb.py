"""Verify a prop/building .glb against this repo's Bevy 0.19 conventions. Pure stdlib, no Blender.

    python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/LogCabin.glb

The sibling of inspect_glb.py, which checks CHARACTERS (rig, skin, facing, wardrobe manifest). This
one checks static/node-animated props, where the contract is different: no skins at all, animation
targets nodes rather than joints, and the thing that must be right is that the asset sits ON the
ground plane rather than floating above it or sunk under it.
"""

import json
import os
import struct
import sys
from collections import Counter

path = sys.argv[1]
with open(path, "rb") as f:
    blob = f.read()

magic, version, total = struct.unpack("<III", blob[:12])
assert magic == 0x46546C67, "not a glb"
off, chunks = 12, {}
while off < total:
    clen, ctype = struct.unpack("<II", blob[off : off + 8])
    chunks[ctype] = blob[off + 8 : off + 8 + clen]
    off += 8 + clen
g = json.loads(chunks[0x4E4F534A])
BIN = chunks.get(0x004E4942, b"")

print(f"{os.path.basename(path)}   glb v{version}  {len(blob)/1024:.0f} KB  bin={len(BIN)/1024:.0f} KB")
print(f"scenes={len(g.get('scenes', []))} default={g.get('scene')} nodes={len(g.get('nodes', []))} "
      f"meshes={len(g.get('meshes', []))} skins={len(g.get('skins', []))} "
      f"anims={len(g.get('animations', []))} images={len(g.get('images', []))} "
      f"materials={len(g.get('materials', []))}")

fails = []

print("\n-- extensions --")
used, req = g.get("extensionsUsed", []), g.get("extensionsRequired", [])
print(f"  used: {used or 'none'}\n  required: {req or 'none'}")
if used:
    fails.append(f"KHR extensions present: {used}")

print("\n-- nodes --")
for n in g.get("nodes", []):
    if "mesh" in n:
        m = g["meshes"][n["mesh"]]
        tris = 0
        vtot = 0
        prims = []
        for p in m["primitives"]:
            a = p["attributes"]
            nv = g["accessors"][a["POSITION"]]["count"]
            vtot += nv
            if "indices" in p:
                tris += g["accessors"][p["indices"]]["count"] // 3
            md = g["materials"][p["material"]] if "material" in p else None
            mat = md["name"] if md else "NO-MATERIAL"
            textured = md is not None and "baseColorTexture" in md.get("pbrMetallicRoughness", {})
            tags = "" if "TEXCOORD_0" in a else ",flat"
            if "COLOR_0" in a:
                tags += ",vcol"
            prims.append(f"{mat}({nv}v{tags})")
            # Missing UVs are only a defect on a TEXTURED primitive. A flat-colour part (the window
            # glass) legitimately has none, and demanding them everywhere would push pointless UVs
            # onto it.
            if textured and "TEXCOORD_0" not in a:
                fails.append(f"{n.get('name')} is textured but has no UVs")
            if md is None:
                fails.append(f"{n.get('name')} has no material")
        loc = n.get("translation", (0, 0, 0))
        print(f"  {n.get('name'):16s} mesh  {vtot:5d}v {tris:5d}tri  "
              f"at ({loc[0]:+.2f},{loc[1]:+.2f},{loc[2]:+.2f})  {', '.join(prims)}")
    else:
        loc = n.get("translation", (0, 0, 0))
        print(f"  {n.get('name'):16s} empty            "
              f"at ({loc[0]:+.2f},{loc[1]:+.2f},{loc[2]:+.2f})   <- light/attach anchor")

if g.get("skins"):
    fails.append(f"{len(g['skins'])} skin(s) on a prop; props should be node-animated, not skinned")

print("\n-- animations --")
acc = g["accessors"]


def _float_accessor(index):
    """Read the tightly/strided FLOAT accessor shapes used by prop clips."""
    a = acc[index]
    assert a["componentType"] == 5126, f"accessor {index} is not FLOAT"
    components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
    view = g["bufferViews"][a["bufferView"]]
    start = view.get("byteOffset", 0) + a.get("byteOffset", 0)
    stride = view.get("byteStride", components * 4)
    return [
        struct.unpack_from("<" + "f" * components, BIN, start + row * stride)
        for row in range(a["count"])
    ]


for a in g.get("animations", []):
    paths = Counter(c["target"]["path"] for c in a["channels"])
    targets = {g["nodes"][c["target"]["node"]].get("name") for c in a["channels"]}
    inp = {s["input"] for s in a["samplers"]}
    dur = max(acc[i]["max"][0] for i in inp)
    n = max(acc[i]["count"] for i in inp)
    print(f"  {a['name']!r:14s} {dur:.3f}s  {n} samples  {dict(paths)}  -> {sorted(targets)}")
    bad = set(paths) - {"translation", "rotation", "scale", "weights"}
    if bad:
        fails.append(f"animation {a['name']} targets {bad}, which core glTF cannot carry")
    if a["name"] == "sails_turn":
        rotation_channels = [c for c in a["channels"] if c["target"]["path"] == "rotation"]
        assert len(rotation_channels) == 1, (
            f"sails_turn has {len(rotation_channels)} rotation channels, expected one")
        sampler = a["samplers"][rotation_channels[0]["sampler"]]
        samples = _float_accessor(sampler["output"])
        sample = max(samples, key=lambda q: q[0] ** 2 + q[1] ** 2 + q[2] ** 2)
        axis_length = sum(v * v for v in sample[:3]) ** 0.5
        axis = tuple(v / axis_length for v in sample[:3])
        print(f"    rotation axis ~= ({axis[0]:+.3f}, {axis[1]:+.3f}, {axis[2]:+.3f})")
        # The exported mill faces -Z, so its windshaft and the only physically
        # valid sail rotation axis are Z. X means the source Blender action was
        # not conjugated through the exporter's -90 degree facing turn.
        if abs(axis[2]) < 0.999 or abs(axis[0]) > 0.001 or abs(axis[1]) > 0.001:
            fails.append(
                f"sails_turn rotates around {axis}, expected the exported windshaft Z axis")

print("\n-- materials --")
for m in g.get("materials", []):
    pbr = m.get("pbrMetallicRoughness", {})
    tex = "texture" if "baseColorTexture" in pbr else str(
        [round(v, 3) for v in pbr.get("baseColorFactor", [1, 1, 1, 1])])
    em = m.get("emissiveFactor", [0, 0, 0])
    # doubleSided matters for anything built from flat strips (the wheat field): without it every
    # straw is invisible from behind and half the crop vanishes as the camera orbits.
    sided = "double" if m.get("doubleSided") else "single"
    print(f"  {m['name']:16s} base={tex:12s} metal={pbr.get('metallicFactor')} "
          f"rough={pbr.get('roughnessFactor')} emissive={[round(v,2) for v in em]} {sided}-sided")
    if pbr.get("metallicFactor") not in (0, 0.0):
        fails.append(f"material {m['name']} is metallic={pbr.get('metallicFactor')}, expected 0")

print("\n-- images --")
for im in g.get("images", []):
    bv = g["bufferViews"][im["bufferView"]]
    print(f"  {im.get('name','?'):24s} {im.get('mimeType')} {bv['byteLength']/1024:.0f} KB")

# --- bounds, in glTF space (+Y up, -Z forward) ------------------------------------------------------
# The check that matters for a building: it must SIT on y=0. A prop whose base floats hovers over
# every terrain tile; one that starts well below y=0 sinks. A little below is right -- a foundation
# should bed INTO the ground so no seam shows on uneven terrain.
print("\n-- bounds (glTF space: +Y up, -Z forward) --")


def _node_matrix(n):
    if "matrix" in n:                       # column-major in glTF
        m = n["matrix"]
        return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
    t = n.get("translation", (0.0, 0.0, 0.0))
    sc = n.get("scale", (1.0, 1.0, 1.0))
    x, y, z, w = n.get("rotation", (0.0, 0.0, 0.0, 1.0))
    rot = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
           [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
           [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
    return ([[rot[r][c] * sc[c] for c in range(3)] + [t[r]] for r in range(3)]
            + [[0.0, 0.0, 0.0, 1.0]])


def _mul(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(4)) for c in range(4)] for r in range(4)]


# WALK THE SCENE GRAPH, DO NOT ITERATE NODES FLATLY. The previous version added only each node's OWN
# translation, so any mesh parented to another node was measured in its parent's local space. On the
# windmill -- whose sails are a child of the yawing cap -- that put the sail tips 3 m underground and
# produced a "base runs deep" warning about a model whose real base is at -0.18. The same bug could
# just as easily FAIL a correct asset for floating, so it is not merely cosmetic.
lo, hi = [1e9] * 3, [-1e9] * 3
_ident = [[1.0 if r == c else 0.0 for c in range(4)] for r in range(4)]


def _walk(i, mat):
    n = g["nodes"][i]
    mat = _mul(mat, _node_matrix(n))
    if "mesh" in n:
        for prim in g["meshes"][n["mesh"]]["primitives"]:
            a = g["accessors"][prim["attributes"]["POSITION"]]
            for corner in range(8):         # every corner of the local AABB, then transform
                v = [a["max"][k] if (corner >> k) & 1 else a["min"][k] for k in range(3)]
                for r in range(3):
                    w = sum(mat[r][c] * v[c] for c in range(3)) + mat[r][3]
                    lo[r] = min(lo[r], w)
                    hi[r] = max(hi[r], w)
    for ch in n.get("children", []):
        _walk(ch, mat)


for _root in g["scenes"][g.get("scene", 0)]["nodes"]:
    _walk(_root, _ident)
print(f"  X {lo[0]:+.3f}..{hi[0]:+.3f}   Y {lo[1]:+.3f}..{hi[1]:+.3f}   Z {lo[2]:+.3f}..{hi[2]:+.3f}")
print(f"  footprint {hi[0]-lo[0]:.2f} x {hi[2]-lo[2]:.2f} m, height {hi[1]-lo[1]:.2f} m")
# Two different rules, because they have different consequences.
# FLOATING is always a defect: a prop whose base sits above 0 hovers over every terrain tile.
# SUNK DEEP is merely unusual. A pier's piles are meant to run below the waterline (SEA_LEVEL = 0),
# so the old single range rejected a correct asset. Report it; do not fail it.
if lo[1] > 0.02:
    fails.append(f"base floats at Y={lo[1]:+.3f}; it must bed into the ground, not hover")
elif lo[1] < -0.40:
    print(f"  note: base runs to Y={lo[1]:+.3f} - deep. Correct for piles below SEA_LEVEL=0,"
          f" wrong for anything meant to sit on terrain.")

print("\n" + ("  OK: meets the prop contract" if not fails else "  FAIL:\n    " + "\n    ".join(fails)))
sys.exit(1 if fails else 0)
