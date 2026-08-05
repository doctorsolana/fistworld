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
lo, hi = [1e9] * 3, [-1e9] * 3
for n in g.get("nodes", []):
    if "mesh" not in n:
        continue
    t = n.get("translation", (0, 0, 0))
    for p in g["meshes"][n["mesh"]]["primitives"]:
        a = g["accessors"][p["attributes"]["POSITION"]]
        for i in range(3):
            lo[i] = min(lo[i], a["min"][i] + t[i])
            hi[i] = max(hi[i], a["max"][i] + t[i])
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
